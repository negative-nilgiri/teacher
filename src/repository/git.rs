use std::collections::BTreeMap;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::diff::{complete_addition, parse_unified_diff, select_diff_ranges};
use super::{
    DiffRequest, DiffTarget, LineRange, RepoPath, RepositoryError, RepositoryErrorKind,
    RepositorySnapshot, ResolvedDiff, SnapshotGuard,
};

#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct GitObjectId(String);

impl GitObjectId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResourceProvenance {
    /// Owning repository relative to the selected root (`.` for the root).
    /// Plain filesystem resources do not have an owning repository.
    pub repository: Option<String>,
    /// Source path relative to the selected root.
    pub path: String,
    pub revision: Option<String>,
    /// Concrete commit selected by a symbolic `revision`.
    pub object_id: Option<GitObjectId>,
    /// Concrete blob containing the embedded bytes.
    pub content_object_id: Option<GitObjectId>,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedResource {
    pub content: String,
    pub provenance: ResourceProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct GitDiffProvenance {
    pub repository: String,
    pub base_revision: String,
    pub base_object_id: GitObjectId,
    pub target: ResolvedGitDiffTarget,
    pub files: Vec<String>,
    pub sha256: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResolvedGitDiffTarget {
    Revision {
        revision: String,
        object_id: GitObjectId,
    },
    Worktree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResolvedGitDiff {
    pub diff: ResolvedDiff,
    pub provenance: GitDiffProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RepositoryGroup {
    pub owning_root: PathBuf,
    pub repository: String,
    pub files: Vec<RepoPath>,
}

/// A filesystem root used to resolve compiler resources.
///
/// Git ownership is discovered independently for each Git-backed path. The
/// root itself does not need to be a repository.
#[derive(Clone, Debug)]
pub struct Repository {
    root: PathBuf,
    revisions: Arc<Mutex<BTreeMap<(PathBuf, String), GitObjectId>>>,
}

impl Repository {
    /// Select and canonicalize the filesystem root. Relative overrides are
    /// resolved from `current_dir`.
    pub fn at_root(
        current_dir: impl AsRef<Path>,
        root_override: Option<&Path>,
    ) -> Result<Self, RepositoryError> {
        let current_dir = current_dir.as_ref();
        let selected = match root_override {
            Some(path) if path.is_absolute() => path.to_path_buf(),
            Some(path) => current_dir.join(path),
            None => current_dir.to_path_buf(),
        };
        let root = fs::canonicalize(&selected).map_err(|error| {
            RepositoryError::at_path(
                RepositoryErrorKind::Io,
                format!("could not canonicalize selected root: {error}"),
                &selected,
            )
        })?;
        if !root.is_dir() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "selected root must be a directory",
                root,
            ));
        }
        Ok(Self {
            root,
            revisions: Arc::new(Mutex::new(BTreeMap::new())),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn resolve_path(&self, path: &RepoPath) -> PathBuf {
        path.join_to(&self.root)
    }

    fn contained_existing_path(&self, path: &RepoPath) -> Result<PathBuf, RepositoryError> {
        let absolute = self.resolve_path(path);
        let canonical = fs::canonicalize(&absolute).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                RepositoryErrorKind::MissingFile
            } else {
                RepositoryErrorKind::Io
            };
            RepositoryError::at_path(
                kind,
                format!("could not resolve source path: {error}"),
                path.to_path_buf(),
            )
        })?;
        if !canonical.starts_with(&self.root) {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "path resolves outside the selected root",
                path.to_path_buf(),
            ));
        }
        Ok(canonical)
    }

