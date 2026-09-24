use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::Value;

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-teacher-lint-cli-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, name: &str, content: &str) {
        fs::write(self.0.join(name), content).unwrap();
    }

    fn learnc(&self, args: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_learnc"))
            .current_dir(&self.0)
            .args(args)
            .output()
            .unwrap()
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn clean_json_and_text_output_are_minimal() {
    let root = TempRoot::new();
    root.write(
        "lesson.json",
        r#"{"schema_version":"2.1.0","title":"Clean","blocks":[
            {"type":"markdown","id":"intro","source":{"kind":"inline","content":"Hello."}},
            {"type":"multiple_choice","id":"quiz","prompt":{"kind":"inline","content":"Pick A."},
             "choices":[{"content":"A","correct":true},{"content":"B"}],"explanation":"A is right."}
        ]}"#,
    );
    let json = root.learnc(&["lint", "lesson.json"]);
    assert!(json.status.success());
    assert_eq!(json_output(&json), serde_json::json!({"diagnostics":[]}));

    let text = root.learnc(&["lint", "-t", "lesson.json"]);
    assert!(text.status.success());
    assert!(text.stdout.is_empty());
}

#[test]
fn severity_threshold_and_filter_control_exit_after_filtering() {
    let root = TempRoot::new();
    root.write(
        "lesson.json",
        r#"{"schema_version":"2.1.0","title":"Plain","blocks":[
            {"type":"code","id":"plain","source":{"kind":"inline","content":"literal plain text"}}
        ]}"#,
    );
    let normal = root.learnc(&["lint", "lesson.json"]);
    assert!(normal.status.success());
    let diagnostics = json_output(&normal)["diagnostics"]
        .as_array()
        .unwrap()
        .clone();
    assert!(
        diagnostics
            .iter()
            .any(|f| f["severity"] == "warning" && f["fatal"] == false)
    );

    let strict = root.learnc(&["lint", "--warning-as-error", "warning", "lesson.json"]);
    assert!(!strict.status.success());
    let strict_json = json_output(&strict);
    let warning = strict_json["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["severity"] == "warning")
        .unwrap();
    assert_eq!(warning["fatal"], true);

    let filtered = root.learnc(&[
        "lint",
        "--ignore-below",
        "critical",
        "--warning-as-error",
        "info",
        "lesson.json",
    ]);
    assert!(filtered.status.success());
    assert_eq!(
        json_output(&filtered),
        serde_json::json!({"diagnostics":[]})
    );

    let text = root.learnc(&["lint", "-t", "lesson.json"]);
    let rendered = String::from_utf8(text.stdout).unwrap();
    assert!(rendered.contains("warning[lint.code.plain_text]"));
    assert!(rendered.contains("lesson.json:2:"));
    assert!(rendered.contains("= help:"));
}

#[test]
fn explicit_config_changes_threshold_and_rejects_unknown_keys() {
    let root = TempRoot::new();
    root.write(
        "lesson.json",
        r#"{"schema_version":"2.1.0","title":"Config","blocks":[
            {"type":"code","id":"sample","language":"rust","source":{"kind":"inline","content":"abcd"}},
            {"type":"multiple_choice","id":"quiz","prompt":{"kind":"inline","content":"Pick A."},
             "choices":[{"content":"A","correct":true},{"content":"B"}],"explanation":"A is right."}
        ]}"#,
    );
    assert!(root.learnc(&["lint", "lesson.json"]).status.success());
    root.write("lint.toml", "max_inline_code_diff_chars = 3\n");
    let configured = root.learnc(&["lint", "--config", "lint.toml", "lesson.json"]);
    assert!(!configured.status.success());
    let finding = &json_output(&configured)["diagnostics"][0];
    assert_eq!(finding["code"], "lint.inline.code_diff.too_large");
    assert_eq!(finding["severity"], "error");
    assert_eq!(finding["fatal"], true);
    assert_eq!(finding["block_id"], "sample");
    assert_eq!(finding["pointer"], "/blocks/0/source/content");
    assert_eq!(finding["location"]["start"]["line"], 2);

    root.write("lint.toml", "max_inline_code_diff_char = 3\n");
    let invalid = root.learnc(&["lint", "--config", "lint.toml", "lesson.json"]);
    assert!(!invalid.status.success());
    assert_eq!(
        json_output(&invalid)["diagnostics"][0]["code"],
        "lint.config.invalid"
    );
}

