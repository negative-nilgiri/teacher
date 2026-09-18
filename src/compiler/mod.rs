//! Source-to-artifact compilation orchestration used exclusively by `learnc`.
//!
//! Parsing and source-only validation live in [`crate::source`]. This module
//! adapts the authored source unions to the controlled repository APIs, freezes
//! every resource, and lowers the result into [`crate::artifact`].

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use sha2::{Digest, Sha256};

use crate::artifact::{
    BuildProvenance, CURRENT_ARTIFACT_VERSION, ChoiceId, CompiledLesson, CompiledNode,
    CompiledNodeContent, FrozenDiffTarget, LessonPresentation, PresentedChoice, PrivateLesson,
    QuizAnswer, ResourceProvenance,
};
use crate::diagnostics::Diagnostic;
use crate::repository::{
    self, DiffFileRequest, DiffRequest, DiffTarget, Repository, RepositoryError,
    ResourceProvenance as RepositoryResourceProvenance, SnapshotGuard,
};
use crate::source::{
    self, Block, CodeSource, DiffSource, GitDiffTarget, LessonSource, LineRange, MarkdownSource,
};

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct CompileOptions {
    /// Directory used for default Git worktree discovery.
    pub current_dir: PathBuf,
    /// Optional directory inside the worktree selected by `--repo`.
    pub repo: Option<PathBuf>,
}

impl CompileOptions {
    pub fn new(current_dir: impl Into<PathBuf>) -> Self {
        Self {
            current_dir: current_dir.into(),
            repo: None,
        }
    }

    pub fn with_repo(mut self, repo: impl Into<PathBuf>) -> Self {
        self.repo = Some(repo.into());
        self
    }
}

/// Compile already-read JSON source. This is the complete `check`/`build`
/// pipeline; callers decide whether to discard or serialize the result.
pub fn compile(input: &str, options: &CompileOptions) -> Result<CompiledLesson, Vec<Diagnostic>> {
    let validated = source::parse_and_validate(input)?;
    let (source, symbols) = validated.into_parts();
    let repository_paths = repository_paths(&source)?;
    let repository = if repository_paths.is_empty() {
        None
    } else {
        Some(
            Repository::discover(&options.current_dir, options.repo.as_deref())
                .map_err(|error| vec![repository_diagnostic("", error)])?,
        )
    };
    let snapshot = match &repository {
        Some(repository) => Some(
            SnapshotGuard::start(repository, &repository_paths)
                .map_err(|error| vec![repository_diagnostic("", error)])?,
        ),
        None => None,
    };

    let schema_version = source.schema_version;
    let mut diagnostics = Vec::new();
    let mut nodes = Vec::with_capacity(source.blocks.len());
    let mut answers = Vec::new();
    let mut next_choice_id: u64 = 0;

    for (index, block) in source.blocks.into_iter().enumerate() {
        let pointer = format!("/blocks/{index}/source");
        let node_id = symbols
            .node_id(block.id())
            .expect("validated blocks are present in the symbol table");
        let source_id = block.id().as_str().to_owned();

        let result = match block {
            Block::Markdown(block) => resolve_markdown(block.source, repository.as_ref(), &pointer),
            Block::Code(block) => resolve_code(block.source, repository.as_ref(), &pointer),
            Block::Diff(block) => resolve_diff(block.source, repository.as_ref(), &pointer),
            Block::MultipleChoice(block) => {
                let choice_start = next_choice_id;
                let mut choices = Vec::with_capacity(block.choices.len());
                let mut correct = None;
                let mut overflow = None;
                for (choice_index, choice) in block.choices.into_iter().enumerate() {
                    let Ok(raw_choice_id) = u32::try_from(next_choice_id) else {
                        overflow = Some(
                            Diagnostic::error(
                                "compiler.choice_id.overflow",
                                format!("/blocks/{index}/choices/{choice_index}"),
                                "lesson contains too many choices to assign artifact-local IDs",
                            )
                            .with_suggestion("Split this lesson into smaller documents."),
                        );
                        break;
                    };
                    let choice_id = ChoiceId::new(raw_choice_id);
                    next_choice_id += 1;
                    if choice.correct {
                        correct = Some(choice_id);
                    }
                    choices.push(PresentedChoice {
                        choice_id,
                        content: choice.content,
                    });
                }
                if let Some(error) = overflow {
                    next_choice_id = choice_start;
                    Err(error)
                } else {
                    let correct_choice_id =
                        correct.expect("source validation requires exactly one correct choice");
                    answers.push(QuizAnswer {
                        node_id,
                        correct_choice_id,
                        explanation: block.explanation,
                    });
                    Ok(CompiledNodeContent::MultipleChoice {
                        prompt: block.prompt,
                        choices,
                        hints: block.hints,
                    })
                }
            }
        };

        match result {
            Ok(content) => nodes.push(CompiledNode {
                node_id,
                source_id,
                content,
            }),
            Err(diagnostic) => diagnostics.push(diagnostic),
        }
    }

    if let Some(repository) = repository.as_ref() {
        if let Err(error) = repository.verify_observed_revisions() {
            diagnostics.push(repository_diagnostic("", error));
        }
        if let Some(snapshot) = snapshot
            && let Err(error) = snapshot.verify(repository)
        {
            diagnostics.push(repository_diagnostic("", error));
        }
    }
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let artifact = CompiledLesson {
        artifact_version: CURRENT_ARTIFACT_VERSION,
        presentation: LessonPresentation {
            title: source.title,
            nodes,
        },
        private: PrivateLesson { answers },
        provenance: BuildProvenance {
            compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
            source_schema_version: schema_version,
        },
    };
    crate::artifact::validate_artifact(&artifact).map_err(|error| {
        vec![Diagnostic::error(
            "compiler.artifact.invalid",
            "",
            format!("compiler produced an invalid artifact: {error}"),
        )]
    })?;
    Ok(artifact)
}

