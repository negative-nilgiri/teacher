use std::fmt;
use std::path::PathBuf;

/// Stable classes of repository failures suitable for compiler diagnostics.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum RepositoryErrorKind {
    InvalidPath,
    Io,
    GitUnavailable,
    GitCommand,
    NotRepository,
    Revision,
    Object,
    Utf8,
    MixedRepositories,
    IgnoredFile,
    MissingFile,
    InvalidPatch,
    EmptySelection,
    SnapshotChanged,
}

/// A repository failure with a stable diagnostic code and safe context.
#[derive(Debug)]
pub struct RepositoryError {
    kind: RepositoryErrorKind,
    message: String,
    path: Option<PathBuf>,
}

impl RepositoryError {
    pub(crate) fn new(kind: RepositoryErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            path: None,
        }
    }

    pub(crate) fn at_path(
        kind: RepositoryErrorKind,
        message: impl Into<String>,
        path: impl Into<PathBuf>,
    ) -> Self {
        Self {
            kind,
            message: message.into(),
            path: Some(path.into()),
        }
    }

    pub fn kind(&self) -> RepositoryErrorKind {
        self.kind
    }

    pub fn code(&self) -> &'static str {
        match self.kind {
            RepositoryErrorKind::InvalidPath => "repository.invalid_path",
            RepositoryErrorKind::Io => "repository.io",
            RepositoryErrorKind::GitUnavailable => "repository.git_unavailable",
            RepositoryErrorKind::GitCommand => "repository.git_command",
            RepositoryErrorKind::NotRepository => "repository.not_a_repository",
            RepositoryErrorKind::Revision => "repository.invalid_revision",
            RepositoryErrorKind::Object => "repository.invalid_object",
            RepositoryErrorKind::Utf8 => "repository.non_utf8_content",
            RepositoryErrorKind::MixedRepositories => "repository.mixed_owners",
            RepositoryErrorKind::IgnoredFile => "repository.ignored_file",
            RepositoryErrorKind::MissingFile => "repository.missing_file",
            RepositoryErrorKind::InvalidPatch => "repository.invalid_patch",
            RepositoryErrorKind::EmptySelection => "repository.empty_diff_selection",
            RepositoryErrorKind::SnapshotChanged => "repository.snapshot_changed",
        }
    }

    pub fn path(&self) -> Option<&std::path::Path> {
        self.path.as_deref()
    }
}

impl fmt::Display for RepositoryError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(path) = &self.path {
            write!(
                formatter,
                "{} ({}): {}",
                self.message,
                self.code(),
                path.display()
            )
        } else {
            write!(formatter, "{} ({})", self.message, self.code())
        }
    }
}

impl std::error::Error for RepositoryError {}
