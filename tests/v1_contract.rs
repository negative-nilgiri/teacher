use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_teacher::artifact::{CompiledLesson, CompiledNodeContent, ResourceProvenance};
use agent_teacher::runtime::project_artifact;

static TEST_SEQUENCE: AtomicU64 = AtomicU64::new(0);

fn manifest_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn learnc() -> &'static str {
    env!("CARGO_BIN_EXE_learnc")
}

fn learn() -> &'static str {
    env!("CARGO_BIN_EXE_learn")
}

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let sequence = TEST_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "agent-teacher-{label}-{}-{sequence}",
            std::process::id()
        ));
        fs::create_dir(&path).expect("create temporary test directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn emitted_schema_and_valid_fixtures_match_the_decoder() {
    let output = output_success(Command::new(learnc()).args(["schema", "--version", "1.0.0"]));
    let emitted: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(emitted, agent_teacher::source::source_json_schema());
    assert_eq!(emitted["title"], "LessonSource");
    assert_eq!(emitted["additionalProperties"], false);
    let required = emitted["required"].as_array().expect("root required list");
    for field in ["schema_version", "title", "blocks"] {
        assert!(
            required.iter().any(|value| value == field),
            "schema did not require {field}"
        );
    }
    assert_schema_objects_are_closed(&emitted, &["kind", "content"]);
    assert_schema_objects_are_closed(&emitted, &["content", "correct"]);
    assert_schema_objects_are_closed(&emitted, &["start", "end"]);

    let valid = manifest_dir().join("tests/fixtures/source/valid");
    for entry in fs::read_dir(valid).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let output = output_success(Command::new(learnc()).arg("check").arg(&path));
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["ok"], true, "fixture {}", path.display());
        assert_eq!(report["command"], "check");
        assert!(
            !path.with_extension("learn").exists(),
            "check wrote an artifact"
        );
    }
}

#[test]
fn invalid_fixtures_return_stable_agent_diagnostics() {
    let fixture_dir = manifest_dir().join("tests/fixtures/source/invalid");
    let expected = [
        ("duplicate_ids.json", "source.id.duplicate"),
        ("invalid_quiz.json", "source.quiz.correct.invalid_count"),
        ("path_traversal.json", "source.path.invalid"),
        ("nested_unknown_field.json", "source.deserialize"),
        ("unknown_field.json", "source.deserialize"),
        ("future_version.json", "source.deserialize"),
    ];

    for (name, code) in expected {
        let output = Command::new(learnc())
            .arg("check")
            .arg(fixture_dir.join(name))
            .output()
            .unwrap();
        assert!(!output.status.success(), "invalid fixture {name} succeeded");
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["ok"], false);
        let diagnostics = report["diagnostics"].as_array().unwrap();
        assert!(
            diagnostics.iter().any(|value| value["code"] == code),
            "fixture {name} omitted diagnostic {code}: {report}"
        );
        assert!(diagnostics.iter().all(|value| {
            value["code"].is_string()
                && value["pointer"].is_string()
                && value["message"].is_string()
        }));
        if name == "nested_unknown_field.json" {
            let diagnostic = diagnostics
                .iter()
                .find(|value| value["code"] == "source.deserialize")
                .unwrap();
            assert_eq!(diagnostic["pointer"], "/blocks/0");
        }
    }
}

