//! JSONC parsing with position tracking.
//!
//! This module provides AST-based parsing of JSONC config files,
//! preserving line/column positions for accurate error messages.

use std::collections::BTreeMap;
use std::path::Path;

use jsonc_parser::ast::{Array, Object, ObjectProp, Value};
use jsonc_parser::common::{Range, Ranged};
use jsonc_parser::{CollectOptions, ParseOptions, parse_to_ast};

use crate::config::{
    Application, Config, Dependency, DependencySource, ExposedModules, GitDep, Package, PathDep,
    Workspace, WorkspaceDep,
};
use crate::error::{ConfigError, Position};
use crate::name::{PackageName, PackageNameError};

/// Parse a config file from a path.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let path = path.as_ref();
    let contents = std::fs::read_to_string(path).map_err(|e| ConfigError::read_error(path, e))?;
    parse(&contents, path)
}

/// Parse a config string with a path for error messages.
pub fn parse(contents: &str, path: impl AsRef<Path>) -> Result<Config, ConfigError> {
    let path = path.as_ref();

    let result = parse_to_ast(
        contents,
        &CollectOptions::default(),
        &ParseOptions::default(),
    )
    .map_err(|e| ConfigError::parse_error(path, e.to_string()))?;

    let root = result
        .value
        .as_ref()
        .ok_or_else(|| ConfigError::empty_file(path))?;

    let obj = root
        .as_object()
        .ok_or_else(|| ConfigError::expected_object(path, position_of(contents, root.range())))?;

    parse_config(contents, path, obj)
}

fn parse_config(contents: &str, path: &Path, obj: &Object) -> Result<Config, ConfigError> {
    let type_prop = find_property(obj, "type").ok_or_else(|| {
        ConfigError::missing_field(path, "type", position_of(contents, obj.range))
    })?;

    let type_value = type_prop.value.as_string_lit().ok_or_else(|| {
        ConfigError::expected_string(path, position_of(contents, type_prop.range()))
    })?;

    match type_value.value.as_ref() {
        "application" => Ok(Config::Application(parse_application(contents, path, obj)?)),
        "package" => Ok(Config::Package(parse_package(contents, path, obj)?)),
        "workspace" => Ok(Config::Workspace(parse_workspace(contents, path, obj)?)),
        other => Err(ConfigError::invalid_type(
            path,
            other.to_string(),
            position_of(contents, type_prop.value.range()),
        )),
    }
}

fn parse_application(
    contents: &str,
    path: &Path,
    obj: &Object,
) -> Result<Application, ConfigError> {
    let compiler = parse_optional_string(contents, path, obj, "compiler")?;

    let source_directories = if let Some(prop) = find_property(obj, "sourceDirectories") {
        parse_string_array(contents, path, &prop.value, "sourceDirectories")?
    } else {
        vec!["src".to_string()]
    };

    let dependencies = if let Some(prop) = find_property(obj, "dependencies") {
        parse_dependencies(contents, path, &prop.value)?
    } else {
        BTreeMap::new()
    };

    let test_dependencies = if let Some(prop) = find_property(obj, "testDependencies") {
        parse_dependencies(contents, path, &prop.value)?
    } else {
        BTreeMap::new()
    };

    Ok(Application {
        compiler,
        source_directories,
        dependencies,
        test_dependencies,
    })
}

fn parse_package(contents: &str, path: &Path, obj: &Object) -> Result<Package, ConfigError> {
    let compiler = parse_optional_string(contents, path, obj, "compiler")?;

    let name = parse_required_string(contents, path, obj, "name")?;
    let name: PackageName = name.parse().map_err(|e: PackageNameError| {
        ConfigError::invalid_package_name(
            path,
            e.to_string(),
            position_of(contents, find_property(obj, "name").unwrap().value.range()),
        )
    })?;

    let version = parse_required_string(contents, path, obj, "version")?;
    let summary = parse_required_string(contents, path, obj, "summary")?;
    let license = parse_required_string(contents, path, obj, "license")?;

    let exposed_modules = {
        let prop = find_property(obj, "exposedModules").ok_or_else(|| {
            ConfigError::missing_field(path, "exposedModules", position_of(contents, obj.range))
        })?;
        parse_exposed_modules(contents, path, &prop.value)?
    };

    let dependencies = if let Some(prop) = find_property(obj, "dependencies") {
        parse_dependencies(contents, path, &prop.value)?
    } else {
        BTreeMap::new()
    };

    let test_dependencies = if let Some(prop) = find_property(obj, "testDependencies") {
        parse_dependencies(contents, path, &prop.value)?
    } else {
        BTreeMap::new()
    };

    Ok(Package {
        compiler,
        name,
        version,
        summary,
        license,
        exposed_modules,
        dependencies,
        test_dependencies,
    })
}

