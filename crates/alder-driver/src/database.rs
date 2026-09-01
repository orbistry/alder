//! Compilation database with caching.
//!
//! The `Database` manages file sources and caches compilation results
//! to enable incremental compilation.

use std::collections::HashMap;
use url::Url;

use crate::error::DriverError;
use crate::source::FileSource;

/// Compilation database managing source files and cached results.
///
/// The database provides:
/// - File reading via a `FileSource` abstraction
/// - Caching of source text
/// - Dependency tracking for invalidation
pub struct Database {
    /// File source for reading/writing files.
    source: Box<dyn FileSource>,

    /// Cached source text keyed by URI.
    files: HashMap<Url, String>,

    /// Import relationships: module -> modules it imports.
    imports: HashMap<Url, Vec<Url>>,

    /// Reverse dependencies: module -> modules that import it.
    reverse_deps: HashMap<Url, Vec<Url>>,
}

impl Database {
    /// Create a new database with the given file source.
    pub fn new(source: impl FileSource + 'static) -> Self {
        Database {
            source: Box::new(source),
            files: HashMap::new(),
            imports: HashMap::new(),
            reverse_deps: HashMap::new(),
        }
    }

    /// Get the source text for a file, reading from cache or disk.
    pub async fn source(&mut self, uri: &Url) -> Result<&str, DriverError> {
        // If not cached, read from source
        if !self.files.contains_key(uri) {
            let content = self.source.read(uri).await?;
            self.files.insert(uri.clone(), content);
        }

        Ok(self.files.get(uri).unwrap())
    }

    /// Check if a file exists.
    pub async fn exists(&self, uri: &Url) -> Result<bool, DriverError> {
        self.source.exists(uri).await
    }

    /// List files matching a glob pattern.
    pub async fn glob(&self, base: &Url, pattern: &str) -> Result<Vec<Url>, DriverError> {
        self.source.glob(base, pattern).await
    }

    /// Write content to a file.
    pub async fn write(&self, uri: &Url, content: &str) -> Result<(), DriverError> {
        self.source.write(uri, content).await
    }

    /// Record that `module` imports `imported`.
    pub fn record_import(&mut self, module: &Url, imported: Url) {
        self.imports
            .entry(module.clone())
            .or_default()
            .push(imported.clone());

        self.reverse_deps
            .entry(imported)
            .or_default()
            .push(module.clone());
    }

    /// Get modules that `module` imports.
    pub fn imports_of(&self, module: &Url) -> &[Url] {
        self.imports
            .get(module)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get modules that import `module`.
    pub fn importers_of(&self, module: &Url) -> &[Url] {
        self.reverse_deps
            .get(module)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Invalidate a file's cached content.
    ///
    /// This removes the file from the cache, forcing a re-read on next access.
    /// It does NOT cascade to reverse dependencies - use `invalidate_cascade` for that.
    pub fn invalidate(&mut self, uri: &Url) {
        self.files.remove(uri);
        self.imports.remove(uri);
    }

    /// Invalidate a file and all files that depend on it.
    pub fn invalidate_cascade(&mut self, uri: &Url) {
        // Get all reverse deps before mutating
        let dependents: Vec<Url> = self.importers_of(uri).to_vec();

        // Invalidate this file
        self.invalidate(uri);

        // Recursively invalidate dependents
        for dep in dependents {
            self.invalidate_cascade(&dep);
        }
    }

    /// Clear the import graph (useful when rebuilding from scratch).
    pub fn clear_imports(&mut self) {
        self.imports.clear();
        self.reverse_deps.clear();
    }

    /// Get the underlying file source (for operations that bypass caching).
    pub fn file_source(&self) -> &dyn FileSource {
        self.source.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::InMemorySource;

    #[tokio::test]
    async fn test_database_source_caching() {
        let mem = InMemorySource::new();
        let uri = Url::parse("file:///test/Main.alder").unwrap();
        mem.insert(uri.clone(), "module Main exposing (..)".to_string());

        let mut db = Database::new(mem);

        // First read - fetches from source
        let content = db.source(&uri).await.unwrap();
        assert_eq!(content, "module Main exposing (..)");

        // Second read - should be cached (we can't easily verify this without internal access)
        let content2 = db.source(&uri).await.unwrap();
        assert_eq!(content2, "module Main exposing (..)");
    }

    #[tokio::test]
    async fn test_database_imports() {
        let mem = InMemorySource::new();
        let mut db = Database::new(mem);

        let main = Url::parse("file:///test/Main.alder").unwrap();
        let utils = Url::parse("file:///test/Utils.alder").unwrap();
        let helpers = Url::parse("file:///test/Helpers.alder").unwrap();

        // Main imports Utils and Helpers
        db.record_import(&main, utils.clone());
        db.record_import(&main, helpers.clone());

        // Check imports
        assert_eq!(db.imports_of(&main).len(), 2);
        assert!(db.imports_of(&main).contains(&utils));
        assert!(db.imports_of(&main).contains(&helpers));

        // Check reverse deps
        assert_eq!(db.importers_of(&utils), std::slice::from_ref(&main));
        assert_eq!(db.importers_of(&helpers), std::slice::from_ref(&main));
    }

    #[tokio::test]
    async fn test_database_invalidation() {
        let mem = InMemorySource::new();
        let uri = Url::parse("file:///test/Main.alder").unwrap();
        mem.insert(uri.clone(), "original".to_string());

        let mut db = Database::new(mem);

        // Read to cache
        let _ = db.source(&uri).await.unwrap();
        assert!(db.files.contains_key(&uri));

        // Invalidate
        db.invalidate(&uri);
        assert!(!db.files.contains_key(&uri));
    }
}
