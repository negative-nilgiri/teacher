//! Repository-backed source resolution for the lesson compiler.
//!
//! This module is intentionally independent of the authored source model: the
//! compiler translates source values into the small request types exported
//! here. All Git commands are invoked directly with argument arrays.

mod diff;
mod error;
mod git;
mod path;
mod snapshot;

pub use diff::{
    DiffFileRequest, DiffLine, DiffLineKind, DiffRequest, DiffTarget, LineRange, ResolvedDiff,
    ResolvedDiffFile, ResolvedDiffHunk, parse_unified_diff, select_diff_ranges,
};
pub use error::{RepositoryError, RepositoryErrorKind};
pub use git::{
    GitDiffProvenance, GitObjectId, Repository, RepositoryGroup, ResolvedGitDiff,
    ResolvedGitDiffTarget, ResolvedResource, ResourceProvenance,
};
pub use path::RepoPath;
pub use snapshot::{RepositorySnapshot, SnapshotGuard};

#[cfg(test)]
mod tests;