fn parse_workspace(contents: &str, path: &Path, obj: &Object) -> Result<Workspace, ConfigError> {
    let compiler = parse_optional_string(contents, path, obj, "compiler")?;

    let members = {
        let prop = find_property(obj, "members").ok_or_else(|| {
            ConfigError::missing_field(path, "members", position_of(contents, obj.range))
        })?;
        parse_string_array(contents, path, &prop.value, "members")?
    };

    let dependencies = if let Some(prop) = find_property(obj, "dependencies") {
        let deps = parse_dependencies(contents, path, &prop.value)?;

        // Validate: workspace config cannot use { "workspace": true } dependencies
        if let Some(dep_obj) = prop.value.as_object() {
            for dep_prop in &dep_obj.properties {
                if let Some(inner_obj) = dep_prop.value.as_object()
                    && find_property(inner_obj, "workspace").is_some()
                {
                    return Err(ConfigError::workspace_dep_in_workspace(
                        path,
                        position_of(contents, dep_prop.value.range()),
                    ));
                }
            }
        }

        deps
    } else {
        BTreeMap::new()
    };

    Ok(Workspace {
        compiler,
        members,
        dependencies,
    })
}

fn parse_dependencies(
    contents: &str,
    path: &Path,
    value: &Value,
) -> Result<BTreeMap<PackageName, Dependency>, ConfigError> {
    let obj = value
        .as_object()
        .ok_or_else(|| ConfigError::expected_object(path, position_of(contents, value.range())))?;

    let mut result = BTreeMap::new();

    for prop in &obj.properties {
        let name: PackageName = prop.name.as_str().parse().map_err(|e: PackageNameError| {
            ConfigError::invalid_package_name(
                path,
                e.to_string(),
                position_of(contents, prop.name.range()),
            )
        })?;

        let dep = parse_dependency(contents, path, &prop.value)?;
        result.insert(name, dep);
    }

    Ok(result)
}

fn parse_dependency(contents: &str, path: &Path, value: &Value) -> Result<Dependency, ConfigError> {
    // String = version constraint
    if let Some(s) = value.as_string_lit() {
        return Ok(Dependency::Constraint(s.value.to_string()));
    }

    // Object = workspace, path, or git dependency
    let obj = value.as_object().ok_or_else(|| {
        ConfigError::expected_dependency(path, position_of(contents, value.range()))
    })?;

    // Check for workspace dependency
    if let Some(prop) = find_property(obj, "workspace") {
        let workspace_value = prop.value.as_boolean_lit().ok_or_else(|| {
            ConfigError::expected_bool(path, position_of(contents, prop.value.range()))
        })?;

        if !workspace_value.value {
            return Err(ConfigError::workspace_must_be_true(
                path,
                position_of(contents, prop.value.range()),
            ));
        }

        return Ok(Dependency::Source(DependencySource::Workspace(
            WorkspaceDep { workspace: true },
        )));
    }

    // Check for path dependency
    if let Some(prop) = find_property(obj, "path") {
        let path_value = prop.value.as_string_lit().ok_or_else(|| {
            ConfigError::expected_string(path, position_of(contents, prop.value.range()))
        })?;

        return Ok(Dependency::Source(DependencySource::Path(PathDep {
            path: path_value.value.to_string(),
        })));
    }

    // Check for git dependency
    if let Some(prop) = find_property(obj, "git") {
        let git_url = prop.value.as_string_lit().ok_or_else(|| {
            ConfigError::expected_string(path, position_of(contents, prop.value.range()))
        })?;

        let branch = find_property(obj, "branch")
            .map(|p| {
                p.value
                    .as_string_lit()
                    .map(|s| s.value.to_string())
                    .ok_or_else(|| {
                        ConfigError::expected_string(path, position_of(contents, p.value.range()))
                    })
            })
            .transpose()?;

        let tag = find_property(obj, "tag")
            .map(|p| {
                p.value
                    .as_string_lit()
                    .map(|s| s.value.to_string())
                    .ok_or_else(|| {
                        ConfigError::expected_string(path, position_of(contents, p.value.range()))
                    })
            })
            .transpose()?;

        let rev = find_property(obj, "rev")
            .map(|p| {
                p.value
                    .as_string_lit()
                    .map(|s| s.value.to_string())
                    .ok_or_else(|| {
                        ConfigError::expected_string(path, position_of(contents, p.value.range()))
                    })
            })
            .transpose()?;

        return Ok(Dependency::Source(DependencySource::Git(GitDep {
            git: git_url.value.to_string(),
            branch,
            tag,
            rev,
        })));
    }

    // Unknown dependency format
    Err(ConfigError::invalid_dependency(
        path,
        position_of(contents, value.range()),
    ))
}

