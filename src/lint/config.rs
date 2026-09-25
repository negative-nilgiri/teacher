use std::fs;
use std::path::Path;

use serde::Deserialize;

use crate::diagnostics::Diagnostic;

#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct LintConfig {
    pub max_code_lines: usize,
    pub highlight_coverage_ratio: f64,
    pub highlight_coverage_min_lines: usize,
    pub many_highlight_ranges: usize,
    pub suggest_highlights_min_lines: usize,
    pub filename_reference_gap: usize,
    pub max_choice_length_spread: f64,
    pub min_choice_length_gap_chars: usize,
    pub min_question_ratio: f64,
    pub max_inline_code_diff_chars: usize,
    pub max_inline_prose_chars: usize,
    /// `info` codes to suppress. Only `info` findings are guesses weak enough
    /// to switch off per rule; stronger findings are filtered by severity.
    pub ignore_codes: Vec<String>,
}

impl Default for LintConfig {
    fn default() -> Self {
        Self {
            max_code_lines: 60,
            highlight_coverage_ratio: 0.70,
            highlight_coverage_min_lines: 20,
            many_highlight_ranges: 3,
            suggest_highlights_min_lines: 30,
            filename_reference_gap: 3,
            max_choice_length_spread: 0.30,
            min_choice_length_gap_chars: 10,
            min_question_ratio: 0.20,
            max_inline_code_diff_chars: 256,
            max_inline_prose_chars: 512,
            ignore_codes: Vec::new(),
        }
    }
}

impl LintConfig {
    pub fn load_with_overrides(
        path: Option<&Path>,
        apply: impl FnOnce(&mut Self),
    ) -> Result<Self, Diagnostic> {
        let mut config = match path {
            Some(path) => Self::parse_file(path)?,
            None => Self::default(),
        };
        apply(&mut config);
        config.validate()?;
        Ok(config)
    }

    fn parse_file(path: &Path) -> Result<Self, Diagnostic> {
        let content = fs::read_to_string(path).map_err(|error| {
            Diagnostic::error(
                "lint.config.read",
                "",
                format!("could not read lint config {}: {error}", path.display()),
            )
        })?;
        toml::from_str(&content).map_err(|error| {
            Diagnostic::error(
                "lint.config.invalid",
                "",
                format!("invalid lint config {}: {error}", path.display()),
            )
        })
    }

    fn validate(&self) -> Result<(), Diagnostic> {
        for code in &self.ignore_codes {
            match super::rules::known_severity(code) {
                Some(super::Severity::Info) => {}
                Some(severity) => {
                    return Err(Diagnostic::error(
                        "lint.config.ignore_code.invalid",
                        "",
                        format!(
                            "{code} is a {} finding; only info codes can be ignored",
                            severity.as_str()
                        ),
                    )
                    .with_suggestion(
                        "Remove it from ignore_codes, or use --ignore-below to hide findings by severity.",
                    ));
                }
                None => {
                    return Err(Diagnostic::error(
                        "lint.config.ignore_code.invalid",
                        "",
                        format!("{code} is not a lint code"),
                    )
                    .with_suggestion("Copy the exact `code` from a lint finding."));
                }
            }
        }
        if self.many_highlight_ranges == 0 {
            return Err(Diagnostic::error(
                "lint.config.threshold.invalid",
                "",
                "many_highlight_ranges must be at least 1",
            ));
        }
        for (key, value, max) in [
            (
                "highlight_coverage_ratio",
                self.highlight_coverage_ratio,
                Some(1.0),
            ),
            ("min_question_ratio", self.min_question_ratio, Some(1.0)),
            (
                "max_choice_length_spread",
                self.max_choice_length_spread,
                None,
            ),
        ] {
            if !value.is_finite() || value < 0.0 || max.is_some_and(|max| value > max) {
                let range = match max {
                    Some(max) => format!("between 0 and {max}"),
                    None => "at least 0".to_owned(),
                };
                return Err(Diagnostic::error(
                    "lint.config.threshold.invalid",
                    "",
                    format!("{key} must be a finite number {range}"),
                ));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_the_design() {
        let config: LintConfig = toml::from_str("max_code_lines = 9").unwrap();
        assert_eq!(config.max_code_lines, 9);
        assert_eq!(config.max_inline_prose_chars, 512);
        assert!(toml::from_str::<LintConfig>("max_code_line = 9").is_err());
        let example: LintConfig =
            toml::from_str(include_str!("../../config.example.toml")).unwrap();
        assert_eq!(example, LintConfig::default());
    }

    #[test]
    fn ignore_codes_accept_only_known_info_codes() {
        let config = |codes: &[&str]| LintConfig {
            ignore_codes: codes.iter().map(|code| (*code).to_owned()).collect(),
            ..LintConfig::default()
        };
        assert!(
            config(&["lint.markdown.unshown_code_reference"])
                .validate()
                .is_ok()
        );
        for code in ["lint.code.plain_text", "lint.diff.new_file", "lint.nope"] {
            let error = config(&[code]).validate().unwrap_err();
            assert_eq!(error.code, "lint.config.ignore_code.invalid", "{code}");
        }
    }
}