    /// Find nearest owning repositories. More than one returned group means a
    /// single authored Git diff must be split (normally at a submodule).
    pub fn group_by_owner(
        &self,
        paths: &[RepoPath],
    ) -> Result<Vec<RepositoryGroup>, RepositoryError> {
        let mut groups: BTreeMap<PathBuf, Vec<RepoPath>> = BTreeMap::new();
        for path in paths {
            let owner = self.owner_for_path(path)?;
            groups.entry(owner).or_default().push(path.clone());
        }

        groups
            .into_iter()
            .map(|(owning_root, files)| {
                let repository = relative_repository(&self.root, &owning_root)?;
                Ok(RepositoryGroup {
                    owning_root,
                    repository,
                    files,
                })
            })
            .collect()
    }

    pub fn resolve_revision(
        &self,
        repository_root: &Path,
        revision: &str,
    ) -> Result<GitObjectId, RepositoryError> {
        let object_id = self.resolve_revision_untracked(repository_root, revision)?;
        let mut revisions = self.revisions.lock().map_err(|_| {
            RepositoryError::new(
                RepositoryErrorKind::Io,
                "repository revision observation state is unavailable",
            )
        })?;
        let key = (repository_root.to_path_buf(), revision.to_owned());
        if let Some(previous) = revisions.get(&key)
            && previous != &object_id
        {
            return Err(revision_changed(revision));
        }
        revisions.insert(key, object_id.clone());
        Ok(object_id)
    }

    /// Re-resolve every Git revision expression used while compiling and fail
    /// if any expression now denotes a different commit.
    pub fn verify_observed_revisions(&self) -> Result<(), RepositoryError> {
        let observations: Vec<_> = self
            .revisions
            .lock()
            .map_err(|_| {
                RepositoryError::new(
                    RepositoryErrorKind::Io,
                    "repository revision observation state is unavailable",
                )
            })?
            .iter()
            .map(|((root, revision), object_id)| {
                (root.clone(), revision.clone(), object_id.clone())
            })
            .collect();
        for (root, revision, expected) in observations {
            if self.resolve_revision_untracked(&root, &revision)? != expected {
                return Err(revision_changed(&revision));
            }
        }
        Ok(())
    }

    fn resolve_revision_untracked(
        &self,
        repository_root: &Path,
        revision: &str,
    ) -> Result<GitObjectId, RepositoryError> {
        validate_revision(revision)?;
        let expression = format!("{revision}^{{commit}}");
        let output = self.git(
            repository_root,
            [
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("--end-of-options"),
                OsStr::new(&expression),
            ],
        )?;
        if !output.status.success() {
            return Err(RepositoryError::new(
                RepositoryErrorKind::Revision,
                format!("Git could not resolve revision {revision:?}"),
            ));
        }
        parse_object_id(&output.stdout, "resolved revision")
    }