/// Read and compile a JSON lesson source. `.learn` files are explicitly
/// rejected so `check` can never become an artifact validator by accident.
pub fn compile_file(
    input_path: &Path,
    options: &CompileOptions,
) -> Result<CompiledLesson, Vec<Diagnostic>> {
    if input_path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(vec![
            Diagnostic::error(
                "compiler.input.not_json",
                "",
                "lesson source must be a .json file; compiled .learn artifacts are not accepted",
            )
            .with_suggestion("Pass the authored lesson JSON to `learnc check` or `learnc build`."),
        ]);
    }
    let input = fs::read_to_string(input_path).map_err(|error| {
        vec![Diagnostic::error(
            "compiler.input.read",
            "",
            format!("could not read lesson source: {error}"),
        )]
    })?;
    compile(&input, options)
}

/// Default `lesson.json` -> `lesson.learn` output naming.
pub fn default_artifact_path(source_path: &Path) -> PathBuf {
    source_path.with_extension("learn")
}

/// Pretty-serialize and atomically replace an artifact in its destination
/// directory. A failed build leaves any previous output untouched.
pub fn write_artifact_atomic(
    output_path: &Path,
    artifact: &CompiledLesson,
) -> Result<(), Diagnostic> {
    let mut encoded = serde_json::to_vec_pretty(artifact).map_err(|error| {
        Diagnostic::error(
            "compiler.artifact.serialize",
            "",
            format!("could not serialize compiled artifact: {error}"),
        )
    })?;
    encoded.push(b'\n');

    let parent = output_path
        .parent()
        .filter(|value| !value.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let stem = output_path
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("lesson.learn");
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let temporary = parent.join(format!(".{stem}.{}.{}.tmp", std::process::id(), sequence));

    let result = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&encoded)?;
        file.sync_all()?;
        drop(file);
        fs::rename(&temporary, output_path)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(Diagnostic::error(
            "compiler.artifact.write",
            "",
            format!("could not atomically write compiled artifact: {error}"),
        ));
    }
    Ok(())
}

fn resolve_markdown(
    source: MarkdownSource,
    repository: Option<&Repository>,
    pointer: &str,
) -> Result<CompiledNodeContent, Diagnostic> {
    match source {
        MarkdownSource::Inline { content } => Ok(CompiledNodeContent::Markdown {
            provenance: inline_provenance(content.as_bytes()),
            content,
        }),
        MarkdownSource::File { path } => {
            let repository = require_repository(repository, pointer)?;
            let path = repository_path(&path, pointer)?;
            let resource = repository
                .read_file(&path, None)
                .map_err(|error| repository_diagnostic(pointer, error))?;
            Ok(CompiledNodeContent::Markdown {
                content: resource.content,
                provenance: resource_provenance(resource.provenance),
            })
        }
    }
}