#[test]
fn cli_help_version_and_usage_errors_follow_the_output_mode() {
    for (binary, name) in [(learnc(), "learnc"), (learn(), "learn")] {
        let help = output_success(Command::new(binary).arg("--help"));
        let help: serde_json::Value = serde_json::from_slice(&help.stdout).unwrap();
        assert_eq!(help["ok"], true);
        assert_eq!(help["kind"], "help");
        assert_eq!(help["help"]["name"], name);
        assert!(help["help"]["usage"].is_string());

        let version = output_success(Command::new(binary).arg("--version"));
        let version: serde_json::Value = serde_json::from_slice(&version.stdout).unwrap();
        assert_eq!(version["ok"], true);
        assert_eq!(version["kind"], "version");
        assert_eq!(version["name"], name);

        let human = output_success(Command::new(binary).args(["-t", "--help"]));
        let human = String::from_utf8(human.stdout).unwrap();
        assert!(human.contains("Usage:"));
        assert!(!human.trim_start().starts_with('{'));
    }

    for arguments in [["check", "--help"], ["help", "check"]] {
        let help = output_success(Command::new(learnc()).args(arguments));
        let help: serde_json::Value = serde_json::from_slice(&help.stdout).unwrap();
        assert_eq!(help["help"]["name"], "check");
        assert_eq!(help["help"]["invocation"], "learnc check");
    }

    let help = output_success(Command::new(learn()).args(["serve", "--help"]));
    let help: serde_json::Value = serde_json::from_slice(&help.stdout).unwrap();
    assert_eq!(help["help"]["name"], "serve");
    assert_eq!(help["help"]["invocation"], "learn serve");

    let invalid = Command::new(learnc()).arg("--invalid").output().unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let invalid: serde_json::Value = serde_json::from_slice(&invalid.stdout).unwrap();
    assert_eq!(invalid["ok"], false);

    let invalid = Command::new(learnc())
        .args(["--text", "--invalid"])
        .output()
        .unwrap();
    assert_eq!(invalid.status.code(), Some(2));
    let invalid = String::from_utf8(invalid.stdout).unwrap();
    assert!(invalid.starts_with("error[cli.arguments.invalid]"));
}

#[test]
fn repository_example_checks_builds_and_freezes_relative_provenance() {
    let repository = repository_example();
    let lesson = repository.path().join("lesson.json");

    let checked = output_success(
        Command::new(learnc())
            .arg("check")
            .arg("--repo")
            .arg(repository.path())
            .arg(&lesson),
    );
    let checked: serde_json::Value = serde_json::from_slice(&checked.stdout).unwrap();
    assert_eq!(checked["ok"], true);
    assert_eq!(checked["nodes"], 6);
    assert!(!repository.path().join("lesson.learn").exists());

    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--repo")
            .arg(repository.path())
            .arg(&lesson),
    );
    let artifact_path = repository.path().join("lesson.learn");
    let bytes = fs::read(&artifact_path).unwrap();
    let artifact: CompiledLesson = serde_json::from_slice(&bytes).unwrap();
    agent_teacher::artifact::validate_artifact(&artifact).unwrap();
    assert_eq!(artifact.presentation.nodes.len(), 6);
    assert_eq!(artifact.private.answers.len(), 1);

    let encoded = String::from_utf8(bytes).unwrap();
    assert!(
        !encoded.contains(&repository.path().to_string_lossy().to_string()),
        "artifact leaked its absolute repository path"
    );
    assert!(artifact.presentation.nodes.iter().any(|node| matches!(
        &node.content,
        CompiledNodeContent::Code {
            provenance: ResourceProvenance::GitBlob {
                revision_object_id,
                content_object_id,
                ..
            },
            ..
        } if revision_object_id.len() == 40 && content_object_id.len() == 40
    )));
    assert!(artifact.presentation.nodes.iter().any(|node| matches!(
        &node.content,
        CompiledNodeContent::Diff {
            provenance: ResourceProvenance::GitDiff { repository, sha256, .. },
            ..
        } if repository == "." && sha256.len() == 64
    )));
}

#[test]
fn compiler_diff_range_excludes_a_nearby_unselected_change_region() {
    let repository = TempDir::new("range-selection");
    configure_repository(repository.path());
    fs::create_dir_all(repository.path().join("src")).unwrap();
    fs::write(
        repository.path().join("src/values.txt"),
        "line-01\nfirst-old\nbetween\nsecond-old\nline-05\nline-06\n",
    )
    .unwrap();
    git(repository.path(), &["add", "src/values.txt"]);
    git(repository.path(), &["commit", "-qm", "base values"]);
    fs::write(
        repository.path().join("src/values.txt"),
        "line-01\nfirst-new\nbetween\nsecond-new\nline-05\nline-06\n",
    )
    .unwrap();

    let lesson = repository.path().join("range.json");
    let source = serde_json::json!({
        "schema_version": "1.0.0",
        "title": "Selected change region",
        "blocks": [{
            "type": "diff",
            "id": "only-second-change",
            "source": {
                "kind": "git",
                "base": "HEAD",
                "target": { "kind": "worktree" },
                "files": [{
                    "path": "src/values.txt",
                    "after_lines": { "start": 4, "end": 4 }
                }],
                "context_lines": 1
            }
        }]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--repo")
            .arg(repository.path())
            .arg(&lesson),
    );

    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(repository.path().join("range.learn")).unwrap()).unwrap();
    let CompiledNodeContent::Diff { diff, .. } = &artifact.presentation.nodes[0].content else {
        panic!("compiled node was not a diff")
    };
    let rendered_lines = diff.files[0]
        .hunks
        .iter()
        .flat_map(|hunk| hunk.lines.iter().map(|line| line.content.as_str()))
        .collect::<Vec<_>>();
    assert!(rendered_lines.contains(&"second-old"));
    assert!(rendered_lines.contains(&"second-new"));
    assert!(!rendered_lines.contains(&"first-old"));
    assert!(!rendered_lines.contains(&"first-new"));
}

