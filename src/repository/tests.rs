use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use super::*;

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

fn unique_temp_path(label: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let sequence = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "agent-teacher-{label}-{}-{nonce}-{sequence}",
        std::process::id()
    ))
}

struct TempRepo {
    path: PathBuf,
}

impl TempRepo {
    fn new() -> Self {
        let path = unique_temp_path("repository-test");
        fs::create_dir_all(&path).unwrap();
        let repository = Self { path };
        repository.git(["init", "--quiet"]);
        repository.git(["config", "user.name", "Agent Teacher Tests"]);
        repository.git(["config", "user.email", "tests@example.invalid"]);
        repository.git(["config", "commit.gpgsign", "false"]);
        repository
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn write(&self, relative: &str, content: &str) {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    fn git<const N: usize>(&self, args: [&str; N]) -> String {
        self.git_dynamic(args)
    }

    fn git_dynamic<I, S>(&self, args: I) -> String
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.path)
            .args(args)
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8(output.stdout).unwrap().trim().to_owned()
    }

    fn commit_all(&self, message: &str) -> String {
        self.git(["add", "--all"]);
        self.git(["commit", "--quiet", "-m", message]);
        self.git(["rev-parse", "HEAD"])
    }
}

impl Drop for TempRepo {
    fn drop(&mut self) {
        // The path is constructed inside std::env::temp_dir with a fixed test
        // prefix and never accepts external input.
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn selected(path: &str) -> DiffFileRequest {
    DiffFileRequest {
        path: RepoPath::parse(path).unwrap(),
        before_lines: None,
        after_lines: None,
    }
}

#[test]
fn discovers_root_and_freezes_file_and_blob_contents() {
    let temp = TempRepo::new();
    temp.write("src/code.rs", "one\ntwo\nthree\n");
    temp.commit_all("initial");
    temp.write("src/code.rs", "changed\ntwo\nthree\n");

    let repository = Repository::discover(temp.path().join("src"), None).unwrap();
    assert_eq!(repository.root(), fs::canonicalize(temp.path()).unwrap());

    let path = RepoPath::parse("src/code.rs").unwrap();
    let current = repository
        .read_file(&path, Some(LineRange::new(2, 3).unwrap()))
        .unwrap();
    assert_eq!(current.content, "two\nthree\n");
    assert_eq!(current.provenance.sha256.len(), 64);
    assert_eq!(current.provenance.repository, ".");

    let committed = repository.read_git_blob("HEAD", &path, None).unwrap();
    assert_eq!(committed.content, "one\ntwo\nthree\n");
    assert_eq!(committed.provenance.revision.as_deref(), Some("HEAD"));
    assert_eq!(committed.provenance.object_id.unwrap().as_str().len(), 40);
    assert_eq!(
        committed
            .provenance
            .content_object_id
            .unwrap()
            .as_str()
            .len(),
        40
    );
}

#[test]
fn resolves_symbolic_revision_diff_and_range_selection() {
    let temp = TempRepo::new();
    temp.write("file.txt", "alpha\nbeta\ngamma\n");
    temp.commit_all("base");
    temp.write("file.txt", "alpha\nchanged\ngamma\nadded\n");
    temp.commit_all("target");

    let repository = Repository::discover(temp.path(), None).unwrap();
    let result = repository
        .resolve_git_diff(&DiffRequest {
            base: "HEAD~1".to_owned(),
            target: DiffTarget::Revision("HEAD".to_owned()),
            files: vec![DiffFileRequest {
                path: RepoPath::parse("file.txt").unwrap(),
                before_lines: None,
                after_lines: Some(LineRange::new(2, 2).unwrap()),
            }],
            context_lines: 2,
        })
        .unwrap();

    assert_eq!(result.diff.files.len(), 1);
    assert!(result.diff.files[0].hunks[0].lines.iter().any(|line| {
        line.kind == DiffLineKind::Addition && line.new_line == Some(2) && line.content == "changed"
    }));
    assert_eq!(result.provenance.base_object_id.as_str().len(), 40);
    assert_eq!(result.provenance.sha256.len(), 64);
    assert!(matches!(
        result.provenance.target,
        ResolvedGitDiffTarget::Revision { .. }
    ));
}

#[test]
fn range_selection_excludes_nearby_change_merged_by_git() {
    let temp = TempRepo::new();
    temp.write("file.txt", "one\ntwo\nthree\nfour\nfive\nsix\n");
    temp.commit_all("base");
    temp.write("file.txt", "one\nTWO\nthree\nFOUR\nfive\nsix\n");

    let repository = Repository::discover(temp.path(), None).unwrap();
    let result = repository
        .resolve_git_diff(&DiffRequest {
            base: "HEAD".to_owned(),
            target: DiffTarget::Worktree,
            files: vec![DiffFileRequest {
                path: RepoPath::parse("file.txt").unwrap(),
                before_lines: None,
                after_lines: Some(LineRange::new(2, 2).unwrap()),
            }],
            context_lines: 1,
        })
        .unwrap();

    let hunks = &result.diff.files[0].hunks;
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].old_start, 1);
    assert_eq!(hunks[0].old_lines, 3);
    assert_eq!(hunks[0].new_start, 1);
    assert_eq!(hunks[0].new_lines, 3);
    assert!(hunks[0].lines.iter().any(|line| line.content == "TWO"));
    assert!(hunks[0].lines.iter().all(|line| line.content != "FOUR"));
    assert!(hunks[0].lines.iter().all(|line| line.content != "four"));
}