fn resolve_code(
    source: CodeSource,
    repository: Option<&Repository>,
    pointer: &str,
) -> Result<CompiledNodeContent, Diagnostic> {
    match source {
        CodeSource::Inline { content } => Ok(CompiledNodeContent::Code {
            provenance: inline_provenance(content.as_bytes()),
            content,
        }),
        CodeSource::File { path, lines } => {
            let repository = require_repository(repository, pointer)?;
            let path = repository_path(&path, pointer)?;
            let resource = repository
                .read_file(
                    &path,
                    lines
                        .map(repository_line_range)
                        .transpose()
                        .map_err(|error| repository_diagnostic(pointer, error))?,
                )
                .map_err(|error| repository_diagnostic(pointer, error))?;
            Ok(CompiledNodeContent::Code {
                content: resource.content,
                provenance: resource_provenance(resource.provenance),
            })
        }
        CodeSource::GitBlob {
            revision,
            path,
            lines,
        } => {
            let repository = require_repository(repository, pointer)?;
            let path = repository_path(&path, pointer)?;
            let resource = repository
                .read_git_blob(
                    revision.as_str(),
                    &path,
                    lines
                        .map(repository_line_range)
                        .transpose()
                        .map_err(|error| repository_diagnostic(pointer, error))?,
                )
                .map_err(|error| repository_diagnostic(pointer, error))?;
            Ok(CompiledNodeContent::Code {
                content: resource.content,
                provenance: resource_provenance(resource.provenance),
            })
        }
    }
}

fn resolve_diff(
    source: DiffSource,
    repository: Option<&Repository>,
    pointer: &str,
) -> Result<CompiledNodeContent, Diagnostic> {
    match source {
        DiffSource::Inline { content } => {
            let diff = repository::parse_unified_diff(&content)
                .map_err(|error| repository_diagnostic(pointer, error))?;
            Ok(CompiledNodeContent::Diff {
                provenance: inline_provenance(content.as_bytes()),
                diff,
            })
        }
        DiffSource::File { path } => {
            let repository = require_repository(repository, pointer)?;
            let path = repository_path(&path, pointer)?;
            let resource = repository
                .read_file(&path, None)
                .map_err(|error| repository_diagnostic(pointer, error))?;
            let diff = repository::parse_unified_diff(&resource.content)
                .map_err(|error| repository_diagnostic(pointer, error))?;
            Ok(CompiledNodeContent::Diff {
                diff,
                provenance: resource_provenance(resource.provenance),
            })
        }
        DiffSource::Git {
            base,
            target,
            files,
            context_lines,
        } => {
            let repository = require_repository(repository, pointer)?;
            let mut requests = Vec::with_capacity(files.len());
            for file in &files {
                requests.push(DiffFileRequest {
                    path: repository_path(&file.path, pointer)?,
                    before_lines: file
                        .before_lines
                        .map(repository_line_range)
                        .transpose()
                        .map_err(|error| repository_diagnostic(pointer, error))?,
                    after_lines: file
                        .after_lines
                        .map(repository_line_range)
                        .transpose()
                        .map_err(|error| repository_diagnostic(pointer, error))?,
                });
            }
            let request = DiffRequest {
                base: base.as_str().to_owned(),
                target: source_diff_target(&target),
                files: requests,
                context_lines,
            };
            let resolved = repository
                .resolve_git_diff(&request)
                .map_err(|error| repository_diagnostic(pointer, error))?;
            let frozen_target = match resolved.provenance.target {
                repository::ResolvedGitDiffTarget::Revision {
                    revision,
                    object_id,
                } => FrozenDiffTarget::Revision {
                    revision,
                    object_id: object_id.as_str().to_owned(),
                },
                repository::ResolvedGitDiffTarget::Worktree => FrozenDiffTarget::Worktree,
            };
            Ok(CompiledNodeContent::Diff {
                diff: resolved.diff,
                provenance: ResourceProvenance::GitDiff {
                    repository: resolved.provenance.repository,
                    base_revision: resolved.provenance.base_revision,
                    base_object_id: resolved.provenance.base_object_id.as_str().to_owned(),
                    target: frozen_target,
                    files: resolved.provenance.files,
                    sha256: resolved.provenance.sha256,
                },
            })
        }
    }
}

