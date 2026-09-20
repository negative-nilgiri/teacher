use std::fmt;
use std::path::{Component, Path, PathBuf};
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::{RepositoryError, RepositoryErrorKind};

/// A non-empty, normalized path relative to the selected filesystem root.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(transparent)]
pub struct RepoPath(String);

impl RepoPath {
    pub fn parse(value: impl AsRef<str>) -> Result<Self, RepositoryError> {
        let value = value.as_ref();
        if value.is_empty() || value.chars().any(char::is_control) {
            return Err(RepositoryError::new(
                RepositoryErrorKind::InvalidPath,
                "repository paths must be non-empty and contain no control characters",
            ));
        }

        if value.starts_with('/')
            || value.contains('\\')
            || (value.len() >= 2
                && value.as_bytes()[0].is_ascii_alphabetic()
                && value.as_bytes()[1] == b':')
        {
            return Err(RepositoryError::new(
                RepositoryErrorKind::InvalidPath,
                "repository paths must be portable relative paths using forward slashes",
            ));
        }

        if value
            .split('/')
            .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(RepositoryError::new(
                RepositoryErrorKind::InvalidPath,
                "repository paths must not contain empty, '.' or '..' components",
            ));
        }

        let path = Path::new(value);

        let mut parts = Vec::new();
        for component in path.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
                Component::CurDir => {
                    return Err(RepositoryError::new(
                        RepositoryErrorKind::InvalidPath,
                        "repository paths must not contain '.' components",
                    ));
                }
                Component::ParentDir => {
                    return Err(RepositoryError::new(
                        RepositoryErrorKind::InvalidPath,
                        "repository paths must not contain '..' components",
                    ));
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(RepositoryError::new(
                        RepositoryErrorKind::InvalidPath,
                        "repository paths must be relative",
                    ));
                }
            }
        }

        if parts.is_empty() {
            return Err(RepositoryError::new(
                RepositoryErrorKind::InvalidPath,
                "repository paths must name a file",
            ));
        }

        Ok(Self(parts.join("/")))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn to_path_buf(&self) -> PathBuf {
        self.0.split('/').collect()
    }

    pub(crate) fn join_to(&self, root: &Path) -> PathBuf {
        root.join(self.to_path_buf())
    }
}

impl fmt::Display for RepoPath {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl FromStr for RepoPath {
    type Err = RepositoryError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::parse(value)
    }
}

impl<'de> Deserialize<'de> for RepoPath {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_normal_relative_paths() {
        let path = RepoPath::parse("src/queue.rs").unwrap();
        assert_eq!(path.as_str(), "src/queue.rs");
    }

    #[test]
    fn rejects_paths_that_can_escape_or_are_ambiguous() {
        for value in [
            "",
            "/etc/passwd",
            "../secret",
            "src/../secret",
            "./src/lib.rs",
            "src//lib.rs",
            "src\\lib.rs",
            "C:/repo/file",
            "src/line\nbreak",
        ] {
            assert!(RepoPath::parse(value).is_err(), "accepted {value:?}");
        }
    }
}
