use std::fmt;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use super::SourceId;

/// Source schema decoder selected by the authored document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SchemaVersion {
    #[serde(rename = "1.0.0")]
    #[schemars(rename = "1.0.0")]
    V1_0_0,
    #[serde(rename = "1.1.0")]
    #[schemars(rename = "1.1.0")]
    V1_1_0,
    #[serde(rename = "1.2.0")]
    #[schemars(rename = "1.2.0")]
    V1_2_0,
    #[serde(rename = "1.3.0")]
    #[schemars(rename = "1.3.0")]
    V1_3_0,
    #[serde(rename = "2.0.0")]
    #[schemars(rename = "2.0.0")]
    V2_0_0,
}

impl SchemaVersion {
    pub const CURRENT: Self = Self::V2_0_0;
    pub const SUPPORTED: [Self; 5] = [
        Self::V1_0_0,
        Self::V1_1_0,
        Self::V1_2_0,
        Self::V1_3_0,
        Self::V2_0_0,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::V1_0_0 => "1.0.0",
            Self::V1_1_0 => "1.1.0",
            Self::V1_2_0 => "1.2.0",
            Self::V1_3_0 => "1.3.0",
            Self::V2_0_0 => "2.0.0",
        }
    }
}

impl fmt::Display for SchemaVersion {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LessonSource {
    pub schema_version: SchemaVersion,
    /// Non-whitespace lesson title.
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub title: String,
    /// Ordered lesson blocks. Every block source ID must be document-unique.
    pub blocks: Vec<Block>,
}

/// Exact decoder/schema model for the original source format.
///
/// `language` was introduced in source schema 1.1.0, `caption` in 1.2.0, and
/// `highlights` in 1.3.0, and file-backed quiz prompts in 2.0.0, so older blocks
/// intentionally remain separate closed shapes. Every versioned wire model
/// lowers into the same internal [`LessonSource`] representation.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "LessonSource")]
pub(crate) struct LessonSourceV1_0_0 {
    schema_version: SchemaVersionV1_0_0,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    title: String,
    blocks: Vec<BlockV1_0_0>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
enum SchemaVersionV1_0_0 {
    #[serde(rename = "1.0.0")]
    #[schemars(rename = "1.0.0")]
    V1_0_0,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BlockV1_0_0 {
    Markdown(MarkdownBlock),
    Code(CodeBlockV1_0_0),
    Diff(DiffBlockBeforeV1_2_0),
    MultipleChoice(MultipleChoiceBlockBeforeV2_0_0),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeBlockV1_0_0 {
    id: SourceId,
    source: CodeSource,
}

impl From<LessonSourceV1_0_0> for LessonSource {
    fn from(source: LessonSourceV1_0_0) -> Self {
        let LessonSourceV1_0_0 {
            schema_version: _,
            title,
            blocks,
        } = source;
        Self {
            schema_version: SchemaVersion::V1_0_0,
            title,
            blocks: blocks.into_iter().map(Block::from).collect(),
        }
    }
}

impl From<BlockV1_0_0> for Block {
    fn from(block: BlockV1_0_0) -> Self {
        match block {
            BlockV1_0_0::Markdown(block) => Self::Markdown(block),
            BlockV1_0_0::Code(block) => Self::Code(CodeBlock {
                id: block.id,
                language: None,
                caption: None,
                highlights: Vec::new(),
                source: block.source,
            }),
            BlockV1_0_0::Diff(block) => Self::Diff(block.into()),
            BlockV1_0_0::MultipleChoice(block) => Self::MultipleChoice(block.into()),
        }
    }
}

/// Exact decoder/schema model for source schema 1.1.0.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "LessonSource")]
pub(crate) struct LessonSourceV1_1_0 {
    schema_version: SchemaVersionV1_1_0,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    title: String,
    blocks: Vec<BlockV1_1_0>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
enum SchemaVersionV1_1_0 {
    #[serde(rename = "1.1.0")]
    #[schemars(rename = "1.1.0")]
    V1_1_0,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BlockV1_1_0 {
    Markdown(MarkdownBlock),
    Code(CodeBlockV1_1_0),
    Diff(DiffBlockBeforeV1_2_0),
    MultipleChoice(MultipleChoiceBlockBeforeV2_0_0),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeBlockV1_1_0 {
    id: SourceId,
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    language: Option<String>,
    source: CodeSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct DiffBlockBeforeV1_2_0 {
    id: SourceId,
    source: DiffSource,
}

impl From<LessonSourceV1_1_0> for LessonSource {
    fn from(source: LessonSourceV1_1_0) -> Self {
        let LessonSourceV1_1_0 {
            schema_version: _,
            title,
            blocks,
        } = source;
        Self {
            schema_version: SchemaVersion::V1_1_0,
            title,
            blocks: blocks.into_iter().map(Block::from).collect(),
        }
    }
}

impl From<BlockV1_1_0> for Block {
    fn from(block: BlockV1_1_0) -> Self {
        match block {
            BlockV1_1_0::Markdown(block) => Self::Markdown(block),
            BlockV1_1_0::Code(block) => Self::Code(CodeBlock {
                id: block.id,
                language: block.language,
                caption: None,
                highlights: Vec::new(),
                source: block.source,
            }),
            BlockV1_1_0::Diff(block) => Self::Diff(block.into()),
            BlockV1_1_0::MultipleChoice(block) => Self::MultipleChoice(block.into()),
        }
    }
}

impl From<DiffBlockBeforeV1_2_0> for DiffBlock {
    fn from(block: DiffBlockBeforeV1_2_0) -> Self {
        Self {
            id: block.id,
            caption: None,
            source: block.source,
        }
    }
}

/// Exact decoder/schema model for source schema 1.2.0.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "LessonSource")]
pub(crate) struct LessonSourceV1_2_0 {
    schema_version: SchemaVersionV1_2_0,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    title: String,
    blocks: Vec<BlockV1_2_0>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
enum SchemaVersionV1_2_0 {
    #[serde(rename = "1.2.0")]
    #[schemars(rename = "1.2.0")]
    V1_2_0,
}

impl From<LessonSourceV1_2_0> for LessonSource {
    fn from(source: LessonSourceV1_2_0) -> Self {
        Self {
            schema_version: SchemaVersion::V1_2_0,
            title: source.title,
            blocks: source.blocks.into_iter().map(Block::from).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BlockV1_2_0 {
    Markdown(MarkdownBlock),
    Code(CodeBlockV1_2_0),
    Diff(DiffBlock),
    MultipleChoice(MultipleChoiceBlockBeforeV2_0_0),
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct CodeBlockV1_2_0 {
    id: SourceId,
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    language: Option<String>,
    #[serde(default)]
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    caption: Option<String>,
    source: CodeSource,
}

impl From<BlockV1_2_0> for Block {
    fn from(block: BlockV1_2_0) -> Self {
        match block {
            BlockV1_2_0::Markdown(block) => Self::Markdown(block),
            BlockV1_2_0::Code(block) => Self::Code(CodeBlock {
                id: block.id,
                language: block.language,
                caption: block.caption,
                highlights: Vec::new(),
                source: block.source,
            }),
            BlockV1_2_0::Diff(block) => Self::Diff(block),
            BlockV1_2_0::MultipleChoice(block) => Self::MultipleChoice(block.into()),
        }
    }
}

/// Exact decoder/schema model for source schema 1.3.0.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "LessonSource")]
pub(crate) struct LessonSourceV1_3_0 {
    schema_version: SchemaVersionV1_3_0,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    title: String,
    blocks: Vec<BlockV1_3_0>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
enum SchemaVersionV1_3_0 {
    #[serde(rename = "1.3.0")]
    #[schemars(rename = "1.3.0")]
    V1_3_0,
}

impl From<LessonSourceV1_3_0> for LessonSource {
    fn from(source: LessonSourceV1_3_0) -> Self {
        Self {
            schema_version: SchemaVersion::V1_3_0,
            title: source.title,
            blocks: source.blocks.into_iter().map(Block::from).collect(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum BlockV1_3_0 {
    Markdown(MarkdownBlock),
    Code(CodeBlock),
    Diff(DiffBlock),
    MultipleChoice(MultipleChoiceBlockBeforeV2_0_0),
}

impl From<BlockV1_3_0> for Block {
    fn from(block: BlockV1_3_0) -> Self {
        match block {
            BlockV1_3_0::Markdown(block) => Self::Markdown(block),
            BlockV1_3_0::Code(block) => Self::Code(block),
            BlockV1_3_0::Diff(block) => Self::Diff(block),
            BlockV1_3_0::MultipleChoice(block) => Self::MultipleChoice(block.into()),
        }
    }
}

/// Exact decoder/schema model for source schema 2.0.0.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "LessonSource")]
pub(crate) struct LessonSourceV2_0_0 {
    schema_version: SchemaVersionV2_0_0,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    title: String,
    blocks: Vec<Block>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
enum SchemaVersionV2_0_0 {
    #[serde(rename = "2.0.0")]
    #[schemars(rename = "2.0.0")]
    V2_0_0,
}

impl From<LessonSourceV2_0_0> for LessonSource {
    fn from(source: LessonSourceV2_0_0) -> Self {
        Self {
            schema_version: SchemaVersion::V2_0_0,
            title: source.title,
            blocks: source.blocks,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Block {
    Markdown(MarkdownBlock),
    Code(CodeBlock),
    Diff(DiffBlock),
    MultipleChoice(MultipleChoiceBlock),
}

impl Block {
    pub fn id(&self) -> &SourceId {
        match self {
            Self::Markdown(block) => &block.id,
            Self::Code(block) => &block.id,
            Self::Diff(block) => &block.id,
            Self::MultipleChoice(block) => &block.id,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MarkdownBlock {
    pub id: SourceId,
    pub source: MarkdownSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeBlock {
    pub id: SourceId,
    /// Optional language name or common alias. Unknown values safely render as text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub language: Option<String>,
    /// Optional Markdown for non-obvious, block-specific explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub caption: Option<String>,
    /// Optional attention ranges for file-backed code. Inline code cannot be highlighted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub highlights: Vec<CodeHighlight>,
    pub source: CodeSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct CodeHighlight {
    /// One or more one-based, inclusive ranges in the original source file.
    #[schemars(length(min = 1))]
    pub lines: Vec<LineRange>,
    /// Pastel presentation color. Omission defaults to yellow.
    #[serde(default, skip_serializing_if = "HighlightColor::is_default")]
    pub color: HighlightColor,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum HighlightColor {
    #[default]
    Yellow,
    Green,
    Red,
    Blue,
}

impl HighlightColor {
    fn is_default(&self) -> bool {
        *self == Self::Yellow
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct DiffBlock {
    pub id: SourceId,
    /// Optional Markdown for non-obvious, block-specific explanation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub caption: Option<String>,
    pub source: DiffSource,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct MultipleChoiceBlock {
    pub id: SourceId,
    /// Markdown prompt shown before the choices.
    pub prompt: MarkdownSource,
    /// At least two Markdown choices, exactly one of which must set `correct`.
    #[schemars(length(min = 2))]
    pub choices: Vec<Choice>,
    /// Markdown hints included in public presentation data.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[schemars(inner(length(min = 1), regex(pattern = r"\S")))]
    pub hints: Vec<String>,
    /// Markdown shown only after a correct answer or explicit reveal.
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub explanation: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct MultipleChoiceBlockBeforeV2_0_0 {
    id: SourceId,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    prompt: String,
    #[schemars(length(min = 2))]
    choices: Vec<Choice>,
    #[serde(default)]
    #[schemars(inner(length(min = 1), regex(pattern = r"\S")))]
    hints: Vec<String>,
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    explanation: String,
}

impl From<MultipleChoiceBlockBeforeV2_0_0> for MultipleChoiceBlock {
    fn from(block: MultipleChoiceBlockBeforeV2_0_0) -> Self {
        Self {
            id: block.id,
            prompt: MarkdownSource::Inline {
                content: block.prompt,
            },
            choices: block.choices,
            hints: block.hints,
            explanation: block.explanation,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Choice {
    /// Markdown content. Choice IDs are generated by the compiler.
    #[schemars(length(min = 1), regex(pattern = r"\S"))]
    pub content: String,
    /// Omission means false. The compiler removes this from presentation data.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub correct: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MarkdownSource {
    Inline {
        #[schemars(length(min = 1), regex(pattern = r"\S"))]
        content: String,
    },
    File {
        path: RepoPath,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CodeSource {
    Inline {
        #[schemars(length(min = 1), regex(pattern = r"\S"))]
        content: String,
    },
    File {
        path: RepoPath,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lines: Option<LineRange>,
    },
    GitBlob {
        revision: GitRevision,
        path: RepoPath,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        lines: Option<LineRange>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DiffSource {
    Inline {
        #[schemars(length(min = 1), regex(pattern = r"\S"))]
        content: String,
    },
    File {
        path: RepoPath,
    },
    Git {
        base: GitRevision,
        target: GitDiffTarget,
        /// Nonempty file selections. A path may appear only once.
        #[schemars(length(min = 1))]
        files: Vec<GitDiffFile>,
        context_lines: u16,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitDiffTarget {
    Revision { revision: GitRevision },
    Worktree,
}

impl<'de> Deserialize<'de> for GitDiffTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        // Serde's internally tagged unit variants ignore extra map fields even
        // when the enum denies unknown fields. Decode the unit-shaped public
        // variant through an empty struct variant so the source contract stays
        // closed without changing the public `GitDiffTarget::Worktree` API.
        #[derive(Deserialize)]
        #[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
        enum StrictGitDiffTarget {
            Revision { revision: GitRevision },
            Worktree {},
        }

        match StrictGitDiffTarget::deserialize(deserializer)? {
            StrictGitDiffTarget::Revision { revision } => Ok(Self::Revision { revision }),
            StrictGitDiffTarget::Worktree {} => Ok(Self::Worktree),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct GitDiffFile {
    pub path: RepoPath,
    /// One-based inclusive range in the base-side file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before_lines: Option<LineRange>,
    /// One-based inclusive range in the target-side file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after_lines: Option<LineRange>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct LineRange {
    /// First selected line, one-based and inclusive.
    #[schemars(range(min = 1))]
    pub start: u32,
    /// Last selected line, one-based and inclusive; must not precede `start`.
    #[schemars(range(min = 1))]
    pub end: u32,
}

/// A selected-root-relative path in the platform-neutral source language.
///
/// Constraints are checked by the semantic validation pass so failures can be
/// reported alongside a precise JSON Pointer.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct RepoPath(
    #[schemars(
        length(min = 1),
        regex(
            pattern = r"^(?!/)(?![A-Za-z]:)(?!.*\\)(?!.*//)(?!.*\/$)(?!\.{1,2}(?:/|$))(?!.*\/\.{1,2}(?:/|$))[^\u0000-\u001F\u007F]+$"
        )
    )]
    String,
);

impl RepoPath {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        validate_repo_path(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validation_error(&self) -> Option<&'static str> {
        validate_repo_path(&self.0).err()
    }
}

fn validate_repo_path(path: &str) -> Result<(), &'static str> {
    if path.is_empty() {
        return Err("must not be empty");
    }
    if path.starts_with('/') {
        return Err("must be relative to the selected filesystem root");
    }
    if path.contains('\\') {
        return Err("must use forward slashes as separators");
    }
    if path.contains('\0') || path.chars().any(|character| character.is_control()) {
        return Err("must not contain control characters");
    }
    if path.len() >= 2 && path.as_bytes()[0].is_ascii_alphabetic() && path.as_bytes()[1] == b':' {
        return Err("must not have a Windows drive prefix");
    }
    if path.split('/').any(|component| component.is_empty()) {
        return Err("must not contain empty path components");
    }
    if path
        .split('/')
        .any(|component| component == "." || component == "..")
    {
        return Err("must not contain `.` or `..` path components");
    }
    Ok(())
}

/// Symbolic Git revision expression, resolved and frozen during compilation.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(transparent)]
pub struct GitRevision(
    #[schemars(
        length(min = 1),
        regex(pattern = r"^(?!-)(?=\S)(?=.*\S$)[^\u0000-\u001F\u007F]+$")
    )]
    String,
);

impl GitRevision {
    pub fn new(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        validate_git_revision(&value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub(crate) fn validation_error(&self) -> Option<&'static str> {
        validate_git_revision(&self.0).err()
    }
}

fn validate_git_revision(revision: &str) -> Result<(), &'static str> {
    if revision.is_empty() {
        return Err("must not be empty");
    }
    if revision.trim() != revision {
        return Err("must not start or end with whitespace");
    }
    if revision.starts_with('-') {
        return Err("must not start with `-`");
    }
    if revision.chars().any(char::is_control) {
        return Err("must not contain control characters");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_are_platform_neutral_and_root_relative() {
        assert!(RepoPath::new("src/queue.rs").is_ok());
        assert!(RepoPath::new("../secret").is_err());
        assert!(RepoPath::new("/etc/passwd").is_err());
        assert!(RepoPath::new("C:/repo/file").is_err());
        assert!(RepoPath::new("src\\queue.rs").is_err());
    }

    #[test]
    fn tagged_block_shape_round_trips() {
        let json = r#"{
            "type":"markdown",
            "id":"intro",
            "source":{"kind":"inline","content":"Hello"}
        }"#;
        let block: Block = serde_json::from_str(json).expect("valid block");
        assert_eq!(block.id().as_str(), "intro");
        let value = serde_json::to_value(block).expect("serializable");
        assert_eq!(value["type"], "markdown");
        assert_eq!(value["source"]["kind"], "inline");
    }

    #[test]
    fn choices_do_not_accept_authored_ids() {
        let json = r#"{"content":"A","id":"authored","correct":true}"#;
        assert!(serde_json::from_str::<Choice>(json).is_err());
    }

    #[test]
    fn markdown_source_variants_reject_inapplicable_fields() {
        for value in [
            serde_json::json!({"kind": "inline", "content": "text", "path": "README.md"}),
            serde_json::json!({"kind": "file", "path": "README.md", "content": "text"}),
        ] {
            assert!(
                serde_json::from_value::<MarkdownSource>(value.clone()).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }

    #[test]
    fn code_source_variants_reject_inapplicable_fields() {
        for value in [
            serde_json::json!({"kind": "inline", "content": "let x = 1;", "path": "src/lib.rs"}),
            serde_json::json!({"kind": "file", "path": "src/lib.rs", "revision": "HEAD"}),
            serde_json::json!({
                "kind": "git_blob",
                "revision": "HEAD",
                "path": "src/lib.rs",
                "content": "let x = 1;"
            }),
        ] {
            assert!(
                serde_json::from_value::<CodeSource>(value.clone()).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }

    #[test]
    fn diff_source_variants_reject_inapplicable_fields() {
        for value in [
            serde_json::json!({"kind": "inline", "content": "diff --git", "path": "change.patch"}),
            serde_json::json!({"kind": "file", "path": "change.patch", "content": "diff --git"}),
            serde_json::json!({
                "kind": "git",
                "base": "HEAD~1",
                "target": {"kind": "worktree"},
                "files": [{"path": "src/lib.rs"}],
                "context_lines": 3,
                "content": "diff --git"
            }),
        ] {
            assert!(
                serde_json::from_value::<DiffSource>(value.clone()).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }

    #[test]
    fn git_diff_target_variants_reject_inapplicable_fields() {
        for value in [
            serde_json::json!({"kind": "worktree", "revision": "HEAD"}),
            serde_json::json!({"kind": "revision", "revision": "HEAD", "worktree": true}),
        ] {
            assert!(
                serde_json::from_value::<GitDiffTarget>(value.clone()).is_err(),
                "unexpectedly accepted {value}"
            );
        }
    }
}