fn repository_paths(source: &LessonSource) -> Result<Vec<repository::RepoPath>, Vec<Diagnostic>> {
    let mut paths = Vec::new();
    let mut diagnostics = Vec::new();
    for (index, block) in source.blocks.iter().enumerate() {
        let pointer = format!("/blocks/{index}/source/path");
        let values: Vec<&crate::source::RepoPath> = match block {
            Block::Markdown(block) => match &block.source {
                MarkdownSource::File { path } => vec![path],
                MarkdownSource::Inline { .. } => vec![],
            },
            Block::Code(block) => match &block.source {
                CodeSource::File { path, .. } | CodeSource::GitBlob { path, .. } => vec![path],
                CodeSource::Inline { .. } => vec![],
            },
            Block::Diff(block) => match &block.source {
                DiffSource::File { path } => vec![path],
                DiffSource::Git { files, .. } => files.iter().map(|file| &file.path).collect(),
                DiffSource::Inline { .. } => vec![],
            },
            Block::MultipleChoice(_) => vec![],
        };
        for value in values {
            match repository_path(value, &pointer) {
                Ok(path) => paths.push(path),
                Err(error) => diagnostics.push(error),
            }
        }
    }
    if diagnostics.is_empty() {
        Ok(paths)
    } else {
        Err(diagnostics)
    }
}

fn repository_path(
    path: &crate::source::RepoPath,
    pointer: &str,
) -> Result<repository::RepoPath, Diagnostic> {
    repository::RepoPath::parse(path.as_str())
        .map_err(|error| repository_diagnostic(pointer, error))
}

fn repository_line_range(range: LineRange) -> Result<repository::LineRange, RepositoryError> {
    repository::LineRange::new(range.start, range.end)
}

fn source_diff_target(target: &GitDiffTarget) -> DiffTarget {
    match target {
        GitDiffTarget::Revision { revision } => DiffTarget::Revision(revision.as_str().to_owned()),
        GitDiffTarget::Worktree => DiffTarget::Worktree,
    }
}

fn require_repository<'a>(
    repository: Option<&'a Repository>,
    pointer: &str,
) -> Result<&'a Repository, Diagnostic> {
    repository.ok_or_else(|| {
        Diagnostic::error(
            "compiler.repository.unavailable",
            pointer,
            "repository-backed source could not be resolved",
        )
    })
}

fn repository_diagnostic(pointer: &str, error: RepositoryError) -> Diagnostic {
    let code = error.code();
    let message = error.to_string();
    let mut diagnostic = Diagnostic::error(code, pointer, message);
    if code == "repository.mixed_owners" {
        diagnostic = diagnostic.with_suggestion(
            "Split the selected paths into one diff block per owning repository or submodule.",
        );
    }
    diagnostic
}

fn inline_provenance(bytes: &[u8]) -> ResourceProvenance {
    ResourceProvenance::Inline {
        sha256: sha256(bytes),
    }
}

fn resource_provenance(value: RepositoryResourceProvenance) -> ResourceProvenance {
    match (value.revision, value.object_id, value.content_object_id) {
        (Some(revision), Some(revision_object_id), Some(content_object_id)) => {
            ResourceProvenance::GitBlob {
                repository: value.repository,
                path: value.path,
                revision,
                revision_object_id: revision_object_id.as_str().to_owned(),
                content_object_id: content_object_id.as_str().to_owned(),
                sha256: value.sha256,
            }
        }
        _ => ResourceProvenance::File {
            repository: value.repository,
            path: value.path,
            sha256: value.sha256,
        },
    }
}

