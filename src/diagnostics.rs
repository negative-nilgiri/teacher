use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// One actionable problem found while reading or validating lesson source.
///
/// `pointer` and related-location pointers are RFC 6901 JSON Pointers. The
/// empty string identifies the whole document.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Diagnostic {
    /// Stable, machine-readable identifier intended for agent branching.
    pub code: String,
    /// RFC 6901 pointer to the value primarily responsible for this problem.
    pub pointer: String,
    /// Human-readable explanation of the problem.
    pub message: String,
    /// Other source locations relevant to the problem.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedLocation>,
    /// Safe, concrete ways to fix the problem. No fix is applied automatically.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggestions: Vec<String>,
}

impl Diagnostic {
    pub fn error(
        code: impl Into<String>,
        pointer: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            code: code.into(),
            pointer: pointer.into(),
            message: message.into(),
            related: Vec::new(),
            suggestions: Vec::new(),
        }
    }

    pub fn with_related(mut self, pointer: impl Into<String>, message: impl Into<String>) -> Self {
        self.related.push(RelatedLocation {
            pointer: pointer.into(),
            message: message.into(),
        });
        self
    }

    pub fn with_suggestion(mut self, suggestion: impl Into<String>) -> Self {
        self.suggestions.push(suggestion.into());
        self
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct RelatedLocation {
    /// RFC 6901 pointer to the related value.
    pub pointer: String,
    pub message: String,
}

/// Accumulates independent diagnostics so an author can repair them in one pass.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DiagnosticBag {
    diagnostics: Vec<Diagnostic>,
}

impl DiagnosticBag {
    pub fn push(&mut self, diagnostic: Diagnostic) {
        self.diagnostics.push(diagnostic);
    }

    pub fn is_empty(&self) -> bool {
        self.diagnostics.is_empty()
    }

    pub fn into_vec(self) -> Vec<Diagnostic> {
        self.diagnostics
    }
}

/// Escapes one path segment according to RFC 6901.
pub fn escape_json_pointer_segment(segment: &str) -> String {
    segment.replace('~', "~0").replace('/', "~1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pointer_segments_are_rfc_6901_escaped() {
        assert_eq!(escape_json_pointer_segment("a~/b"), "a~0~1b");
    }

    #[test]
    fn empty_optional_fields_are_not_serialized() {
        let value = serde_json::to_value(Diagnostic::error("source.test", "/x", "bad"))
            .expect("diagnostic serializes");
        assert!(value.get("related").is_none());
        assert!(value.get("suggestions").is_none());
    }
}