fn parse_exposed_modules(
    contents: &str,
    path: &Path,
    value: &Value,
) -> Result<ExposedModules, ConfigError> {
    // Array = flat list
    if let Some(arr) = value.as_array() {
        let modules = parse_string_array_inner(contents, path, arr, "exposedModules")?;
        return Ok(ExposedModules::List(modules));
    }

    // Object = categorized
    if let Some(obj) = value.as_object() {
        let mut categories = BTreeMap::new();

        for prop in &obj.properties {
            let category = prop.name.as_str().to_string();
            let modules = parse_string_array(contents, path, &prop.value, &category)?;
            categories.insert(category, modules);
        }

        return Ok(ExposedModules::Categorized(categories));
    }

    Err(ConfigError::expected_array_or_object(
        path,
        position_of(contents, value.range()),
    ))
}

fn parse_string_array(
    contents: &str,
    path: &Path,
    value: &Value,
    field_name: &str,
) -> Result<Vec<String>, ConfigError> {
    let arr = value.as_array().ok_or_else(|| {
        ConfigError::expected_array(path, field_name, position_of(contents, value.range()))
    })?;
    parse_string_array_inner(contents, path, arr, field_name)
}

fn parse_string_array_inner(
    contents: &str,
    path: &Path,
    arr: &Array,
    _field_name: &str,
) -> Result<Vec<String>, ConfigError> {
    let mut result = Vec::new();

    for elem in &arr.elements {
        let s = elem.as_string_lit().ok_or_else(|| {
            ConfigError::expected_string(path, position_of(contents, elem.range()))
        })?;
        result.push(s.value.to_string());
    }

    Ok(result)
}

fn parse_optional_string(
    contents: &str,
    path: &Path,
    obj: &Object,
    field_name: &str,
) -> Result<Option<String>, ConfigError> {
    let Some(prop) = find_property(obj, field_name) else {
        return Ok(None);
    };

    let value = prop.value.as_string_lit().ok_or_else(|| {
        ConfigError::expected_string(path, position_of(contents, prop.value.range()))
    })?;

    Ok(Some(value.value.to_string()))
}

fn parse_required_string(
    contents: &str,
    path: &Path,
    obj: &Object,
    field_name: &str,
) -> Result<String, ConfigError> {
    let prop = find_property(obj, field_name).ok_or_else(|| {
        ConfigError::missing_field(path, field_name, position_of(contents, obj.range))
    })?;

    let value = prop.value.as_string_lit().ok_or_else(|| {
        ConfigError::expected_string(path, position_of(contents, prop.value.range()))
    })?;

    Ok(value.value.to_string())
}

fn find_property<'a>(obj: &'a Object, name: &str) -> Option<&'a ObjectProp<'a>> {
    obj.properties.iter().find(|p| p.name.as_str() == name)
}