fn sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    const INLINE_LESSON: &str = r##"{
        "schema_version":"1.0.0",
        "title":"Queue lesson",
        "blocks":[
            {"type":"markdown","id":"intro","source":{"kind":"inline","content":"# Queue"}},
            {"type":"code","id":"sample","source":{"kind":"inline","content":"push(1);"}},
            {
                "type":"multiple_choice",
                "id":"question",
                "prompt":"Which leaves first?",
                "choices":[
                    {"content":"Oldest","correct":true},
                    {"content":"Newest"}
                ],
                "hints":["Think FIFO"],
                "explanation":"FIFO means first in, first out."
            }
        ]
    }"##;

    #[test]
    fn compiles_inline_content_without_requiring_git() {
        let options = CompileOptions::new("/a/path/that/need/not/exist");
        let artifact = compile(INLINE_LESSON, &options).unwrap();
        assert_eq!(artifact.presentation.nodes.len(), 3);
        assert_eq!(artifact.presentation.nodes[2].node_id.get(), 2);
        assert_eq!(artifact.private.answers.len(), 1);
        let question_json = serde_json::to_value(&artifact.presentation.nodes[2]).unwrap();
        assert!(question_json.get("correct_choice_id").is_none());
        assert!(question_json.get("explanation").is_none());
    }

    #[test]
    fn invalid_inline_patch_has_source_pointer_and_stable_code() {
        let lesson = r#"{
            "schema_version":"1.0.0",
            "title":"Diff",
            "blocks":[{"type":"diff","id":"d","source":{"kind":"inline","content":"@@ broken"}}]
        }"#;
        let diagnostics = compile(lesson, &CompileOptions::new(".")).unwrap_err();
        assert_eq!(diagnostics[0].code, "repository.invalid_patch");
        assert_eq!(diagnostics[0].pointer, "/blocks/0/source");
    }

    #[test]
    fn default_output_replaces_only_the_extension() {
        assert_eq!(
            default_artifact_path(Path::new("lessons/a.json")),
            Path::new("lessons/a.learn")
        );
    }

    #[test]
    fn atomic_writer_emits_readable_round_trippable_json() {
        let artifact = compile(INLINE_LESSON, &CompileOptions::new(".")).unwrap();
        let unique = format!(
            "agent-teacher-compiler-test-{}-{}",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(unique);
        fs::create_dir(&directory).unwrap();
        let output = directory.join("lesson.learn");
        write_artifact_atomic(&output, &artifact).unwrap();
        let decoded: CompiledLesson = serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(decoded, artifact);
        fs::remove_file(output).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn check_input_rejects_compiled_artifact_extension() {
        let diagnostics =
            compile_file(Path::new("lesson.learn"), &CompileOptions::new(".")).unwrap_err();
        assert_eq!(diagnostics[0].code, "compiler.input.not_json");
    }

    #[test]
    fn freezes_file_blob_and_worktree_diff_provenance() {
        let unique = format!(
            "agent-teacher-compiler-git-test-{}-{}",
            std::process::id(),
            TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        );
        let directory = std::env::temp_dir().join(unique);
        fs::create_dir(&directory).unwrap();
        git(&directory, &["init", "-q"]);
        git(
            &directory,
            &["config", "user.email", "tests@example.invalid"],
        );
        git(&directory, &["config", "user.name", "Tests"]);
        git(&directory, &["config", "commit.gpgsign", "false"]);
        fs::write(directory.join("notes.txt"), "before\n").unwrap();
        git(&directory, &["add", "notes.txt"]);
        git(&directory, &["commit", "-qm", "base"]);
        fs::write(directory.join("notes.txt"), "after\n").unwrap();

        let lesson = r#"{
            "schema_version":"1.0.0",
            "title":"Frozen inputs",
            "blocks":[
                {"type":"code","id":"at-head","source":{"kind":"git_blob","revision":"HEAD","path":"notes.txt"}},
                {"type":"code","id":"in-worktree","source":{"kind":"file","path":"notes.txt"}},
                {"type":"diff","id":"change","source":{"kind":"git","base":"HEAD","target":{"kind":"worktree"},"files":[{"path":"notes.txt"}],"context_lines":3}}
            ]
        }"#;
        let artifact = compile(lesson, &CompileOptions::new(&directory)).unwrap();

        match &artifact.presentation.nodes[0].content {
            CompiledNodeContent::Code {
                content,
                provenance,
            } => {
                assert_eq!(content, "before\n");
                let ResourceProvenance::GitBlob {
                    repository,
                    revision_object_id,
                    content_object_id,
                    ..
                } = provenance
                else {
                    panic!("expected Git blob provenance")
                };
                assert_eq!(repository, ".");
                assert_eq!(revision_object_id.len(), 40);
                assert_eq!(content_object_id.len(), 40);
            }
            _ => panic!("expected code node"),
        }
        match &artifact.presentation.nodes[1].content {
            CompiledNodeContent::Code {
                content,
                provenance,
            } => {
                assert_eq!(content, "after\n");
                assert!(
                    matches!(provenance, ResourceProvenance::File { path, .. } if path == "notes.txt")
                );
            }
            _ => panic!("expected code node"),
        }
        match &artifact.presentation.nodes[2].content {
            CompiledNodeContent::Diff { diff, provenance } => {
                assert_eq!(diff.files.len(), 1);
                assert!(
                    matches!(provenance, ResourceProvenance::GitDiff { repository, base_object_id, target: FrozenDiffTarget::Worktree, .. } if repository == "." && base_object_id.len() == 40)
                );
            }
            _ => panic!("expected diff node"),
        }

        fs::remove_dir_all(directory).unwrap();
    }

    fn git(directory: &Path, args: &[&str]) {
        let output = Command::new("git")
            .arg("-C")
            .arg(directory)
            .args(args)
            .output()
            .expect("git is installed");
        assert!(
            output.status.success(),
            "git failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