#[cfg(unix)]
#[test]
fn compiler_rejects_a_symbolic_ref_that_moves_during_resolution() {
    use std::os::unix::fs::PermissionsExt;

    let repository = TempDir::new("moving-ref");
    configure_repository(repository.path());
    fs::create_dir_all(repository.path().join("src")).unwrap();
    fs::write(repository.path().join("src/value.txt"), "first\n").unwrap();
    git(repository.path(), &["add", "src/value.txt"]);
    git(repository.path(), &["commit", "-qm", "first"]);
    let first = git_stdout(repository.path(), &["rev-parse", "HEAD"]);

    fs::write(repository.path().join("src/value.txt"), "second\n").unwrap();
    git(repository.path(), &["commit", "-qam", "second"]);
    let second = git_stdout(repository.path(), &["rev-parse", "HEAD"]);
    git(repository.path(), &["branch", "moving", &first]);

    let lesson = repository.path().join("moving.json");
    let source = serde_json::json!({
        "schema_version": "1.0.0",
        "title": "Moving symbolic ref",
        "blocks": [{
            "type": "diff",
            "id": "moving-comparison",
            "source": {
                "kind": "git",
                "base": "moving",
                "target": { "kind": "worktree" },
                "files": [{ "path": "src/value.txt" }],
                "context_lines": 1
            }
        }]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();

    // Interpose a deterministic Git wrapper that advances the authored base
    // ref immediately after the comparison. The compiler's final revision
    // verification must observe the move and reject the mixed build.
    let real_git = executable_on_path("git");
    let wrapper_dir = repository.path().join("wrapper-bin");
    fs::create_dir(&wrapper_dir).unwrap();
    let wrapper = wrapper_dir.join("git");
    let script = format!(
        "#!/bin/sh\nis_diff=0\nfor arg in \"$@\"; do\n  if [ \"$arg\" = diff ]; then is_diff=1; fi\ndone\n{git} \"$@\"\nstatus=$?\nif [ \"$is_diff\" = 1 ]; then\n  {git} -C {repo} update-ref refs/heads/moving {second} || exit $?\nfi\nexit $status\n",
        git = shell_quote(&real_git),
        repo = shell_quote(repository.path()),
        second = shell_quote(Path::new(&second)),
    );
    fs::write(&wrapper, script).unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(wrapper_dir.clone()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH is set")),
    ))
    .unwrap();

    let output = Command::new(learnc())
        .arg("build")
        .arg("--repo")
        .arg(repository.path())
        .arg(&lesson)
        .env("PATH", path)
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "moving-ref build unexpectedly succeeded"
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        report["diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|diagnostic| diagnostic["code"] == "repository.snapshot_changed")
    );
    assert!(!repository.path().join("moving.learn").exists());
    assert_eq!(
        git_stdout(repository.path(), &["rev-parse", "moving"]),
        second
    );
}

#[test]
fn artifact_public_projection_and_live_api_keep_quiz_answers_private() {
    let directory = TempDir::new("api-contract");
    let lesson = directory.path().join("lesson.json");
    fs::copy(manifest_dir().join("examples/inline-lesson.json"), &lesson).unwrap();
    output_success(Command::new(learnc()).arg("build").arg(&lesson));
    let artifact_path = directory.path().join("lesson.learn");
    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(&artifact_path).unwrap()).unwrap();
    let private_answer = artifact.private.answers[0].clone();
    let public = serde_json::to_value(project_artifact(&artifact)).unwrap();
    let public_text = serde_json::to_string(&public).unwrap();
    assert!(!public_text.contains("correct_choice_id"));
    assert!(!public_text.contains("explanation"));

    let mut server = ChildGuard::spawn(&artifact_path);
    let startup = server.startup();
    assert_eq!(
        startup["status"], "serving",
        "learn failed to start: {startup}"
    );
    let address = startup["url"]
        .as_str()
        .unwrap()
        .strip_prefix("http://")
        .unwrap()
        .trim_end_matches('/')
        .to_owned();

    let state = request_json(&address, "GET", "/api/v1/state", None);
    assert_eq!(state["lesson"]["title"], artifact.presentation.title);
    let state_text = serde_json::to_string(&state).unwrap();
    assert!(!state_text.contains("correct_choice_id"));
    assert!(!state_text.contains("explanation"));

    let choices = state["lesson"]["nodes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|node| node["node_id"] == private_answer.node_id.get())
        .unwrap()["choices"]
        .as_array()
        .unwrap();
    let wrong = choices
        .iter()
        .map(|choice| choice["choice_id"].as_u64().unwrap())
        .find(|choice| *choice != u64::from(private_answer.correct_choice_id.get()))
        .unwrap();
    let submit_path = format!("/api/v1/questions/{}/submit", private_answer.node_id.get());
    let wrong_result = request_json(
        &address,
        "POST",
        &submit_path,
        Some(&format!(r#"{{"choice_id":{wrong}}}"#)),
    );
    assert!(wrong_result["question"]["answer"].is_null());

    let correct_result = request_json(
        &address,
        "POST",
        &submit_path,
        Some(&format!(
            r#"{{"choice_id":{}}}"#,
            private_answer.correct_choice_id.get()
        )),
    );
    assert_eq!(
        correct_result["question"]["answer"]["choice_id"],
        private_answer.correct_choice_id.get()
    );
    assert_eq!(
        correct_result["question"]["answer"]["explanation"],
        private_answer.explanation
    );

    let refreshed = request_json(&address, "GET", "/api/v1/state", None);
    assert_eq!(refreshed["progress"]["completed_questions"], 1);
    server.stop();
}

#[test]
fn production_frontend_bundle_is_present_and_self_contained() {
    let dist = manifest_dir().join("web/dist");
    let index = fs::read_to_string(dist.join("index.html")).expect("prebuilt index.html");
    assert!(index.contains("id=\"root\""));

    let assets = dist.join("assets");
    let bundled = fs::read_dir(&assets)
        .expect("prebuilt assets directory")
        .map(|entry| entry.unwrap().path())
        .collect::<Vec<_>>();
    assert!(
        bundled
            .iter()
            .any(|path| path.extension().and_then(|x| x.to_str()) == Some("js"))
    );
    assert!(
        bundled
            .iter()
            .any(|path| path.extension().and_then(|x| x.to_str()) == Some("css"))
    );
    for path in referenced_assets(&index) {
        assert!(
            dist.join(path).is_file(),
            "index references a missing bundled asset"
        );
    }
}

/// Slow release hook: verifies `cargo install` produces both usable binaries
/// from the Rust package and needs no frontend toolchain at install time.
#[test]
#[ignore = "slow cargo-install smoke; run explicitly before release"]
fn cargo_install_smoke() {
    let installation = TempDir::new("cargo-install");
    let target = installation.path().join("target");
    output_success(
        Command::new("cargo")
            .arg("install")
            .arg("--path")
            .arg(manifest_dir())
            .arg("--root")
            .arg(installation.path())
            .arg("--force")
            .env("CARGO_TARGET_DIR", &target),
    );
    let suffix = std::env::consts::EXE_SUFFIX;
    let installed_learnc = installation
        .path()
        .join("bin")
        .join(format!("learnc{suffix}"));
    let installed_learn = installation
        .path()
        .join("bin")
        .join(format!("learn{suffix}"));
    assert!(installed_learnc.is_file());
    assert!(installed_learn.is_file());
    let schema = output_success(Command::new(installed_learnc).arg("schema"));
    let value: serde_json::Value = serde_json::from_slice(&schema.stdout).unwrap();
    assert_eq!(value["title"], "LessonSource");
}

fn repository_example() -> TempDir {
    let directory = TempDir::new("repository-example");
    let output = output_success(
        Command::new("bash")
            .arg(manifest_dir().join("examples/create-repository-lesson.sh"))
            .arg(directory.path()),
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        directory.path().to_string_lossy()
    );
    directory
}

fn configure_repository(directory: &Path) {
    git(directory, &["init", "-q"]);
    git(directory, &["config", "user.name", "Agent Teacher Tests"]);
    git(
        directory,
        &["config", "user.email", "tests@example.invalid"],
    );
    git(directory, &["config", "commit.gpgsign", "false"]);
}

fn git(directory: &Path, arguments: &[&str]) {
    output_success(Command::new("git").arg("-C").arg(directory).args(arguments));
}

fn git_stdout(directory: &Path, arguments: &[&str]) -> String {
    let output = output_success(Command::new("git").arg("-C").arg(directory).args(arguments));
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[cfg(unix)]
fn executable_on_path(name: &str) -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").expect("PATH is set"))
        .map(|directory| directory.join(name))
        .find(|candidate| candidate.is_file())
        .unwrap_or_else(|| panic!("could not find {name} on PATH"))
}

#[cfg(unix)]
fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\"'\"'"))
}

