//! Owned, versioned semantic interfaces for incremental and package builds.

mod owned;

use std::path::{Path, PathBuf};

use bumpalo::Bump;
pub use owned::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::DriverError;

pub const INTERFACE_FORMAT_VERSION: u32 = 11;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InterfaceFile {
    pub format_version: u32,
    pub compiler_version: String,
    pub module: OwnedModuleId,
    pub values: Vec<OwnedValue>,
    pub types: Vec<OwnedTypeDecl>,
    pub traits: Vec<OwnedTrait>,
    pub instances: Vec<OwnedImplHeader>,
    pub modules: Vec<OwnedModuleExport>,
    pub private_names: Vec<OwnedPrivateName>,
    pub fingerprint: [u8; 32],
}

impl InterfaceFile {
    pub fn dehydrate(interface: &alder_ast::Interface<'_>) -> Result<Self, DriverError> {
        let mut owned = owned::own_interface(interface);
        owned.fingerprint = owned.compute_fingerprint()?;
        Ok(owned)
    }

    pub fn dehydrate_with_source(
        interface: &alder_ast::Interface<'_>,
        source_uri: &str,
    ) -> Result<Self, DriverError> {
        let mut owned = owned::own_interface(interface);
        for implementation in &mut owned.instances {
            implementation.source_uri = Some(source_uri.to_owned());
        }
        owned.fingerprint = owned.compute_fingerprint()?;
        Ok(owned)
    }

