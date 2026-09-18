//! The versioned, self-contained data contract shared by compiler and runtime.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use crate::repository::ResolvedDiff;
use crate::source::{NodeId, SchemaVersion};

/// Artifact format emitted by this version of `learnc`.
pub const CURRENT_ARTIFACT_VERSION: ArtifactVersion = ArtifactVersion::V1_0_0;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum ArtifactVersion {
    #[serde(rename = "1.0.0")]
    V1_0_0,
}

impl ArtifactVersion {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1_0_0 => "1.0.0",
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
        provenance: ResourceProvenance,
    },
    Diff {
        diff: ResolvedDiff,
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
#[serde(deny_unknown_fields)]
pub struct PresentedChoice {
    pub choice_id: ChoiceId,
    pub content: String,
}

/// Generated, artifact-local choice identity. Values are dense across the
/// artifact in authored question and choice order.
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
        repository: String,
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
    fn validation_rejects_private_answer_leaks_to_wrong_question() {
        let mut artifact = quiz_artifact();
        artifact.private.answers[0].correct_choice_id = ChoiceId::new(99);
        assert!(validate_artifact(&artifact).is_err());
    }
}
