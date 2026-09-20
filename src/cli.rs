//! Shared command-line output helpers.
//!
//! Clap remains the sole argument parser. The lightweight output-mode probe is
//! a second Clap command used only when the primary parser returns help,
//! version, or invalid-usage output before it can construct the typed CLI.

use std::ffi::OsString;

use clap::{Arg, ArgAction, ArgMatches, Command};
use serde::Serialize;

/// Output preferences recovered by a permissive Clap parse.
#[derive(Debug, Default, Eq, PartialEq)]
pub struct OutputRequest {
    pub text: bool,
    pub command_path: Vec<String>,
}

/// Determine output preferences even when the primary parser exits early for
/// help, version, or invalid usage.
pub fn output_request_from<I, T>(command: Command, arguments: I) -> OutputRequest
where
    I: IntoIterator<Item = T>,
    T: Into<OsString> + Clone,
{
    let arguments = arguments
        .into_iter()
        .map(Into::into)
        .collect::<Vec<OsString>>();
    let probe = relax_for_output_probe(command)
        .arg(
            Arg::new("__output_probe_help")
                .short('h')
                .long("help")
                .global(true)
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("__output_probe_version")
                .short('V')
                .long("version")
                .action(ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("help").arg(
                Arg::new("__output_probe_help_path")
                    .num_args(0..)
                    .action(ArgAction::Append),
            ),
        );

    let text = text_requested_anywhere(&arguments);
    let Ok(matches) = probe.try_get_matches_from(&arguments) else {
        return OutputRequest {
            text,
            ..OutputRequest::default()
        };
    };
    OutputRequest {
        text: text || matches.get_flag("text"),
        command_path: matched_command_path(&matches),
    }
}

/// Generate structured, machine-readable help from the same Clap command that
/// parses the invocation.
pub fn help_document(command: Command) -> HelpDocument {
    help_document_for(command, &[])
}

/// Generate help for the requested subcommand path.
pub fn help_document_for(mut command: Command, command_path: &[String]) -> HelpDocument {
    command.build();
    let selected = command_path
        .iter()
        .try_fold(&command, |selected, name| selected.find_subcommand(name))
        .unwrap_or(&command);
    HelpDocument {
        ok: true,
        kind: "help",
        help: describe_command(selected),
    }
}

/// Generate the machine-readable equivalent of Clap's version output.
pub fn version_document(command: Command) -> VersionDocument {
    VersionDocument {
        ok: true,
        kind: "version",
        name: command.get_name().to_owned(),
        version: command.get_version().unwrap_or_default().to_owned(),
    }
}

#[derive(Debug, Serialize)]
pub struct HelpDocument {
    ok: bool,
    kind: &'static str,
    help: CommandHelp,
}

#[derive(Debug, Serialize)]
struct CommandHelp {
    name: String,
    invocation: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    about: Option<String>,
    usage: String,
    arguments: Vec<ArgumentHelp>,
    options: Vec<OptionHelp>,
    subcommands: Vec<CommandHelp>,
}

#[derive(Debug, Serialize)]
struct ArgumentHelp {
    name: String,
    index: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    value_names: Vec<String>,
    required: bool,
    action: String,
    num_values: ValueCount,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    possible_values: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    default_values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
}

#[derive(Debug, Serialize)]
struct OptionHelp {
    name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    short: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    long: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    value_names: Vec<String>,
    required: bool,
    global: bool,
    action: String,
    num_values: ValueCount,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    possible_values: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    default_values: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    help: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct VersionDocument {
    ok: bool,
    kind: &'static str,
    name: String,
    version: String,
}

#[derive(Debug, Serialize)]
struct ValueCount {
    min: usize,
    /// `None` means that Clap accepts an unbounded number of values.
    max: Option<usize>,
}

fn describe_command(command: &Command) -> CommandHelp {
    let mut usage_command = command.clone();
    let usage = usage_command
        .render_usage()
        .to_string()
        .trim()
        .strip_prefix("Usage: ")
        .unwrap_or_else(|| usage_command.get_name())
        .to_owned();

    let mut arguments = Vec::new();
    let mut options = Vec::new();
    for argument in command
        .get_arguments()
        .filter(|argument| !argument.is_hide_set())
    {
        let value_names = value_names(argument);
        let action = action_name(argument.get_action()).to_owned();
        let num_values = value_count(argument);
        let possible_values = argument
            .get_possible_values()
            .iter()
            .map(|value| value.get_name().to_owned())
            .collect();
        let default_values = argument
            .get_default_values()
            .iter()
            .map(|value| value.to_string_lossy().into_owned())
            .collect();
        let help = argument.get_help().map(ToString::to_string);
        if argument.get_index().is_some() {
            arguments.push(ArgumentHelp {
                name: argument.get_id().to_string(),
                index: argument
                    .get_index()
                    .expect("positional argument has an index"),
                value_names,
                required: argument.is_required_set(),
                action,
                num_values,
                possible_values,
                default_values,
                help,
            });
        } else {
            options.push(OptionHelp {
                name: argument.get_id().to_string(),
                short: argument.get_short().map(|value| format!("-{value}")),
                long: argument.get_long().map(|value| format!("--{value}")),
                value_names,
                required: argument.is_required_set(),
                global: argument.is_global_set(),
                action,
                num_values,
                possible_values,
                default_values,
                help,
            });
        }
    }

    CommandHelp {
        name: command.get_name().to_owned(),
        invocation: command
            .get_bin_name()
            .unwrap_or_else(|| command.get_name())
            .to_owned(),
        version: command.get_version().map(str::to_owned),
        about: command.get_about().map(ToString::to_string),
        usage,
        arguments,
        options,
        subcommands: command
            .get_subcommands()
            .filter(|subcommand| subcommand.get_name() != "help")
            .map(describe_command)
            .collect(),
    }
}

fn value_names(argument: &Arg) -> Vec<String> {
    if !takes_values(argument) {
        return Vec::new();
    }
    argument
        .get_value_names()
        .map(|names| names.iter().map(ToString::to_string).collect::<Vec<_>>())
        .or_else(|| {
            takes_values(argument).then(|| vec![argument.get_id().as_str().to_ascii_uppercase()])
        })
        .unwrap_or_default()
}

fn takes_values(argument: &Arg) -> bool {
    argument
        .get_num_args()
        .map(|range| range.takes_values())
        .unwrap_or_else(|| argument.get_action().takes_values())
}

fn value_count(argument: &Arg) -> ValueCount {
    let range = argument.get_num_args().unwrap_or_else(|| {
        if argument.get_action().takes_values() {
            1.into()
        } else {
            0.into()
        }
    });
    ValueCount {
        min: range.min_values(),
        max: (range.max_values() != usize::MAX).then(|| range.max_values()),
    }
}

fn action_name(action: &ArgAction) -> &'static str {
    match action {
        ArgAction::Set => "set",
        ArgAction::Append => "append",
        ArgAction::SetTrue => "set_true",
        ArgAction::SetFalse => "set_false",
        ArgAction::Count => "count",
        ArgAction::Help => "help",
        ArgAction::HelpShort => "help_short",
        ArgAction::HelpLong => "help_long",
        ArgAction::Version => "version",
        _ => "other",
    }
}

fn relax_for_output_probe(command: Command) -> Command {
    command
        .disable_help_flag(true)
        .disable_version_flag(true)
        .disable_help_subcommand(true)
        .subcommand_required(false)
        .arg_required_else_help(false)
        .ignore_errors(true)
        .mut_args(|argument| argument.required(false))
        .mut_subcommands(relax_for_output_probe)
}

fn matched_command_path(matches: &ArgMatches) -> Vec<String> {
    if let Some(help) = matches.subcommand_matches("help") {
        return help
            .get_many::<String>("__output_probe_help_path")
            .into_iter()
            .flatten()
            .cloned()
            .collect();
    }
    let mut path = Vec::new();
    let mut current = matches;
    while let Some((name, subcommand)) = current.subcommand() {
        path.push(name.to_owned());
        current = subcommand;
    }
    path
}

fn text_requested_anywhere(arguments: &[OsString]) -> bool {
    Command::new("output-mode")
        .disable_help_flag(true)
        .disable_version_flag(true)
        .arg(
            Arg::new("text")
                .short('t')
                .long("text")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("help")
                .short('h')
                .long("help")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("version")
                .short('V')
                .long("version")
                .action(ArgAction::SetTrue),
        )
        .arg(
            Arg::new("arguments")
                .num_args(0..)
                .allow_hyphen_values(true),
        )
        .try_get_matches_from(arguments)
        .ok()
        .is_some_and(|matches| matches.get_flag("text"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_mode_is_parsed_by_clap_at_any_normal_position() {
        for arguments in [
            vec!["learn", "-t", "serve", "lesson.learn"],
            vec!["learn", "serve", "--text", "lesson.learn"],
            vec!["learn", "serve", "lesson.learn", "-t"],
            vec!["learn", "--help", "--text"],
        ] {
            let request = output_request_from(
                Command::new("learn")
                    .arg(
                        Arg::new("text")
                            .short('t')
                            .long("text")
                            .global(true)
                            .action(ArgAction::SetTrue),
                    )
                    .subcommand(Command::new("serve").arg(Arg::new("artifact").required(true))),
                arguments.clone(),
            );
            assert!(request.text, "{arguments:?}");
        }
        let request = output_request_from(
            Command::new("learn")
                .arg(
                    Arg::new("text")
                        .short('t')
                        .long("text")
                        .global(true)
                        .action(ArgAction::SetTrue),
                )
                .subcommand(Command::new("serve").arg(Arg::new("artifact").required(true))),
            ["learn", "serve", "lesson.learn"],
        );
        assert_eq!(request.command_path, ["serve"]);
        assert!(!request.text);

        let request = output_request_from(
            Command::new("learnc")
                .arg(
                    Arg::new("text")
                        .short('t')
                        .long("text")
                        .global(true)
                        .action(ArgAction::SetTrue),
                )
                .subcommand(Command::new("check")),
            ["learnc", "help", "check"],
        );
        assert_eq!(request.command_path, ["check"]);
    }

    #[test]
    fn structured_help_contains_usage_options_and_subcommands() {
        let document = help_document(
            Command::new("example")
                .version("1.2.3")
                .about("Example command")
                .arg(Arg::new("text").short('t').long("text"))
                .arg(Arg::new("open").long("open").action(ArgAction::SetTrue))
                .subcommand(
                    Command::new("run")
                        .about("Run it")
                        .arg(Arg::new("input").required(true)),
                ),
        );
        let value = serde_json::to_value(document).unwrap();
        assert_eq!(value["kind"], "help");
        assert_eq!(value["help"]["version"], "1.2.3");
        assert_eq!(value["help"]["options"][0]["long"], "--text");
        assert_eq!(value["help"]["options"][0]["action"], "set");
        assert_eq!(value["help"]["options"][0]["num_values"]["min"], 1);
        let open = value["help"]["options"]
            .as_array()
            .unwrap()
            .iter()
            .find(|option| option["long"] == "--open")
            .unwrap();
        assert!(open.get("value_names").is_none());
        assert_eq!(open["action"], "set_true");
        assert_eq!(value["help"]["subcommands"][0]["name"], "run");
        assert_eq!(
            value["help"]["subcommands"][0]["arguments"][0]["name"],
            "input"
        );
    }

    #[test]
    fn structured_help_can_target_a_subcommand() {
        let path = vec!["run".to_owned()];
        let document = help_document_for(
            Command::new("example").subcommand(
                Command::new("run")
                    .about("Run it")
                    .arg(Arg::new("input").required(true)),
            ),
            &path,
        );
        let value = serde_json::to_value(document).unwrap();
        assert_eq!(value["help"]["name"], "run");
        assert_eq!(value["help"]["invocation"], "example run");
        assert_eq!(value["help"]["arguments"][0]["required"], true);
    }
}