fn output_success(command: &mut Command) -> Output {
    let debug = format!("{command:?}");
    let output = command
        .output()
        .unwrap_or_else(|error| panic!("could not run {debug}: {error}"));
    assert!(
        output.status.success(),
        "command failed: {debug}\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

struct ChildGuard(Option<Child>);

impl ChildGuard {
    fn spawn(artifact: &Path) -> Self {
        let child = Command::new(learn())
            .arg("serve")
            .arg(artifact)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn learn server");
        Self(Some(child))
    }

    fn startup(&mut self) -> serde_json::Value {
        let stdout = self.0.as_mut().unwrap().stdout.take().unwrap();
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("read server startup line");
        serde_json::from_str(&line)
            .unwrap_or_else(|error| panic!("invalid server startup output {line:?}: {error}"))
    }

    fn stop(&mut self) {
        if let Some(mut child) = self.0.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for ChildGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

fn request_json(address: &str, method: &str, path: &str, body: Option<&str>) -> serde_json::Value {
    let body = body.unwrap_or("");
    let mut stream = TcpStream::connect(address).expect("connect to local lesson server");
    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {address}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
    .unwrap();
    stream.flush().unwrap();
    let mut response = Vec::new();
    stream.read_to_end(&mut response).unwrap();
    let split = response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .expect("HTTP response has headers");
    let headers = String::from_utf8_lossy(&response[..split]);
    assert!(
        headers.starts_with("HTTP/1.1 200"),
        "unexpected response: {headers}"
    );
    serde_json::from_slice(&response[split + 4..]).unwrap()
}

fn referenced_assets(index: &str) -> Vec<&str> {
    let mut assets = Vec::new();
    for marker in ["src=\"/", "href=\"/"] {
        let mut remaining = index;
        while let Some(start) = remaining.find(marker) {
            remaining = &remaining[start + marker.len()..];
            let Some(end) = remaining.find('"') else {
                break;
            };
            let value = &remaining[..end];
            if value.starts_with("assets/") {
                assets.push(value);
            }
            remaining = &remaining[end + 1..];
        }
    }
    assets
}

fn assert_schema_objects_are_closed(schema: &serde_json::Value, expected_fields: &[&str]) {
    fn visit<'a>(
        value: &'a serde_json::Value,
        expected_fields: &[&str],
        matches: &mut Vec<&'a serde_json::Value>,
    ) {
        match value {
            serde_json::Value::Object(object) => {
                if let Some(properties) =
                    object.get("properties").and_then(|value| value.as_object())
                    && expected_fields
                        .iter()
                        .all(|field| properties.contains_key(*field))
                {
                    matches.push(value);
                }
                for child in object.values() {
                    visit(child, expected_fields, matches);
                }
            }
            serde_json::Value::Array(values) => {
                for child in values {
                    visit(child, expected_fields, matches);
                }
            }
            _ => {}
        }
    }

    let mut matches = Vec::new();
    visit(schema, expected_fields, &mut matches);
    assert!(
        !matches.is_empty(),
        "schema had no object containing fields {expected_fields:?}"
    );
    for object in matches {
        assert_eq!(
            object["additionalProperties"], false,
            "schema object with fields {expected_fields:?} was open: {object}"
        );
    }
}
