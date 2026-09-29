//! Authoring-policy diagnostics, separate from compiler correctness checks.

mod config;
mod rules;
mod span;

use std::fs;
use std::path::{Path, PathBuf};

use clap::ValueEnum;
use serde::Serialize;

use crate::artifact::CompiledLesson;
use crate::compiler::{CompileOptions, compile};
use crate::diagnostics::Diagnostic;
use crate::source::{self, LessonSource, SchemaVersion};

pub use config::LintConfig;
pub use span::{Position, SourceLocation, SpanIndex};

pub fn lint_file(
    lesson_path: &Path,
    options: &CompileOptions,
    config: &LintConfig,
) -> Result<Vec<LintDiagnostic>, Vec<Diagnostic>> {
    let lesson = load_lesson(lesson_path, options, "learnc lint")?;
    Ok(rules::collect(
        &lesson.source,
        &lesson.artifact,
        &lesson.spans,
        &lesson.root,
        config,
    ))
}

/// A lesson that passed the complete `check` pipeline, with everything needed
/// to point findings at editable source spans.
pub(crate) struct LoadedLesson {
    pub(crate) source: LessonSource,
    pub(crate) artifact: CompiledLesson,
    pub(crate) spans: SpanIndex,
    /// Absolute filesystem root that authored paths are relative to.
    pub(crate) root: PathBuf,
}

/// Read and compile `lesson_path` exactly as `learnc check` does, then index
/// the original JSON for source spans. `command` names the caller in the
/// suggestion given for a non-JSON input.
pub(crate) fn load_lesson(
    lesson_path: &Path,
    options: &CompileOptions,
    command: &str,
) -> Result<LoadedLesson, Vec<Diagnostic>> {
    if lesson_path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(vec![
            Diagnostic::error(
                "compiler.input.not_json",
                "",
                "lesson source must be a .json file; compiled .learn artifacts are not accepted",
            )
            .with_suggestion(format!("Pass the authored lesson JSON to `{command}`.")),
        ]);
    }
    let input = fs::read_to_string(lesson_path).map_err(|error| {
        vec![Diagnostic::error(
            "compiler.input.read",
            "",
            format!("could not read lesson source: {error}"),
        )]
    })?;
    let artifact = compile(&input, &options.clone().with_lesson_path(lesson_path))?;
    let (source, _) = source::parse_and_validate(&input)
        .expect("compilation already validated the same source text")
        .into_parts();
    let absolute_lesson = if lesson_path.is_absolute() {
        lesson_path.to_path_buf()
    } else {
        options.current_dir.join(lesson_path)
    };
    let spans = SpanIndex::new(&input, absolute_lesson)
        .map_err(|message| vec![Diagnostic::error("lint.source_span.internal", "", message)])?;
    let root = options.root.as_deref().unwrap_or(&options.current_dir);
    let root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        options.current_dir.join(root)
    };
    Ok(LoadedLesson {
        source,
        artifact,
        spans,
        root,
    })
}

/// Pointer to a quiz prompt's editable text. Schemas before 2.0.0 author the
/// prompt as a plain string rather than a Markdown source object.
pub(crate) fn prompt_pointer(schema_version: SchemaVersion, index: usize) -> String {
    if matches!(
        schema_version,
        SchemaVersion::V1_0_0
            | SchemaVersion::V1_1_0
            | SchemaVersion::V1_2_0
            | SchemaVersion::V1_3_0
    ) {
        format!("/blocks/{index}/prompt")
    } else {
        format!("/blocks/{index}/prompt/content")
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, ValueEnum)]
#[serde(rename_all = "snake_case")]
#[clap(rename_all = "snake_case")]
pub enum Severity {
    Error,
    Critical,
    Warning,
    Info,
}

