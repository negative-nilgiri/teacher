use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[cfg(test)]
use crate::artifact::CURRENT_ARTIFACT_VERSION;
use crate::artifact::{
    ArtifactVersion, ChoiceExplanation, ChoiceId, CompiledLesson, CompiledNodeContent,
    validate_artifact,
};
use crate::repository::{DiffLine, ResolvedDiff};
use crate::source::NodeId;

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
            };
            PublicLessonNode {
                node_id: node.node_id,
                source_id: node.source_id.clone(),
                content,
            }
        })
        .collect();

    RuntimeLesson {
        public: PublicLesson {
            title: artifact.presentation.title.clone(),
            nodes,
        },
        answers,
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
        })
        .collect()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicLesson {
    pub title: String,
    pub nodes: Vec<PublicLessonNode>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicLessonNode {
    pub node_id: NodeId,
    pub source_id: String,
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
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PublicDiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub language: crate::language::Language,
    pub hunks: Vec<PublicDiffHunk>,
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
