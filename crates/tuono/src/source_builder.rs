use glob::glob;
use glob::GlobError;
use regex::Regex;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::io::prelude::*;
use std::path::Path;
use std::path::PathBuf;

mod static_files;

const ROOT_FOLDER: &str = "src/routes";
const DEV_FOLDER: &str = ".tuono";

pub enum Mode {
    Prod,
    Dev,
}

#[derive(Debug, PartialEq, Eq)]
struct Route {
    /// Every module import is the path with a _ instead of /
    pub module_import: String,
    pub axum_route: String,
}

fn has_dynamic_path(route: &str) -> bool {
    let regex = Regex::new(r"\[(.*?)\]").expect("Failed to create the regex");
    regex.is_match(route)
}

impl Route {
    pub fn new(path: &str) -> Self {
        let route_name = path.replace(".rs", "");
        // Remove first slash
        let mut module = route_name.as_str().chars();
        module.next();

        let axum_route = path.replace("/index.rs", "").replace(".rs", "");

        if axum_route.is_empty() {
            return Route {
                module_import: module.as_str().to_string().replace('/', "_"),
                axum_route: "/".to_string(),
            };
        }

        if has_dynamic_path(&route_name) {
            return Route {
                module_import: module
                    .as_str()
                    .to_string()
                    .replace('/', "_")
                    .replace('[', "dyn_")
                    .replace(']', ""),
                axum_route: axum_route.replace('[', ":").replace(']', ""),
            };
        }

        Route {
            module_import: module.as_str().to_string().replace('/', "_"),
            axum_route,
        }
    }
}

struct SourceBuilder {
    route_map: HashMap<PathBuf, Route>,
    mode: Mode,
    base_path: PathBuf,
}

impl SourceBuilder {
    pub fn new(mode: Mode) -> Self {
        let base_path = std::env::current_dir().unwrap();

        SourceBuilder {
            route_map: HashMap::new(),
            mode,
            base_path,
        }
    }

    fn collect_routes(&mut self) {
        glob(self.base_path.join("src/routes/**/*.rs").to_str().unwrap())
            .unwrap()
            .for_each(|entry| self.collect_route(entry))
    }

    fn collect_route(&mut self, path_buf: Result<PathBuf, GlobError>) {
        let entry = path_buf.unwrap();
        let base_path_str = self.base_path.to_str().unwrap();
        let path = entry
            .to_str()
            .unwrap()
            .replace(&format!("{base_path_str}/src/routes"), "");

        let route = Route::new(&path);

        self.route_map.insert(PathBuf::from(path), route);
    }
}

fn create_main_file(base_path: &Path, bundled_file: &String) {
    let mut data_file =
        fs::File::create(base_path.join(".tuono/main.rs")).expect("creation failed");

    data_file
        .write_all(bundled_file.as_bytes())
        .expect("write failed");
}

fn create_routes_declaration(routes: &HashMap<PathBuf, Route>) -> String {
    let mut route_declarations = String::from("// ROUTE_BUILDER\n");

    for (_, route) in routes.iter() {
        let Route {
            axum_route,
            module_import,
        } = &route;

        route_declarations.push_str(&format!(
            r#".route("{axum_route}", get({module_import}::route))"#
        ));
        route_declarations.push_str(&format!(
            r#".route("/__tuono/data{axum_route}", get({module_import}::api))"#
        ));
    }

    route_declarations
}

fn create_modules_declaration(routes: &HashMap<PathBuf, Route>) -> String {
    let mut route_declarations = String::from("// MODULE_IMPORTS\n");

    for (path, route) in routes.iter() {
        let module_name = &route.module_import;
        let path_str = path.to_str().unwrap();
        route_declarations.push_str(&format!(
            r#"#[path="../{ROOT_FOLDER}{path_str}"]
mod {module_name};
"#
        ))
    }

    route_declarations
}

