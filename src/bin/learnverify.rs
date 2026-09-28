//! Optional agent-facing semantic checker for compiled lessons.

use std::path::PathBuf;
use std::process::ExitCode;

use agent_teacher::cli::{help_document_for, output_request_from, version_document};
use agent_teacher::compiler::CompileOptions;
use agent_teacher::diagnostics::Diagnostic;
use agent_teacher::learnverify::{VerifyConfig, VerifyReport, verify_file};
use agent_teacher::lint::Severity;
use clap::{Args, CommandFactory, Parser, error::ErrorKind};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "learnverify",
    version,
    about = "Ask the TypeSafe API about likely semantic mistakes in a lesson's quizzes and highlights",
    long_about = "Ask the TypeSafe API about likely semantic mistakes in a lesson's quizzes and highlights.\n\nThe lesson is compiled exactly like `learnc check` first. Quiz text with the nearby lesson content (Markdown, code, and diff excerpts), and highlighted code blocks with their annotations, caption, and adjacent Markdown, are sent to TypeSafe. The API key is read only from TYPESAFE_API_KEY and sent only in the Authorization header. Findings are advisory and never change whether the lesson compiles."
)]
struct Cli {
    /// Print concise human-readable output instead of the default JSON.
    #[arg(short = 't', long)]
    text: bool,
    /// Authored JSON lesson document.
    lesson: PathBuf,
    /// Filesystem root used to resolve relative lesson paths.
    #[arg(long)]
    root: Option<PathBuf>,
    /// Explicit TOML file overriding verify thresholds (separate from the lint config).
    #[arg(long)]
    config: Option<PathBuf>,
    #[command(flatten)]
    overrides: Overrides,
    /// Lowest category that causes a nonzero exit status; verify.unavailable never does.
    #[arg(long, default_value = "error")]
    warning_as_error: Severity,
    /// Omit findings below this category; verify.unavailable is always kept.
    #[arg(long)]
    ignore_below: Option<Severity>,
    /// Neither read nor write cached answers.
    #[arg(long)]
    no_cache: bool,
}

#[derive(Debug, Args)]
struct Overrides {
    /// Problem probability from which a finding is reported (0 to 1).
    #[arg(long)]
    min_info_probability: Option<f64>,
    /// Problem probability from which a finding is a warning (0 to 1).
    #[arg(long)]
    min_warning_probability: Option<f64>,
    /// Warning threshold for verify.annotation_contradicts_code (0 to 1).
    #[arg(long)]
    min_contradiction_warning_probability: Option<f64>,
    /// Maximum characters of surrounding lesson content sent per request.
    #[arg(long)]
    max_context_chars: Option<usize>,
    /// Suppress one check code; repeat for several. Adds to ignore_codes.
    #[arg(long = "ignore-code", value_name = "CODE")]
    ignore_codes: Vec<String>,
}

impl Overrides {
    fn apply(self, config: &mut VerifyConfig) {
        if let Some(value) = self.min_info_probability {
            config.min_info_probability = value;
        }
        if let Some(value) = self.min_warning_probability {
            config.min_warning_probability = value;
        }
        if let Some(value) = self.min_contradiction_warning_probability {
            config.min_contradiction_warning_probability = value;
        }
        if let Some(value) = self.max_context_chars {
            config.max_context_chars = value;
        }
        config.ignore_codes.extend(self.ignore_codes);
    }
}

fn main() -> ExitCode {
    let output_request = output_request_from(Cli::command(), std::env::args_os());
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if error.kind() == ErrorKind::DisplayHelp => {
            if output_request.text {
                let _ = error.print();
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&help_document_for(
                        Cli::command(),
                        &output_request.command_path,
                    ))
                    .expect("help document serializes")
                );
            }
            return ExitCode::SUCCESS;
        }
        Err(error) if error.kind() == ErrorKind::DisplayVersion => {
            if output_request.text {
                let _ = error.print();
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&version_document(Cli::command()))
                        .expect("version document serializes")
                );
            }
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let exit_code = error.exit_code();
            emit_failure(
                output_request.text,
                &[Diagnostic::error(
                    "cli.arguments.invalid",
                    "",
                    error.to_string().trim().to_owned(),
                )],
            );
            return ExitCode::from(exit_code as u8);
        }
    };

    match run(
        cli.lesson,
        cli.root,
        cli.config,
        cli.overrides,
        cli.no_cache,
    ) {
        Ok((findings, warnings)) => {
            for warning in warnings {
                eprintln!("learnverify: warning: {warning}");
            }
            let report =
                VerifyReport::from_findings(findings, cli.ignore_below, cli.warning_as_error);
            if cli.text {
                print!("{}", report.text());
            } else {
                println!(
                    "{}",
                    serde_json::to_string(&report).expect("verify report serializes")
                );
            }
            if report.is_fatal() {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(diagnostics) => {
            emit_failure(cli.text, &diagnostics);
            ExitCode::FAILURE
        }
    }
}

type Findings = (
    Vec<agent_teacher::learnverify::VerifyDiagnostic>,
    Vec<String>,
);

fn run(
    lesson: PathBuf,
    root: Option<PathBuf>,
    config: Option<PathBuf>,
    overrides: Overrides,
    no_cache: bool,
) -> Result<Findings, Vec<Diagnostic>> {
    let config = VerifyConfig::load_with_overrides(config.as_deref(), |config| {
        overrides.apply(config);
    })
    .map_err(|error| vec![error])?;
    let current_dir = std::env::current_dir().map_err(|error| {
        vec![Diagnostic::error(
            "cli.current_directory",
            "",
            format!("could not determine current directory: {error}"),
        )]
    })?;
    let mut options = CompileOptions::new(current_dir);
    if let Some(root) = root {
        options = options.with_root(root);
    }
    let verification = verify_file(&lesson, &options, &config, !no_cache)?;
    Ok((verification.findings, verification.warnings))
}

fn emit_failure(text: bool, diagnostics: &[Diagnostic]) {
    if text {
        for diagnostic in diagnostics {
            let location = if diagnostic.pointer.is_empty() {
                String::new()
            } else {
                format!(" at {}", diagnostic.pointer)
            };
            println!(
                "error[{}]{location}: {}",
                diagnostic.code, diagnostic.message
            );
            for related in &diagnostic.related {
                println!("  related {}: {}", related.pointer, related.message);
            }
            for suggestion in &diagnostic.suggestions {
                println!("  help: {suggestion}");
            }
        }
    } else {
        println!(
            "{}",
            serde_json::to_string(&json!({ "ok": false, "diagnostics": diagnostics }))
                .expect("diagnostics serialize")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_command_definition_is_consistent() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_config_key_has_a_flag() {
        let cli = Cli::try_parse_from([
            "learnverify",
            "--min-info-probability",
            "0.5",
            "--min-warning-probability",
            "0.9",
            "--min-contradiction-warning-probability",
            "0.6",
            "--max-context-chars",
            "10",
            "--ignore-code",
            "verify.implausible_distractor",
            "--no-cache",
            "lesson.json",
        ])
        .unwrap();
        let mut config = VerifyConfig::default();
        cli.overrides.apply(&mut config);
        assert_eq!(
            config,
            VerifyConfig {
                min_info_probability: 0.5,
                min_warning_probability: 0.9,
                min_contradiction_warning_probability: 0.6,
                max_context_chars: 10,
                ignore_codes: vec!["verify.implausible_distractor".into()],
            }
        );
        assert!(cli.no_cache);
    }
}