    pub fn read_file(
        &self,
        path: &RepoPath,
        lines: Option<LineRange>,
    ) -> Result<ResolvedResource, RepositoryError> {
        let metadata = fs::symlink_metadata(self.resolve_path(path)).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                RepositoryErrorKind::MissingFile
            } else {
                RepositoryErrorKind::Io
            };
            RepositoryError::at_path(
                kind,
                format!("could not inspect source file: {error}"),
                path.to_path_buf(),
            )
        })?;
        if !metadata.file_type().is_file() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "plain file sources must be regular files",
                path.to_path_buf(),
            ));
        }
        let absolute = self.contained_existing_path(path)?;
        let bytes = fs::read(&absolute).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                RepositoryErrorKind::MissingFile
            } else {
                RepositoryErrorKind::Io
            };
            RepositoryError::at_path(
                kind,
                format!("could not read source file: {error}"),
                path.to_path_buf(),
            )
        })?;
        let content = String::from_utf8(bytes).map_err(|_| {
            RepositoryError::at_path(
                RepositoryErrorKind::Utf8,
                "source files must contain UTF-8 text",
                path.to_path_buf(),
            )
        })?;
        let content = select_lines(&content, lines)?;
        Ok(ResolvedResource {
            provenance: ResourceProvenance {
                repository: None,
                path: path.as_str().to_owned(),
                revision: None,
                object_id: None,
                content_object_id: None,
                sha256: sha256(content.as_bytes()),
            },
            content,
        })
    }

    pub fn read_git_blob(
        &self,
        revision: &str,
        path: &RepoPath,
        lines: Option<LineRange>,
    ) -> Result<ResolvedResource, RepositoryError> {
        let owner = self.owner_for_path(path)?;
        let owner_relative = self.path_relative_to_owner(path, &owner)?;
        let commit_id = self.resolve_revision(&owner, revision)?;
        let expression = format!("{}:{}", commit_id.as_str(), owner_relative.as_str());

        let object_output = self.git(
            &owner,
            [
                OsStr::new("rev-parse"),
                OsStr::new("--verify"),
                OsStr::new("--end-of-options"),
                OsStr::new(&expression),
            ],
        )?;
        if !object_output.status.success() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::Object,
                format!("file does not exist at resolved revision {revision:?}"),
                path.to_path_buf(),
            ));
        }
        let blob_id = parse_object_id(&object_output.stdout, "blob object")?;

        let output = self.git(
            &owner,
            [
                OsStr::new("cat-file"),
                OsStr::new("blob"),
                OsStr::new(blob_id.as_str()),
            ],
        )?;
        if !output.status.success() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::Object,
                "Git could not read the resolved blob",
                path.to_path_buf(),
            ));
        }
        let content = String::from_utf8(output.stdout).map_err(|_| {
            RepositoryError::at_path(
                RepositoryErrorKind::Utf8,
                "Git blob must contain UTF-8 text",
                path.to_path_buf(),
            )
        })?;
        let content = select_lines(&content, lines)?;
        Ok(ResolvedResource {
            provenance: ResourceProvenance {
                repository: Some(relative_repository(&self.root, &owner)?),
                path: path.as_str().to_owned(),
                revision: Some(revision.to_owned()),
                object_id: Some(commit_id),
                content_object_id: Some(blob_id),
                sha256: sha256(content.as_bytes()),
            },
            content,
        })
    }

    /// Resolve a Git diff and discard its build metadata. Compiler code that
    /// emits provenance should prefer [`Self::resolve_git_diff`].
    pub fn resolve_diff(&self, request: &DiffRequest) -> Result<ResolvedDiff, RepositoryError> {
        Ok(self.resolve_git_diff(request)?.diff)
    }

    pub fn resolve_git_diff(
        &self,
        request: &DiffRequest,
    ) -> Result<ResolvedGitDiff, RepositoryError> {
        if request.files.is_empty() {
            return Err(RepositoryError::new(
                RepositoryErrorKind::EmptySelection,
                "a Git diff must select at least one file",
            ));
        }
        let paths: Vec<_> = request.files.iter().map(|file| file.path.clone()).collect();
        for path in &paths {
            if self.resolve_path(path).is_dir() {
                return Err(RepositoryError::at_path(
                    RepositoryErrorKind::InvalidPath,
                    "Git diff selections must name files, not directories",
                    path.to_path_buf(),
                ));
            }
        }
        // Observe ownership before selecting the group used by the diff. If a
        // nested repository appears or disappears after this point, final
        // verification detects the ownership-boundary change.
        let guard = SnapshotGuard::start(self, &paths)?;
        let groups = self.group_by_owner(&paths)?;
        if groups.len() != 1 {
            let summary = groups
                .iter()
                .map(|group| {
                    format!(
                        "{}: {}",
                        group.repository,
                        group
                            .files
                            .iter()
                            .map(RepoPath::as_str)
                            .collect::<Vec<_>>()
                            .join(", ")
                    )
                })
                .collect::<Vec<_>>()
                .join("; ");
            return Err(RepositoryError::new(
                RepositoryErrorKind::MixedRepositories,
                format!(
                    "one Git diff may cover only one owning repository; split these groups: {summary}"
                ),
            ));
        }
        let group = &groups[0];
        let base = self.resolve_revision(&group.owning_root, &request.base)?;
        let target = match &request.target {
            DiffTarget::Revision(revision) => Some((
                revision.clone(),
                self.resolve_revision(&group.owning_root, revision)?,
            )),
            DiffTarget::Worktree => None,
        };

        let owner_paths: Vec<_> = paths
            .iter()
            .map(|path| self.path_relative_to_owner(path, &group.owning_root))
            .collect::<Result<_, _>>()?;
        let mut args = vec![
            OsString::from("-c"),
            OsString::from("core.quotePath=false"),
            OsString::from("diff"),
            OsString::from("--no-ext-diff"),
            OsString::from("--no-color"),
            OsString::from("--no-renames"),
            // Pin output details that user configuration can otherwise change:
            // textconv drivers, `diff.noprefix`/`diff.mnemonicPrefix`, and
            // `diff.relative`.
            OsString::from("--no-textconv"),
            OsString::from("--src-prefix=a/"),
            OsString::from("--dst-prefix=b/"),
            OsString::from("--no-relative"),
            OsString::from("--text"),
            OsString::from(format!("--unified={}", request.context_lines)),
            OsString::from(base.as_str()),
        ];
        if let Some((_, target)) = &target {
            args.push(OsString::from(target.as_str()));
        }
        args.push(OsString::from("--"));
        args.extend(owner_paths.iter().map(|path| OsString::from(path.as_str())));
        let output = self.git(&group.owning_root, args.iter().map(OsString::as_os_str))?;
        if !output.status.success() {
            return Err(git_failure("could not generate selected Git diff", &output));
        }
        let patch = String::from_utf8(output.stdout).map_err(|_| {
            RepositoryError::new(RepositoryErrorKind::Utf8, "generated Git diff is not UTF-8")
        })?;
        let mut diff = parse_unified_diff(&patch)?;
        rebase_diff_paths(&mut diff, &group.repository);

        if matches!(request.target, DiffTarget::Worktree) {
            for (source_path, owner_path) in paths.iter().zip(&owner_paths) {
                if self.is_ignored(&group.owning_root, owner_path)? {
                    return Err(RepositoryError::at_path(
                        RepositoryErrorKind::IgnoredFile,
                        "ignored files cannot be included in worktree diffs",
                        source_path.to_path_buf(),
                    ));
                }
                if self.is_untracked(&group.owning_root, owner_path)? {
                    self.require_regular_untracked_file(source_path)?;
                    let resource = self.read_file(source_path, None)?;
                    diff.files
                        .push(complete_addition(source_path, &resource.content));
                }
            }
        }

        let diff = select_diff_ranges(diff, &request.files, request.context_lines)?;

        // A symbolic ref moving during resolution must not silently produce a
        // comparison assembled from different repository moments.
        if self.resolve_revision(&group.owning_root, &request.base)? != base {
            return Err(RepositoryError::new(
                RepositoryErrorKind::SnapshotChanged,
                format!(
                    "base revision {:?} changed while resolving the diff",
                    request.base
                ),
            ));
        }
        if let Some((revision, object_id)) = &target
            && self.resolve_revision(&group.owning_root, revision)? != *object_id
        {
            return Err(RepositoryError::new(
                RepositoryErrorKind::SnapshotChanged,
                format!("target revision {revision:?} changed while resolving the diff"),
            ));
        }
        guard.verify(self)?;
        let digest_bytes = serde_json::to_vec(&diff.files).map_err(|error| {
            RepositoryError::new(
                RepositoryErrorKind::Io,
                format!("could not hash resolved diff representation: {error}"),
            )
        })?;
        let provenance = GitDiffProvenance {
            repository: group.repository.clone(),
            base_revision: request.base.clone(),
            base_object_id: base,
            target: match target {
                Some((revision, object_id)) => ResolvedGitDiffTarget::Revision {
                    revision,
                    object_id,
                },
                None => ResolvedGitDiffTarget::Worktree,
            },
            files: paths.iter().map(|path| path.as_str().to_owned()).collect(),
            sha256: sha256(&digest_bytes),
        };
        Ok(ResolvedGitDiff { diff, provenance })
    }

    pub(crate) fn capture_snapshot(
        &self,
        paths: &[RepoPath],
        include_repository_state: bool,
    ) -> Result<RepositorySnapshot, RepositoryError> {
        let mut material = Vec::new();
        let mut owners: BTreeMap<PathBuf, Vec<RepoPath>> = BTreeMap::new();

        for path in paths {
            material.push(0);
            material.extend_from_slice(path.as_str().as_bytes());
            let absolute = self.resolve_path(path);
            match fs::symlink_metadata(&absolute) {
                Ok(metadata) if metadata.file_type().is_symlink() => {
                    let target = fs::read_link(&absolute).map_err(|error| {
                        RepositoryError::at_path(
                            RepositoryErrorKind::Io,
                            format!("could not snapshot selected symlink: {error}"),
                            path.to_path_buf(),
                        )
                    })?;
                    material.extend_from_slice(b"<symlink>");
                    material.extend_from_slice(target.to_string_lossy().as_bytes());
                }
                Ok(metadata) if metadata.file_type().is_file() => {
                    let value = self.contained_existing_path(path)?;
                    let bytes = fs::read(value).map_err(|error| {
                        RepositoryError::at_path(
                            RepositoryErrorKind::Io,
                            format!("could not snapshot selected path: {error}"),
                            path.to_path_buf(),
                        )
                    })?;
                    material.extend_from_slice(sha256(&bytes).as_bytes());
                }
                Ok(_) => material.extend_from_slice(b"<non-regular>"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    material.extend_from_slice(b"<missing>");
                }
                Err(error) => {
                    return Err(RepositoryError::at_path(
                        RepositoryErrorKind::Io,
                        format!("could not snapshot selected path: {error}"),
                        path.to_path_buf(),
                    ));
                }
            }
            if include_repository_state {
                let owner = self.owner_for_path(path)?;
                owners
                    .entry(owner.clone())
                    .or_default()
                    .push(self.path_relative_to_owner(path, &owner)?);
            }
        }

        for (owner, owner_paths) in owners {
            material.push(0);
            material.extend_from_slice(relative_repository(&self.root, &owner)?.as_bytes());
            let head = self.git(
                &owner,
                [
                    OsStr::new("rev-parse"),
                    OsStr::new("--verify"),
                    OsStr::new("HEAD"),
                ],
            )?;
            if head.status.success() {
                material.extend_from_slice(&head.stdout);
            }

            let mut args = vec![
                OsString::from("status"),
                OsString::from("--porcelain=v2"),
                OsString::from("-z"),
                OsString::from("--untracked-files=all"),
                OsString::from("--ignored=no"),
                OsString::from("--"),
            ];
            args.extend(owner_paths.iter().map(|path| OsString::from(path.as_str())));
            let status = self.git(&owner, args.iter().map(OsString::as_os_str))?;
            if !status.status.success() {
                return Err(git_failure(
                    "could not inspect repository snapshot",
                    &status,
                ));
            }
            material.extend_from_slice(&status.stdout);
        }
        Ok(RepositorySnapshot::new(sha256(&material)))
    }

    fn owner_for_path(&self, path: &RepoPath) -> Result<PathBuf, RepositoryError> {
        let absolute = self.resolve_path(path);
        let mut probe = if absolute.is_dir() {
            absolute.as_path()
        } else {
            absolute.parent().unwrap_or(&self.root)
        };
        while !probe.exists() {
            probe = probe.parent().ok_or_else(|| {
                RepositoryError::at_path(
                    RepositoryErrorKind::MissingFile,
                    "no existing parent for repository path",
                    path.to_path_buf(),
                )
            })?;
        }
        let output = run_git_raw(probe, ["rev-parse", "--show-toplevel"])?;
        if !output.status.success() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::NotRepository,
                "path is not owned by a Git repository",
                path.to_path_buf(),
            ));
        }
        let owner =
            fs::canonicalize(output_path(&output, "owning repository")?).map_err(|error| {
                RepositoryError::at_path(
                    RepositoryErrorKind::Io,
                    format!("could not canonicalize owning repository: {error}"),
                    path.to_path_buf(),
                )
            })?;
        if !owner.starts_with(&self.root) {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::OwnerAboveRoot,
                format!(
                    "the owning Git repository {} is above the selected root; pass that repository (or a directory containing it) as the root",
                    owner.display()
                ),
                path.to_path_buf(),
            ));
        }
        Ok(owner)
    }

    fn path_relative_to_owner(
        &self,
        path: &RepoPath,
        owner: &Path,
    ) -> Result<RepoPath, RepositoryError> {
        let absolute = self.resolve_path(path);
        let relative = absolute.strip_prefix(owner).map_err(|_| {
            RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "path is outside its owning repository",
                path.to_path_buf(),
            )
        })?;
        RepoPath::parse(relative.to_string_lossy().replace('\\', "/"))
    }

    fn is_ignored(&self, owner: &Path, path: &RepoPath) -> Result<bool, RepositoryError> {
        let output = self.git(
            owner,
            [
                OsStr::new("check-ignore"),
                OsStr::new("-q"),
                OsStr::new("--"),
                OsStr::new(path.as_str()),
            ],
        )?;
        match output.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(git_failure(
                "could not determine whether file is ignored",
                &output,
            )),
        }
    }

    fn is_untracked(&self, owner: &Path, path: &RepoPath) -> Result<bool, RepositoryError> {
        let output = self.git(
            owner,
            [
                OsStr::new("ls-files"),
                OsStr::new("--error-unmatch"),
                OsStr::new("--"),
                OsStr::new(path.as_str()),
            ],
        )?;
        match output.status.code() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(git_failure(
                "could not determine whether file is tracked",
                &output,
            )),
        }
    }

    fn require_regular_untracked_file(&self, path: &RepoPath) -> Result<(), RepositoryError> {
        let metadata = fs::symlink_metadata(self.resolve_path(path)).map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                RepositoryErrorKind::MissingFile
            } else {
                RepositoryErrorKind::Io
            };
            RepositoryError::at_path(
                kind,
                format!("could not inspect untracked diff selection: {error}"),
                path.to_path_buf(),
            )
        })?;
        if !metadata.file_type().is_file() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "untracked worktree diff selections must be regular files",
                path.to_path_buf(),
            ));
        }
        Ok(())
    }

    fn git<I, S>(&self, directory: &Path, args: I) -> Result<Output, RepositoryError>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        if !directory.starts_with(&self.root) {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "refusing to invoke Git outside the selected root",
                directory,
            ));
        }
        run_git_raw(directory, args)
    }
}

