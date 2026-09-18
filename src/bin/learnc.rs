//! Agent-centric lesson compiler command line.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use agent_teacher::artifact::CURRENT_ARTIFACT_VERSION;
use agent_teacher::compiler::{
    CompileOptions, compile_file, default_artifact_path, write_artifact_atomic,
};
use agent_teacher::diagnostics::Diagnostic;
use agent_teacher::source::{SchemaVersion, source_json_schema};
use clap::{Parser, Subcommand, error::ErrorKind};
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
        /// Directory inside the Git worktree to use as the selected repository.
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Compile an authored JSON lesson into a self-contained `.learn` artifact.
    Build {
        /// Authored JSON lesson document.
        lesson: PathBuf,
        /// Artifact path; defaults to the source name with a `.learn` extension.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Directory inside the Git worktree to use as the selected repository.
        #[arg(long)]
        repo: Option<PathBuf>,
    },
    /// Emit the exact authored-document JSON Schema.
    Schema {
        /// Source schema version to emit.
        #[arg(long, default_value = "1.0.0")]
        version: String,
    },
}

enum Success {
    Json(Value),
    Schema(Value),
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return ExitCode::SUCCESS;
        }
        Err(error) => {
            let exit_code = error.exit_code();
            emit_failure(
                false,
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
            emit_success(cli.text, success);
            ExitCode::SUCCESS
        }
        Err(diagnostics) => {
            emit_failure(cli.text, &diagnostics);
            ExitCode::FAILURE
        }
    }
}

fn execute(command: Command, current_dir: &Path) -> Result<Success, Vec<Diagnostic>> {
    match command {
        Command::Check { lesson, repo } => {
            let options = compile_options(current_dir, repo);
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
        Command::Build {
            lesson,
            output,
            repo,
        } => {
            let options = compile_options(current_dir, repo);
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
            if version != SchemaVersion::CURRENT.as_str() {
                return Err(vec![
                    Diagnostic::error(
                        "schema.version.unsupported",
                        "",
                        format!("source schema version {version:?} is not supported"),
                    )
                    .with_suggestion(format!(
                        "Use `--version {}` with this compiler.",
                        SchemaVersion::CURRENT.as_str()
                    )),
                ]);
            }
            Ok(Success::Schema(source_json_schema()))
        }
    }
}

fn compile_options(current_dir: &Path, repo: Option<PathBuf>) -> CompileOptions {
    let mut options = CompileOptions::new(current_dir);
    if let Some(repo) = repo {
        options = options.with_repo(repo);
    }
    options
}

fn emit_success(text: bool, success: Success) {
    match success {
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
    fn check_runs_full_compile_without_writing() {
        let directory = test_directory();
        let source = directory.join("lesson.json");
        fs::write(&source, lesson_json()).unwrap();
        let success = execute(
            Command::Check {
                lesson: source.clone(),
                repo: None,
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
                repo: None,
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
                version: "2.0.0".into(),
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
