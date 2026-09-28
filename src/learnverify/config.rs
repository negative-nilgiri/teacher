use std::fs;
use std::path::Path;

use serde::Deserialize;

use super::checks::Check;
use crate::diagnostics::Diagnostic;

/// Effective `learnverify` settings: defaults, then an explicit TOML file,
/// then CLI flags. The file is flat and rejects unknown keys, like lint's.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(default, deny_unknown_fields)]
pub struct VerifyConfig {
    pub min_info_probability: f64,
    pub min_warning_probability: f64,
    /// Warning threshold for `verify.annotation_contradicts_code`, lower than
    /// the others because a wrong annotation teaches something false.
    pub min_contradiction_warning_probability: f64,
    pub max_context_chars: usize,
    /// Check codes to suppress. `verify.unavailable` is not a check and can
    /// never be suppressed.
    pub ignore_codes: Vec<String>,
}

impl Default for VerifyConfig {
    fn default() -> Self {
        Self {
            min_info_probability: 0.60,
            min_warning_probability: 0.85,
            min_contradiction_warning_probability: 0.70,
            max_context_chars: 6000,
            ignore_codes: Vec::new(),
        }
    }
}

impl VerifyConfig {
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
                "verify.config.read",
                "",
                format!("could not read verify config {}: {error}", path.display()),
            )
        })?;
        toml::from_str(&content).map_err(|error| {
            Diagnostic::error(
                "verify.config.invalid",
                "",
                format!("invalid verify config {}: {error}", path.display()),
            )
        })
    }

    fn validate(&self) -> Result<(), Diagnostic> {
        for (key, value) in [
            ("min_info_probability", self.min_info_probability),
            ("min_warning_probability", self.min_warning_probability),
            (
                "min_contradiction_warning_probability",
                self.min_contradiction_warning_probability,
            ),
        ] {
            if !value.is_finite() || !(0.0..=1.0).contains(&value) {
                return Err(threshold_error(format!(
                    "{key} must be a finite number between 0 and 1"
                )));
            }
        }
        for (key, value) in [
            ("min_warning_probability", self.min_warning_probability),
            (
                "min_contradiction_warning_probability",
                self.min_contradiction_warning_probability,
            ),
        ] {
            if value < self.min_info_probability {
                return Err(threshold_error(format!(
                    "{key} must be at least min_info_probability ({})",
                    self.min_info_probability
                )));
            }
        }
        for code in &self.ignore_codes {
            if Check::from_code(code).is_some() {
                continue;
            }
            let (message, suggestion) = if code == "verify.unavailable" {
                (
                    "verify.unavailable cannot be ignored: it reports checks that did not run"
                        .to_owned(),
                    "Remove it from ignore_codes; a skipped run must stay visible.",
                )
            } else {
                (
                    format!("{code} is not a learnverify check code"),
                    "Copy the exact `code` from a learnverify finding.",
                )
            };
            return Err(
                Diagnostic::error("verify.config.ignore_code.invalid", "", message)
                    .with_suggestion(suggestion),
            );
        }
        Ok(())
    }
}

fn threshold_error(message: String) -> Diagnostic {
    Diagnostic::error("verify.config.threshold.invalid", "", message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn example_file_matches_the_defaults_and_unknown_keys_are_rejected() {
        let example: VerifyConfig =
            toml::from_str(include_str!("../../verify.example.toml")).unwrap();
        assert_eq!(example, VerifyConfig::default());
        let partial: VerifyConfig = toml::from_str("max_context_chars = 10").unwrap();
        assert_eq!(partial.max_context_chars, 10);
        assert_eq!(partial.min_info_probability, 0.60);
        assert!(toml::from_str::<VerifyConfig>("min_info_probabilty = 0.5").is_err());
    }

    #[test]
    fn thresholds_are_bounded_and_ordered() {
        let check = |apply: fn(&mut VerifyConfig)| {
            let mut config = VerifyConfig::default();
            apply(&mut config);
            config.validate().map_err(|error| error.code)
        };
        assert!(check(|_| {}).is_ok());
        assert!(check(|c| c.min_contradiction_warning_probability = 0.60).is_ok());
        for invalid in [
            (|c: &mut VerifyConfig| c.min_info_probability = 1.5) as fn(&mut VerifyConfig),
            |c| c.min_warning_probability = f64::NAN,
            |c| c.min_warning_probability = 0.5,
            |c| c.min_contradiction_warning_probability = 0.59,
        ] {
            assert_eq!(
                check(invalid).unwrap_err(),
                "verify.config.threshold.invalid"
            );
        }
    }

    #[test]
    fn only_check_codes_can_be_ignored() {
        let with = |code: &str| VerifyConfig {
            ignore_codes: vec![code.to_owned()],
            ..VerifyConfig::default()
        };
        assert!(with("verify.implausible_distractor").validate().is_ok());
        for code in ["verify.unavailable", "lint.code.plain_text", "verify.nope"] {
            assert_eq!(
                with(code).validate().unwrap_err().code,
                "verify.config.ignore_code.invalid",
                "{code}"
            );
        }
    }
}
