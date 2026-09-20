use super::{RepoPath, Repository, RepositoryError, RepositoryErrorKind};

/// Opaque digest of selected filesystem and repository state.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositorySnapshot(String);

impl RepositorySnapshot {
    pub(crate) fn new(digest: String) -> Self {
        Self(digest)
    }

    pub fn digest(&self) -> &str {
        &self.0
    }
}

/// Captures state before resolution and verifies that it did not change.
#[derive(Clone, Debug)]
pub struct SnapshotGuard {
    paths: Vec<RepoPath>,
    before: RepositorySnapshot,
    include_repository_state: bool,
}

impl SnapshotGuard {
    pub fn start(repository: &Repository, paths: &[RepoPath]) -> Result<Self, RepositoryError> {
        Ok(Self {
            paths: paths.to_vec(),
            before: repository.capture_snapshot(paths, true)?,
            include_repository_state: true,
        })
    }

    pub fn start_filesystem(
        repository: &Repository,
        paths: &[RepoPath],
    ) -> Result<Self, RepositoryError> {
        Ok(Self {
            paths: paths.to_vec(),
            before: repository.capture_snapshot(paths, false)?,
            include_repository_state: false,
        })
    }

    pub fn verify(self, repository: &Repository) -> Result<(), RepositoryError> {
        let after = repository.capture_snapshot(&self.paths, self.include_repository_state)?;
        if self.before != after {
            return Err(RepositoryError::new(
                RepositoryErrorKind::SnapshotChanged,
                "filesystem or repository inputs changed while the lesson was being resolved; retry the build",
            ));
        }
        Ok(())
    }
}
