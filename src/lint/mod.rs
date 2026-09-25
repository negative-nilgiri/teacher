//! Authoring-policy diagnostics, separate from compiler correctness checks.

mod config;
mod rules;
mod span;

use std::fs;
use std::path::Path;

use clap::ValueEnum;
use serde::Serialize;

use crate::compiler::{CompileOptions, compile};
use crate::diagnostics::Diagnostic;
use crate::source;

pub use config::LintConfig;
pub use span::{Position, SourceLocation, SpanIndex};

pub fn lint_file(
    lesson_path: &Path,
    options: &CompileOptions,
    config: &LintConfig,
) -> Result<Vec<LintDiagnostic>, Vec<Diagnostic>> {
    if lesson_path.extension().and_then(|value| value.to_str()) != Some("json") {
        return Err(vec![
            Diagnostic::error(
                "compiler.input.not_json",
                "",
                "lesson source must be a .json file; compiled .learn artifacts are not accepted",
            )
            .with_suggestion("Pass the authored lesson JSON to `learnc lint`."),
        ]);
    }
    let input = fs::read_to_string(lesson_path).map_err(|error| {
        vec![Diagnostic::error(
            "compiler.input.read",
            "",
            format!("could not read lesson source: {error}"),
        )]
    })?;
    let artifact = compile(&input, options)?;
    let source = source::parse_and_validate(&input)
        .expect("compilation already validated the same source text");
    let absolute_lesson = if lesson_path.is_absolute() {
        lesson_path.to_path_buf()
    } else {
        options.current_dir.join(lesson_path)
    };
    let spans = SpanIndex::new(&input, absolute_lesson)
        .map_err(|message| vec![Diagnostic::error("lint.source_span.internal", "", message)])?;
    let root = options.root.as_deref().unwrap_or(&options.current_dir);
    let absolute_root = if root.is_absolute() {
        root.to_path_buf()
    } else {
        options.current_dir.join(root)
    };
    Ok(rules::collect(
        source.source(),
        &artifact,
        &spans,
        &absolute_root,
        config,
    ))
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
    const fn rank(self) -> u8 {
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
            output.push_str(&format!("  = help: {}\n", finding.suggestion));
        }
        output
    }
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