/// Convert a jsonc-parser Range to a line/column position.
fn position_of(contents: &str, range: Range) -> Position {
    let mut line = 1;
    let mut column = 1;

    for (i, c) in contents.char_indices() {
        if i >= range.start {
            break;
        }
        if c == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    Position { line, column }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;

    #[test]
    fn parse_standalone_application() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {
                    "alder/core": "1.0.0 <= v < 2.0.0"
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Application(app) => {
                assert_eq!(app.source_directories, vec!["src"]);
                assert_eq!(app.dependencies.len(), 1);
                let dep = app
                    .dependencies
                    .get(&"alder/core".parse().unwrap())
                    .unwrap();
                assert_eq!(dep.as_constraint(), Some("1.0.0 <= v < 2.0.0"));
            }
            _ => panic!("expected application config"),
        }
    }

    #[test]
    fn parse_application_with_workspace_dep() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {
                    "alder/core": { "workspace": true }
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Application(app) => {
                let dep = app
                    .dependencies
                    .get(&"alder/core".parse().unwrap())
                    .unwrap();
                assert!(dep.is_workspace());
            }
            _ => panic!("expected application config"),
        }
    }

    #[test]
    fn parse_application_with_path_dep() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {
                    "bob/my-lib": { "path": "../packages/my-lib" }
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Application(app) => {
                let dep = app
                    .dependencies
                    .get(&"bob/my-lib".parse().unwrap())
                    .unwrap();
                assert!(dep.is_path());
            }
            _ => panic!("expected application config"),
        }
    }

    #[test]
    fn parse_application_with_git_dep() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {
                    "alice/experimental": {
                        "git": "https://github.com/alice/experimental",
                        "branch": "main"
                    }
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Application(app) => {
                let dep = app
                    .dependencies
                    .get(&"alice/experimental".parse().unwrap())
                    .unwrap();
                assert!(dep.is_git());
            }
            _ => panic!("expected application config"),
        }
    }

    #[test]
    fn parse_workspace() {
        let json = indoc! {r#"
            {
                "type": "workspace",
                "members": ["packages/*", "apps/my-app"],
                "dependencies": {
                    "alder/core": "1.0.0 <= v < 2.0.0",
                    "alice/json": { "path": "../json" }
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Workspace(ws) => {
                assert_eq!(ws.members, vec!["packages/*", "apps/my-app"]);
                assert_eq!(ws.dependencies.len(), 2);
                let dep = ws.dependencies.get(&"alder/core".parse().unwrap()).unwrap();
                assert_eq!(dep.as_constraint(), Some("1.0.0 <= v < 2.0.0"));
                let json_dep = ws.dependencies.get(&"alice/json".parse().unwrap()).unwrap();
                assert!(json_dep.is_path());
            }
            _ => panic!("expected workspace config"),
        }
    }

    #[test]
    fn reject_workspace_dep_in_workspace() {
        let json = indoc! {r#"
            {
                "type": "workspace",
                "members": ["packages/*"],
                "dependencies": {
                    "alder/core": { "workspace": true }
                }
            }
        "#};

        let result = parse(json, "test.jsonc");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("workspace config cannot use"));
    }

    #[test]
    fn parse_package() {
        let json = indoc! {r#"
            {
                "type": "package",
                "name": "alice/json-parser",
                "version": "1.0.0",
                "summary": "A JSON parser for Alder",
                "license": "MIT",
                "exposedModules": ["Json", "Json.Decode", "Json.Encode"],
                "dependencies": {
                    "alder/core": "1.0.0 <= v < 2.0.0"
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Package(pkg) => {
                assert_eq!(pkg.name, "alice/json-parser".parse().unwrap());
                assert_eq!(pkg.version, "1.0.0");
                assert_eq!(pkg.summary, "A JSON parser for Alder");
                assert_eq!(pkg.license, "MIT");
                assert_eq!(
                    pkg.exposed_modules.flatten(),
                    vec!["Json", "Json.Decode", "Json.Encode"]
                );
            }
            _ => panic!("expected package config"),
        }
    }

    #[test]
    fn parse_package_with_categorized_modules() {
        let json = indoc! {r#"
            {
                "type": "package",
                "name": "alice/json-parser",
                "version": "1.0.0",
                "summary": "A JSON parser",
                "license": "MIT",
                "exposedModules": {
                    "Decoding": ["Json.Decode"],
                    "Encoding": ["Json.Encode"]
                },
                "dependencies": {}
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Package(pkg) => match &pkg.exposed_modules {
                ExposedModules::Categorized(cats) => {
                    assert_eq!(cats.len(), 2);
                    assert!(cats.contains_key("Decoding"));
                    assert!(cats.contains_key("Encoding"));
                }
                _ => panic!("expected categorized modules"),
            },
            _ => panic!("expected package config"),
        }
    }

    #[test]
    fn parse_jsonc_with_comments() {
        let json = indoc! {r#"
            {
                // This is a comment
                "type": "application",
                "dependencies": {
                    /* Multi-line
                       comment */
                    "alder/core": "1.0.0"
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();
        assert!(matches!(config, Config::Application(_)));
    }

    #[test]
    fn reject_workspace_false() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {
                    "alder/core": { "workspace": false }
                }
            }
        "#};

        let result = parse(json, "test.jsonc");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("must be true"));
    }

    #[test]
    fn parse_application_with_compiler_field() {
        let json = indoc! {r#"
            {
                "type": "application",
                "compiler": "0.2.0",
                "dependencies": {
                    "alder/core": "1.0.0 <= v < 2.0.0"
                }
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match &config {
            Config::Application(app) => {
                assert_eq!(app.compiler.as_deref(), Some("0.2.0"));
            }
            _ => panic!("expected application config"),
        }
        assert_eq!(config.compiler(), Some("0.2.0"));
    }

    #[test]
    fn parse_workspace_with_compiler_field() {
        let json = indoc! {r#"
            {
                "type": "workspace",
                "compiler": "0.3.0",
                "members": ["packages/*"]
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();

        match config {
            Config::Workspace(ws) => {
                assert_eq!(ws.compiler.as_deref(), Some("0.3.0"));
            }
            _ => panic!("expected workspace config"),
        }
    }

    #[test]
    fn compiler_field_is_optional() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {}
            }
        "#};

        let config = parse(json, "test.jsonc").unwrap();
        assert_eq!(config.compiler(), None);
    }

    #[test]
    fn error_has_position() {
        let json = indoc! {r#"
            {
                "type": "application",
                "dependencies": {
                    "Invalid Name": "1.0.0"
                }
            }
        "#};

        let result = parse(json, "test.jsonc");
        assert!(result.is_err());
        let err = result.unwrap_err();
        // Error should contain line/column info
        let msg = err.to_string();
        assert!(msg.contains("4:") || msg.contains("line 4"));
    }
}