fn run_git_raw<I, S>(directory: &Path, args: I) -> Result<Output, RepositoryError>
where
    I: IntoIterator<Item = S>,
    S: AsRef<OsStr>,
{
    Command::new("git")
        .arg("-C")
        .arg(directory)
        .args(args)
        // A parent Git process (for example a hook running `learnc`) exports
        // repository-selecting variables that would override per-path owner
        // discovery.
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env_remove("GIT_INDEX_FILE")
        .env_remove("GIT_COMMON_DIR")
        .env_remove("GIT_OBJECT_DIRECTORY")
        .env_remove("GIT_ALTERNATE_OBJECT_DIRECTORIES")
        .env_remove("GIT_NAMESPACE")
        .env_remove("GIT_PREFIX")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .output()
        .map_err(|error| {
            let kind = if error.kind() == std::io::ErrorKind::NotFound {
                RepositoryErrorKind::GitUnavailable
            } else {
                RepositoryErrorKind::Io
            };
            RepositoryError::at_path(kind, format!("could not execute Git: {error}"), directory)
        })
}

fn git_failure(message: &str, output: &Output) -> RepositoryError {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail = stderr.lines().next().unwrap_or("Git exited unsuccessfully");
    RepositoryError::new(
        RepositoryErrorKind::GitCommand,
        format!("{message}: {detail}"),
    )
}