    pub fn hydrate<'a>(&self, bump: &'a Bump) -> alder_ast::Interface<'a> {
        owned::hydrate_interface(bump, self)
    }

    pub fn load(path: &Path) -> Result<Self, DriverError> {
        let bytes = std::fs::read(path).map_err(|source| DriverError::ReadError {
            path: path.to_path_buf(),
            source,
        })?;
        let interface: Self = bincode::deserialize(&bytes)?;
        interface.validate()?;
        Ok(interface)
    }

    pub fn save(&self, path: &Path) -> Result<(), DriverError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DriverError::WriteError {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let bytes = bincode::serialize(self)?;
        std::fs::write(path, bytes).map_err(|source| DriverError::WriteError {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn differs_from(&self, other: &Self) -> bool {
        self.fingerprint != other.fingerprint
    }

    fn validate(&self) -> Result<(), DriverError> {
        if self.format_version != INTERFACE_FORMAT_VERSION {
            return Err(DriverError::IncompatibleInterface {
                reason: format!(
                    "format version {} is not supported (expected {INTERFACE_FORMAT_VERSION})",
                    self.format_version
                ),
            });
        }
        if self.compiler_version != env!("CARGO_PKG_VERSION") {
            return Err(DriverError::IncompatibleInterface {
                reason: format!(
                    "compiler version {} does not match {}",
                    self.compiler_version,
                    env!("CARGO_PKG_VERSION")
                ),
            });
        }
        if self.compute_fingerprint()? != self.fingerprint {
            return Err(DriverError::IncompatibleInterface {
                reason: "semantic fingerprint does not match the interface contents".to_owned(),
            });
        }
        Ok(())
    }

    fn compute_fingerprint(&self) -> Result<[u8; 32], DriverError> {
        let mut canonical = self.clone();
        canonical.fingerprint = [0; 32];
        Ok(Sha256::digest(bincode::serialize(&canonical)?).into())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PackageInstanceIndexFile {
    pub format_version: u32,
    pub compiler_version: String,
    pub package: OwnedPackageId,
    pub modules: Vec<OwnedModuleId>,
    pub instances: Vec<OwnedImplHeader>,
    pub fingerprint: [u8; 32],
}

impl PackageInstanceIndexFile {
    pub fn new(
        package: OwnedPackageId,
        mut modules: Vec<OwnedModuleId>,
        mut instances: Vec<OwnedImplHeader>,
    ) -> Result<Self, DriverError> {
        modules.sort();
        modules.dedup();
        instances.sort_by(|left, right| left.id.cmp(&right.id));
        instances.dedup_by(|left, right| left.id == right.id);
        if modules
            .iter()
            .any(|module| !belongs_to_package(&package, &module.package))
        {
            return Err(DriverError::IncompatibleInterface {
                reason: "package instance index lists a module from another package".to_owned(),
            });
        }
        if instances
            .iter()
            .any(|implementation| !modules.contains(&implementation.id.module))
        {
            return Err(DriverError::IncompatibleInterface {
                reason: "package instance index contains an impl from an unlisted module"
                    .to_owned(),
            });
        }
        let mut index = Self {
            format_version: INTERFACE_FORMAT_VERSION,
            compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
            package,
            modules,
            instances,
            fingerprint: [0; 32],
        };
        index.fingerprint = index.compute_fingerprint()?;
        Ok(index)
    }

    pub fn validate(&self) -> Result<(), DriverError> {
        if self.format_version != INTERFACE_FORMAT_VERSION
            || self.compiler_version != env!("CARGO_PKG_VERSION")
            || self.compute_fingerprint()? != self.fingerprint
        {
            return Err(DriverError::IncompatibleInterface {
                reason: "package instance index is incompatible or corrupt".to_owned(),
            });
        }
        if self
            .modules
            .iter()
            .any(|module| !belongs_to_package(&self.package, &module.package))
            || self
                .instances
                .iter()
                .any(|implementation| !self.modules.contains(&implementation.id.module))
        {
            return Err(DriverError::IncompatibleInterface {
                reason: "package instance index contains an impl from an unlisted module"
                    .to_owned(),
            });
        }
        Ok(())
    }

    pub fn load(path: &Path) -> Result<Self, DriverError> {
        let bytes = std::fs::read(path).map_err(|source| DriverError::ReadError {
            path: path.to_path_buf(),
            source,
        })?;
        let index: Self = bincode::deserialize(&bytes)?;
        index.validate()?;
        Ok(index)
    }

    pub fn save(&self, path: &Path) -> Result<(), DriverError> {
        self.validate()?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|source| DriverError::WriteError {
                path: parent.to_path_buf(),
                source,
            })?;
        }
        let bytes = bincode::serialize(self)?;
        std::fs::write(path, bytes).map_err(|source| DriverError::WriteError {
            path: path.to_path_buf(),
            source,
        })
    }

    pub fn hydrate_instances<'a>(&self, bump: &'a Bump) -> &'a [alder_ast::InterfaceImpl<'a>] {
        bump.alloc_slice_fill_iter(
            self.instances
                .iter()
                .map(|implementation| owned::hydrate_impl(bump, implementation)),
        )
    }

    fn compute_fingerprint(&self) -> Result<[u8; 32], DriverError> {
        let mut canonical = self.clone();
        canonical.fingerprint = [0; 32];
        Ok(Sha256::digest(bincode::serialize(&canonical)?).into())
    }
}

fn belongs_to_package(package: &OwnedPackageId, module: &OwnedPackageId) -> bool {
    package == module
        || matches!(
            (package, module),
            (
                OwnedPackageId::Application,
                OwnedPackageId::ApplicationMember(_)
            )
        )
}

pub struct InterfaceCache {
    cache_dir: PathBuf,
}

impl InterfaceCache {
    pub fn new(project_root: &Path) -> Self {
        Self {
            cache_dir: project_root.join(".alder").join("interfaces"),
        }
    }

    pub fn interface_path(&self, module: &OwnedModuleId) -> PathBuf {
        let package = match &module.package {
            OwnedPackageId::Application => "application".to_owned(),
            OwnedPackageId::Named { author, project } => format!("packages/{author}/{project}"),
            OwnedPackageId::ApplicationMember(member) => format!("members/{member}"),
            OwnedPackageId::Builtin => "builtin".to_owned(),
        };
        self.cache_dir
            .join(package)
            .join(format!("{}.aldi", module.path.join("/")))
    }

