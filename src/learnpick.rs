//! Isolated lesson-block recommendation logic used only by the `learnpick` binary.
//!
//! The module is exposed by the library so the companion binary can use the
//! package normally. No compiler, runtime, source, artifact, or repository
//! module depends on this optional network-backed adviser.

#[path = "learnpick/client.rs"]
mod client;

use std::collections::BTreeMap;
use std::fmt;

use crate::source::SchemaVersion;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const QUESTION_ID: &str = "block_type";

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockType {
    Markdown,
    Code,
    Diff,
    MultipleChoice,
}

impl BlockType {
    const ALL: [Self; 4] = [Self::Markdown, Self::Code, Self::Diff, Self::MultipleChoice];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Markdown => "markdown",
            Self::Code => "code",
            Self::Diff => "diff",
            Self::MultipleChoice => "multiple_choice",
        }
    }
}

impl fmt::Display for BlockType {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Recommendation {
    pub source_schema_version: String,
    pub model: String,
    pub block_type: BlockType,
    pub confidence: f64,
    pub probabilities: BTreeMap<BlockType, f64>,
    pub usage: TokenUsage,
}

#[derive(Debug)]
pub enum PickError {
    EmptyUnit,
    MissingApiKey,
    InvalidConfiguration(&'static str),
    Transport(String),
    Api { status: u16 },
    Decode(String),
    InvalidResponse(String),
}

impl PickError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::EmptyUnit => "learnpick.unit.empty",
            Self::MissingApiKey => "learnpick.credentials.missing",
            Self::InvalidConfiguration(_) => "learnpick.configuration.invalid",
            Self::Transport(_) => "learnpick.transport.failed",
            Self::Api { .. } => "learnpick.api.failed",
            Self::Decode(_) => "learnpick.response.decode_failed",
            Self::InvalidResponse(_) => "learnpick.response.invalid",
        }
    }

    pub const fn suggestion(&self) -> Option<&'static str> {
        match self {
            Self::EmptyUnit => Some("Provide one concise teaching-unit description."),
            Self::MissingApiKey => Some("Set TYPESAFE_API_KEY and run learnpick again."),
            Self::InvalidConfiguration(_) => Some(
                "Set the variable to a non-empty UTF-8 value; TYPESAFE_BASE_URL and TYPESAFE_DEFAULT_MODEL may instead be unset to use the defaults.",
            ),
            Self::Transport(_) | Self::Api { .. } => {
                Some("Choose the lesson block manually or retry the optional adviser later.")
            }
            Self::Decode(_) | Self::InvalidResponse(_) => Some(
                "Choose the lesson block manually; the TypeSafe response did not match the expected Choice contract.",
            ),
        }
    }
}

impl fmt::Display for PickError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyUnit => formatter.write_str("teaching unit must not be empty or whitespace"),
            Self::MissingApiKey => formatter.write_str("TYPESAFE_API_KEY is not set or is empty"),
            Self::InvalidConfiguration(name) => {
                write!(formatter, "{name} is set but empty or not valid UTF-8")
            }
            Self::Transport(message) => write!(formatter, "TypeSafe request failed: {message}"),
            Self::Api { status } => write!(formatter, "TypeSafe API returned HTTP {status}"),
            Self::Decode(message) => {
                write!(
                    formatter,
                    "could not decode the TypeSafe response: {message}"
                )
            }
            Self::InvalidResponse(message) => {
                write!(formatter, "invalid TypeSafe Choice response: {message}")
            }
        }
    }
}

impl std::error::Error for PickError {}

pub fn recommend(unit: &str) -> Result<Recommendation, PickError> {
    let unit = unit.trim();
    if unit.is_empty() {
        return Err(PickError::EmptyUnit);
    }

    let config = client::Config::from_env()?;
    let request = build_request(unit, &config.model);
    let response = client::send(&config, &request)?;
    project_response(response)
}

fn build_request(unit: &str, model: &str) -> Value {
    json!({
        "state": {
            "teaching_unit": unit
        },
        "model": model,
        "questions": {
            QUESTION_ID: {
                "type": "choice",
                "instructions": {
                    "question": "Which lesson block best presents `teaching_unit`?",
                    "focus": "Classify what the learner needs to see, not merely which source material is available."
                },
                "criteria": {
                    "markdown": {
                        "what": "Rendered explanation, concepts, narrative, lists, emphasis, or surrounding teaching prose.",
                        "not_for": "Literal source code, diagrams, before-and-after repository changes, or a learner question."
                    },
                    "code": {
                        "what": "Final source content, a selected file range, literal syntax, or a Mermaid diagram.",
                        "not_for": "A before-and-after transition where the change itself is what the learner must understand."
                    },
                    "diff": {
                        "what": "Additions, deletions, or the relationship between content before and after a change.",
                        "not_for": "Showing file contents merely because the file is new, or selecting a few interesting lines."
                    },
                    "multiple_choice": {
                        "what": "A factual check where the learner must select one answer from explicit choices.",
                        "not_for": "Explanatory prose, source presentation, or open-ended discussion."
                    }
                }
            }
        }
    })
}

