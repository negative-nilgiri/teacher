use std::collections::HashMap;
use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Human-readable identity authored by an agent.
///
/// Deserialization intentionally does not enforce the semantic constraints: the
/// validation pass does so with an actionable JSON Pointer and can collect
/// duplicate-ID errors alongside other independent errors.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct SourceId(
    #[schemars(
        length(min = 1),
        regex(pattern = r"^(?=\S)(?=.*\S$)[^\u0000-\u001F\u007F-\u009F]+$")
    )]
    String,
);

impl SourceId {
    pub fn new(value: impl Into<String>) -> Result<Self, SourceIdError> {
        let value = value.into();
        Self::validate_str(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validate(&self) -> Result<(), SourceIdError> {
        Self::validate_str(&self.0)
    }

    fn validate_str(value: &str) -> Result<(), SourceIdError> {
        if value.is_empty() {
            return Err(SourceIdError::Empty);
        }
        if value.trim() != value {
            return Err(SourceIdError::SurroundingWhitespace);
        }
        if value.chars().any(char::is_control) {
            return Err(SourceIdError::ControlCharacter);
        }
        Ok(())
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceIdError {
    Empty,
    SurroundingWhitespace,
    ControlCharacter,
}

impl fmt::Display for SourceIdError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => formatter.write_str("must not be empty"),
            Self::SurroundingWhitespace => {
                formatter.write_str("must not start or end with whitespace")
            }
            Self::ControlCharacter => formatter.write_str("must not contain control characters"),
        }
    }
}

impl std::error::Error for SourceIdError {}

/// Dense, artifact-local identity generated in authored block order.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema,
)]
#[serde(transparent)]
pub struct NodeId(u32);

impl NodeId {
    pub const fn new(value: u32) -> Self {
        Self(value)
    }

    pub const fn get(self) -> u32 {
        self.0
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.0)
    }
}

/// Deterministic source-name to runtime-ID mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SymbolTable {
    source_to_node: HashMap<SourceId, NodeId>,
    node_to_source: Vec<SourceId>,
}

impl SymbolTable {
    pub(crate) fn from_unique_ids<'a>(ids: impl IntoIterator<Item = &'a SourceId>) -> Self {
        let node_to_source: Vec<_> = ids.into_iter().cloned().collect();
        debug_assert!(u32::try_from(node_to_source.len()).is_ok());
        let source_to_node = node_to_source
            .iter()
            .cloned()
            .enumerate()
            .map(|(index, id)| (id, NodeId(index as u32)))
            .collect();
        Self {
            source_to_node,
            node_to_source,
        }
    }

    pub fn node_id(&self, source_id: &SourceId) -> Option<NodeId> {
        self.source_to_node.get(source_id).copied()
    }

    pub fn source_id(&self, node_id: NodeId) -> Option<&SourceId> {
        self.node_to_source.get(node_id.get() as usize)
    }

    pub fn len(&self) -> usize {
        self.node_to_source.len()
    }

    pub fn is_empty(&self) -> bool {
        self.node_to_source.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_ids_allow_human_readable_unicode() {
        let id = SourceId::new("queue behavior — overview").expect("valid ID");
        assert_eq!(id.as_str(), "queue behavior — overview");
    }

    #[test]
    fn source_ids_reject_context_independent_hazards_only() {
        assert_eq!(SourceId::new(""), Err(SourceIdError::Empty));
        assert_eq!(
            SourceId::new(" queue"),
            Err(SourceIdError::SurroundingWhitespace)
        );
        assert_eq!(
            SourceId::new("queue\nbehavior"),
            Err(SourceIdError::ControlCharacter)
        );
    }

    #[test]
    fn symbol_table_is_dense_and_ordered() {
        let first = SourceId::new("first").unwrap();
        let second = SourceId::new("second").unwrap();
        let symbols = SymbolTable::from_unique_ids([&first, &second]);
        assert_eq!(symbols.node_id(&first), Some(NodeId::new(0)));
        assert_eq!(symbols.node_id(&second), Some(NodeId::new(1)));
        assert_eq!(symbols.source_id(NodeId::new(1)), Some(&second));
    }
}