fn revision_changed(revision: &str) -> RepositoryError {
    RepositoryError::new(
        RepositoryErrorKind::SnapshotChanged,
        format!("revision {revision:?} changed while resolving the lesson; retry the build"),
    )
}

fn output_path(output: &Output, description: &str) -> Result<PathBuf, RepositoryError> {
    let value = std::str::from_utf8(&output.stdout).map_err(|_| {
        RepositoryError::new(
            RepositoryErrorKind::Utf8,
            format!("Git returned a non-UTF-8 {description}"),
        )
    })?;
    let value = value.trim_end_matches(['\r', '\n']);
    if value.is_empty() {
        return Err(RepositoryError::new(
            RepositoryErrorKind::GitCommand,
            format!("Git returned an empty {description}"),
        ));
    }
    Ok(PathBuf::from(value))
}

fn parse_object_id(bytes: &[u8], description: &str) -> Result<GitObjectId, RepositoryError> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| {
            RepositoryError::new(
                RepositoryErrorKind::Utf8,
                format!("Git returned a non-UTF-8 {description}"),
            )
        })?
        .trim();
    if (value.len() != 40 && value.len() != 64)
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(RepositoryError::new(
            RepositoryErrorKind::Object,
            format!("Git returned an invalid {description}"),
        ));
    }
    Ok(GitObjectId(value.to_ascii_lowercase()))
}

