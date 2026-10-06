use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::artifact::CURRENT_ARTIFACT_VERSION;
use crate::artifact::{
    ArtifactVersion, ChoiceExplanation, ChoiceId, CompiledLesson, CompiledNode,
    CompiledNodeContent, RunCodeSource, validate_artifact,
};
use crate::repository::{DiffLine, ResolvedDiff};
use crate::source::NodeId;

use super::runner::{RunResult, RunSpec};

/// Load and structurally validate a self-contained `.learn` artifact.
///
/// The explicit version probe makes incompatibility actionable instead of
/// reporting it as a generic Serde enum error. Artifact validation then checks
/// cross-field invariants relied upon by the runtime.
pub fn load_artifact(path: impl AsRef<Path>) -> Result<CompiledLesson, ArtifactLoadError> {
    let path = path.as_ref();
    let bytes = fs::read(path).map_err(|source| ArtifactLoadError::Read {
        path: path.to_owned(),
        source,
    })?;
    decode_artifact(&bytes)
}

fn decode_artifact(bytes: &[u8]) -> Result<CompiledLesson, ArtifactLoadError> {
    let value: serde_json::Value = serde_json::from_slice(bytes)
        .map_err(|source| ArtifactLoadError::MalformedJson { source })?;
    let found_version = value
        .get("artifact_version")
        .and_then(serde_json::Value::as_str)
        .ok_or(ArtifactLoadError::MissingVersion)?;
    if ArtifactVersion::parse(found_version).is_none() {
        return Err(ArtifactLoadError::UnsupportedVersion {
            found: found_version.to_owned(),
            supported: ArtifactVersion::SUPPORTED
                .map(ArtifactVersion::as_str)
                .join(" or "),
        });
    }

    let artifact: CompiledLesson = serde_json::from_value(value)
        .map_err(|source| ArtifactLoadError::InvalidStructure { source })?;
    validate_artifact(&artifact).map_err(|source| ArtifactLoadError::InvalidArtifact {
        message: source.to_string(),
    })?;
    Ok(artifact)
}

#[derive(Debug)]
pub enum ArtifactLoadError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    MalformedJson {
        source: serde_json::Error,
    },
    MissingVersion,
    UnsupportedVersion {
        found: String,
        supported: String,
    },
    InvalidStructure {
        source: serde_json::Error,
    },
    InvalidArtifact {
        message: String,
    },
}

impl ArtifactLoadError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Read { .. } => "artifact_read_failed",
            Self::MalformedJson { .. } => "artifact_malformed_json",
            Self::MissingVersion => "artifact_version_missing",
            Self::UnsupportedVersion { .. } => "artifact_version_unsupported",
            Self::InvalidStructure { .. } => "artifact_invalid_structure",
            Self::InvalidArtifact { .. } => "artifact_invalid",
        }
    }
}

impl fmt::Display for ArtifactLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(
                    formatter,
                    "could not read artifact {}: {source}",
                    path.display()
                )
            }
            Self::MalformedJson { source } => {
                write!(formatter, "artifact is not valid JSON: {source}")
            }
            Self::MissingVersion => formatter.write_str(
                "artifact must contain a string `artifact_version`; rebuild it with learnc",
            ),
            Self::UnsupportedVersion { found, supported } => write!(
                formatter,
                "artifact version {found:?} is unsupported (this runtime accepts {supported}); rebuild it with a compatible learnc",
            ),
            Self::InvalidStructure { source } => {
                write!(formatter, "artifact has an invalid structure: {source}")
            }
            Self::InvalidArtifact { message } => {
                write!(formatter, "artifact invariants are invalid: {message}")
            }
        }
    }
}

impl std::error::Error for ArtifactLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Read { source, .. } => Some(source),
            Self::MalformedJson { source } | Self::InvalidStructure { source } => Some(source),
            Self::MissingVersion
            | Self::UnsupportedVersion { .. }
            | Self::InvalidArtifact { .. } => None,
        }
    }
}