    pub fn package_index_path(&self, package: &OwnedPackageId) -> PathBuf {
        let name = match package {
            OwnedPackageId::Named { author, project } => format!("packages/{author}/{project}"),
            OwnedPackageId::Application => "application".to_owned(),
            OwnedPackageId::ApplicationMember(member) => format!("members/{member}"),
            OwnedPackageId::Builtin => "builtin".to_owned(),
        };
        self.cache_dir
            .parent()
            .expect("interface cache always has an .alder parent")
            .join("instances")
            .join(format!("{name}.aldi"))
    }

    pub fn load_interface(&self, module: &OwnedModuleId) -> Result<InterfaceFile, DriverError> {
        InterfaceFile::load(&self.interface_path(module))
    }

    pub fn save(&self, interface: &InterfaceFile) -> Result<(), DriverError> {
        interface.save(&self.interface_path(&interface.module))
    }

    pub fn load_package_index_checked(
        &self,
        package: &OwnedPackageId,
    ) -> Result<PackageInstanceIndexFile, DriverError> {
        PackageInstanceIndexFile::load(&self.package_index_path(package))
    }

    pub fn save_package_index(&self, index: &PackageInstanceIndexFile) -> Result<(), DriverError> {
        index.save(&self.package_index_path(&index.package))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alder_ast::{ModuleId, PackageId};

    #[test]
    fn private_store_dependencies_survive_cache_and_arena_copy_and_affect_fingerprints() {
        let stored = {
            let bump = Bump::new();
            let interface = compile_interface(
                &bump,
                "let count: Number = state(1)\nfn hidden() Number { count }\npub fn read() Number { hidden() }\npub fn increment() { count += 1 }",
            );
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        assert!(
            stored
                .values
                .iter()
                .all(|value| value.store_dependencies.len() == 1
                    && value.store_dependencies[0].name == "count")
        );
        let copied_arena = Bump::new();
        let copied = {
            let hydrate_arena = Bump::new();
            let hydrated = stored.hydrate(&hydrate_arena);
            alder_ast::copy_interface(&copied_arena, &hydrated)
        };
        assert_eq!(stored, InterfaceFile::dehydrate(&copied).unwrap());
        let bump = Bump::new();
        let independent = compile_interface(
            &bump,
            "let count: Number = state(1)\npub fn read() Number { 1 }\npub fn increment() { count += 1 }",
        );
        let independent = InterfaceFile::dehydrate(&independent).unwrap();
        assert_ne!(
            stored.fingerprint, independent.fingerprint,
            "changing a hidden store capture invalidates dependent module caches"
        );
    }

    fn compile_interface<'a>(bump: &'a Bump, source: &str) -> alder_ast::Interface<'a> {
        let source = bump.alloc_str(source);
        let parsed = alder_parse::parse_module(bump, source).expect("source parses");
        let canonical = alder_can::canonicalize(
            bump,
            alder_can::Context {
                home: ModuleId {
                    package: PackageId::Application,
                    path: &["Main"],
                },
                imports: &[],
                interfaces: &[],
            },
            &parsed,
        )
        .expect("source canonicalizes");
        let constraints = alder_constrain::constrain(bump, canonical.module);
        let database = alder_solve::TraitDatabase::build(bump, canonical.module, &[]);
        let solved = alder_solve::solve(bump, &constraints, &database).expect("source solves");
        alder_can::from_module(bump, canonical.module, &solved.annotations, &[])
    }

    fn empty_interface<'a>() -> alder_ast::Interface<'a> {
        alder_ast::Interface {
            home: ModuleId {
                package: PackageId::Application,
                path: &["Main"],
            },
            values: &[],
            types: &[],
            enums: &[],
            traits: &[],
            instances: &[],
            modules: &[],
            private_names: &[],
        }
    }