impl Severity {
    pub(crate) const fn rank(self) -> u8 {
        match self {
            Self::Error => 4,
            Self::Critical => 3,
            Self::Warning => 2,
            Self::Info => 1,
        }
    }

    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Error => "error",
            Self::Critical => "critical",
            Self::Warning => "warning",
            Self::Info => "info",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RelatedLintLocation {
    pub message: String,
    pub location: SourceLocation,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LintDiagnostic {
    pub code: String,
    pub severity: Severity,
    pub fatal: bool,
    pub message: String,
    pub location: SourceLocation,
    /// `null` for findings about the complete lesson.
    pub block_id: Option<String>,
    pub pointer: String,
    pub suggestion: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub related: Vec<RelatedLintLocation>,
}

impl LintDiagnostic {
    pub fn new(
        code: &'static str,
        severity: Severity,
        message: impl Into<String>,
        location: SourceLocation,
        block_id: Option<&str>,
        pointer: impl Into<String>,
        suggestion: impl Into<String>,
    ) -> Self {
        Self {
            code: code.to_owned(),
            severity,
            fatal: false,
            message: message.into(),
            location,
            block_id: block_id.map(str::to_owned),
            pointer: pointer.into(),
            suggestion: suggestion.into(),
            related: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct LintReport {
    pub diagnostics: Vec<LintDiagnostic>,
}

impl LintReport {
    pub fn from_findings(
        findings: Vec<LintDiagnostic>,
        ignore_below: Option<Severity>,
        warning_as_error: Severity,
    ) -> Self {
        let diagnostics = findings
            .into_iter()
            .filter(|finding| {
                ignore_below.is_none_or(|floor| finding.severity.rank() >= floor.rank())
            })
            .map(|mut finding| {
                finding.fatal = finding.severity.rank() >= warning_as_error.rank();
                finding
            })
            .collect();
        Self { diagnostics }
    }

    pub fn is_fatal(&self) -> bool {
        self.diagnostics.iter().any(|finding| finding.fatal)
    }

    pub fn text(&self) -> String {
        let mut output = String::new();
        for finding in &self.diagnostics {
            write_finding_text(&mut output, finding, None);
        }
        output
    }
}

/// Render one finding as Rustc-like text, with an optional `= note:` line
/// before its suggestion.
pub(crate) fn write_finding_text(
    output: &mut String,
    finding: &LintDiagnostic,
    note: Option<&str>,
) {
    output.push_str(&format!(
        "{}[{}]: {}\n --> {}:{}:{}\n",
        finding.severity.as_str(),
        finding.code,
        finding.message,
        finding.location.path,
        finding.location.start.line,
        finding.location.start.column,
    ));
    if let Ok(source) = fs::read_to_string(&finding.location.path)
        && let Some(line) = source.lines().nth(finding.location.start.line - 1)
    {
        let width = finding
            .location
            .end
            .column
            .saturating_sub(finding.location.start.column)
            .clamp(1, 80);
        let number = finding.location.start.line.to_string();
        let gutter = " ".repeat(number.len());
        output.push_str(&format!(
            "{gutter} |\n{number} | {}\n{gutter} | {}{}\n",
            line,
            " ".repeat(finding.location.start.column.saturating_sub(1)),
            "^".repeat(width),
        ));
    }
    for related in &finding.related {
        output.push_str(&format!(
            "  = related: {} at {}:{}:{}\n",
            related.message,
            related.location.path,
            related.location.start.line,
            related.location.start.column,
        ));
    }
    if let Some(note) = note {
        output.push_str(&format!("  = note: {note}\n"));
    }
    output.push_str(&format!("  = help: {}\n", finding.suggestion));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filtering_precedes_fatality() {
        let location = SourceLocation::file_line("lesson.json".into(), 1, 1, 2);
        let findings = vec![
            LintDiagnostic::new("a", Severity::Warning, "a", location.clone(), None, "", "a"),
            LintDiagnostic::new("b", Severity::Critical, "b", location, None, "", "b"),
        ];
        let report =
            LintReport::from_findings(findings, Some(Severity::Critical), Severity::Warning);
        assert_eq!(report.diagnostics.len(), 1);
        assert_eq!(report.diagnostics[0].code, "b");
        assert!(report.is_fatal());
    }
}
