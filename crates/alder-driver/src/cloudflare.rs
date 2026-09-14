//! Pure Cloudflare metadata validation and Wrangler configuration generation.
//! Resource identifiers are explicit inputs; no account discovery or remote writes.
use std::collections::BTreeSet;

use alder_codegen::support::cloudflare::{Adapter, AdapterKind, ProviderBinding};
use serde_json::{Value, json};

use crate::interface::{
    InterfaceFile, OwnedDictionaryKind, OwnedPackageId, OwnedPublicTypeBody, OwnedQualifiedName,
    OwnedType,
};

#[derive(Clone, Debug, Default)]
pub struct Metadata {
    pub adapters: Vec<Adapter>,
    pub bindings: Vec<Binding>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub context_key: String,
    pub name: String,
    pub kind: String,
    pub arguments: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    pub source_uri: String,
    pub region: Option<alder_region::Region>,
    pub message: String,
}

impl Metadata {
    pub fn providers(&self) -> Vec<ProviderBinding> {
        self.bindings
            .iter()
            .map(|binding| (binding.context_key.clone(), binding.name.clone()))
            .collect()
    }

    pub fn extend(&mut self, other: Self) {
        self.adapters.extend(other.adapters);
        self.bindings.extend(other.bindings);
    }
}

/// Joins source-bearing emitted artifacts to their exact solved module identity.
/// Generated modules have no source text and are intentionally skipped.
pub fn extract_build(build: &crate::BuildResult) -> Result<Metadata, Vec<Diagnostic>> {
    let bump = bumpalo::Bump::new();
    let interfaces = build
        .interfaces
        .iter()
        .map(|interface| {
            (
                alder_codegen::module_specifier(interface.hydrate(&bump).home),
                interface,
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
    let mut artifacts = build.artifacts.iter().collect::<Vec<_>>();
    artifacts.sort_by(|(left, _), (right, _)| left.as_str().cmp(right.as_str()));
    let mut metadata = Metadata::default();
    let mut errors = Vec::new();
    for (uri, artifact) in artifacts {
        let Some(source) = &artifact.source_text else {
            continue;
        };
        let Some(interface) = interfaces.get(&artifact.module_id) else {
            errors.push(Diagnostic {
                source_uri: uri.to_string(),
                region: None,
                message: format!(
                    "No solved interface for emitted module {}",
                    artifact.module_id
                ),
            });
            continue;
        };
        match extract(uri.as_str(), source, interface) {
            Ok(found) => metadata.extend(found),
            Err(found) => errors.extend(found),
        }
    }
    if errors.is_empty() {
        Ok(metadata)
    } else {
        Err(errors)
    }
}

fn builtin(reference: &OwnedQualifiedName, name: &str) -> bool {
    reference.module.package == OwnedPackageId::Builtin
        && reference.module.path == ["cloudflare"]
        && reference.name == name
}

fn matches_type(typ: &OwnedType, reference: &OwnedQualifiedName) -> bool {
    matches!(typ, OwnedType::Alias { reference: actual, .. } | OwnedType::Named { reference: actual, .. } if actual == reference)
}

fn platform_type(typ: &OwnedType, name: &str) -> bool {
    match typ {
        OwnedType::Alias { target, .. } => platform_type(&target.typ, name),
        OwnedType::Named { reference, args } => args.is_empty() && builtin(reference, name),
        _ => false,
    }
}

/// Attributes live on public concrete type declarations. Separate resource
/// bindings from adapter state: a DO namespace is not a DO instance state value.
pub fn extract(
    source_uri: &str,
    source: &str,
    interface: &InterfaceFile,
) -> Result<Metadata, Vec<Diagnostic>> {
    let bump = bumpalo::Bump::new();
    let source = bump.alloc_str(source);
    let parsed = alder_parse::parse_module(&bump, source).map_err(|error| {
        vec![Diagnostic {
            source_uri: source_uri.to_owned(),
            region: None,
            message: format!("Cannot read Cloudflare attributes: {error:?}"),
        }]
    })?;
    let hydrated = interface.hydrate(&bump);
    let module = alder_codegen::module_specifier(hydrated.home);
    let mut metadata = Metadata::default();
    let mut errors = Vec::new();
    for item in parsed.items {
        for attribute in item.value.attributes {
            let kind = attribute.value.name.value;
            if !matches!(kind, "binding" | "durable_object" | "queue" | "workflow") {
                continue;
            }
            let mut fail = |message: String| {
                errors.push(Diagnostic {
                    source_uri: source_uri.to_owned(),
                    region: Some(attribute.region),
                    message,
                })
            };
            let name = match item.value.kind {
                alder_source::ItemKind::TypeAlias(alias) => alias.name.value,
                alder_source::ItemKind::OpaqueType(name) => name.value,
                alder_source::ItemKind::Enum(declaration) => declaration.name.value,
                _ => {
                    fail(format!(
                        "#[{kind}] requires a public concrete type declaration"
                    ));
                    continue;
                }
            };
            let Some(declaration) = interface.types.iter().find(|declaration| {
                declaration.exported_as == name && declaration.reference.module == interface.module
            }) else {
                fail(format!("#[{kind}] type {name} must be public"));
                continue;
            };
            if !declaration.params.is_empty() {
                fail(format!(
                    "#[{kind}] type {name} must not have type parameters"
                ));
                continue;
            }
            let arguments = attribute
                .value
                .args
                .iter()
                .map(|argument| match argument.value {
                    alder_source::Expr::Str(value) => Some(value.to_owned()),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>();
            let Some(arguments) = arguments else {
                fail(format!("#[{kind}] arguments must be string literals"));
                continue;
            };
            if arguments.iter().any(|argument| {
                argument.trim().is_empty() || argument.chars().any(char::is_control)
            }) {
                fail(format!(
                    "#[{kind}] arguments must be nonempty strings without control characters"
                ));
                continue;
            }
            if kind == "binding" {
                if arguments.len() < 3 {
                    fail("#[binding] requires an environment name, resource kind, and explicit resource identifiers".to_owned());
                    continue;
                }
                let (expected, count) = match arguments[1].as_str() {
                    "kv" => ("Kv", 1),
                    "r2" => ("R2", 1),
                    "d1" => ("D1", 2),
                    "hyperdrive" => ("Hyperdrive", 1),
                    "service" => ("Service", 1),
                    "durable_object" => ("DurableObjectNamespace", 1),
                    "queue" => ("QueueBinding", 1),
                    "workflow" => ("WorkflowBinding", 2),
                    other => {
                        fail(format!("Unknown Cloudflare resource kind {other}"));
                        continue;
                    }
                };
                if arguments.len() != count + 2 {
                    fail(format!(
                        "{} binding requires {count} resource identifier(s)",
                        arguments[1]
                    ));
                    continue;
                }
                if !identifier(&arguments[0]) {
                    fail("Binding names must be JavaScript identifiers".to_owned());
                    continue;
                }
                if !matches!(&declaration.body, OwnedPublicTypeBody::Alias(target) if platform_type(&target.typ, expected))
                {
                    fail(format!(
                        "Binding type {name} must alias cloudflare.{expected}"
                    ));
                    continue;
                }
                metadata.bindings.push(Binding {
                    context_key: format!("{module}::{}", declaration.reference.name),
                    name: arguments[0].clone(),
                    kind: arguments[1].clone(),
                    arguments: arguments[2..].to_vec(),
                });
            } else {
                if arguments.len() != 1 {
                    fail(format!(
                        "#[{kind}] requires one explicit class or queue name"
                    ));
                    continue;
                }
                let (adapter_kind, trait_name) = match kind {
                    "durable_object" => (AdapterKind::DurableObject, "DurableObject"),
                    "queue" => (AdapterKind::Queue, "Queue"),
                    _ => (AdapterKind::Workflow, "Workflow"),
                };
                if adapter_kind != AdapterKind::Queue
                    && (!identifier(&arguments[0])
                        || matches!(arguments[0].as_str(), "default" | "providers" | "queue"))
                {
                    fail(
                        "Cloudflare class exports must be non-reserved JavaScript identifiers"
                            .to_owned(),
                    );
                    continue;
                }
                let implementations =
                    interface
                        .instances
                        .iter()
                        .filter(|implementation| {
                            builtin(&implementation.trait_ref.trait_.0, trait_name)
                                && implementation.trait_ref.args.first().is_some_and(|typ| {
                                    matches_type(&typ.typ, &declaration.reference)
                                })
                        })
                        .collect::<Vec<_>>();
                let [implementation] = implementations.as_slice() else {
                    fail(format!(
                        "{name} requires exactly one solved cloudflare.{trait_name} implementation"
                    ));
                    continue;
                };
                if implementation.dictionary_kind != OwnedDictionaryKind::Singleton
                    || !implementation.params.is_empty()
                    || !implementation.trait_predicates.is_empty()
                    || !implementation.projection_equalities.is_empty()
                {
                    fail(format!(
                        "{name} requires a concrete singleton platform implementation"
                    ));
                    continue;
                }
                metadata.adapters.push(Adapter {
                    kind: adapter_kind,
                    name: arguments[0].clone(),
                    module: module.clone(),
                    dictionary: implementation.dictionary_symbol.clone(),
                });
            }
        }
    }
    if errors.is_empty() {
        Ok(metadata)
    } else {
        Err(errors)
    }
}

fn identifier(value: &str) -> bool {
    let mut bytes = value.bytes();
    bytes
        .next()
        .is_some_and(|byte| byte.is_ascii_alphabetic() || matches!(byte, b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}

fn hex_id(value: &str) -> bool {
    value.len() == 32 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                byte.is_ascii_hexdigit()
            }
        })
}

fn date_valid(value: &str) -> bool {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| index != 4 && index != 7 && !byte.is_ascii_digit())
    {
        return false;
    }
    let year = value[..4].parse::<u32>().unwrap_or(0);
    let month = value[5..7].parse::<usize>().unwrap_or(0);
    let day = value[8..].parse::<u32>().unwrap_or(0);
    let leap = year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400));
    let days = [
        0,
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    year >= 2021 && month > 0 && month <= 12 && day > 0 && day <= days[month]
}

#[derive(Clone, Debug)]
pub struct ConfigOptions {
    pub name: String,
    pub compatibility_date: String,
    pub main: String,
    pub assets_directory: String,
    pub account_id: Option<String>,
    /// Explicit historical migration records. Never inferred from prior builds.
    /// None uses current declarative SQLite Durable Object exports.
    pub legacy_migrations: Option<Vec<Value>>,
}

/// Generates config without provisioning resources or inferring destructive
/// migrations. Relative paths are relative to the resulting Wrangler config.
pub fn wrangler_config(metadata: &Metadata, options: &ConfigOptions) -> Result<Value, Vec<String>> {
    let mut errors = Vec::new();
    if options.name.is_empty()
        || options.name.len() > 63
        || !options
            .name
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        errors
            .push("Worker name must contain 1–63 lowercase letters, digits, or hyphens".to_owned());
    }
    if !date_valid(&options.compatibility_date) {
        errors.push(
            "compatibility_date must be an explicit valid YYYY-MM-DD date from 2021 onward"
                .to_owned(),
        );
    }
    for (name, path) in [
        ("main", &options.main),
        ("assets_directory", &options.assets_directory),
    ] {
        if path.is_empty() || path.chars().any(char::is_control) {
            errors.push(format!("{name} must be an explicit nonempty path"));
        }
    }
    if options.account_id.as_ref().is_some_and(|id| !hex_id(id)) {
        errors.push("account_id must be an explicit 32-digit hexadecimal account ID".to_owned());
    }
    // Alder already bundles every application dependency into this entry.
    // Wrangler otherwise defaults to recursively collecting adjacent modules
    // in no-bundle mode, including client/prerender files and old dry runs.
    let mut config = json!({"name": options.name, "compatibility_date": options.compatibility_date,"main":options.main,"no_bundle":true,"find_additional_modules":false,"compatibility_flags":["nodejs_compat"],"observability":{"enabled":true},"assets":{"directory":options.assets_directory,"binding":"ASSETS","run_worker_first":true}});
    if let Some(account) = &options.account_id {
        config["account_id"] = json!(account);
    }
    let mut names = BTreeSet::from(["ASSETS".to_owned()]);
    let mut contexts = BTreeSet::new();
    let mut classes = BTreeSet::new();
    let mut queues = BTreeSet::new();
    for adapter in &metadata.adapters {
        if adapter.kind == AdapterKind::Queue {
            if !queues.insert(&adapter.name) {
                errors.push(format!("Duplicate queue consumer {}", adapter.name));
            }
            append(
                &mut config,
                &["queues", "consumers"],
                json!({"queue":adapter.name}),
            );
        } else {
            if !classes.insert(&adapter.name) {
                errors.push(format!("Duplicate class export {}", adapter.name));
            }
            if adapter.kind == AdapterKind::DurableObject && options.legacy_migrations.is_none() {
                config["exports"][&adapter.name] =
                    json!({"type":"durable-object","storage":"sqlite"});
            }
        }
    }
    for binding in &metadata.bindings {
        if !identifier(&binding.name) {
            errors.push(format!("Invalid environment binding name {}", binding.name));
        }
        if !names.insert(binding.name.clone()) {
            errors.push(format!("Duplicate environment binding {}", binding.name));
        }
        if !contexts.insert(&binding.context_key) {
            errors.push(format!("Duplicate provider type {}", binding.context_key));
        }
        let args = &binding.arguments;
        let expected_count = match binding.kind.as_str() {
            "d1" | "workflow" => 2,
            "kv" | "r2" | "hyperdrive" | "service" | "durable_object" | "queue" => 1,
            _ => 0,
        };
        if expected_count == 0 || args.len() != expected_count {
            errors.push(format!("Invalid {} binding arguments", binding.kind));
            continue;
        }
        if args
            .iter()
            .any(|argument| argument.trim().is_empty() || argument.chars().any(char::is_control))
        {
            errors.push(format!(
                "Empty or invalid resource identifier for {}",
                binding.name
            ));
        }
        if matches!(binding.kind.as_str(), "kv" | "hyperdrive") && !hex_id(&args[0]) {
            errors.push(format!(
                "{} requires an explicit 32-digit hexadecimal resource ID",
                binding.name
            ));
        }
        if binding.kind == "d1" && !uuid(&args[1]) {
            errors.push(format!(
                "{} requires an explicit D1 database UUID",
                binding.name
            ));
        }
        match binding.kind.as_str() {
            "kv" => append(
                &mut config,
                &["kv_namespaces"],
                json!({"binding":binding.name,"id":args[0]}),
            ),
            "r2" => append(
                &mut config,
                &["r2_buckets"],
                json!({"binding":binding.name,"bucket_name":args[0]}),
            ),
            "d1" => append(
                &mut config,
                &["d1_databases"],
                json!({"binding":binding.name,"database_name":args[0],"database_id":args[1]}),
            ),
            "hyperdrive" => append(
                &mut config,
                &["hyperdrive"],
                json!({"binding":binding.name,"id":args[0]}),
            ),
            "service" => append(
                &mut config,
                &["services"],
                json!({"binding":binding.name,"service":args[0]}),
            ),
            "queue" => append(
                &mut config,
                &["queues", "producers"],
                json!({"binding":binding.name,"queue":args[0]}),
            ),
            "durable_object" => {
                if !metadata.adapters.iter().any(|adapter| {
                    adapter.kind == AdapterKind::DurableObject && adapter.name == args[0]
                }) {
                    errors.push(format!(
                        "Binding {} refers to missing Durable Object class {}",
                        binding.name, args[0]
                    ));
                }
                append(
                    &mut config,
                    &["durable_objects", "bindings"],
                    json!({"name":binding.name,"class_name":args[0]}),
                );
            }
            "workflow" => {
                if !metadata
                    .adapters
                    .iter()
                    .any(|adapter| adapter.kind == AdapterKind::Workflow && adapter.name == args[1])
                {
                    errors.push(format!(
                        "Binding {} refers to missing Workflow class {}",
                        binding.name, args[1]
                    ));
                }
                append(
                    &mut config,
                    &["workflows"],
                    json!({"binding":binding.name,"name":args[0],"class_name":args[1]}),
                );
            }
            _ => unreachable!(),
        }
    }
    if let Some(migrations) = &options.legacy_migrations {
        let mut tags = BTreeSet::new();
        for migration in migrations {
            match migration.get("tag").and_then(Value::as_str) {
                Some(tag) if !tag.is_empty() && tags.insert(tag) => {}
                _ => errors.push("Legacy migrations require unique nonempty tags".to_owned()),
            }
        }
        config["migrations"] = json!(migrations);
    }
    if errors.is_empty() {
        Ok(config)
    } else {
        Err(errors)
    }
}

fn append(config: &mut Value, path: &[&str], value: Value) {
    let mut target = config;
    for key in path {
        if target.is_null() {
            *target = json!({});
        }
        target = &mut target[*key];
    }
    if target.is_null() {
        *target = json!([]);
    }
    target
        .as_array_mut()
        .expect("generated config array")
        .push(value);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn options() -> ConfigOptions {
        ConfigOptions {
            name: "fixture".to_owned(),
            compatibility_date: "2026-09-14".to_owned(),
            main: "./worker.mjs".to_owned(),
            assets_directory: "./client".to_owned(),
            account_id: Some("0123456789abcdef0123456789abcdef".to_owned()),
            legacy_migrations: None,
        }
    }

    #[test]
    fn cloudflare_prebundled_worker_does_not_scan_adjacent_outputs() {
        for main in ["./worker.mjs", "./nested/worker.mjs"] {
            let mut options = options();
            options.main = main.to_owned();
            let config = wrangler_config(&Metadata::default(), &options).unwrap();
            assert_eq!(config["no_bundle"], true);
            assert_eq!(config["find_additional_modules"], false);
            assert_eq!(config["main"], main);
            assert_eq!(config["assets"]["directory"], "./client");
            assert!(config.get("rules").is_none());
        }
    }

    #[test]
    fn cloudflare_config_all_binding_kinds_are_explicit_and_deterministic() {
        let mut metadata = Metadata::default();
        for (name, kind, args) in [
            ("CACHE", "kv", vec!["0123456789abcdef0123456789abcdef"]),
            ("FILES", "r2", vec!["files"]),
            (
                "DB",
                "d1",
                vec!["database", "01234567-89ab-cdef-0123-456789abcdef"],
            ),
            (
                "SQL",
                "hyperdrive",
                vec!["0123456789abcdef0123456789abcdef"],
            ),
            ("API", "service", vec!["backend"]),
            ("COUNTERS", "durable_object", vec!["Counter"]),
            ("EVENTS", "queue", vec!["events"]),
            ("JOBS", "workflow", vec!["jobs", "Job"]),
        ] {
            metadata.bindings.push(Binding {
                context_key: format!("alder://app/bindings.mjs::{name}"),
                name: name.to_owned(),
                kind: kind.to_owned(),
                arguments: args.into_iter().map(str::to_owned).collect(),
            });
        }
        for (kind, name) in [
            (AdapterKind::DurableObject, "Counter"),
            (AdapterKind::Queue, "events"),
            (AdapterKind::Workflow, "Job"),
        ] {
            metadata.adapters.push(Adapter {
                kind,
                name: name.to_owned(),
                module: "alder://app/hosts.mjs".to_owned(),
                dictionary: format!("$dict{name}"),
            });
        }
        let config = wrangler_config(&metadata, &options()).unwrap();
        insta::assert_snapshot!(serde_json::to_string_pretty(&config).unwrap());
        assert_eq!(config, wrangler_config(&metadata, &options()).unwrap());
        let mut legacy = options();
        legacy.legacy_migrations = Some(vec![json!({"tag":"v1","new_sqlite_classes":["Counter"]})]);
        let config = wrangler_config(&metadata, &legacy).unwrap();
        assert!(config.get("exports").is_none());
        assert_eq!(config["migrations"][0]["tag"], "v1");
    }

    #[test]
    fn cloudflare_config_rejects_invalid_ids_dates_and_conflicting_names() {
        let mut options = options();
        options.compatibility_date = "2026-02-30".to_owned();
        options.account_id = Some("unknown".to_owned());
        let metadata = Metadata {
            adapters: vec![],
            bindings: vec![
                Binding {
                    context_key: "Cache".to_owned(),
                    name: "ASSETS".to_owned(),
                    kind: "kv".to_owned(),
                    arguments: vec!["guess".to_owned()],
                },
                Binding {
                    context_key: "Counters".to_owned(),
                    name: "COUNTERS".to_owned(),
                    kind: "durable_object".to_owned(),
                    arguments: vec!["Missing".to_owned()],
                },
            ],
        };
        let errors = wrangler_config(&metadata, &options).unwrap_err();
        assert_eq!(errors.len(), 5, "{errors:?}");
        assert!(date_valid("2024-02-29"));
        assert!(!date_valid("2025-02-29"));
        assert!(!date_valid("not-a-date"));
    }
}