fn project_response(mut response: ApiResponse) -> Result<Recommendation, PickError> {
    let answer = response
        .answers
        .remove(QUESTION_ID)
        .ok_or_else(|| PickError::InvalidResponse(format!("missing `{QUESTION_ID}` answer")))?;
    if answer.answer_type != "choice" {
        return Err(PickError::InvalidResponse(format!(
            "`{QUESTION_ID}` answer had type {:?}, expected \"choice\"",
            answer.answer_type
        )));
    }
    if !(0.0..=1.0).contains(&answer.confidence) {
        return Err(PickError::InvalidResponse(
            "confidence must be between 0 and 1".into(),
        ));
    }
    for block_type in BlockType::ALL {
        let probability = answer.probabilities.get(&block_type).ok_or_else(|| {
            PickError::InvalidResponse(format!("missing probability for `{block_type}`"))
        })?;
        if !(0.0..=1.0).contains(probability) {
            return Err(PickError::InvalidResponse(format!(
                "probability for `{block_type}` must be between 0 and 1"
            )));
        }
    }
    let sum = answer.probabilities.values().sum::<f64>();
    if (sum - 1.0).abs() > 0.001 {
        return Err(PickError::InvalidResponse(format!(
            "probabilities sum to {sum}, expected 1"
        )));
    }

    Ok(Recommendation {
        source_schema_version: SchemaVersion::CURRENT.as_str().to_owned(),
        model: response.model,
        block_type: answer.choice,
        confidence: answer.confidence,
        probabilities: answer.probabilities,
        usage: response.usage,
    })
}

#[derive(Debug, Deserialize)]
pub(super) struct ApiResponse {
    model: String,
    answers: BTreeMap<String, ChoiceAnswer>,
    usage: TokenUsage,
}

#[derive(Debug, Deserialize)]
struct ChoiceAnswer {
    #[serde(rename = "type")]
    answer_type: String,
    choice: BlockType,
    confidence: f64,
    probabilities: BTreeMap<BlockType, f64>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_is_one_four_option_choice_question() {
        let request = build_request("Show the final parser implementation", "jev-latest");
        assert_eq!(request["model"], "jev-latest");
        assert_eq!(
            request["state"]["teaching_unit"],
            "Show the final parser implementation"
        );
        let criteria = request["questions"][QUESTION_ID]["criteria"]
            .as_object()
            .expect("criteria is an object");
        assert_eq!(criteria.len(), 4);
        for block_type in BlockType::ALL {
            assert!(criteria.contains_key(block_type.as_str()));
        }
    }

    #[test]
    fn response_projection_preserves_choice_probabilities_and_usage() {
        let response: ApiResponse = serde_json::from_value(json!({
            "model": "jev-1.13.0",
            "answers": {
                "block_type": {
                    "type": "choice",
                    "choice": "diff",
                    "confidence": 0.84,
                    "probabilities": {
                        "markdown": 0.01,
                        "code": 0.14,
                        "diff": 0.85,
                        "multiple_choice": 0.0
                    }
                }
            },
            "usage": {"input_tokens": 320, "output_tokens": 34}
        }))
        .unwrap();

        let recommendation = project_response(response).unwrap();
        assert_eq!(recommendation.block_type, BlockType::Diff);
        assert_eq!(recommendation.model, "jev-1.13.0");
        assert_eq!(recommendation.confidence, 0.84);
        assert_eq!(recommendation.probabilities[&BlockType::Code], 0.14);
        assert_eq!(recommendation.usage.output_tokens, 34);
    }

    #[test]
    fn response_projection_rejects_incomplete_probability_maps() {
        let response: ApiResponse = serde_json::from_value(json!({
            "model": "jev-1.13.0",
            "answers": {
                "block_type": {
                    "type": "choice",
                    "choice": "code",
                    "confidence": 1.0,
                    "probabilities": {"code": 1.0}
                }
            },
            "usage": {"input_tokens": 1, "output_tokens": 1}
        }))
        .unwrap();

        assert!(matches!(
            project_response(response),
            Err(PickError::InvalidResponse(_))
        ));
    }
}
