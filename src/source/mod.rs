//! Authored lesson source model and its validation boundary.
//!
//! This module deliberately stops before resolving files, repositories, Git
//! revisions, or unified patches. Those operations belong to compilation.

mod ids;
mod model;
mod schema;
mod validate;

pub use ids::{NodeId, SourceId, SourceIdError, SymbolTable};
pub use model::{
    Block, Choice, CodeBlock, CodeHighlight, CodeSource, DiffBlock, DiffSource, GitDiffFile,
    GitDiffTarget, GitRevision, HighlightColor, LessonSource, LineRange, MarkdownBlock,
    MarkdownSource, MultipleChoiceBlock, RepoPath, SchemaVersion,
};
pub use schema::{source_json_schema, source_json_schema_for};
pub use validate::{ValidatedLesson, parse_and_validate, validate};
