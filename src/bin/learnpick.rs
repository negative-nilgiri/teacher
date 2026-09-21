//! Optional agent-facing adviser for choosing one lesson block type.

use std::io::{self, Read};
use std::process::ExitCode;

use agent_teacher::cli::{help_document_for, output_request_from, version_document};
use agent_teacher::diagnostics::Diagnostic;
use agent_teacher::learnpick;
use clap::{CommandFactory, Parser, error::ErrorKind};
use serde_json::json;

#[derive(Debug, Parser)]
#[command(
    name = "learnpick",
    version,
    about = "Recommend a lesson block for one teaching unit"
)]
struct Cli {
    /// Print concise human-readable output instead of the default JSON.
    #[arg(short = 't', long)]
    text: bool,

    /// Teaching-unit description, or `-` to read it from standard input.
    unit: String,
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
                Diagnostic::error(
                    "cli.arguments.invalid",
                    "",
                    error.to_string().trim().to_owned(),
                ),
            );
            return ExitCode::from(exit_code as u8);
        }
    };

    let unit = match read_unit(&cli.unit) {
        Ok(unit) => unit,
        Err(error) => {
            emit_failure(
                cli.text,
                Diagnostic::error(
                    "learnpick.input.read_failed",
                    "",
                    format!("could not read teaching unit from standard input: {error}"),
                ),
            );
            return ExitCode::FAILURE;
        }
    };

    match learnpick::recommend(&unit) {
        Ok(recommendation) => {
            emit_success(cli.text, recommendation);
            ExitCode::SUCCESS
        }
        Err(error) => {
            let mut diagnostic = Diagnostic::error(error.code(), "", error.to_string());
            if let Some(suggestion) = error.suggestion() {
                diagnostic = diagnostic.with_suggestion(suggestion);
            }
            emit_failure(cli.text, diagnostic);
            ExitCode::FAILURE
        }
    }
}

fn read_unit(argument: &str) -> io::Result<String> {
    if argument != "-" {
        return Ok(argument.to_owned());
    }
    let mut input = String::new();
    io::stdin().read_to_string(&mut input)?;
    Ok(input)
}

fn emit_success(text: bool, recommendation: learnpick::Recommendation) {
    if text {
        println!(
            "recommended block: {} (confidence {:.0}%, model {})",
            recommendation.block_type,
            recommendation.confidence * 100.0,
            recommendation.model
        );
        let mut alternatives = recommendation
            .probabilities
            .iter()
            .map(|(block_type, probability)| (*block_type, *probability))
            .collect::<Vec<_>>();
        alternatives.sort_by(|left, right| {
            right
                .1
                .total_cmp(&left.1)
                .then_with(|| left.0.cmp(&right.0))
        });
        println!(
            "probabilities: {}",
            alternatives
                .into_iter()
                .map(|(block_type, probability)| {
                    format!("{block_type} {:.0}%", probability * 100.0)
                })
                .collect::<Vec<_>>()
                .join(", ")
        );
    } else {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "ok": true,
                "source_schema_version": recommendation.source_schema_version,
                "model": recommendation.model,
                "block_type": recommendation.block_type,
                "confidence": recommendation.confidence,
                "probabilities": recommendation.probabilities,
                "usage": recommendation.usage,
            }))
            .expect("recommendation serializes")
        );
    }
}

fn emit_failure(text: bool, diagnostic: Diagnostic) {
    if text {
        println!("error[{}]: {}", diagnostic.code, diagnostic.message);
        for suggestion in diagnostic.suggestions {
            println!("  help: {suggestion}");
        }
    } else {
        println!(
            "{}",
            serde_json::to_string(&json!({
                "ok": false,
                "diagnostics": [diagnostic]
            }))
            .expect("diagnostic serializes")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_accepts_literal_and_stdin_units() {
        let literal = Cli::try_parse_from(["learnpick", "Explain queue ordering"]).unwrap();
        assert_eq!(literal.unit, "Explain queue ordering");
        let stdin = Cli::try_parse_from(["learnpick", "-t", "-"]).unwrap();
        assert!(stdin.text);
        assert_eq!(stdin.unit, "-");
    }

    #[test]
    fn clap_requires_exactly_one_unit() {
        assert!(Cli::try_parse_from(["learnpick"]).is_err());
        assert!(Cli::try_parse_from(["learnpick", "first", "second"]).is_err());
    }
}