    #[test]
    fn semantic_interface_fingerprint_is_sha256_and_stable() {
        let first = InterfaceFile::dehydrate(&empty_interface()).unwrap();
        let second = InterfaceFile::dehydrate(&empty_interface()).unwrap();
        assert_eq!(first.fingerprint.len(), 32);
        assert_eq!(first.fingerprint, second.fingerprint);
    }

    #[test]
    fn semantic_interface_round_trips_through_owned_storage() {
        let source = Bump::new();
        let interface = compile_interface(
            &source,
            indoc::indoc! {r#"
                #[derive(Show, Json)]
                pub enum Box[a] {
                    Empty,
                    Full { value: a },
                }

                pub trait Inspect[i] {
                    type Item
                }

                impl Inspect[Box[a]] {
                    type Item = a
                }

                pub fn identity(value: a) a { value }
            "#},
        );
        let file = InterfaceFile::dehydrate(&interface).unwrap();
        assert!(!file.values.is_empty());
        assert!(!file.types.is_empty());
        assert!(!file.traits.is_empty());
        assert!(!file.instances.is_empty());
        let bump = Bump::new();
        let hydrated = file.hydrate(&bump);
        let round_trip = InterfaceFile::dehydrate(&hydrated).unwrap();
        assert_eq!(file, round_trip);
    }

    #[test]
    fn record_overlay_metadata_survives_storage_and_arena_copy() {
        let file = {
            let source = Bump::new();
            let interface = compile_interface(
                &source,
                indoc::indoc! {r#"
                    pub fn merge(left, right) {
                        { ..left, ..right }
                    }
                "#},
            );
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        let scheme = &file.values[0].scheme;
        let OwnedType::Fn { params, ret } = &scheme.typ.typ else {
            panic!("function")
        };
        assert_eq!(scheme.record_overlays.len(), 1);
        let overlay = &scheme.record_overlays[0];
        assert_eq!(overlay.operands.len(), params.len());
        for (operand, parameter) in overlay.operands.iter().zip(params) {
            assert_eq!(operand.typ, parameter.typ);
        }
        assert_eq!(overlay.result.typ, ret.typ);
        let bytes = bincode::serialize(&file).unwrap();
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let destination = Bump::new();
        let copied = {
            let hydrated_arena = Bump::new();
            let hydrated = stored.hydrate(&hydrated_arena);
            alder_ast::copy_interface(&destination, &hydrated)
        };
        let restored = InterfaceFile::dehydrate(&copied).unwrap();
        assert_eq!(file, restored);
        let mut reversed = restored.clone();
        reversed.values[0].scheme.record_overlays[0]
            .operands
            .reverse();
        assert_ne!(file.fingerprint, reversed.compute_fingerprint().unwrap());
    }

    #[test]
    fn sparse_tuple_shape_metadata_survives_storage_and_arena_copy() {
        let file = {
            let source = Bump::new();
            let interface = compile_interface(&source, "pub fn last(value) { value.4294967295 }");
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        let shapes = &file.values[0].scheme.tuple_shapes;
        assert_eq!(shapes.len(), 1);
        assert_eq!(shapes[0].length, u64::from(u32::MAX) + 1);
        assert_eq!(shapes[0].elements.len(), 1);
        assert_eq!(shapes[0].elements[0].0, u32::MAX);
        let bytes = bincode::serialize(&file).unwrap();
        assert!(
            bytes.len() < 4096,
            "sparse metadata must not scale with tuple length"
        );
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let destination = Bump::new();
        let copied = {
            let hydrated_arena = Bump::new();
            let hydrated = stored.hydrate(&hydrated_arena);
            alder_ast::copy_interface(&destination, &hydrated)
        };
        let restored = InterfaceFile::dehydrate(&copied).unwrap();
        assert_eq!(file, restored);
        let mut changed = restored.clone();
        changed.values[0].scheme.tuple_shapes[0].length -= 1;
        assert_ne!(file.fingerprint, changed.compute_fingerprint().unwrap());
    }

    #[test]
    fn single_open_spread_preserves_overlay_without_inventing_optional_fields() {
        let file = {
            let source = Bump::new();
            let interface = compile_interface(
                &source,
                indoc::indoc! {r#"
                    pub fn overwrite(record) { { value: 42, ..record } }
                "#},
            );
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        let scheme = &file.values[0].scheme;
        let OwnedType::Fn { params, .. } = &scheme.typ.typ else {
            panic!("expected function");
        };
        let OwnedType::Record { fields, ext } = &params[0].typ else {
            panic!("expected record parameter");
        };
        assert!(
            fields.is_empty(),
            "an unknown overwrite must not become a field requirement"
        );
        assert!(ext.is_some());
        assert_eq!(scheme.record_overlays.len(), 1);
        assert_eq!(scheme.record_overlays[0].operands.len(), 2);
        let bytes = bincode::serialize(&file).unwrap();
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let destination = Bump::new();
        let copied = {
            let source = Bump::new();
            alder_ast::copy_interface(&destination, &stored.hydrate(&source))
        };
        assert_eq!(file, InterfaceFile::dehydrate(&copied).unwrap());
    }

    #[test]
    fn error_row_inclusion_metadata_survives_storage_and_arena_copy() {
        let file = {
            let source = Bump::new();
            let interface = compile_interface(
                &source,
                indoc::indoc! {r#"
                pub fn forward(value: Result[Number, [:known | e]]) {
                    let number = value?
                    Ok(number)
                }
            "#},
            );
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        let scheme = &file.values[0].scheme;
        let OwnedType::Fn { params, ret } = &scheme.typ.typ else {
            panic!("function")
        };
        let OwnedType::Named {
            args: source_args, ..
        } = &params[0].typ
        else {
            panic!("Result parameter")
        };
        let OwnedType::Named {
            args: target_args, ..
        } = &ret.typ
        else {
            panic!("Result return")
        };
        assert!(!scheme.error_row_inclusions.is_empty());
        assert!(
            scheme.error_row_inclusions.iter().any(|inclusion| {
                inclusion.exact_target
                    && inclusion.source.typ == source_args[1].typ
                    && inclusion.target.typ == target_args[1].typ
            }),
            "inference must publish the input-to-output inclusion"
        );
        let bytes = bincode::serialize(&file).unwrap();
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let destination = Bump::new();
        let copied = {
            let hydrated_arena = Bump::new();
            let hydrated = stored.hydrate(&hydrated_arena);
            alder_ast::copy_interface(&destination, &hydrated)
        };
        let restored = InterfaceFile::dehydrate(&copied).unwrap();
        assert_eq!(stored.values, restored.values);
        assert_eq!(
            restored.values[0].scheme.error_row_inclusions.len(),
            scheme.error_row_inclusions.len()
        );
    }

    #[test]
    fn independent_record_tails_survive_serialization_and_rehydration() {
        let file = {
            let source = Bump::new();
            let interface = compile_interface(
                &source,
                indoc::indoc! {r#"
                    pub fn preserve(left: { r | x: Number }, right: { s | y?: String }) {
                        (left, right)
                    }
                "#},
            );
            InterfaceFile::dehydrate(&interface).unwrap()
        };
        let bytes = bincode::serialize(&file).unwrap();
        let stored: InterfaceFile = bincode::deserialize(&bytes).unwrap();
        let bump = Bump::new();
        let hydrated = stored.hydrate(&bump);
        let restored = InterfaceFile::dehydrate(&hydrated).unwrap();
        assert_eq!(file, restored);
        let value = restored
            .values
            .iter()
            .find(|v| v.exported_as == "preserve")
            .unwrap();
        let OwnedType::Fn { params, ret } = &value.scheme.typ.typ else {
            panic!("expected a function");
        };
        let OwnedType::Record { ext: left, .. } = &params[0].typ else {
            panic!("expected the first record parameter");
        };
        let OwnedType::Record { ext: right, fields } = &params[1].typ else {
            panic!("expected the second record parameter");
        };
        assert!(left.is_some() && right.is_some());
        assert_ne!(left, right, "independent tails must not be conflated");
        let OwnedType::Named { reference, args } = &fields[0].typ.typ else {
            panic!("optional shorthand must survive as an ordinary Option type");
        };
        assert_eq!(reference.name, "Option");
        assert_eq!(reference.module.package, OwnedPackageId::Builtin);
        assert_eq!(args.len(), 1);
        let OwnedType::Named { reference, args } = &args[0].typ else {
            panic!("expected the String payload");
        };
        assert_eq!(reference.name, "String");
        assert_eq!(reference.module.package, OwnedPackageId::Builtin);
        assert!(args.is_empty());
        let OwnedType::Tuple(items) = &ret.typ else {
            panic!("expected a tuple result");
        };
        assert_eq!(items.len(), 2);
        for (item, expected) in items.iter().zip([left, right]) {
            let OwnedType::Record { ext, .. } = &item.typ else {
                panic!("expected a record result");
            };
            assert_eq!(ext, expected, "each output preserves its input tail");
        }
    }

    #[test]
    fn inferred_error_rows_round_trip_with_payloads_and_an_open_tail() {
        let source = Bump::new();
        let interface = compile_interface(
            &source,
            indoc::indoc! {r#"
                pub fn fail(id: Number) Result[String] {
                    Err(:not_found(id))
                }
            "#},
        );
        let file = InterfaceFile::dehydrate(&interface).unwrap();
        let value = file
            .values
            .iter()
            .find(|value| value.exported_as == "fail")
            .expect("public function is exported");
        let owned::OwnedType::Fn { ret, .. } = &value.scheme.typ.typ else {
            panic!("function interface has a function type")
        };
        let owned::OwnedType::Named { args, .. } = &ret.typ else {
            panic!("function returns Result")
        };
        let owned::OwnedType::ErrorRow { tags, ext } = &args[1].typ else {
            panic!("Result error argument is an error row")
        };
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "not_found");
        assert_eq!(tags[0].args.len(), 1);
        assert!(ext.is_some(), "inferred Result shorthand remains open");

        let hydrated_arena = Bump::new();
        let hydrated = file.hydrate(&hydrated_arena);
        assert_eq!(file, InterfaceFile::dehydrate(&hydrated).unwrap());
    }

    #[test]
    fn trait_signature_changes_change_the_fingerprint() {
        let first_bump = Bump::new();
        let first = compile_interface(
            &first_bump,
            "pub trait Convert[a] { fn convert(value: a) String }",
        );
        let second_bump = Bump::new();
        let second = compile_interface(
            &second_bump,
            "pub trait Convert[a] { fn convert(value: a) Number }",
        );
        let first = InterfaceFile::dehydrate(&first).unwrap();
        let second = InterfaceFile::dehydrate(&second).unwrap();
        assert!(first.differs_from(&second));
    }

    #[test]
    fn incompatible_versions_and_tampering_are_rejected() {
        let file = InterfaceFile::dehydrate(&empty_interface()).unwrap();
        let mut wrong_version = file.clone();
        wrong_version.format_version += 1;
        assert!(matches!(
            wrong_version.validate(),
            Err(DriverError::IncompatibleInterface { .. })
        ));

        let mut tampered = file;
        tampered.module.path.push("Changed".to_owned());
        assert!(matches!(
            tampered.validate(),
            Err(DriverError::IncompatibleInterface { .. })
        ));
    }

    #[test]
    fn semantic_interface_saves_and_loads_with_validation() {
        let interface = InterfaceFile::dehydrate(&empty_interface()).unwrap();
        let directory = std::env::temp_dir().join(format!(
            "alder-interface-test-{}-{}",
            std::process::id(),
            interface.fingerprint[0]
        ));
        let path = directory.join("Main.aldi");
        interface.save(&path).unwrap();
        let loaded = InterfaceFile::load(&path).unwrap();
        assert_eq!(interface, loaded);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn package_instance_index_round_trips_source_locations_and_hydrates() {
        let source = Bump::new();
        let interface = compile_interface(
            &source,
            indoc::indoc! {r#"
                pub trait Inspect[a] {}
                impl Inspect[Number] {}
            "#},
        );
        let interface =
            InterfaceFile::dehydrate_with_source(&interface, "file:///project/src/Main.ald")
                .unwrap();
        assert_eq!(
            interface.instances[0].source_uri.as_deref(),
            Some("file:///project/src/Main.ald")
        );
        assert!(interface.instances[0].region.is_some());

        let index = PackageInstanceIndexFile::new(
            OwnedPackageId::Application,
            vec![interface.module.clone()],
            interface.instances.clone(),
        )
        .unwrap();
        let directory = std::env::temp_dir().join(format!(
            "alder-instance-index-test-{}-{}",
            std::process::id(),
            index.fingerprint[0]
        ));
        let path = directory.join("application.aldi");
        index.save(&path).unwrap();
        let loaded = PackageInstanceIndexFile::load(&path).unwrap();
        assert_eq!(index, loaded);

        let hydrated_arena = Bump::new();
        let hydrated = loaded.hydrate_instances(&hydrated_arena);
        assert_eq!(hydrated[0].source_uri, Some("file:///project/src/Main.ald"));
        assert_eq!(hydrated[0].region, interface.instances[0].region);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn package_index_rejects_foreign_modules() {
        let result = PackageInstanceIndexFile::new(
            OwnedPackageId::Application,
            vec![OwnedModuleId {
                package: OwnedPackageId::Named {
                    author: "other".to_owned(),
                    project: "package".to_owned(),
                },
                path: vec!["Foreign".to_owned()],
            }],
            vec![],
        );
        assert!(matches!(
            result,
            Err(DriverError::IncompatibleInterface { .. })
        ));
    }

    #[test]
    fn cache_paths_separate_package_identity_variants() {
        let cache = InterfaceCache::new(Path::new("/project"));
        let modules = [
            OwnedModuleId {
                package: OwnedPackageId::Application,
                path: vec!["builtin".into(), "value".into()],
            },
            OwnedModuleId {
                package: OwnedPackageId::Builtin,
                path: vec!["value".into()],
            },
            OwnedModuleId {
                package: OwnedPackageId::Application,
                path: vec!["members".into(), "member".into(), "value".into()],
            },
            OwnedModuleId {
                package: OwnedPackageId::ApplicationMember("member".into()),
                path: vec!["value".into()],
            },
        ];
        let paths = modules
            .iter()
            .map(|module| cache.interface_path(module))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(
            paths.len(),
            modules.len(),
            "distinct module identities must not overwrite caches"
        );
        assert_ne!(
            cache.package_index_path(&OwnedPackageId::ApplicationMember("member".into())),
            cache.package_index_path(&OwnedPackageId::Named {
                author: "members".into(),
                project: "member".into(),
            }),
            "named packages must not overwrite workspace member indexes"
        );
    }

    #[test]
    fn test_cache_path() {
        let cache = InterfaceCache::new(Path::new("/project"));
        assert_eq!(
            cache.interface_path(&OwnedModuleId {
                package: OwnedPackageId::Application,
                path: vec!["Json".to_owned(), "Decode".to_owned()],
            }),
            PathBuf::from("/project/.alder/interfaces/application/Json/Decode.aldi")
        );
        assert_eq!(
            cache.interface_path(&OwnedModuleId {
                package: OwnedPackageId::Named {
                    author: "alice".to_owned(),
                    project: "json".to_owned(),
                },
                path: vec!["Decode".to_owned()],
            }),
            PathBuf::from("/project/.alder/interfaces/packages/alice/json/Decode.aldi")
        );
        assert_eq!(
            cache.package_index_path(&OwnedPackageId::Named {
                author: "alice".to_owned(),
                project: "json".to_owned(),
            }),
            PathBuf::from("/project/.alder/instances/packages/alice/json.aldi")
        );
    }
}
