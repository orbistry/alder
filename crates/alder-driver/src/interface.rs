//! Owned, versioned semantic interfaces for incremental and package builds.

mod owned;

use std::path::{Path, PathBuf};
use std::time::SystemTime;

use bumpalo::Bump;
pub use owned::*;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::DriverError;

pub const INTERFACE_FORMAT_VERSION: u32 = 2;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMeta {
    pub source_time: SystemTime,
    pub last_compile: u64,
    pub interface_hash: [u8; 32],
}

impl ModuleMeta {
    pub fn new(source_time: SystemTime, build_id: u64, interface_hash: [u8; 32]) -> Self {
        Self {
            source_time,
            last_compile: build_id,
            interface_hash,
        }
    }
}

pub struct InterfaceCache {
    cache_dir: PathBuf,
    build_id: u64,
}

impl InterfaceCache {
    pub fn new(project_root: &Path) -> Self {
        Self {
            cache_dir: project_root.join(".alder").join("interfaces"),
            build_id: 0,
        }
    }

    pub fn start_build(&mut self) -> u64 {
        self.build_id += 1;
        self.build_id
    }

    pub fn cache_path(&self, module_name: &str) -> PathBuf {
        self.cache_dir
            .join(format!("{}.aldi", module_name.replace('.', "/")))
    }

    pub fn interface_path(&self, module: &OwnedModuleId) -> PathBuf {
        let package = match &module.package {
            OwnedPackageId::Application => None,
            OwnedPackageId::Named { author, project } => Some(format!("@{author}/{project}")),
            OwnedPackageId::ApplicationMember(member) => Some(format!("members/{member}")),
            OwnedPackageId::Builtin => Some("builtin".to_owned()),
        };
        let mut path = self.cache_dir.clone();
        if let Some(package) = package {
            path = path.join(package);
        }
        path.join(format!("{}.aldi", module.path.join("/")))
    }

    pub fn package_index_path(&self, package: &OwnedPackageId) -> PathBuf {
        let name = match package {
            OwnedPackageId::Named { author, project } => format!("{author}/{project}"),
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

    pub fn load(&self, module_name: &str) -> Option<InterfaceFile> {
        InterfaceFile::load(&self.cache_path(module_name)).ok()
    }

    pub fn load_interface(&self, module: &OwnedModuleId) -> Result<InterfaceFile, DriverError> {
        InterfaceFile::load(&self.interface_path(module))
    }

    pub fn save(&self, interface: &InterfaceFile) -> Result<(), DriverError> {
        interface.save(&self.interface_path(&interface.module))
    }

    pub fn load_package_index(&self, package: &OwnedPackageId) -> Option<PackageInstanceIndexFile> {
        PackageInstanceIndexFile::load(&self.package_index_path(package)).ok()
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

    pub fn needs_rebuild(
        &self,
        meta: &ModuleMeta,
        current_source_time: SystemTime,
        dep_metas: &[&ModuleMeta],
    ) -> bool {
        current_source_time > meta.source_time
            || dep_metas
                .iter()
                .any(|dependency| dependency.last_compile > meta.last_compile)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alder_ast::{ModuleId, PackageId};

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
        alder_can::from_module(bump, canonical.module, &solved.annotations)
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
    fn test_cache_path() {
        let cache = InterfaceCache::new(Path::new("/project"));
        assert_eq!(
            cache.cache_path("Json.Decode"),
            PathBuf::from("/project/.alder/interfaces/Json/Decode.aldi")
        );
        assert_eq!(
            cache.interface_path(&OwnedModuleId {
                package: OwnedPackageId::Named {
                    author: "alice".to_owned(),
                    project: "json".to_owned(),
                },
                path: vec!["Decode".to_owned()],
            }),
            PathBuf::from("/project/.alder/interfaces/@alice/json/Decode.aldi")
        );
        assert_eq!(
            cache.package_index_path(&OwnedPackageId::Named {
                author: "alice".to_owned(),
                project: "json".to_owned(),
            }),
            PathBuf::from("/project/.alder/instances/alice/json.aldi")
        );
    }
}