/// Server-owned representation prepared from a validated artifact.
#[derive(Clone, Debug)]
pub(crate) struct RuntimeLesson {
    pub public: PublicLesson,
    pub answers: BTreeMap<NodeId, RuntimeAnswer>,
    /// What a run of each `run_code` block needs. The command and scratch file
    /// stay here, never in the public lesson.
    pub runs: BTreeMap<NodeId, RunSpec>,
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeAnswer {
    pub valid_choices: BTreeSet<ChoiceId>,
    pub correct_choice_id: ChoiceId,
    pub explanation: String,
    pub choice_explanations: Vec<ChoiceExplanation>,
}

impl RuntimeAnswer {
    /// The answer material exposed after a correct attempt or explicit reveal.
    pub fn revealed(&self) -> RevealedAnswer {
        RevealedAnswer {
            choice_id: self.correct_choice_id,
            explanation: self.explanation.clone(),
            choice_explanations: self.choice_explanations.clone(),
        }
    }
}

pub fn project_artifact(artifact: &CompiledLesson) -> PublicLesson {
    project_runtime_lesson(artifact).public
}

pub(crate) fn project_runtime_lesson(artifact: &CompiledLesson) -> RuntimeLesson {
    let mut answers = artifact
        .private
        .answers
        .iter()
        .map(|answer| {
            (
                answer.node_id,
                RuntimeAnswer {
                    valid_choices: BTreeSet::new(),
                    correct_choice_id: answer.correct_choice_id,
                    explanation: answer.explanation.clone(),
                    choice_explanations: answer.choice_explanations.clone(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();

    let mut runs = BTreeMap::new();
    let nodes = artifact
        .presentation
        .nodes
        .iter()
        .map(|node| {
            let content = match &node.content {
                CompiledNodeContent::Markdown { content, .. } => {
                    PublicLessonNodeContent::Markdown {
                        content: content.clone(),
                    }
                }
                CompiledNodeContent::Code {
                    content,
                    language,
                    caption,
                    highlights,
                    first_line,
                    provenance,
                } => PublicLessonNodeContent::Code {
                    content: content.clone(),
                    language: *language,
                    caption: caption.clone(),
                    highlights: highlights.clone(),
                    first_line: *first_line,
                    filename: provenance_basename(provenance),
                },
                CompiledNodeContent::Diff { diff, caption, .. } => PublicLessonNodeContent::Diff {
                    files: project_diff(diff),
                    caption: caption.clone(),
                },
                CompiledNodeContent::MultipleChoice {
                    prompt,
                    choices,
                    hints,
                } => {
                    let answer = answers
                        .get_mut(&node.node_id)
                        .expect("validated artifact has an answer for every question");
                    answer
                        .valid_choices
                        .extend(choices.iter().map(|choice| choice.choice_id));
                    PublicLessonNodeContent::MultipleChoice {
                        prompt: prompt.clone(),
                        choices: choices.clone(),
                        hints: hints.clone(),
                    }
                }
                CompiledNodeContent::RunCode {
                    code,
                    language,
                    caption,
                    argv,
                    file_name,
                    timeout_secs,
                    expected_output,
                } => {
                    if let Some(code) = code_to_run(&artifact.presentation.nodes, code) {
                        runs.insert(
                            node.node_id,
                            RunSpec {
                                argv: argv.clone(),
                                file_name: file_name.clone(),
                                code: code.to_owned(),
                                timeout: Duration::from_secs(u64::from(*timeout_secs)),
                            },
                        );
                    }
                    let (content, first_line, filename, of) = match code {
                        RunCodeSource::Own {
                            content,
                            first_line,
                            provenance,
                        } => (
                            Some(content.clone()),
                            *first_line,
                            provenance_basename(provenance),
                            None,
                        ),
                        RunCodeSource::Of { node } => (None, None, None, Some(*node)),
                    };
                    PublicLessonNodeContent::RunCode {
                        content,
                        of,
                        language: *language,
                        caption: caption.clone(),
                        first_line,
                        filename,
                        timeout_secs: *timeout_secs,
                        expected_output: expected_output.clone(),
                    }
                }
            };
            PublicLessonNode {
                node_id: node.node_id,
                source_id: node.source_id.clone(),
                reference: node_reference(&artifact.presentation.nodes, &node.content),
                content,
            }
        })
        .collect();

    RuntimeLesson {
        public: PublicLesson {
            title: artifact.presentation.title.clone(),
            lesson_path: artifact.provenance.lesson_path.clone(),
            artifact_path: None,
            nodes,
            links: artifact.presentation.links.clone(),
            definitions: artifact.presentation.definitions.clone(),
        },
        answers,
        runs,
    }
}

/// The code a run block runs: its own, or what the code block it points at
/// shows. Artifact validation makes that node a code block.
fn code_to_run<'a>(nodes: &'a [CompiledNode], code: &'a RunCodeSource) -> Option<&'a str> {
    match code {
        RunCodeSource::Own { content, .. } => Some(content),
        RunCodeSource::Of { node } => match &nodes.get(node.get() as usize)?.content {
            CompiledNodeContent::Code { content, .. } => Some(content),
            _ => None,
        },
    }
}

/// Where a block's content came from, for references the learner copies to an
/// agent. Quizzes have no source resource; their references name the block.
/// A run block of `of` has none of its own and refers to the code it runs.
fn node_reference(
    nodes: &[CompiledNode],
    content: &CompiledNodeContent,
) -> Option<crate::artifact::ResourceProvenance> {
    match content {
        CompiledNodeContent::Markdown { provenance, .. }
        | CompiledNodeContent::Code { provenance, .. }
        | CompiledNodeContent::Diff { provenance, .. } => Some(provenance.clone()),
        CompiledNodeContent::RunCode {
            code: RunCodeSource::Own { provenance, .. },
            ..
        } => Some((**provenance).clone()),
        CompiledNodeContent::RunCode {
            code: RunCodeSource::Of { node },
            ..
        } => nodes
            .get(node.get() as usize)
            .and_then(|target| node_reference(nodes, &target.content)),
        CompiledNodeContent::MultipleChoice { .. } => None,
    }
}

fn provenance_basename(provenance: &crate::artifact::ResourceProvenance) -> Option<String> {
    let path = match provenance {
        crate::artifact::ResourceProvenance::File { path, .. }
        | crate::artifact::ResourceProvenance::GitBlob { path, .. } => path,
        crate::artifact::ResourceProvenance::Inline { .. }
        | crate::artifact::ResourceProvenance::GitDiff { .. } => return None,
    };
    path.rsplit('/').next().map(str::to_owned)
}

fn project_diff(diff: &ResolvedDiff) -> Vec<PublicDiffFile> {
    diff.files
        .iter()
        .map(|file| PublicDiffFile {
            old_path: file.old_path.clone(),
            new_path: file.new_path.clone(),
            language: file.language,
            hunks: file
                .hunks
                .iter()
                .map(|hunk| {
                    let suffix = if hunk.heading.is_empty() {
                        String::new()
                    } else {
                        format!(" {}", hunk.heading)
                    };
                    PublicDiffHunk {
                        header: format!(
                            "@@ -{},{} +{},{} @@{}",
                            hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines, suffix
                        ),
                        lines: hunk.lines.clone(),
                    }
                })
                .collect(),
            rendered: file.rendered.clone(),
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicLesson {
    pub title: String,
    /// The lesson source relative to the filesystem root, when recorded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lesson_path: Option<String>,
    /// The `.learn` file as given to `learn serve`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub artifact_path: Option<String>,
    pub nodes: Vec<PublicLessonNode>,
    /// Block links by destination without the `#`.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub links: std::collections::BTreeMap<String, crate::artifact::BlockLink>,
    /// Shown definitions by name, for go-to-definition.
    #[serde(skip_serializing_if = "std::collections::BTreeMap::is_empty")]
    pub definitions: std::collections::BTreeMap<String, Vec<crate::artifact::DefinitionSite>>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicLessonNode {
    pub node_id: NodeId,
    pub source_id: String,
    /// Frozen provenance of the block's content; `None` for quizzes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference: Option<crate::artifact::ResourceProvenance>,
    #[serde(flatten)]
    pub content: PublicLessonNodeContent,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PublicLessonNodeContent {
    Markdown {
        content: String,
    },
    Code {
        content: String,
        language: crate::language::Language,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        highlights: Vec<crate::artifact::CompiledCodeHighlight>,
        /// Source-file line of the first displayed line, when known.
        #[serde(skip_serializing_if = "Option::is_none")]
        first_line: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
    },
    Diff {
        files: Vec<PublicDiffFile>,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
    },
    MultipleChoice {
        prompt: String,
        choices: Vec<crate::artifact::PresentedChoice>,
        hints: Vec<String>,
    },
    /// Code to run, which is either the block's own `content` or the code
    /// block `of` that it runs. The command and scratch file never leave the
    /// runtime.
    RunCode {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        of: Option<NodeId>,
        language: crate::language::Language,
        #[serde(skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        first_line: Option<u32>,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        timeout_secs: u32,
        /// Output frozen when the lesson was built.
        #[serde(skip_serializing_if = "Option::is_none")]
        expected_output: Option<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicDiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub language: crate::language::Language,
    pub hunks: Vec<PublicDiffHunk>,
    /// Complete Markdown blocks of the displayed change, when available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rendered: Option<crate::repository::RenderedMarkdownDiff>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicDiffHunk {
    pub header: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct StateResponse {
    pub lesson: PublicLesson,
    pub progress: LessonProgress,
    pub run: RunStatus,
    /// The last result of each run block that has run, by node.
    pub runs: BTreeMap<NodeId, RunResult>,
}

/// Whether this launch lets the learner run code.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunStatus {
    pub enabled: bool,
    /// The secret a run request must carry; `null` unless running is enabled.
    pub token: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunResponse {
    pub run: RunResult,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct QuestionMutationResponse {
    pub progress: LessonProgress,
    pub question: QuestionState,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct LessonProgress {
    pub completed_questions: usize,
    pub total_questions: usize,
    pub questions: BTreeMap<NodeId, QuestionState>,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize)]
pub struct QuestionState {
    pub attempts: Vec<Attempt>,
    pub completed: bool,
    pub revealed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<RevealedAnswer>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct Attempt {
    pub choice_id: ChoiceId,
    pub correct: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RevealedAnswer {
    pub choice_id: ChoiceId,
    pub explanation: String,
    /// Why individual distractors are wrong; only present once resolved.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub choice_explanations: Vec<ChoiceExplanation>,
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SubmitRequest {
    pub choice_id: ChoiceId,
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::artifact::{
        ArtifactVersion, BuildProvenance, CompiledNode, LessonPresentation, PresentedChoice,
        PrivateLesson, QuizAnswer,
    };
    use crate::source::SchemaVersion;

    pub(crate) fn quiz_artifact() -> CompiledLesson {
        CompiledLesson {
            artifact_version: CURRENT_ARTIFACT_VERSION,
            presentation: LessonPresentation {
                title: "Runtime test".into(),
                links: Default::default(),
                definitions: Default::default(),
                nodes: vec![CompiledNode {
                    node_id: NodeId::new(0),
                    source_id: "question".into(),
                    content: CompiledNodeContent::MultipleChoice {
                        prompt: "Pick one".into(),
                        choices: vec![
                            PresentedChoice {
                                choice_id: ChoiceId::new(0),
                                content: "No".into(),
                            },
                            PresentedChoice {
                                choice_id: ChoiceId::new(1),
                                content: "Yes".into(),
                            },
                        ],
                        hints: vec!["Think".into()],
                    },
                }],
            },
            private: PrivateLesson {
                answers: vec![QuizAnswer {
                    node_id: NodeId::new(0),
                    correct_choice_id: ChoiceId::new(1),
                    explanation: "Yes is correct".into(),
                    choice_explanations: vec![crate::artifact::ChoiceExplanation {
                        choice_id: ChoiceId::new(0),
                        explanation: "No ignores the question".into(),
                    }],
                }],
            },
            provenance: BuildProvenance {
                compiler_version: "0.1.0".into(),
                source_schema_version: SchemaVersion::CURRENT,
                lesson_path: None,
            },
        }
    }

    #[test]
    fn public_projection_never_contains_private_answers() {
        let projection = project_artifact(&quiz_artifact());
        let value = serde_json::to_value(projection).unwrap();
        let serialized = serde_json::to_string(&value).unwrap();
        assert!(!serialized.contains("correct_choice_id"));
        assert!(!serialized.contains("explanation"));
        assert!(!serialized.contains("private"));
        assert_eq!(value["nodes"][0]["choices"][1]["choice_id"], 1);
    }

    #[test]
    fn public_projection_preserves_code_captions() {
        let mut artifact = quiz_artifact();
        artifact.presentation.nodes.push(CompiledNode {
            node_id: NodeId::new(1),
            source_id: "diagram".into(),
            content: CompiledNodeContent::Code {
                content: "flowchart LR\nA --> B".into(),
                language: crate::language::Language::Mermaid,
                caption: Some("The edge represents an asynchronous handoff.".into()),
                highlights: Vec::new(),
                first_line: None,
                provenance: crate::artifact::ResourceProvenance::Inline {
                    sha256: "0".repeat(64),
                },
            },
        });

        let projection = project_artifact(&artifact);
        let value = serde_json::to_value(projection).unwrap();
        assert_eq!(
            value["nodes"][1]["caption"],
            "The edge represents an asynchronous handoff."
        );
        assert!(value["nodes"][1].get("filename").is_none());
    }

    #[test]
    fn public_projection_derives_code_basename_from_frozen_provenance() {
        let mut artifact = quiz_artifact();
        artifact.presentation.nodes.push(CompiledNode {
            node_id: NodeId::new(1),
            source_id: "parser".into(),
            content: CompiledNodeContent::Code {
                content: "fn parse() {}".into(),
                language: crate::language::Language::Rust,
                caption: None,
                highlights: vec![crate::artifact::CompiledCodeHighlight {
                    lines: vec![crate::artifact::CompiledLineRange { start: 1, end: 1 }],
                    color: crate::source::HighlightColor::Blue,
                    annotation: None,
                }],
                first_line: Some(40),
                provenance: crate::artifact::ResourceProvenance::GitBlob {
                    repository: ".".into(),
                    path: "src/compiler/parser.rs".into(),
                    revision: "HEAD".into(),
                    revision_object_id: "0".repeat(40),
                    content_object_id: "1".repeat(40),
                    sha256: "2".repeat(64),
                },
            },
        });

        let projection = serde_json::to_value(project_artifact(&artifact)).unwrap();
        assert_eq!(projection["nodes"][1]["filename"], "parser.rs");
        // References carry the frozen provenance; quizzes have none.
        assert_eq!(projection["nodes"][1]["reference"]["kind"], "git_blob");
        assert_eq!(
            projection["nodes"][1]["reference"]["revision_object_id"],
            "0".repeat(40)
        );
        assert!(projection["nodes"][0].get("reference").is_none());
        assert_eq!(projection["nodes"][1]["first_line"], 40);
        assert!(projection["nodes"][0].get("first_line").is_none());
        assert_eq!(
            projection["nodes"][1]["highlights"][0]["lines"][0]["start"],
            1
        );
        assert_eq!(
            projection["nodes"][1]["highlights"][0]["lines"][0]["end"],
            1
        );
        assert_eq!(projection["nodes"][1]["highlights"][0]["color"], "blue");
    }

    #[test]
    fn public_projection_of_run_blocks_shows_code_or_the_block_it_runs() {
        use crate::artifact::{ResourceProvenance, RunCodeSource};
        use crate::language::Language;

        let mut artifact = quiz_artifact();
        let file = ResourceProvenance::File {
            path: "tools/count.py".into(),
            sha256: "1".repeat(64),
            blob_id: Some("2".repeat(40)),
            head: None,
        };
        artifact.presentation.nodes.push(CompiledNode {
            node_id: NodeId::new(1),
            source_id: "shown".into(),
            content: CompiledNodeContent::Code {
                content: "print(1)\n".into(),
                language: Language::Python,
                caption: None,
                highlights: Vec::new(),
                first_line: Some(7),
                provenance: file.clone(),
            },
        });
        let run = |node_id: u32, source_id: &str, code: RunCodeSource| CompiledNode {
            node_id: NodeId::new(node_id),
            source_id: source_id.into(),
            content: CompiledNodeContent::RunCode {
                code,
                language: Language::Python,
                caption: Some("Run it.".into()),
                argv: vec!["python3".into(), "{file}".into()],
                file_name: "main.py".into(),
                timeout_secs: 10,
                expected_output: Some("1\n".into()),
            },
        };
        artifact.presentation.nodes.push(run(
            2,
            "run-shown",
            RunCodeSource::Of {
                node: NodeId::new(1),
            },
        ));
        artifact.presentation.nodes.push(run(
            3,
            "run-own",
            RunCodeSource::Own {
                content: "print(2)\n".into(),
                first_line: Some(1),
                provenance: Box::new(file.clone()),
            },
        ));
        crate::artifact::validate_artifact(&artifact).unwrap();

        let projection = serde_json::to_value(project_artifact(&artifact)).unwrap();
        let of = &projection["nodes"][2];
        assert_eq!(of["type"], "run_code");
        assert_eq!(of["of"], 1);
        assert!(of.get("content").is_none());
        assert_eq!(of["language"], "python");
        assert_eq!(of["caption"], "Run it.");
        assert_eq!(of["timeout_secs"], 10);
        assert_eq!(of["expected_output"], "1\n");
        // It refers to the file the code it runs came from.
        assert_eq!(of["reference"]["path"], "tools/count.py");

        let own = &projection["nodes"][3];
        assert_eq!(own["content"], "print(2)\n");
        assert!(own.get("of").is_none());
        assert_eq!(own["first_line"], 1);
        assert_eq!(own["filename"], "count.py");
        assert_eq!(own["reference"]["blob_id"], "2".repeat(40));

        // Nothing about how to run the code reaches the browser.
        let text = projection.to_string();
        assert!(!text.contains("argv") && !text.contains("file_name"));
        assert!(!text.contains("python3"));
    }

    /// A quiz (0), a shell code block (1), a run block that runs it (2), and a
    /// run block with shell code of its own (3).
    pub(crate) fn run_artifact() -> CompiledLesson {
        use crate::artifact::{ResourceProvenance, RunCodeSource};
        use crate::language::Language;

        let mut artifact = quiz_artifact();
        let inline = || ResourceProvenance::Inline {
            sha256: "0".repeat(64),
        };
        artifact.presentation.nodes.push(CompiledNode {
            node_id: NodeId::new(1),
            source_id: "shown".into(),
            content: CompiledNodeContent::Code {
                content: "echo shown\n".into(),
                language: Language::Shell,
                caption: None,
                highlights: Vec::new(),
                first_line: None,
                provenance: inline(),
            },
        });
        let run =
            |node_id: u32, source_id: &str, code: RunCodeSource, timeout_secs: u32| CompiledNode {
                node_id: NodeId::new(node_id),
                source_id: source_id.into(),
                content: CompiledNodeContent::RunCode {
                    code,
                    language: Language::Shell,
                    caption: None,
                    argv: vec!["sh".into(), "{file}".into()],
                    file_name: "main.sh".into(),
                    timeout_secs,
                    expected_output: None,
                },
            };
        artifact.presentation.nodes.push(run(
            2,
            "run-shown",
            RunCodeSource::Of {
                node: NodeId::new(1),
            },
            10,
        ));
        artifact.presentation.nodes.push(run(
            3,
            "run-own",
            RunCodeSource::Own {
                content: "echo own\n".into(),
                first_line: None,
                provenance: Box::new(inline()),
            },
            3,
        ));
        crate::artifact::validate_artifact(&artifact).unwrap();
        artifact
    }

    #[test]
    fn the_runtime_keeps_what_a_run_needs_and_reads_of_code_from_the_node_it_runs() {
        let lesson = project_runtime_lesson(&run_artifact());
        assert_eq!(lesson.runs.len(), 2);
        let of = &lesson.runs[&NodeId::new(2)];
        assert_eq!(of.code, "echo shown\n");
        assert_eq!(of.argv, ["sh", "{file}"]);
        assert_eq!(of.file_name, "main.sh");
        assert_eq!(of.timeout, Duration::from_secs(10));
        let own = &lesson.runs[&NodeId::new(3)];
        assert_eq!(own.code, "echo own\n");
        assert_eq!(own.timeout, Duration::from_secs(3));
        // Only run blocks can run.
        assert!(!lesson.runs.contains_key(&NodeId::new(0)));
        assert!(!lesson.runs.contains_key(&NodeId::new(1)));
    }

    #[test]
    fn version_gate_reports_rebuild_guidance() {
        let mut value = serde_json::to_value(quiz_artifact()).unwrap();
        value["artifact_version"] = serde_json::json!("2.0.0");
        let error = decode_artifact(&serde_json::to_vec(&value).unwrap()).unwrap_err();
        assert!(matches!(
            error,
            ArtifactLoadError::UnsupportedVersion { .. }
        ));
        assert!(error.to_string().contains("rebuild"));
    }

    #[test]
    fn matching_version_still_requires_the_full_structure() {
        let invalid = serde_json::json!({
            "artifact_version": ArtifactVersion::V1_0_0,
            "presentation": {"title": "missing nodes"}
        });
        assert!(matches!(
            decode_artifact(&serde_json::to_vec(&invalid).unwrap()),
            Err(ArtifactLoadError::InvalidStructure { .. })
        ));
    }

    #[test]
    fn runtime_loads_legacy_v1_artifacts_without_language_fields() {
        let artifact = decode_artifact(include_bytes!(
            "../../tests/fixtures/artifact/v1.0.0-without-language-fields.learn.json"
        ))
        .expect("legacy v1 artifact remains readable");
        let public = serde_json::to_value(project_artifact(&artifact)).unwrap();
        assert_eq!(public["nodes"][1]["language"], "text");
        assert_eq!(public["nodes"][2]["files"][0]["language"], "text");
    }
}
