use std::collections::BTreeMap;

use crate::Error;

/// Production browsers consume the complete graph. Execution-only callers can
/// request inlining and use the checked single-file adapter.
#[derive(Clone, Copy, Debug, Default)]
pub struct BundleOptions {
    pub splitting: bool,
    pub minify: bool,
    pub hashed_names: bool,
    /// Hidden maps against generated JavaScript, never original Alder source.
    pub sourcemap: bool,
}

impl BundleOptions {
    pub const fn production() -> Self {
        Self {
            splitting: true,
            minify: true,
            hashed_names: true,
            sourcemap: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub filename: String,
    pub name: String,
    pub is_entry: bool,
    pub is_dynamic_entry: bool,
    pub facade_module_id: Option<String>,
    pub module_ids: Vec<String>,
    pub exports: Vec<String>,
    pub imports: Vec<String>,
    pub dynamic_imports: Vec<String>,
    pub sourcemap_filename: Option<String>,
    pub sourcemap: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AssetInfo {
    pub names: Vec<String>,
    pub original_file_names: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BundleOutput {
    /// All emitted bytes, including binary assets and source-map files.
    pub files: BTreeMap<String, Vec<u8>>,
    pub chunks: BTreeMap<String, Chunk>,
    pub assets: BTreeMap<String, AssetInfo>,
}

impl BundleOutput {
    pub fn entry(&self, module_id: &str) -> Result<&Chunk, Error> {
        let mut matches = self
            .chunks
            .values()
            .filter(|chunk| chunk.is_entry && chunk.facade_module_id.as_deref() == Some(module_id));
        let entry = matches.next().ok_or(Error::MissingChunk)?;
        if matches.next().is_some() {
            return Err(Error::InvalidOutput(format!(
                "ambiguous entry for {module_id}"
            )));
        }
        Ok(entry)
    }

    pub fn into_single_code(self, module_id: &str) -> Result<String, Error> {
        let entry = self.entry(module_id)?;
        if self.files.len() != 1 || self.chunks.len() != 1 {
            return Err(Error::InvalidOutput("single-file execution received additional chunks/assets; use the complete bundle graph".into()));
        }
        let bytes = self.files.get(&entry.filename).ok_or_else(|| {
            Error::InvalidOutput(format!("missing entry artifact {}", entry.filename))
        })?;
        String::from_utf8(bytes.clone()).map_err(|error| Error::InvalidOutput(error.to_string()))
    }

    pub(crate) fn from_rolldown(outputs: Vec<rolldown_common::Output>) -> Result<Self, Error> {
        let mut result = Self::default();
        for output in outputs {
            let filename = output.filename().to_owned();
            // These names become filesystem paths and public URLs. Reject invalid
            // plugin output before publication rather than trusting the emitter.
            if filename.is_empty()
                || filename.split('/').any(|part| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || part.contains(['\\', '\0', '?', '#', ':'])
                })
            {
                return Err(Error::InvalidOutput(format!(
                    "invalid artifact path {filename:?}"
                )));
            }
            if result
                .files
                .insert(filename.clone(), output.content_as_bytes().to_vec())
                .is_some()
            {
                return Err(Error::InvalidOutput(format!(
                    "duplicate artifact {filename}"
                )));
            }
            match output {
                rolldown_common::Output::Chunk(chunk) => {
                    result.chunks.insert(
                        filename.clone(),
                        Chunk {
                            filename,
                            name: chunk.name.to_string(),
                            is_entry: chunk.is_entry,
                            is_dynamic_entry: chunk.is_dynamic_entry,
                            facade_module_id: chunk
                                .facade_module_id
                                .as_ref()
                                .map(|id| id.as_ref().to_owned()),
                            module_ids: chunk
                                .module_ids
                                .iter()
                                .map(|id| id.as_ref().to_owned())
                                .collect(),
                            exports: chunk.exports.iter().map(ToString::to_string).collect(),
                            imports: chunk.imports.iter().map(ToString::to_string).collect(),
                            dynamic_imports: chunk
                                .dynamic_imports
                                .iter()
                                .map(ToString::to_string)
                                .collect(),
                            sourcemap_filename: chunk.sourcemap_filename.clone(),
                            sourcemap: chunk.map.as_ref().map(|map| map.to_json_string()),
                        },
                    );
                }
                rolldown_common::Output::Asset(asset) => {
                    result.assets.insert(
                        filename,
                        AssetInfo {
                            names: asset.names.clone(),
                            original_file_names: asset.original_file_names.clone(),
                        },
                    );
                }
            }
        }
        Ok(result)
    }
}