pub fn bundle_axum_source() -> io::Result<()> {
    println!("Axum project bundling");

    let base_path = std::env::current_dir().unwrap();

    let mut source_builder = SourceBuilder::new(Mode::Dev);

    source_builder.collect_routes();

    let bundled_file = static_files::AXUM_ENTRY_POINT
        .replace(
            "// ROUTE_BUILDER\n",
            &create_routes_declaration(&source_builder.route_map),
        )
        .replace(
            "// MODULE_IMPORTS\n",
            &create_modules_declaration(&source_builder.route_map),
        );

    create_main_file(&base_path, &bundled_file);

    Ok(())
}

pub fn check_tuono_folder() -> io::Result<()> {
    let dev_folder = Path::new(DEV_FOLDER);
    if !&dev_folder.is_dir() {
        println!("exists");
        fs::create_dir(dev_folder)?;
    }

    Ok(())
}

pub fn create_client_entry_files() -> io::Result<()> {
    let dev_folder = Path::new(DEV_FOLDER);

    let mut server_entry = fs::File::create(dev_folder.join("server-main.tsx"))?;
    let mut client_entry = fs::File::create(dev_folder.join("client-main.tsx"))?;

    server_entry.write_all(static_files::SERVER_ENTRY_DATA.as_bytes())?;
    client_entry.write_all(static_files::CLIENT_ENTRY_DATA.as_bytes())?;

    Ok(())
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn find_dynamic_paths() {
        let routes = [
            ("/home/user/Documents/tuono/src/routes/about.rs", false),
            ("/home/user/Documents/tuono/src/routes/index.rs", false),
            (
                "/home/user/Documents/tuono/src/routes/posts/index.rs",
                false,
            ),
            (
                "/home/user/Documents/tuono/src/routes/posts/[post].rs",
                true,
            ),
        ];

        routes
            .into_iter()
            .for_each(|route| assert_eq!(has_dynamic_path(route.0), route.1));
    }

    #[test]
    fn collect_routes() {
        let mut source_builder = SourceBuilder::new(Mode::Dev);
        source_builder.base_path = "/home/user/Documents/tuono".into();

        let routes = [
            "/home/user/Documents/tuono/src/routes/about.rs",
            "/home/user/Documents/tuono/src/routes/index.rs",
            "/home/user/Documents/tuono/src/routes/posts/index.rs",
            "/home/user/Documents/tuono/src/routes/posts/[post].rs",
        ];

        routes
            .into_iter()
            .for_each(|route| source_builder.collect_route(Ok(PathBuf::from(route))));

        let results = [
            ("/index.rs", "index"),
            ("/about.rs", "about"),
            ("/posts/index.rs", "posts_index"),
            ("/posts/[post].rs", "posts_dyn_post"),
        ];

        results.into_iter().for_each(|(path, module_import)| {
            assert_eq!(
                source_builder
                    .route_map
                    .get(&PathBuf::from(path))
                    .unwrap()
                    .module_import,
                String::from(module_import)
            )
        })
    }

    #[test]
    fn create_multi_level_axum_paths() {
        let mut source_builder = SourceBuilder::new(Mode::Dev);
        source_builder.base_path = "/home/user/Documents/tuono".into();

        let routes = [
            "/home/user/Documents/tuono/src/routes/about.rs",
            "/home/user/Documents/tuono/src/routes/index.rs",
            "/home/user/Documents/tuono/src/routes/posts/index.rs",
            "/home/user/Documents/tuono/src/routes/posts/any-post.rs",
            "/home/user/Documents/tuono/src/routes/posts/[post].rs",
        ];

        routes
            .into_iter()
            .for_each(|route| source_builder.collect_route(Ok(PathBuf::from(route))));

        let results = [
            ("/index.rs", "/"),
            ("/about.rs", "/about"),
            ("/posts/index.rs", "/posts"),
            ("/posts/any-post.rs", "/posts/any-post"),
            ("/posts/[post].rs", "/posts/:post"),
        ];

        results.into_iter().for_each(|(path, expected_path)| {
            assert_eq!(
                source_builder
                    .route_map
                    .get(&PathBuf::from(path))
                    .unwrap()
                    .axum_route,
                String::from(expected_path)
            )
        })
    }
}