fn validate_revision(revision: &str) -> Result<(), RepositoryError> {
    if revision.is_empty()
        || revision.trim() != revision
        || revision.starts_with('-')
        || revision.chars().any(char::is_control)
    {
        return Err(RepositoryError::new(
            RepositoryErrorKind::Revision,
            "Git revisions must be non-empty, trimmed, contain no control characters, and not begin with '-'",
        ));
    }
    Ok(())
}

fn relative_repository(root: &Path, owner: &Path) -> Result<String, RepositoryError> {
    let relative = owner.strip_prefix(root).map_err(|_| {
        RepositoryError::at_path(
            RepositoryErrorKind::InvalidPath,
            "owning repository is outside the selected root",
            owner,
        )
    })?;
    if relative.as_os_str().is_empty() {
        Ok(".".to_owned())
    } else {
        Ok(relative.to_string_lossy().replace('\\', "/"))
    }
}

fn select_lines(content: &str, lines: Option<LineRange>) -> Result<String, RepositoryError> {
    let Some(lines) = lines else {
        return Ok(content.to_owned());
    };
    if lines.start == 0 || lines.end < lines.start {
        return Err(RepositoryError::new(
            RepositoryErrorKind::LineRange,
            "line ranges must be one-based, inclusive, and non-empty",
        ));
    }
    let values: Vec<_> = content.split_inclusive('\n').collect();
    if lines.end as usize > values.len() {
        return Err(RepositoryError::new(
            RepositoryErrorKind::LineRange,
            format!(
                "line range {}-{} exceeds the source's {} lines",
                lines.start,
                lines.end,
                values.len()
            ),
        ));
    }
    Ok(values[(lines.start - 1) as usize..lines.end as usize].concat())
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn rebase_diff_paths(diff: &mut ResolvedDiff, repository: &str) {
    if repository == "." {
        return;
    }
    for file in &mut diff.files {
        if let Some(path) = &mut file.old_path {
            *path = format!("{repository}/{path}");
        }
        if let Some(path) = &mut file.new_path {
            *path = format!("{repository}/{path}");
        }
    }
}
