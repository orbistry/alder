//! Interface file serialization for incremental compilation.
//!
//! Interfaces capture the public API of a module, allowing downstream
//! modules to be skipped during recompilation if their dependencies'
//! interfaces haven't changed.

use serde::{Deserialize, Serialize};
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crate::error::DriverError;

/// Module interface for incremental compilation.
///
/// Contains the public exports of a module and a fingerprint
/// for change detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Interface {
    /// Module name (e.g., "Json.Decode").
    pub module_name: String,

    /// Public exports from this module.
    pub exports: Vec<Export>,

    /// Hash of the interface content for change detection.
    pub fingerprint: u64,
}

/// An exported item from a module.
#[derive(Debug, Clone, Serialize, Deserialize, Hash)]
pub enum Export {
    /// A value export (function or constant).
    Value {
        name: String,
        // Type signature would go here in a full implementation
    },

    /// A type export (type alias or custom type).
    Type {
        name: String,
        /// Whether constructors are exposed.
        constructors_exposed: bool,
    },
}

impl Interface {
    /// Create a new interface from exports.
    pub fn new(module_name: String, exports: Vec<Export>) -> Self {
        let fingerprint = compute_fingerprint(&exports);
        Interface {
            module_name,
            exports,
            fingerprint,
        }
    }

    /// Load an interface from a file.
    pub fn load(path: &Path) -> Result<Self, DriverError> {
        let bytes = std::fs::read(path).map_err(|source| DriverError::ReadError {
            path: path.to_path_buf(),
            source,
        })?;

        bincode::deserialize(&bytes).map_err(DriverError::SerializeError)
    }

    /// Save the interface to a file.
    pub fn save(&self, path: &Path) -> Result<(), DriverError> {
        // Create parent directories if needed
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

    /// Check if this interface differs from another.
    pub fn differs_from(&self, other: &Interface) -> bool {
        self.fingerprint != other.fingerprint
    }
}

/// Compute a fingerprint hash for a set of exports.
fn compute_fingerprint(exports: &[Export]) -> u64 {
    let mut hasher = DefaultHasher::new();
    exports.hash(&mut hasher);
    hasher.finish()
}

/// Metadata about a compiled module for caching decisions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleMeta {
    /// Source file modification time.
    pub source_time: SystemTime,

    /// Build ID when this module was last compiled.
    pub last_compile: u64,

    /// Hash of the generated interface.
    pub interface_hash: u64,
}

impl ModuleMeta {
    /// Create metadata for a newly compiled module.
    pub fn new(source_time: SystemTime, build_id: u64, interface_hash: u64) -> Self {
        ModuleMeta {
            source_time,
            last_compile: build_id,
            interface_hash,
        }
    }
}

/// Cache directory manager for interface files.
pub struct InterfaceCache {
    /// Root directory for cached interfaces (e.g., `.alder/interfaces/`).
    cache_dir: PathBuf,

    /// Current build ID (incremented each build).
    build_id: u64,
}

impl InterfaceCache {
    /// Create a new interface cache in the given directory.
    pub fn new(project_root: &Path) -> Self {
        let cache_dir = project_root.join(".alder").join("interfaces");
        InterfaceCache {
            cache_dir,
            build_id: 0,
        }
    }

    /// Start a new build, incrementing the build ID.
    pub fn start_build(&mut self) -> u64 {
        self.build_id += 1;
        self.build_id
    }

    /// Get the cache path for a module.
    pub fn cache_path(&self, module_name: &str) -> PathBuf {
        // Convert module name to path: "Json.Decode" -> "Json/Decode.aldi"
        let relative = module_name.replace('.', "/");
        self.cache_dir.join(format!("{}.aldi", relative))
    }

    /// Load a cached interface for a module.
    pub fn load(&self, module_name: &str) -> Option<Interface> {
        let path = self.cache_path(module_name);
        Interface::load(&path).ok()
    }

    /// Save an interface to the cache.
    pub fn save(&self, interface: &Interface) -> Result<(), DriverError> {
        let path = self.cache_path(&interface.module_name);
        interface.save(&path)
    }

    /// Check if a module needs to be rebuilt.
    ///
    /// A module needs rebuilding if:
    /// - Source file changed (mtime is newer)
    /// - Any dependency's interface changed since last compile
    pub fn needs_rebuild(
        &self,
        meta: &ModuleMeta,
        current_source_time: SystemTime,
        dep_metas: &[&ModuleMeta],
    ) -> bool {
        // Source file changed?
        if current_source_time > meta.source_time {
            return true;
        }

        // Any dependency interface changed after our last compile?
        for dep in dep_metas {
            if dep.last_compile > meta.last_compile {
                return true;
            }
        }

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_interface_fingerprint() {
        let exports1 = vec![Export::Value {
            name: "foo".to_string(),
        }];
        let exports2 = vec![Export::Value {
            name: "bar".to_string(),
        }];

        let iface1 = Interface::new("Test".to_string(), exports1);
        let iface2 = Interface::new("Test".to_string(), exports2);

        assert!(iface1.differs_from(&iface2));
    }

    #[test]
    fn test_interface_same_fingerprint() {
        let exports1 = vec![Export::Value {
            name: "foo".to_string(),
        }];
        let exports2 = vec![Export::Value {
            name: "foo".to_string(),
        }];

        let iface1 = Interface::new("Test".to_string(), exports1);
        let iface2 = Interface::new("Test".to_string(), exports2);

        assert!(!iface1.differs_from(&iface2));
    }

    #[test]
    fn test_cache_path() {
        let cache = InterfaceCache::new(Path::new("/project"));

        assert_eq!(
            cache.cache_path("Main"),
            PathBuf::from("/project/.alder/interfaces/Main.aldi")
        );

        assert_eq!(
            cache.cache_path("Json.Decode"),
            PathBuf::from("/project/.alder/interfaces/Json/Decode.aldi")
        );
    }
}
