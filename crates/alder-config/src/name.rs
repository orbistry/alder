//! Package name type in `author/project` format.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer, de};

/// A package name in `author/project` format.
///
/// Both author and project must be lowercase with optional hyphens.
/// Examples: `alder/core`, `alice/json-parser`, `bob/my-lib`
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageName {
    author: String,
    project: String,
}

impl PackageName {
    /// Create a new package name.
    ///
    /// Returns `None` if either component is invalid.
    pub fn new(author: impl Into<String>, project: impl Into<String>) -> Option<Self> {
        let author = author.into();
        let project = project.into();

        if is_valid_component(&author) && is_valid_component(&project) {
            Some(Self { author, project })
        } else {
            None
        }
    }

    /// Get the author part of the package name.
    pub fn author(&self) -> &str {
        &self.author
    }

    /// Get the project part of the package name.
    pub fn project(&self) -> &str {
        &self.project
    }
}

/// Check if a name component is valid.
///
/// Valid components:
/// - Start with a lowercase letter
/// - Contain only lowercase letters, digits, and hyphens
/// - Don't end with a hyphen
/// - Don't have consecutive hyphens
fn is_valid_component(s: &str) -> bool {
    if s.is_empty() {
        return false;
    }

    let mut chars = s.chars().peekable();

    // Must start with lowercase letter
    match chars.next() {
        Some(c) if c.is_ascii_lowercase() => {}
        _ => return false,
    }

    let mut prev_was_hyphen = false;

    for c in chars {
        if c == '-' {
            if prev_was_hyphen {
                return false; // No consecutive hyphens
            }
            prev_was_hyphen = true;
        } else if c.is_ascii_lowercase() || c.is_ascii_digit() {
            prev_was_hyphen = false;
        } else {
            return false; // Invalid character
        }
    }

    // Must not end with hyphen
    !prev_was_hyphen
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PackageNameError {
    #[error("missing '/' separator in package name")]
    MissingSeparator,
    #[error("invalid author name: must be lowercase letters, digits, and hyphens")]
    InvalidAuthor,
    #[error("invalid project name: must be lowercase letters, digits, and hyphens")]
    InvalidProject,
}

impl FromStr for PackageName {
    type Err = PackageNameError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (author, project) = s
            .split_once('/')
            .ok_or(PackageNameError::MissingSeparator)?;

        if !is_valid_component(author) {
            return Err(PackageNameError::InvalidAuthor);
        }

        if !is_valid_component(project) {
            return Err(PackageNameError::InvalidProject);
        }

        Ok(Self {
            author: author.to_string(),
            project: project.to_string(),
        })
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.author, self.project)
    }
}

impl Serialize for PackageName {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for PackageName {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        s.parse().map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_names() {
        assert_eq!(
            "alder/core".parse::<PackageName>().unwrap(),
            PackageName::new("alder", "core").unwrap()
        );
        assert_eq!(
            "alice/json-parser".parse::<PackageName>().unwrap(),
            PackageName::new("alice", "json-parser").unwrap()
        );
        assert_eq!(
            "bob123/my-lib2".parse::<PackageName>().unwrap(),
            PackageName::new("bob123", "my-lib2").unwrap()
        );
    }

    #[test]
    fn parse_invalid_names() {
        // Missing separator
        assert!("aldercore".parse::<PackageName>().is_err());

        // Uppercase not allowed
        assert!("Alder/core".parse::<PackageName>().is_err());
        assert!("alder/Core".parse::<PackageName>().is_err());

        // Starting with digit not allowed
        assert!("123alder/core".parse::<PackageName>().is_err());

        // Ending with hyphen not allowed
        assert!("alder-/core".parse::<PackageName>().is_err());

        // Consecutive hyphens not allowed
        assert!("alder/my--lib".parse::<PackageName>().is_err());

        // Empty components not allowed
        assert!("/core".parse::<PackageName>().is_err());
        assert!("alder/".parse::<PackageName>().is_err());
    }

    #[test]
    fn display_name() {
        let name = PackageName::new("alice", "json").unwrap();
        assert_eq!(name.to_string(), "alice/json");
    }

    #[test]
    fn accessors() {
        let name = PackageName::new("alice", "json").unwrap();
        assert_eq!(name.author(), "alice");
        assert_eq!(name.project(), "json");
    }
}
