//! The versioned, self-contained data contract shared by compiler and runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::language::Language;
use crate::repository::ResolvedDiff;
use crate::source::{HighlightColor, NodeId, SchemaVersion};

/// Artifact format emitted by this version of `learnc`.
pub const CURRENT_ARTIFACT_VERSION: ArtifactVersion = ArtifactVersion::V1_1_0;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ArtifactVersion {
    #[serde(rename = "1.0.0")]
    V1_0_0,
    #[serde(rename = "1.1.0")]
    V1_1_0,
}

impl ArtifactVersion {
    /// Every artifact version the runtime can load.
    pub const SUPPORTED: [Self; 2] = [Self::V1_0_0, Self::V1_1_0];

    pub fn parse(value: &str) -> Option<Self> {
        Self::SUPPORTED
            .into_iter()
            .find(|version| version.as_str() == value)
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1_0_0 => "1.0.0",
            Self::V1_1_0 => "1.1.0",
        }
    }
}

impl fmt::Display for ArtifactVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// A complete `.learn` file. The `private` section is never included in the
/// browser's initial lesson projection by the runtime.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledLesson {
    pub artifact_version: ArtifactVersion,
    pub presentation: LessonPresentation,
    pub private: PrivateLesson,
    pub provenance: BuildProvenance,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LessonPresentation {
    pub title: String,
    /// Authored order is preserved. Node IDs are dense indices into this list.
    pub nodes: Vec<CompiledNode>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CompiledNode {
    pub node_id: NodeId,
    /// Retained for diagnostics and future exports, never used as runtime identity.
    pub source_id: String,
    #[serde(flatten)]
    pub content: CompiledNodeContent,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CompiledNodeContent {
    Markdown {
        content: String,
        provenance: ResourceProvenance,
    },
    Code {
        content: String,
        #[serde(default)]
        language: Language,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        /// One-based inclusive positions in the compiled code fragment.
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        highlights: Vec<CompiledCodeHighlight>,
        provenance: ResourceProvenance,
    },
    Diff {
        diff: ResolvedDiff,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        caption: Option<String>,
        provenance: ResourceProvenance,
    },
    MultipleChoice {
        prompt: String,
        choices: Vec<PresentedChoice>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        hints: Vec<String>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "CompiledCodeHighlightWire")]
#[serde(deny_unknown_fields)]
pub struct CompiledCodeHighlight {
    /// One or more one-based inclusive ranges in the compiled fragment.
    pub lines: Vec<CompiledLineRange>,
    pub color: HighlightColor,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub annotation: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompiledLineRange {
    pub start: u32,
    pub end: u32,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum CompiledCodeHighlightWire {
    Current(CompiledCodeHighlightCurrentWire),
    Legacy(CompiledCodeHighlightLegacyWire),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompiledCodeHighlightCurrentWire {
    lines: Vec<CompiledLineRange>,
    color: HighlightColor,
    #[serde(default)]
    annotation: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CompiledCodeHighlightLegacyWire {
    start: u32,
    end: u32,
    color: HighlightColor,
}

impl TryFrom<CompiledCodeHighlightWire> for CompiledCodeHighlight {
    type Error = &'static str;

    fn try_from(value: CompiledCodeHighlightWire) -> Result<Self, Self::Error> {
        match value {
            CompiledCodeHighlightWire::Current(CompiledCodeHighlightCurrentWire {
                lines,
                color,
                annotation,
            }) => {
                if lines.is_empty() {
                    return Err("a compiled code highlight must contain at least one line range");
                }
                Ok(Self {
                    lines,
                    color,
                    annotation,
                })
            }
            CompiledCodeHighlightWire::Legacy(CompiledCodeHighlightLegacyWire {
                start,
                end,
                color,
            }) => Ok(Self {
                lines: vec![CompiledLineRange { start, end }],
                color,
                annotation: None,
            }),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PresentedChoice {
    pub choice_id: ChoiceId,
    pub content: String,
}

/// Generated, artifact-local choice identity. Values are dense across the
/// artifact in authored question order and, within a question, in the
/// compiler's shuffled presentation order.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ChoiceId(u32);

impl ChoiceId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for ChoiceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PrivateLesson {
    /// Kept as a list in JSON for readability; runtime consumers can index it.
    pub answers: Vec<QuizAnswer>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuizAnswer {
    pub node_id: NodeId,
    pub correct_choice_id: ChoiceId,
    /// Markdown revealed after success or an explicit reveal action.
    pub explanation: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildProvenance {
    pub compiler_version: String,
    pub source_schema_version: SchemaVersion,
}

/// Describes where embedded bytes came from without leaking an absolute path.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ResourceProvenance {
    Inline {
        sha256: String,
    },
    File {
        path: String,
        sha256: String,
    },
    GitBlob {
        repository: String,
        path: String,
        revision: String,
        revision_object_id: String,
        content_object_id: String,
        sha256: String,
    },
    GitDiff {
        repository: String,
        base_revision: String,
        base_object_id: String,
        target: FrozenDiffTarget,
        files: Vec<String>,
        sha256: String,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FrozenDiffTarget {
    Revision { revision: String, object_id: String },
    Worktree,
}

/// Structural artifact checks used by runtime loading and contract tests.
/// Deserialization gates the version; this pass checks cross-field invariants.
pub fn validate_artifact(artifact: &CompiledLesson) -> Result<(), ArtifactValidationError> {
    let mut source_ids = BTreeSet::new();
    let mut choices = BTreeSet::new();
    let mut questions = BTreeMap::new();
    let mut next_choice_id = 0_u32;

    for (index, node) in artifact.presentation.nodes.iter().enumerate() {
        if node.node_id.get() as usize != index {
            return Err(ArtifactValidationError::new(format!(
                "node IDs must be dense and ordered; index {index} has ID {}",
                node.node_id
            )));
        }
        if !source_ids.insert(&node.source_id) {
            return Err(ArtifactValidationError::new(format!(
                "duplicate retained source ID {:?}",
                node.source_id
            )));
        }
        if let CompiledNodeContent::Code {
            content,
            language,
            highlights,
            ..
        } = &node.content
        {
            if *language == Language::Mermaid && !highlights.is_empty() {
                return Err(ArtifactValidationError::new(format!(
                    "Mermaid node {} cannot contain source-line highlights",
                    node.node_id
                )));
            }
            let line_count = content.lines().count();
            for (highlight_index, highlight) in highlights.iter().enumerate() {
                if highlight.lines.is_empty() {
                    return Err(ArtifactValidationError::new(format!(
                        "code highlight {highlight_index} on node {} has no ranges",
                        node.node_id
                    )));
                }
                if highlight
                    .annotation
                    .as_ref()
                    .is_some_and(|value| value.trim().is_empty())
                {
                    return Err(ArtifactValidationError::new(format!(
                        "code highlight {highlight_index} on node {} has an empty annotation",
                        node.node_id
                    )));
                }
                for (range_index, range) in highlight.lines.iter().enumerate() {
                    if range.start == 0 || range.end < range.start {
                        return Err(ArtifactValidationError::new(format!(
                            "code highlight {highlight_index} range {range_index} on node {} has an invalid range {}-{}",
                            node.node_id, range.start, range.end
                        )));
                    }
                    if usize::try_from(range.end).map_or(true, |end| end > line_count) {
                        return Err(ArtifactValidationError::new(format!(
                            "code highlight {highlight_index} range {range_index} on node {} ends after line {line_count}",
                            node.node_id
                        )));
                    }
                }
            }
            for left_index in 0..highlights.len() {
                for right_index in (left_index + 1)..highlights.len() {
                    let left = &highlights[left_index];
                    let right = &highlights[right_index];
                    if left.color != right.color {
                        for left_range in &left.lines {
                            for right_range in &right.lines {
                                if left_range.start <= right_range.end
                                    && right_range.start <= left_range.end
                                {
                                    return Err(ArtifactValidationError::new(format!(
                                        "differently colored code highlights {left_index} and {right_index} overlap on node {}",
                                        node.node_id
                                    )));
                                }
                            }
                        }
                    }
                }
            }
        }
        if let CompiledNodeContent::MultipleChoice {
            choices: values, ..
        } = &node.content
        {
            if values.len() < 2 {
                return Err(ArtifactValidationError::new(format!(
                    "question {} has fewer than two choices",
                    node.node_id
                )));
            }
            for choice in values {
                if choice.choice_id.get() != next_choice_id {
                    return Err(ArtifactValidationError::new(format!(
                        "choice IDs must be dense and ordered; expected {next_choice_id}, found {}",
                        choice.choice_id
                    )));
                }
                next_choice_id = next_choice_id.checked_add(1).ok_or_else(|| {
                    ArtifactValidationError::new("artifact contains too many choices")
                })?;
                if !choices.insert(choice.choice_id) {
                    return Err(ArtifactValidationError::new(format!(
                        "duplicate choice ID {}",
                        choice.choice_id
                    )));
                }
            }
            questions.insert(node.node_id, values);
        }
    }

    let mut answered = BTreeSet::new();
    for answer in &artifact.private.answers {
        if !answered.insert(answer.node_id) {
            return Err(ArtifactValidationError::new(format!(
                "question {} has more than one private answer",
                answer.node_id
            )));
        }
        let Some(question_choices) = questions.get(&answer.node_id) else {
            return Err(ArtifactValidationError::new(format!(
                "private answer refers to non-question node {}",
                answer.node_id
            )));
        };
        if !question_choices
            .iter()
            .any(|choice| choice.choice_id == answer.correct_choice_id)
        {
            return Err(ArtifactValidationError::new(format!(
                "private answer for node {} refers to absent choice {}",
                answer.node_id, answer.correct_choice_id
            )));
        }
    }

    if answered.len() != questions.len() {
        return Err(ArtifactValidationError::new(
            "every multiple-choice node must have exactly one private answer",
        ));
    }
    Ok(())
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ArtifactValidationError {
    message: String,
}

impl ArtifactValidationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ArtifactValidationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for ArtifactValidationError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiz_artifact() -> CompiledLesson {
        CompiledLesson {
            artifact_version: CURRENT_ARTIFACT_VERSION,
            presentation: LessonPresentation {
                title: "Test".into(),
                nodes: vec![CompiledNode {
                    node_id: NodeId::new(0),
                    source_id: "question".into(),
                    content: CompiledNodeContent::MultipleChoice {
                        prompt: "Pick one".into(),
                        choices: vec![
                            PresentedChoice {
                                choice_id: ChoiceId::new(0),
                                content: "A".into(),
                            },
                            PresentedChoice {
                                choice_id: ChoiceId::new(1),
                                content: "B".into(),
                            },
                        ],
                        hints: vec![],
                    },
                }],
            },
            private: PrivateLesson {
                answers: vec![QuizAnswer {
                    node_id: NodeId::new(0),
                    correct_choice_id: ChoiceId::new(1),
                    explanation: "Because B".into(),
                }],
            },
            provenance: BuildProvenance {
                compiler_version: "0.1.0".into(),
                source_schema_version: SchemaVersion::CURRENT,
            },
        }
    }

    #[test]
    fn private_answers_are_not_nested_in_public_questions() {
        let value = serde_json::to_value(quiz_artifact()).unwrap();
        let question = &value["presentation"]["nodes"][0];
        assert!(question.get("correct_choice_id").is_none());
        assert!(question.get("explanation").is_none());
        assert_eq!(value["private"]["answers"][0]["correct_choice_id"], 1);
    }

    #[test]
    fn readable_json_round_trips_and_validates() {
        let artifact = quiz_artifact();
        let json = serde_json::to_string_pretty(&artifact).unwrap();
        let decoded: CompiledLesson = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, artifact);
        validate_artifact(&decoded).unwrap();
    }

    #[test]
    fn legacy_flat_highlight_ranges_decode_into_groups() {
        let mut artifact = quiz_artifact();
        artifact.artifact_version = ArtifactVersion::V1_0_0;
        artifact.presentation.nodes.push(CompiledNode {
            node_id: NodeId::new(1),
            source_id: "legacy-code".into(),
            content: CompiledNodeContent::Code {
                content: "first\nsecond\n".into(),
                language: Language::Text,
                caption: None,
                highlights: Vec::new(),
                provenance: ResourceProvenance::Inline {
                    sha256: "0".repeat(64),
                },
            },
        });
        let mut value = serde_json::to_value(artifact).unwrap();
        value["presentation"]["nodes"][1]["highlights"] = serde_json::json!([{
            "start": 1,
            "end": 2,
            "color": "blue"
        }]);

        let decoded: CompiledLesson = serde_json::from_value(value).unwrap();
        validate_artifact(&decoded).unwrap();
        let CompiledNodeContent::Code { highlights, .. } = &decoded.presentation.nodes[1].content
        else {
            panic!("expected code node")
        };
        assert_eq!(
            highlights[0],
            CompiledCodeHighlight {
                lines: vec![CompiledLineRange { start: 1, end: 2 }],
                color: HighlightColor::Blue,
                annotation: None,
            }
        );
    }

    #[test]
    fn legacy_v1_code_and_diff_languages_default_to_text() {
        let artifact: CompiledLesson = serde_json::from_str(include_str!(
            "../../tests/fixtures/artifact/v1.0.0-without-language-fields.learn.json"
        ))
        .unwrap();
        validate_artifact(&artifact).unwrap();

        let CompiledNodeContent::Code {
            language, caption, ..
        } = &artifact.presentation.nodes[1].content
        else {
            panic!("expected legacy code node")
        };
        assert_eq!(*language, Language::Text);
        assert_eq!(caption, &None);

        let CompiledNodeContent::Diff { diff, caption, .. } =
            &artifact.presentation.nodes[2].content
        else {
            panic!("expected legacy diff node")
        };
        assert_eq!(diff.files[0].language, Language::Text);
        assert_eq!(caption, &None);
    }

    #[test]
    fn validation_rejects_private_answer_leaks_to_wrong_question() {
        let mut artifact = quiz_artifact();
        artifact.private.answers[0].correct_choice_id = ChoiceId::new(99);
        assert!(validate_artifact(&artifact).is_err());
    }

    #[test]
    fn validation_rejects_code_highlights_outside_the_fragment() {
        let mut artifact = quiz_artifact();
        artifact.presentation.nodes.push(CompiledNode {
            node_id: NodeId::new(1),
            source_id: "code".into(),
            content: CompiledNodeContent::Code {
                content: "only one line".into(),
                language: Language::Rust,
                caption: None,
                highlights: vec![CompiledCodeHighlight {
                    lines: vec![CompiledLineRange { start: 2, end: 2 }],
                    color: HighlightColor::Yellow,
                    annotation: None,
                }],
                provenance: ResourceProvenance::Inline {
                    sha256: "0".repeat(64),
                },
            },
        });
        let error = validate_artifact(&artifact).unwrap_err();
        assert!(error.to_string().contains("ends after line 1"));
    }
}
