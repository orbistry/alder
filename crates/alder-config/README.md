# alder-config

Configuration file parsing for Alder projects.

This crate handles parsing and validation of `alder.jsonc` configuration files with accurate error messages including line and column positions.

## JSON Schema

For IDE support and validation, use the [JSON Schema](https://github.com/orbistry/alder/blob/main/crates/alder-config/alder.schema.json):

```jsonc
{
    "$schema": "https://raw.githubusercontent.com/orbistry/alder/main/crates/alder-config/alder.schema.json",
    "type": "application",
    // ...
}
```

## Config Types

Alder supports three configuration types:

### Application

An application compiles to UPLC validators for the Cardano blockchain.

```jsonc
{
    "type": "application",
    "sourceDirectories": ["src"],
    "dependencies": {
        "alder/core": "1.0.0 <= v < 2.0.0",
        "alice/json": { "workspace": true }
    },
    "testDependencies": {
        "alder/test": "1.0.0 <= v < 2.0.0"
    }
}
```

### Package

A publishable library that can be used as a dependency.

```jsonc
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
```

Exposed modules can also be categorized:

```jsonc
{
    "exposedModules": {
        "Decoding": ["Json.Decode", "Json.Decode.Pipeline"],
        "Encoding": ["Json.Encode"]
    }
}
```

### Workspace

A collection of related projects that share dependencies.

```jsonc
{
    "type": "workspace",
    "members": ["packages/*", "apps/my-app"],
    "dependencies": {
        "alder/core": "1.0.0 <= v < 2.0.0",
        "alice/json": "2.0.0 <= v < 3.0.0"
    }
}
```

Members can inherit workspace dependencies:

```jsonc
// packages/my-lib/alder.jsonc
{
    "type": "package",
    "name": "bob/my-lib",
    "version": "1.0.0",
    "summary": "My library",
    "license": "MIT",
    "exposedModules": ["MyLib"],
    "dependencies": {
        "alder/core": { "workspace": true }
    }
}
```

## Dependency Types

Dependencies can be specified in several ways:

### Version Constraint

```jsonc
"alder/core": "1.0.0 <= v < 2.0.0"
```

### Workspace Inheritance

Inherit the constraint from the workspace root:

```jsonc
"alder/core": { "workspace": true }
```

### Path Dependency

Reference a local package:

```jsonc
"my-lib": { "path": "../packages/my-lib" }
```

### Git Dependency

Reference a git repository:

```jsonc
"alice/experimental": {
    "git": "https://github.com/alice/experimental",
    "branch": "main"
}
```

Git dependencies support `branch`, `tag`, or `rev` (commit hash).

## Package Names

Package names follow the `author/project` format:

- Both parts must start with a lowercase letter
- Can contain lowercase letters, digits, and hyphens
- Cannot end with a hyphen or have consecutive hyphens

Examples: `alder/core`, `alice/json-parser`, `bob123/my-lib2`

## Usage

```rust
use alder_config::{parse_file, Config};

fn main() -> Result<(), alder_config::ConfigError> {
    let config = parse_file("alder.jsonc")?;

    match config {
        Config::Application(app) => {
            println!("Application with {} dependencies", app.dependencies.len());
        }
        Config::Package(pkg) => {
            println!("Package: {}", pkg.name);
        }
        Config::Workspace(ws) => {
            println!("Workspace with {} members", ws.members.len());
        }
    }

    Ok(())
}
```

## License

Apache-2.0