#[test]
fn final_revision_guard_detects_moved_git_blob_ref() {
    let temp = TempRepo::new();
    temp.write("file.txt", "first\n");
    temp.commit_all("first");
    temp.git(["branch", "lesson-ref", "HEAD"]);

    let repository = Repository::discover(temp.path(), None).unwrap();
    repository
        .read_git_blob("lesson-ref", &RepoPath::parse("file.txt").unwrap(), None)
        .unwrap();

    temp.write("file.txt", "second\n");
    temp.commit_all("second");
    temp.git(["branch", "--force", "lesson-ref", "HEAD"]);
    let error = repository.verify_observed_revisions().unwrap_err();
    assert_eq!(error.kind(), RepositoryErrorKind::SnapshotChanged);
}

#[test]
fn final_revision_guard_covers_diff_base_and_target_refs() {
    let temp = TempRepo::new();
    temp.write("file.txt", "first\n");
    temp.commit_all("first");
    temp.git(["branch", "lesson-base", "HEAD"]);
    temp.write("file.txt", "second\n");
    temp.commit_all("second");
    temp.git(["branch", "lesson-target", "HEAD"]);

    let repository = Repository::discover(temp.path(), None).unwrap();
    repository
        .resolve_git_diff(&DiffRequest {
            base: "lesson-base".to_owned(),
            target: DiffTarget::Revision("lesson-target".to_owned()),
            files: vec![selected("file.txt")],
            context_lines: 1,
        })
        .unwrap();

    temp.git(["branch", "--force", "lesson-base", "lesson-target"]);
    let error = repository.verify_observed_revisions().unwrap_err();
    assert_eq!(error.kind(), RepositoryErrorKind::SnapshotChanged);
}

#[test]
fn worktree_diff_includes_explicit_untracked_file_as_complete_addition() {
    let temp = TempRepo::new();
    temp.write("tracked.txt", "old\n");
    temp.write(".gitignore", "*.ignored\n");
    temp.commit_all("base");
    temp.write("tracked.txt", "new\n");
    temp.write("notes.txt", "first\nsecond\n");

    let repository = Repository::discover(temp.path(), None).unwrap();
    let result = repository
        .resolve_git_diff(&DiffRequest {
            base: "HEAD".to_owned(),
            target: DiffTarget::Worktree,
            files: vec![selected("tracked.txt"), selected("notes.txt")],
            context_lines: 3,
        })
        .unwrap();

    assert_eq!(result.diff.files.len(), 2);
    let untracked = result
        .diff
        .files
        .iter()
        .find(|file| file.new_path.as_deref() == Some("notes.txt"))
        .unwrap();
    assert!(untracked.is_new);
    assert_eq!(untracked.hunks[0].new_lines, 2);
    assert!(
        untracked.hunks[0]
            .lines
            .iter()
            .all(|line| line.kind == DiffLineKind::Addition)
    );
}

