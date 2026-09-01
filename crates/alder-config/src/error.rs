//! Configuration error types.

use std::fmt;
use std::path::PathBuf;

/// A position in the source file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

impl fmt::Display for Position {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.line, self.column)
    }
}

/// Errors that can occur when reading or parsing a configuration file.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("could not read '{path}': {source}")]
    ReadError {
        path: PathBuf,
        source: std::io::Error,
    },

    #[error("invalid JSONC in '{path}': {message}")]
    ParseError { path: PathBuf, message: String },

    #[error("'{path}' is empty")]
    EmptyFile { path: PathBuf },

    #[error("'{path}' at {pos}: expected an object")]
    ExpectedObject { path: PathBuf, pos: Position },

    #[error("'{path}' at {pos}: expected a string")]
    ExpectedString { path: PathBuf, pos: Position },

    #[error("'{path}' at {pos}: expected a boolean")]
    ExpectedBool { path: PathBuf, pos: Position },

    #[error("'{path}' at {pos}: expected an array for '{field}'")]
    ExpectedArray {
        path: PathBuf,
        field: String,
        pos: Position,
    },

    #[error("'{path}' at {pos}: expected an array or object")]
    ExpectedArrayOrObject { path: PathBuf, pos: Position },

    #[error("'{path}' at {pos}: expected a version constraint string or dependency object")]
    ExpectedDependency { path: PathBuf, pos: Position },

    #[error("'{path}' at {pos}: missing required field '{field}'")]
    MissingField {
        path: PathBuf,
        field: String,
        pos: Position,
    },

    #[error(
        "'{path}' at {pos}: invalid config type '{value}' (expected 'application', 'package', or 'workspace')"
    )]
    InvalidType {
        path: PathBuf,
        value: String,
        pos: Position,
    },

    #[error("'{path}' at {pos}: invalid package name: {message}")]
    InvalidPackageName {
        path: PathBuf,
        message: String,
        pos: Position,
    },

    #[error(
        "'{path}' at {pos}: invalid dependency format (expected 'workspace', 'path', or 'git' key)"
    )]
    InvalidDependency { path: PathBuf, pos: Position },

    #[error("'{path}' at {pos}: 'workspace' must be true")]
    WorkspaceMustBeTrue { path: PathBuf, pos: Position },

    #[error(
        "'{path}' at {pos}: workspace config cannot use {{ \"workspace\": true }} dependencies"
    )]
    WorkspaceDepInWorkspace { path: PathBuf, pos: Position },

    #[error("summary too long: must be under 80 characters, got {length}")]
    SummaryTooLong { length: usize },

    #[error("no source directories specified")]
    NoSourceDirectories,

    #[error("duplicate source directory: '{path}'")]
    DuplicateSourceDirectory { path: String },
}

impl ConfigError {
    pub(crate) fn read_error(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::ReadError {
            path: path.into(),
            source,
        }
    }

    pub(crate) fn parse_error(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::ParseError {
            path: path.into(),
            message: message.into(),
        }
    }

    pub(crate) fn empty_file(path: impl Into<PathBuf>) -> Self {
        Self::EmptyFile { path: path.into() }
    }

    pub(crate) fn expected_object(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::ExpectedObject {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn expected_string(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::ExpectedString {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn expected_bool(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::ExpectedBool {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn expected_array(
        path: impl Into<PathBuf>,
        field: impl Into<String>,
        pos: Position,
    ) -> Self {
        Self::ExpectedArray {
            path: path.into(),
            field: field.into(),
            pos,
        }
    }

    pub(crate) fn expected_array_or_object(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::ExpectedArrayOrObject {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn expected_dependency(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::ExpectedDependency {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn missing_field(
        path: impl Into<PathBuf>,
        field: impl Into<String>,
        pos: Position,
    ) -> Self {
        Self::MissingField {
            path: path.into(),
            field: field.into(),
            pos,
        }
    }

    pub(crate) fn invalid_type(
        path: impl Into<PathBuf>,
        value: impl Into<String>,
        pos: Position,
    ) -> Self {
        Self::InvalidType {
            path: path.into(),
            value: value.into(),
            pos,
        }
    }

    pub(crate) fn invalid_package_name(
        path: impl Into<PathBuf>,
        message: impl Into<String>,
        pos: Position,
    ) -> Self {
        Self::InvalidPackageName {
            path: path.into(),
            message: message.into(),
            pos,
        }
    }

    pub(crate) fn invalid_dependency(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::InvalidDependency {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn workspace_must_be_true(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::WorkspaceMustBeTrue {
            path: path.into(),
            pos,
        }
    }

    pub(crate) fn workspace_dep_in_workspace(path: impl Into<PathBuf>, pos: Position) -> Self {
        Self::WorkspaceDepInWorkspace {
            path: path.into(),
            pos,
        }
    }
}
