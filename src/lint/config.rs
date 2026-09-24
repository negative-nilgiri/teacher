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
        }
    }
}

impl LintConfig {
    pub fn from_file(path: &Path) -> Result<Self, Diagnostic> {
        Self::load_with_overrides(Some(path), |_| {})
    }

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
                1.0,
            ),
            ("min_question_ratio", self.min_question_ratio, 1.0),
            (
                "max_choice_length_spread",
                self.max_choice_length_spread,
                f64::MAX,
            ),
        ] {
            if !value.is_finite() || value < 0.0 || value > max {
                return Err(Diagnostic::error(
                    "lint.config.threshold.invalid",
                    "",
                    format!("{key} must be a finite number between 0 and {max}"),
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
}