#[test]
fn worktree_diff_rejects_explicit_ignored_file() {
    let temp = TempRepo::new();
    temp.write(".gitignore", "*.ignored\n");
    temp.write("tracked.txt", "base\n");
    temp.commit_all("base");
    temp.write("secret.ignored", "do not include\n");

    let repository = Repository::discover(temp.path(), None).unwrap();
    let error = repository
        .resolve_git_diff(&DiffRequest {
            base: "HEAD".to_owned(),
            target: DiffTarget::Worktree,
            files: vec![selected("secret.ignored")],
            context_lines: 3,
        })
        .unwrap_err();
    assert_eq!(error.kind(), RepositoryErrorKind::IgnoredFile);
}

#[test]
fn reports_owning_repository_groups_for_submodule() {
    let outer = TempRepo::new();
    outer.write("outer.txt", "outer\n");
    outer.commit_all("outer");
    let inner = TempRepo::new();
    inner.write("inner.txt", "inner\n");
    inner.commit_all("inner");
    outer.git_dynamic([
        OsString::from("-c"),
        OsString::from("protocol.file.allow=always"),
        OsString::from("submodule"),
        OsString::from("add"),
        inner.path().as_os_str().to_owned(),
        OsString::from("deps/inner"),
    ]);
    outer.commit_all("add submodule");

    let repository = Repository::discover(outer.path(), None).unwrap();
    let groups = repository
        .group_by_owner(&[
            RepoPath::parse("outer.txt").unwrap(),
            RepoPath::parse("deps/inner/inner.txt").unwrap(),
        ])
        .unwrap();
    assert_eq!(groups.len(), 2);
    assert!(groups.iter().any(|group| group.repository == "."));
    assert!(groups.iter().any(|group| group.repository == "deps/inner"));

    let error = repository
        .resolve_git_diff(&DiffRequest {
            base: "HEAD".to_owned(),
            target: DiffTarget::Worktree,
            files: vec![selected("outer.txt"), selected("deps/inner/inner.txt")],
            context_lines: 3,
        })
        .unwrap_err();
    assert_eq!(error.kind(), RepositoryErrorKind::MixedRepositories);
}

#[test]
fn discovers_linked_worktree_as_the_selected_root() {
    let main = TempRepo::new();
    main.write("file.txt", "content\n");
    main.commit_all("base");
    let linked_path = unique_temp_path("linked-worktree");
    main.git_dynamic([
        OsString::from("worktree"),
        OsString::from("add"),
        OsString::from("--detach"),
        linked_path.as_os_str().to_owned(),
        OsString::from("HEAD"),
    ]);

    let repository = Repository::discover(&linked_path, None).unwrap();
    assert_eq!(repository.root(), fs::canonicalize(&linked_path).unwrap());
    let resource = repository
        .read_file(&RepoPath::parse("file.txt").unwrap(), None)
        .unwrap();
    assert_eq!(resource.content, "content\n");

    main.git_dynamic([
        OsString::from("worktree"),
        OsString::from("remove"),
        OsString::from("--force"),
        linked_path.as_os_str().to_owned(),
    ]);
}

#[test]
fn generated_diff_handles_spaces_and_git_quoted_characters_in_paths() {
    let temp = TempRepo::new();
    let path = "odd name\".txt";
    temp.write(path, "old\n");
    temp.commit_all("base");
    temp.write(path, "new\n");

    let repository = Repository::discover(temp.path(), None).unwrap();
    let result = repository
        .resolve_git_diff(&DiffRequest {
            base: "HEAD".to_owned(),
            target: DiffTarget::Worktree,
            files: vec![selected(path)],
            context_lines: 3,
        })
        .unwrap();
    assert_eq!(result.diff.files[0].display_path(), Some(path));
}

#[test]
fn snapshot_guard_detects_selected_content_changes() {
    let temp = TempRepo::new();
    temp.write("file.txt", "before\n");
    temp.commit_all("base");
    let repository = Repository::discover(temp.path(), None).unwrap();
    let path = RepoPath::parse("file.txt").unwrap();
    let guard = SnapshotGuard::start(&repository, std::slice::from_ref(&path)).unwrap();

    temp.write("file.txt", "after\n");
    let error = guard.verify(&repository).unwrap_err();
    assert_eq!(error.kind(), RepositoryErrorKind::SnapshotChanged);
}
