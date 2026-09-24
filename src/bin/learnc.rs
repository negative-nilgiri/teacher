//! Agent-centric lesson compiler command line.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use agent_teacher::artifact::CURRENT_ARTIFACT_VERSION;
use agent_teacher::cli::{help_document_for, output_request_from, version_document};
use agent_teacher::compiler::{
    CompileOptions, compile_file, default_artifact_path, write_artifact_atomic,
};
use agent_teacher::diagnostics::Diagnostic;
use agent_teacher::lint::{LintConfig, LintReport, Severity, lint_file};
use agent_teacher::source::{SchemaVersion, source_json_schema_for};
use clap::{Args, CommandFactory, Parser, Subcommand, error::ErrorKind};
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "learnc",
    version,
    about = "Compile interactive lesson documents"
)]
struct Cli {
    /// Print concise human-readable output instead of the default JSON.
    #[arg(short = 't', long, global = true)]
    text: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Run the complete source and repository pipeline without writing an artifact.
    Check {
        /// Authored JSON lesson document.
        lesson: PathBuf,
        /// Filesystem root used to resolve relative lesson paths.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Check authoring policy after the complete compilation checks pass.
    Lint {
        /// Authored JSON lesson document.
        lesson: PathBuf,
        /// Filesystem root used to resolve relative lesson paths.
        #[arg(long)]
        root: Option<PathBuf>,
        /// Explicit TOML file overriding lint thresholds.
        #[arg(long)]
        config: Option<PathBuf>,
        #[command(flatten)]
        overrides: LintOverrides,
        /// Lowest lint category that causes a nonzero exit status.
        #[arg(long, default_value = "error")]
        warning_as_error: Severity,
        /// Omit lint findings below this category.
        #[arg(long)]
        ignore_below: Option<Severity>,
    },
    /// Compile an authored JSON lesson into a self-contained `.learn` artifact.
    Build {
        /// Authored JSON lesson document.
        lesson: PathBuf,
        /// Artifact path; defaults to the source name with a `.learn` extension.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Filesystem root used to resolve relative lesson paths.
        #[arg(long)]
        root: Option<PathBuf>,
    },
    /// Emit the exact authored-document JSON Schema.
    Schema {
        /// Source schema version to emit.
        #[arg(long, default_value = "2.1.0")]
        version: String,
    },
}

#[derive(Debug, Args)]
struct LintOverrides {
    /// Maximum displayed lines before a code block is considered long.
    #[arg(long)]
    max_code_lines: Option<usize>,
    /// Highlighted-line ratio that triggers a coverage warning (0 to 1).
    #[arg(long)]
    highlight_coverage_ratio: Option<f64>,
    /// Minimum displayed lines for a highlight coverage warning.
    #[arg(long)]
    highlight_coverage_min_lines: Option<usize>,
    /// Number of highlight ranges that triggers an info finding.
    #[arg(long)]
    many_highlight_ranges: Option<usize>,
    /// Minimum lines before suggesting highlights on a code block.
    #[arg(long)]
    suggest_highlights_min_lines: Option<usize>,
    /// Minimum blocks between a filename mention and its code block.
    #[arg(long)]
    filename_reference_gap: Option<usize>,
    /// Maximum proportional difference between answer choice lengths.
    #[arg(long)]
    max_choice_length_spread: Option<f64>,
    /// Minimum character difference between answer choice lengths.
    #[arg(long)]
    min_choice_length_gap_chars: Option<usize>,
    /// Minimum proportion of lesson blocks that should be questions (0 to 1).
    #[arg(long)]
    min_question_ratio: Option<f64>,
    /// Maximum decoded characters in an inline code or diff source.
    #[arg(long)]
    max_inline_code_diff_chars: Option<usize>,
    /// Maximum decoded characters in an inline Markdown source or quiz prompt.
    #[arg(long)]
    max_inline_prose_chars: Option<usize>,
}

impl LintOverrides {
    fn apply(self, config: &mut LintConfig) {
        if let Some(value) = self.max_code_lines {
            config.max_code_lines = value;
        }
        if let Some(value) = self.highlight_coverage_ratio {
            config.highlight_coverage_ratio = value;
        }
        if let Some(value) = self.highlight_coverage_min_lines {
            config.highlight_coverage_min_lines = value;
        }
        if let Some(value) = self.many_highlight_ranges {
            config.many_highlight_ranges = value;
        }
        if let Some(value) = self.suggest_highlights_min_lines {
            config.suggest_highlights_min_lines = value;
        }
        if let Some(value) = self.filename_reference_gap {
            config.filename_reference_gap = value;
        }
        if let Some(value) = self.max_choice_length_spread {
            config.max_choice_length_spread = value;
        }
        if let Some(value) = self.min_choice_length_gap_chars {
            config.min_choice_length_gap_chars = value;
        }
        if let Some(value) = self.min_question_ratio {
            config.min_question_ratio = value;
        }
        if let Some(value) = self.max_inline_code_diff_chars {
            config.max_inline_code_diff_chars = value;
        }
        if let Some(value) = self.max_inline_prose_chars {
            config.max_inline_prose_chars = value;
        }
    }
}

enum Success {
    Json(Value),
    Schema(Value),
    Lint(LintReport),
}

impl Success {
    fn is_fatal(&self) -> bool {
        matches!(self, Self::Lint(report) if report.is_fatal())
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

    let current_dir = match std::env::current_dir() {
        Ok(value) => value,
        Err(error) => {
            emit_failure(
                cli.text,
                &[Diagnostic::error(
                    "cli.current_directory",
                    "",
                    format!("could not determine current directory: {error}"),
                )],
            );
            return ExitCode::FAILURE;
        }
    };

    match execute(cli.command, &current_dir) {
        Ok(success) => {
            let fatal = success.is_fatal();
            emit_success(cli.text, success);
            if fatal {
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

fn execute(command: Command, current_dir: &Path) -> Result<Success, Vec<Diagnostic>> {
    match command {
        Command::Check { lesson, root } => {
            let options = compile_options(current_dir, root);
            let artifact = compile_file(&lesson, &options)?;
            Ok(Success::Json(json!({
                "ok": true,
                "command": "check",
                "source": display_path(&lesson),
                "schema_version": artifact.provenance.source_schema_version.as_str(),
                "nodes": artifact.presentation.nodes.len(),
                "questions": artifact.private.answers.len()
            })))
        }
        Command::Lint {
            lesson,
            root,
            config,
            overrides,
            warning_as_error,
            ignore_below,
        } => {
            let config = LintConfig::load_with_overrides(config.as_deref(), |config| {
                overrides.apply(config);
            })
            .map_err(|error| vec![error])?;
            let options = compile_options(current_dir, root);
            let findings = lint_file(&lesson, &options, &config)?;
            Ok(Success::Lint(LintReport::from_findings(
                findings,
                ignore_below,
                warning_as_error,
            )))
        }
        Command::Build {
            lesson,
            output,
            root,
        } => {
            let options = compile_options(current_dir, root);
            let artifact = compile_file(&lesson, &options)?;
            let output = output.unwrap_or_else(|| default_artifact_path(&lesson));
            write_artifact_atomic(&output, &artifact).map_err(|error| vec![error])?;
            Ok(Success::Json(json!({
                "ok": true,
                "command": "build",
                "source": display_path(&lesson),
                "output": display_path(&output),
                "schema_version": artifact.provenance.source_schema_version.as_str(),
                "artifact_version": artifact.artifact_version.as_str(),
                "nodes": artifact.presentation.nodes.len(),
                "questions": artifact.private.answers.len()
            })))
        }
        Command::Schema { version } => {
            let version = SchemaVersion::SUPPORTED
                .into_iter()
                .find(|candidate| candidate.as_str() == version)
                .ok_or_else(|| {
                    vec![
                        Diagnostic::error(
                            "schema.version.unsupported",
                            "",
                            format!("source schema version {version:?} is not supported"),
                        )
                        .with_suggestion(format!(
                            "Use one of: {}.",
                            SchemaVersion::SUPPORTED
                                .iter()
                                .map(|version| version.as_str())
                                .collect::<Vec<_>>()
                                .join(", ")
                        )),
                    ]
                })?;
            Ok(Success::Schema(source_json_schema_for(version)))
        }
    }
}

fn compile_options(current_dir: &Path, root: Option<PathBuf>) -> CompileOptions {
    let mut options = CompileOptions::new(current_dir);
    if let Some(root) = root {
        options = options.with_root(root);
    }
    options
}

fn emit_success(text: bool, success: Success) {
    match success {
        Success::Lint(report) if text => print!("{}", report.text()),
        Success::Lint(report) => {
            println!(
                "{}",
                serde_json::to_string(&report).expect("lint report serializes")
            );
        }
        Success::Schema(schema) => {
            // The schema command's JSON value is its exact output contract, not
            // wrapped in a command-status envelope.
            println!(
                "{}",
                serde_json::to_string_pretty(&schema).expect("schema serializes")
            );
        }
        Success::Json(value) if text => {
            let command = value["command"].as_str().unwrap_or("command");
            match command {
                "check" => println!(
                    "checked {}: {} nodes, {} questions",
                    value["source"].as_str().unwrap_or("lesson"),
                    value["nodes"],
                    value["questions"]
                ),
                "build" => println!(
                    "built {} -> {}: {} nodes, {} questions (artifact {})",
                    value["source"].as_str().unwrap_or("lesson"),
                    value["output"].as_str().unwrap_or("artifact"),
                    value["nodes"],
                    value["questions"],
                    value["artifact_version"]
                        .as_str()
                        .unwrap_or(CURRENT_ARTIFACT_VERSION.as_str())
                ),
                _ => println!("{command} succeeded"),
            }
        }
        Success::Json(value) => {
            println!(
                "{}",
                serde_json::to_string(&value).expect("success output serializes")
            );
        }
    }
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
        let value = json!({ "ok": false, "diagnostics": diagnostics });
        println!(
            "{}",
            serde_json::to_string(&value).expect("diagnostics serialize")
        );
    }
}

fn display_path(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

    fn lesson_json() -> &'static str {
        r##"{
            "schema_version":"1.0.0",
            "title":"CLI test",
            "blocks":[{"type":"markdown","id":"intro","source":{"kind":"inline","content":"# Hi"}}]
        }"##
    }

    fn test_directory() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "agent-teacher-learnc-test-{}-{}",
            std::process::id(),
            TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn global_text_flag_is_accepted_after_the_subcommand() {
        for arguments in [
            ["learnc", "--text", "check", "lesson.json"],
            ["learnc", "check", "--text", "lesson.json"],
            ["learnc", "check", "lesson.json", "--text"],
        ] {
            let cli = Cli::try_parse_from(arguments).unwrap();
            assert!(cli.text);
        }
    }

    #[test]
    fn root_option_selects_the_filesystem_anchor() {
        let cli =
            Cli::try_parse_from(["learnc", "check", "lesson.json", "--root", "workspace"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Check { root: Some(root), .. } if root == Path::new("workspace")
        ));
        assert!(
            Cli::try_parse_from(["learnc", "check", "lesson.json", "--repo", "workspace"]).is_err()
        );
    }

    #[test]
    fn check_runs_full_compile_without_writing() {
        let directory = test_directory();
        let source = directory.join("lesson.json");
        fs::write(&source, lesson_json()).unwrap();
        let success = execute(
            Command::Check {
                lesson: source.clone(),
                root: None,
            },
            &directory,
        )
        .expect("check succeeds");
        assert!(!directory.join("lesson.learn").exists());
        assert!(matches!(success, Success::Json(_)));
        fs::remove_file(source).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn build_uses_default_name_and_writes_valid_artifact() {
        let directory = test_directory();
        let source = directory.join("lesson.json");
        fs::write(&source, lesson_json()).unwrap();
        execute(
            Command::Build {
                lesson: source.clone(),
                output: None,
                root: None,
            },
            &directory,
        )
        .expect("build succeeds");
        let output = directory.join("lesson.learn");
        let artifact: agent_teacher::artifact::CompiledLesson =
            serde_json::from_slice(&fs::read(&output).unwrap()).unwrap();
        assert_eq!(artifact.artifact_version, CURRENT_ARTIFACT_VERSION);
        fs::remove_file(source).unwrap();
        fs::remove_file(output).unwrap();
        fs::remove_dir(directory).unwrap();
    }

    #[test]
    fn schema_rejects_unknown_versions_with_stable_diagnostic() {
        let diagnostics = execute(
            Command::Schema {
                version: "3.0.0".into(),
            },
            Path::new("."),
        )
        .err()
        .expect("unsupported schema fails");
        assert_eq!(diagnostics[0].code, "schema.version.unsupported");
    }

    #[test]
    fn clap_command_definition_is_consistent() {
        Cli::command().debug_assert();
    }
}
