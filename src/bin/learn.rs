//! Local lesson runtime entrypoint.

use std::io::{self, Write};
use std::path::{Path, PathBuf};

use agent_teacher::runtime::{RuntimeError, bind};
use clap::{Parser, Subcommand, error::ErrorKind};
use serde::Serialize;

#[cfg(test)]
use clap::CommandFactory;

#[derive(Debug, Parser)]
#[command(name = "learn", version, about = "Serve a compiled interactive lesson")]
struct Cli {
    /// Use concise human-readable output instead of JSON.
    #[arg(short = 't', long, global = true)]
    text: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Serve one compiled `.learn` artifact on a random loopback port.
    Serve {
        /// Open the generated loopback URL in the default browser.
        #[arg(long)]
        open: bool,
        /// Compiled lesson artifact produced by `learnc build`.
        artifact: PathBuf,
    },
}

#[derive(Serialize)]
struct StartupOutput<'a> {
    status: &'static str,
    url: &'a str,
    artifact: String,
}

#[derive(Serialize)]
struct ErrorOutput<'a> {
    status: &'static str,
    error: ErrorDetail<'a>,
}

#[derive(Serialize)]
struct ErrorDetail<'a> {
    code: &'a str,
    message: String,
}

#[tokio::main]
async fn main() {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return;
        }
        Err(error) => {
            emit_error("invalid_arguments", error.to_string(), false);
            std::process::exit(error.exit_code());
        }
    };

    let text = cli.text;
    let result = match cli.command {
        Command::Serve { open, artifact } => serve(&artifact, open, text).await,
    };
    if let Err(error) = result {
        emit_error(error.code(), error.to_string(), text);
        std::process::exit(1);
    }
}

async fn serve(artifact: &Path, open: bool, text: bool) -> Result<(), RuntimeError> {
    let server = bind(artifact).await?;
    let url = server.url();
    if text {
        println!("Serving {} at {url}", artifact.display());
    } else {
        let output = StartupOutput {
            status: "serving",
            url: &url,
            artifact: artifact.display().to_string(),
        };
        println!(
            "{}",
            serde_json::to_string(&output).expect("startup output serializes")
        );
    }
    let _ = io::stdout().flush();

    if open {
        // `url` is generated exclusively from the bound IPv4 loopback address.
        if let Err(error) = webbrowser::open(&url) {
            eprintln!("could not open the browser: {error}");
        }
    }
    server.run().await
}

fn emit_error(code: &str, message: String, text: bool) {
    if text {
        println!("error[{code}]: {message}");
    } else {
        let output = ErrorOutput {
            status: "error",
            error: ErrorDetail { code, message },
        };
        println!(
            "{}",
            serde_json::to_string(&output).expect("error output serializes")
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_text_flag_is_accepted_before_and_after_the_subcommand() {
        for arguments in [
            ["learn", "--text", "serve", "lesson.learn"],
            ["learn", "serve", "--text", "lesson.learn"],
            ["learn", "serve", "lesson.learn", "--text"],
        ] {
            let cli = Cli::try_parse_from(arguments).unwrap();
            assert!(cli.text);
        }
    }

    #[test]
    fn command_shape_is_serve_artifact() {
        Cli::command().debug_assert();
        let cli = Cli::try_parse_from(["learn", "serve", "--open", "lesson.learn"]).unwrap();
        assert!(matches!(
            cli.command,
            Command::Serve {
                open: true,
                artifact
            } if artifact == Path::new("lesson.learn")
        ));
    }
}