#[test]
fn cli_thresholds_override_file_values_and_validate_effective_config() {
    let root = TempRoot::new();
    root.write(
        "lesson.json",
        r#"{"schema_version":"2.1.0","title":"Overrides","blocks":[
            {"type":"code","id":"sample","language":"rust","source":{"kind":"inline","content":"abcd"}},
            {"type":"multiple_choice","id":"quiz","prompt":{"kind":"inline","content":"Pick A."},
             "choices":[{"content":"A","correct":true},{"content":"B"}],"explanation":"A is right."}
        ]}"#,
    );
    root.write("lint.toml", "max_inline_code_diff_chars = 3\n");

    let overridden = root.learnc(&[
        "lint",
        "--config",
        "lint.toml",
        "--max-inline-code-diff-chars",
        "4",
        "lesson.json",
    ]);
    assert!(overridden.status.success());
    assert_eq!(
        json_output(&overridden),
        serde_json::json!({"diagnostics":[]})
    );

    let cli_only = root.learnc(&["lint", "--max-inline-code-diff-chars", "3", "lesson.json"]);
    assert!(!cli_only.status.success());
    assert_eq!(
        json_output(&cli_only)["diagnostics"][0]["code"],
        "lint.inline.code_diff.too_large"
    );

    let ratio = root.learnc(&["lint", "--min-question-ratio", "0.6", "lesson.json"]);
    assert!(ratio.status.success());
    assert_eq!(
        json_output(&ratio)["diagnostics"][0]["code"],
        "lint.lesson.few_questions"
    );

    root.write("lint.toml", "many_highlight_ranges = 0\n");
    let repaired = root.learnc(&[
        "lint",
        "--config",
        "lint.toml",
        "--many-highlight-ranges",
        "3",
        "lesson.json",
    ]);
    assert!(repaired.status.success());
    let invalid = root.learnc(&["lint", "--many-highlight-ranges", "0", "lesson.json"]);
    assert_eq!(
        json_output(&invalid)["diagnostics"][0]["code"],
        "lint.config.threshold.invalid"
    );
}

#[test]
fn lint_help_exposes_each_config_threshold_as_a_flag() {
    let root = TempRoot::new();
    let output = root.learnc(&["lint", "-t", "--help"]);
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    for flag in [
        "--max-code-lines",
        "--highlight-coverage-ratio",
        "--highlight-coverage-min-lines",
        "--many-highlight-ranges",
        "--suggest-highlights-min-lines",
        "--filename-reference-gap",
        "--max-choice-length-spread",
        "--min-choice-length-gap-chars",
        "--min-question-ratio",
        "--max-inline-code-diff-chars",
        "--max-inline-prose-chars",
    ] {
        assert!(help.contains(flag), "missing {flag} from lint help");
    }
}

#[test]
fn compiler_failures_remain_unfilterable_and_mermaid_blocks_check_and_build() {
    let root = TempRoot::new();
    root.write(
        "lesson.json",
        r#"{"schema_version":"2.1.0","title":" ","blocks":[]}"#,
    );
    let invalid = root.learnc(&["lint", "--ignore-below", "error", "lesson.json"]);
    assert!(!invalid.status.success());
    let failure = json_output(&invalid);
    assert_eq!(failure["ok"], false);
    assert_eq!(failure["diagnostics"][0]["code"], "source.title.empty");

    root.write(
        "lesson.json",
        r#"{"schema_version":"2.1.0","title":"Bad diagram","blocks":[
            {"type":"code","id":"diagram","language":"mermaid",
             "source":{"kind":"inline","content":"flowchart SIDEWAYS\n  A --> B"}}
        ]}"#,
    );
    for command in ["check", "build", "lint"] {
        let output = root.learnc(&[command, "lesson.json"]);
        assert!(!output.status.success());
        assert_eq!(
            json_output(&output)["diagnostics"][0]["code"],
            "compiler.mermaid.syntax"
        );
    }
    assert!(!root.path().join("lesson.learn").exists());
}
