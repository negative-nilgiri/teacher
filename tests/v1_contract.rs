use std::collections::{HashSet, VecDeque};
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};

use agent_teacher::artifact::{CompiledLesson, CompiledNodeContent, ResourceProvenance};
use agent_teacher::language::Language;
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

fn learnverify() -> &'static str {
    env!("CARGO_BIN_EXE_learnverify")
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
    let output = output_success(Command::new(learnc()).args(["schema", "--version", "2.5.0"]));
    let emitted: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(emitted, agent_teacher::source::source_json_schema());
    let default_output = output_success(Command::new(learnc()).arg("schema"));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&default_output.stdout).unwrap(),
        emitted
    );
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

    let legacy = output_success(Command::new(learnc()).args(["schema", "--version", "1.0.0"]));
    let legacy: serde_json::Value = serde_json::from_slice(&legacy.stdout).unwrap();
    assert_ne!(legacy, emitted);
    assert!(
        !schema_code_block_has_language(&legacy),
        "source schema 1.0.0 must not advertise the 1.1.0 language field"
    );
    assert!(schema_code_block_has_language(&emitted));
    let v1_1 = output_success(Command::new(learnc()).args(["schema", "--version", "1.1.0"]));
    let v1_1: serde_json::Value = serde_json::from_slice(&v1_1.stdout).unwrap();
    assert!(schema_code_block_has_language(&v1_1));
    assert!(!schema_block_has_property(&v1_1, "code", "caption"));
    assert!(!schema_block_has_property(&v1_1, "diff", "caption"));
    assert!(schema_block_has_property(&emitted, "code", "caption"));
    assert!(schema_block_has_property(&emitted, "diff", "caption"));
    let v1_2 = output_success(Command::new(learnc()).args(["schema", "--version", "1.2.0"]));
    let v1_2: serde_json::Value = serde_json::from_slice(&v1_2.stdout).unwrap();
    assert!(!schema_block_has_property(&v1_2, "code", "highlights"));
    assert!(schema_block_has_property(&emitted, "code", "highlights"));
    let v1_3 = output_success(Command::new(learnc()).args(["schema", "--version", "1.3.0"]));
    let v1_3: serde_json::Value = serde_json::from_slice(&v1_3.stdout).unwrap();
    let v2_0 = output_success(Command::new(learnc()).args(["schema", "--version", "2.0.0"]));
    let v2_0: serde_json::Value = serde_json::from_slice(&v2_0.stdout).unwrap();
    assert_eq!(
        schema_block(&v1_3, "multiple_choice")["properties"]["prompt"]["type"],
        "string"
    );
    assert_eq!(
        schema_block(&emitted, "multiple_choice")["properties"]["prompt"]["$ref"],
        "#/$defs/MarkdownSource"
    );
    assert!(
        v2_0["$defs"]["CodeHighlight"]["properties"]
            .get("annotation")
            .is_none()
    );
    assert!(
        emitted["$defs"]["CodeHighlight"]["properties"]
            .get("annotation")
            .is_some()
    );

    // Source schema 2.4.0 adds the run_code block; 2.3.0 keeps rejecting it.
    assert!(schema_block_has_property(&emitted, "run_code", "of"));
    assert!(schema_block_has_property(&emitted, "run_code", "argv"));
    let v2_3 = output_success(Command::new(learnc()).args(["schema", "--version", "2.3.0"]));
    let v2_3: serde_json::Value = serde_json::from_slice(&v2_3.stdout).unwrap();
    assert_ne!(v2_3, emitted);
    assert!(find_schema_block(&v2_3, "run_code").is_none());
    assert!(find_schema_block(&v2_3, "multiple_choice").is_some());

    // Source schema 2.5.0 adds the external_artifact block; 2.4.0 keeps
    // rejecting it and keeps run_code.
    assert!(schema_block_has_property(
        &emitted,
        "external_artifact",
        "fallback"
    ));
    assert!(schema_block_has_property(
        &emitted,
        "external_artifact",
        "alt"
    ));
    let v2_4 = output_success(Command::new(learnc()).args(["schema", "--version", "2.4.0"]));
    let v2_4: serde_json::Value = serde_json::from_slice(&v2_4.stdout).unwrap();
    assert_ne!(v2_4, emitted);
    assert!(find_schema_block(&v2_4, "external_artifact").is_none());
    assert!(find_schema_block(&v2_4, "run_code").is_some());

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
fn compiler_freezes_file_backed_question_prompts() {
    let root = TempDir::new("question-prompt");
    fs::write(
        root.path().join("question.md"),
        "## Check the queue\n\nWhich call removes the oldest item?\n",
    )
    .unwrap();
    let lesson = root.path().join("lesson.json");
    let source = serde_json::json!({
        "schema_version": "2.1.0",
        "title": "Prompt source",
        "blocks": [{
            "type": "multiple_choice",
            "id": "question",
            "prompt": {"kind": "file", "path": "question.md"},
            "choices": [
                {"content": "`pop_front`", "correct": true},
                {"content": "`pop_back`"}
            ],
            "explanation": "The oldest item is at the front."
        }]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();

    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--root")
            .arg(root.path())
            .arg(&lesson),
    );
    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(root.path().join("lesson.learn")).unwrap()).unwrap();
    assert_eq!(artifact.provenance.source_schema_version.as_str(), "2.1.0");
    assert_eq!(artifact.artifact_version.as_str(), "1.9.0");
    assert_eq!(
        artifact.provenance.compiler_version,
        env!("CARGO_PKG_VERSION")
    );
    assert!(matches!(
        &artifact.presentation.nodes[0].content,
        CompiledNodeContent::MultipleChoice { prompt, .. }
            if prompt == "## Check the queue\n\nWhich call removes the oldest item?\n"
    ));
}

#[test]
fn compiler_freezes_normalized_and_inferred_languages_into_public_data() {
    let root = TempDir::new("languages");
    fs::create_dir(root.path().join("src")).unwrap();
    fs::write(root.path().join("src/tool.py"), "print('hello')\n").unwrap();
    let lesson = root.path().join("languages.json");
    let source = serde_json::json!({
        "schema_version": "1.1.0",
        "title": "Languages",
        "blocks": [
            {
                "type": "code",
                "id": "explicit-alias",
                "language": "JS",
                "source": {"kind": "inline", "content": "console.log('hello')"}
            },
            {
                "type": "code",
                "id": "inferred-file",
                "source": {"kind": "file", "path": "src/tool.py"}
            },
            {
                "type": "code",
                "id": "safe-fallback",
                "language": "future-language",
                "source": {"kind": "inline", "content": "some content"}
            },
            {
                "type": "diff",
                "id": "yaml-change",
                "source": {
                    "kind": "inline",
                    "content": "--- config.yaml\n+++ config.yaml\n@@ -1 +1 @@\n-old: true\n+new: true\n"
                }
            }
        ]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();

    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--root")
            .arg(root.path())
            .arg(&lesson),
    );

    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(root.path().join("languages.learn")).unwrap()).unwrap();
    let code_languages = artifact.presentation.nodes[..3]
        .iter()
        .map(|node| match &node.content {
            CompiledNodeContent::Code { language, .. } => *language,
            _ => panic!("expected code node"),
        })
        .collect::<Vec<_>>();
    assert_eq!(
        code_languages,
        [Language::JavaScript, Language::Python, Language::Text]
    );
    let CompiledNodeContent::Diff { diff, .. } = &artifact.presentation.nodes[3].content else {
        panic!("expected diff node")
    };
    assert_eq!(diff.files[0].language, Language::Yaml);

    let public = serde_json::to_value(project_artifact(&artifact)).unwrap();
    assert_eq!(public["nodes"][0]["language"], "javascript");
    assert_eq!(public["nodes"][1]["language"], "python");
    assert_eq!(public["nodes"][2]["language"], "text");
    assert_eq!(public["nodes"][3]["files"][0]["language"], "yaml");
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
        ("run_code_no_runner.json", "source.run_code.no_runner"),
        (
            "external_artifact_wrong_extension.json",
            "source.external_artifact.extension_not_allowed",
        ),
        (
            "external_artifact_path_file.json",
            "source.external_artifact.invalid_file_name",
        ),
        (
            "external_artifact_blank_fallback.json",
            "source.content.empty",
        ),
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
    for (binary, name) in [
        (learnc(), "learnc"),
        (learn(), "learn"),
        (learnverify(), "learnverify"),
    ] {
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
        assert_eq!(version["version"], env!("CARGO_PKG_VERSION"));
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
        let options = help["help"]["options"].as_array().unwrap();
        assert!(options.iter().any(|option| option["long"] == "--root"));
        assert!(options.iter().all(|option| option["long"] != "--repo"));
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
fn learnverify_is_optional_agent_readable_and_source_isolated() {
    // Without credentials the checks are skipped visibly and the run succeeds.
    let missing_credential = Command::new(learnverify())
        .args(["--no-cache", "tests/fixtures/smoke-lesson.json"])
        .current_dir(manifest_dir())
        .env_remove("TYPESAFE_API_KEY")
        .output()
        .unwrap();
    assert!(missing_credential.status.success());
    let report: serde_json::Value = serde_json::from_slice(&missing_credential.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "verify.unavailable");
    assert_eq!(report["diagnostics"][0]["fatal"], false);

    // A lesson that does not compile fails exactly as `learnc check` does.
    let not_json = Command::new(learnverify())
        .arg("lesson.learn")
        .output()
        .unwrap();
    assert!(!not_json.status.success());
    let report: serde_json::Value = serde_json::from_slice(&not_json.stdout).unwrap();
    assert_eq!(report["ok"], false);
    assert_eq!(report["diagnostics"][0]["code"], "compiler.input.not_json");

    let source_root = manifest_dir().join("src");
    let allowed_file = source_root.join("bin/learnverify.rs");
    let allowed_library_root = source_root.join("lib.rs");
    let allowed_module = source_root.join("learnverify.rs");
    let allowed_directory = source_root.join("learnverify");
    let mut pending = vec![source_root];
    while let Some(directory) = pending.pop() {
        for entry in fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|extension| extension.to_str()) != Some("rs")
                || path == allowed_file
                || path == allowed_library_root
                || path == allowed_module
                || path.starts_with(&allowed_directory)
            {
                continue;
            }
            let source = fs::read_to_string(&path).unwrap();
            assert!(
                !source.contains("learnverify"),
                "{} references the isolated learnverify module",
                path.display()
            );
        }
    }
}

#[test]
fn repository_example_checks_builds_and_freezes_relative_provenance() {
    let repository = repository_example();
    let lesson = repository.path().join("lesson.json");

    let checked = output_success(
        Command::new(learnc())
            .arg("check")
            .arg("--root")
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
            .arg("--root")
            .arg(repository.path())
            .arg(&lesson),
    );
    let artifact_path = repository.path().join("lesson.learn");
    let bytes = fs::read(&artifact_path).unwrap();
    let artifact: CompiledLesson = serde_json::from_slice(&bytes).unwrap();
    agent_teacher::artifact::validate_artifact(&artifact).unwrap();
    assert_eq!(artifact.presentation.nodes.len(), 6);
    assert_eq!(artifact.private.answers.len(), 1);

    let public = serde_json::to_value(project_artifact(&artifact)).unwrap();
    assert_eq!(public["nodes"][1]["filename"], "queue.rs");
    assert_eq!(public["nodes"][2]["filename"], "queue.rs");
    assert_eq!(public["nodes"][1]["highlights"][0]["lines"][0]["start"], 2);
    assert_eq!(public["nodes"][1]["highlights"][0]["color"], "green");
    assert!(
        public["nodes"][1]["highlights"][0]["annotation"]
            .as_str()
            .unwrap()
            .contains("preserving FIFO")
    );
    assert_eq!(public["nodes"][2]["highlights"][0]["lines"][0]["start"], 2);
    assert_eq!(public["nodes"][2]["highlights"][0]["color"], "red");
    assert!(
        public["nodes"][2]["highlights"][0]["annotation"]
            .as_str()
            .unwrap()
            .contains("removal still happens")
    );

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
fn filesystem_root_resolves_two_unrelated_sibling_repositories() {
    let root = sibling_repository_root("sibling-repositories");
    let lesson = root.path().join("siblings.json");
    let source = serde_json::json!({
        "schema_version": "1.0.0",
        "title": "Sibling repositories",
        "blocks": [
            {
                "type": "code",
                "id": "repo-a-at-head",
                "source": {
                    "kind": "git_blob",
                    "revision": "HEAD",
                    "path": "repo-a/src/value.txt"
                }
            },
            {
                "type": "code",
                "id": "repo-b-at-head",
                "source": {
                    "kind": "git_blob",
                    "revision": "HEAD",
                    "path": "repo-b/src/value.txt"
                }
            },
            {
                "type": "diff",
                "id": "repo-a-change",
                "source": {
                    "kind": "git",
                    "base": "HEAD",
                    "target": { "kind": "worktree" },
                    "files": [{ "path": "repo-a/src/value.txt" }],
                    "context_lines": 1
                }
            },
            {
                "type": "diff",
                "id": "repo-b-change",
                "source": {
                    "kind": "git",
                    "base": "HEAD",
                    "target": { "kind": "worktree" },
                    "files": [{ "path": "repo-b/src/value.txt" }],
                    "context_lines": 1
                }
            }
        ]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();

    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--root")
            .arg(root.path())
            .arg(&lesson),
    );

    let bytes = fs::read(root.path().join("siblings.learn")).unwrap();
    let artifact: CompiledLesson = serde_json::from_slice(&bytes).unwrap();
    agent_teacher::artifact::validate_artifact(&artifact).unwrap();
    assert!(
        !String::from_utf8(bytes)
            .unwrap()
            .contains(&root.path().to_string_lossy().to_string())
    );

    for (source_id, owner, expected_content) in [
        ("repo-a-at-head", "repo-a", "a-before\n"),
        ("repo-b-at-head", "repo-b", "b-before\n"),
    ] {
        let node = artifact
            .presentation
            .nodes
            .iter()
            .find(|node| node.source_id == source_id)
            .unwrap();
        let CompiledNodeContent::Code {
            content,
            provenance:
                ResourceProvenance::GitBlob {
                    repository,
                    path,
                    revision_object_id,
                    content_object_id,
                    ..
                },
            ..
        } = &node.content
        else {
            panic!("{source_id} was not a Git blob")
        };
        assert_eq!(content, expected_content);
        assert_eq!(repository, owner);
        assert_eq!(path, &format!("{owner}/src/value.txt"));
        assert_eq!(revision_object_id.len(), 40);
        assert_eq!(content_object_id.len(), 40);
    }

    for (source_id, owner) in [("repo-a-change", "repo-a"), ("repo-b-change", "repo-b")] {
        let node = artifact
            .presentation
            .nodes
            .iter()
            .find(|node| node.source_id == source_id)
            .unwrap();
        let CompiledNodeContent::Diff {
            diff,
            provenance:
                ResourceProvenance::GitDiff {
                    repository,
                    files,
                    base_object_id,
                    sha256,
                    ..
                },
            ..
        } = &node.content
        else {
            panic!("{source_id} was not a Git diff")
        };
        let expected_path = format!("{owner}/src/value.txt");
        assert_eq!(repository, owner);
        assert_eq!(files, std::slice::from_ref(&expected_path));
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].display_path(), Some(expected_path.as_str()));
        assert_eq!(base_object_id.len(), 40);
        assert_eq!(sha256.len(), 64);
    }
}

#[test]
fn one_git_diff_rejects_files_from_sibling_repositories() {
    let root = sibling_repository_root("mixed-sibling-repositories");
    let lesson = root.path().join("mixed.json");
    let source = serde_json::json!({
        "schema_version": "1.0.0",
        "title": "Invalid mixed repository diff",
        "blocks": [{
            "type": "diff",
            "id": "mixed-change",
            "source": {
                "kind": "git",
                "base": "HEAD",
                "target": { "kind": "worktree" },
                "files": [
                    { "path": "repo-a/src/value.txt" },
                    { "path": "repo-b/src/value.txt" }
                ],
                "context_lines": 1
            }
        }]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();

    let output = Command::new(learnc())
        .arg("check")
        .arg("--root")
        .arg(root.path())
        .arg(&lesson)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let diagnostic = report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .find(|diagnostic| diagnostic["code"] == "repository.mixed_owners")
        .expect("mixed-owner diagnostic");
    assert_eq!(diagnostic["pointer"], "/blocks/0/source");
    let message = diagnostic["message"].as_str().unwrap();
    assert!(message.contains("repo-a: repo-a/src/value.txt"));
    assert!(message.contains("repo-b: repo-b/src/value.txt"));
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
            .arg("--root")
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
        .arg("--root")
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
    // References: the artifact path as served, and frozen provenance per
    // non-quiz node, still without any private answer data.
    assert_eq!(
        state["lesson"]["artifact_path"],
        artifact_path.to_string_lossy().as_ref()
    );
    let nodes = state["lesson"]["nodes"].as_array().unwrap();
    assert!(
        nodes
            .iter()
            .any(|node| node["reference"]["kind"].is_string())
    );
    assert!(
        nodes
            .iter()
            .filter(|node| node["type"] == "multiple_choice")
            .all(|node| node.get("reference").is_none())
    );
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
fn run_blocks_compile_serve_and_stay_display_only() {
    let root = TempDir::new("run-code");
    fs::write(
        root.path().join("greet.py"),
        "name = \"Ada\"\nprint(f\"hi {name}\")\n",
    )
    .unwrap();
    fs::write(root.path().join("greet.out"), "hi Ada\n").unwrap();
    let lesson = root.path().join("lesson.json");
    let source = serde_json::json!({
        "schema_version": "2.4.0",
        "title": "Run it",
        "blocks": [
            {
                "type": "code",
                "id": "shown",
                "source": {"kind": "file", "path": "greet.py", "lines": {"start": 2, "end": 2}}
            },
            {
                "type": "run_code",
                "id": "run-shown",
                "of": "shown",
                "caption": "Runs the [shown line](#shown).",
                "expected_output": {"kind": "inline", "content": "hi Ada\n"}
            },
            {
                "type": "run_code",
                "id": "run-own",
                "language": "python",
                "source": {"kind": "file", "path": "greet.py"},
                "timeout_secs": 5,
                "expected_output": {"kind": "file", "path": "greet.out"}
            },
            {
                "type": "run_code",
                "id": "run-ruby",
                "language": "ruby",
                "source": {"kind": "inline", "content": "puts 1 + 1"},
                "argv": ["ruby", "{file}"]
            }
        ]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--root")
            .arg(root.path())
            .arg(&lesson),
    );

    let artifact_path = root.path().join("lesson.learn");
    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(&artifact_path).unwrap()).unwrap();
    assert_eq!(artifact.artifact_version.as_str(), "1.9.0");
    // The compiler resolves the command and scratch file; nothing else does.
    let frozen: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact_path).unwrap()).unwrap();
    let nodes = frozen["presentation"]["nodes"].as_array().unwrap();
    assert_eq!(
        nodes[1]["code"],
        serde_json::json!({"kind": "of", "node": 0})
    );
    assert_eq!(nodes[1]["argv"], serde_json::json!(["python3", "{file}"]));
    assert_eq!(nodes[1]["file_name"], "main.py");
    assert_eq!(nodes[1]["timeout_secs"], 10);
    assert_eq!(nodes[2]["code"]["kind"], "own");
    assert_eq!(nodes[2]["expected_output"], "hi Ada\n");
    assert_eq!(nodes[3]["argv"], serde_json::json!(["ruby", "{file}"]));
    // Languages the compiler does not know run from a plain-text scratch file.
    assert_eq!(nodes[3]["file_name"], "main.txt");

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
    let nodes = state["lesson"]["nodes"].as_array().unwrap();
    let of = &nodes[1];
    assert_eq!(of["type"], "run_code");
    assert_eq!(of["of"], 0);
    assert!(of.get("content").is_none());
    assert_eq!(of["language"], "python");
    assert_eq!(of["timeout_secs"], 10);
    assert_eq!(of["expected_output"], "hi Ada\n");
    // An `of` block refers to the file the code it runs came from.
    assert_eq!(of["reference"], nodes[0]["reference"]);
    assert_eq!(of["reference"]["kind"], "file");
    let own = &nodes[2];
    assert_eq!(own["content"], "name = \"Ada\"\nprint(f\"hi {name}\")\n");
    assert_eq!(own["filename"], "greet.py");
    assert_eq!(own["first_line"], 1);
    assert_eq!(own["timeout_secs"], 5);
    assert_eq!(own["expected_output"], "hi Ada\n");
    assert_eq!(nodes[3]["language"], "text");
    assert_eq!(nodes[3]["reference"]["kind"], "inline");
    let public = serde_json::to_string(&state).unwrap();
    assert!(!public.contains("argv") && !public.contains("file_name"));
    // `of` and a link in the caption both reach the browser's link table.
    assert_eq!(state["lesson"]["links"]["shown"]["target"], 0);
    // Without --allow-run nothing runs and no token exists.
    assert_eq!(
        state["run"],
        serde_json::json!({"enabled": false, "token": null})
    );
    assert_eq!(state["runs"], serde_json::json!({}));
    server.stop();

    // Older schemas keep rejecting the new block.
    source_with_version(&lesson, &source, "2.3.0");
    let output = Command::new(learnc())
        .arg("check")
        .arg("--root")
        .arg(root.path())
        .arg(&lesson)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "source.deserialize");
}

/// Start `learn serve`, wait for its startup record, and return its address.
fn serve_address(server: &mut ChildGuard) -> (String, serde_json::Value) {
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
    (address, startup)
}

fn run_request(address: &str, node: u32, headers: &[(&str, &str)]) -> Http {
    http(
        address,
        "POST",
        &format!("/api/v1/runs/{node}"),
        headers,
        "",
    )
}

fn process_exists(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .stderr(Stdio::null())
        .status()
        .unwrap()
        .success()
}

#[cfg(unix)]
#[test]
fn run_blocks_run_only_with_allow_run_a_token_and_a_loopback_host() {
    let root = TempDir::new("run-code-serve");
    // Scripts report to files the test owns, so it can see what did and did
    // not start.
    let marker = |name: &str| root.path().join(name);
    let lesson = root.path().join("lesson.json");
    let source = serde_json::json!({
        "schema_version": "2.4.0",
        "title": "Run it",
        "blocks": [
            {
                "type": "code", "id": "shown", "language": "shell",
                "source": {"kind": "inline", "content": "echo from-shown\necho oops >&2\n"}
            },
            {"type": "run_code", "id": "run-shown", "of": "shown"},
            {
                "type": "run_code", "id": "fails", "language": "shell",
                "source": {"kind": "inline", "content": "echo before\nexit 3\n"}
            },
            {
                "type": "run_code", "id": "hangs", "language": "shell", "timeout_secs": 1,
                "source": {"kind": "inline", "content": "sleep 30 &\necho $!\nsleep 30\n"}
            },
            {
                "type": "run_code", "id": "noisy", "language": "shell",
                "source": {"kind": "inline",
                    "content": "head -c 100000 /dev/zero | tr '\\0' x\n"}
            },
            {
                "type": "run_code", "id": "slow", "language": "shell", "timeout_secs": 10,
                "source": {"kind": "inline", "content": format!(
                    "touch {}\nsleep 2\necho slow-done\n", shell_quote(&marker("slow-started")))}
            },
            {
                "type": "run_code", "id": "probe", "language": "shell",
                "source": {"kind": "inline", "content": format!(
                    "touch {}\n", shell_quote(&marker("probe-ran")))}
            }
        ]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    output_success(
        Command::new(learnc())
            .arg("build")
            .arg("--root")
            .arg(root.path())
            .arg(&lesson),
    );
    let artifact_path = root.path().join("lesson.learn");

    // Off by default: no token, no run, and nothing is spawned.
    let mut off = ChildGuard::spawn(&artifact_path);
    let (address, startup) = serve_address(&mut off);
    assert_eq!(startup["run_enabled"], false);
    let state = http(&address, "GET", "/api/v1/state", &[], "").json();
    assert_eq!(
        state["run"],
        serde_json::json!({"enabled": false, "token": null})
    );
    let refused = run_request(&address, 6, &[("X-Learn-Token", "anything")]);
    assert_eq!(refused.status, 403);
    assert_eq!(refused.json()["code"], "run_disabled");
    assert!(!marker("probe-ran").exists());
    off.stop();

    let mut server = ChildGuard::spawn_with(&artifact_path, &["--allow-run"]);
    let (address, startup) = serve_address(&mut server);
    assert_eq!(startup["run_enabled"], true);
    assert!(!startup.to_string().contains("token"));

    let state = http(&address, "GET", "/api/v1/state", &[], "").json();
    assert_eq!(state["run"]["enabled"], true);
    let token = state["run"]["token"].as_str().unwrap().to_owned();
    assert_eq!(token.len(), 32);
    assert!(token.bytes().all(|byte| byte.is_ascii_hexdigit()));
    assert_eq!(state["runs"], serde_json::json!({}));
    let public = state.to_string();
    assert!(!public.contains("argv") && !public.contains("file_name"));
    let ok = [("X-Learn-Token", token.as_str())];

    // A missing or wrong token, and a page that reached us by another name.
    for headers in [&[][..], &[("X-Learn-Token", "0000")][..]] {
        let rejected = run_request(&address, 6, headers);
        assert_eq!(rejected.status, 403);
        assert_eq!(rejected.json()["code"], "invalid_token");
    }
    for host in ["evil.example", "evil.example:80", "127.0.0.1:1"] {
        let rejected = run_request(
            &address,
            6,
            &[("X-Learn-Token", token.as_str()), ("Host", host)],
        );
        assert_eq!(rejected.status, 403, "{host}");
        assert_eq!(rejected.json()["code"], "invalid_host");
    }
    // The preflight a cross-origin page needs is never granted.
    let preflight = http(
        &address,
        "OPTIONS",
        "/api/v1/runs/6",
        &[
            ("Origin", "http://evil.example"),
            ("Access-Control-Request-Method", "POST"),
            ("Access-Control-Request-Headers", "x-learn-token"),
        ],
        "",
    );
    assert!(
        !preflight
            .headers
            .to_lowercase()
            .contains("access-control-allow")
    );
    assert!(!marker("probe-ran").exists(), "a rejected request ran code");

    // Only run blocks run.
    let missing = run_request(&address, 99, &ok);
    assert_eq!(
        (missing.status, missing.json()["code"].clone()),
        (404, "unknown_node".into())
    );
    let code_block = run_request(&address, 0, &ok);
    assert_eq!(
        (code_block.status, code_block.json()["code"].clone()),
        (404, "not_runnable".into())
    );

    // A run of code the lesson shows (`of`) returns both streams.
    let response = run_request(&address, 1, &ok);
    assert_eq!(response.status, 200);
    let run = response.json()["run"].clone();
    assert_eq!(run["stdout"], "from-shown\n");
    assert_eq!(run["stderr"], "oops\n");
    assert_eq!(run["exit_code"], 0);
    assert_eq!(run["timed_out"], false);
    assert_eq!(run["truncated"], false);
    assert!(run["duration_ms"].is_u64());
    assert!(run.get("error").is_none());

    // A failing program is a result, not an HTTP error.
    let failed = run_request(&address, 2, &ok);
    assert_eq!(failed.status, 200);
    let failed = failed.json()["run"].clone();
    assert_eq!(
        (failed["stdout"].clone(), failed["exit_code"].clone()),
        ("before\n".into(), 3.into())
    );

    // Timeout: the program and the child it started are both gone.
    let started = std::time::Instant::now();
    let hung = run_request(&address, 3, &ok);
    assert_eq!(hung.status, 200);
    let hung = hung.json()["run"].clone();
    assert_eq!(hung["timed_out"], true);
    assert_eq!(hung["exit_code"], serde_json::Value::Null);
    assert!(started.elapsed() < std::time::Duration::from_secs(10));
    let child = hung["stdout"].as_str().unwrap().trim().to_owned();
    assert!(!child.is_empty());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while process_exists(&child) && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(
        !process_exists(&child),
        "the child of a timed-out run survived"
    );

    // Output past 64 KiB is cut and says so.
    let noisy = run_request(&address, 4, &ok).json()["run"].clone();
    assert_eq!(noisy["stdout"].as_str().unwrap().len(), 64 * 1024);
    assert_eq!(noisy["truncated"], true);

    // One run per block at a time.
    let slow = {
        let (address, token) = (address.clone(), token.clone());
        std::thread::spawn(move || run_request(&address, 5, &[("X-Learn-Token", &token)]))
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !marker("slow-started").exists() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        marker("slow-started").exists(),
        "the slow run never started"
    );
    let busy = run_request(&address, 5, &ok);
    assert_eq!(
        (busy.status, busy.json()["code"].clone()),
        (409, "run_in_progress".into())
    );
    let slow = slow.join().unwrap();
    assert_eq!(slow.status, 200);
    assert_eq!(slow.json()["run"]["stdout"], "slow-done\n");

    // The last result of each block is session state: a fresh load sees it,
    // and quiz progress is untouched.
    let refreshed = http(&address, "GET", "/api/v1/state", &[], "").json();
    assert_eq!(refreshed["runs"]["1"], run);
    assert_eq!(refreshed["runs"]["2"]["exit_code"], 3);
    assert_eq!(refreshed["runs"]["3"]["timed_out"], true);
    assert_eq!(refreshed["runs"]["5"]["stdout"], "slow-done\n");
    assert!(refreshed["runs"].get("6").is_none());
    assert_eq!(refreshed["progress"]["completed_questions"], 0);
    assert_eq!(refreshed["progress"]["questions"], serde_json::json!({}));
    let server_pid = server.pid();
    server.stop();

    // Nothing the server ran leaves its scratch directory behind.
    let leftovers = fs::read_dir(std::env::temp_dir())
        .unwrap()
        .flatten()
        .filter(|entry| {
            entry
                .file_name()
                .to_string_lossy()
                .starts_with(&format!("learn-run-{server_pid}-"))
        })
        .count();
    assert_eq!(leftovers, 0);
}

#[test]
fn external_artifact_blocks_compile_without_inputs_and_serve_their_fallback_without_a_sidecar() {
    let root = TempDir::new("external-artifact");
    let lesson = root.path().join("lesson.json");
    let source = serde_json::json!({
        "schema_version": "2.5.0",
        "title": "Queues in motion",
        "blocks": [
            {
                "type": "markdown",
                "id": "intro",
                "source": {"kind": "inline", "content": "A queue releases its oldest item first."}
            },
            {
                "type": "external_artifact",
                "id": "queue-demo",
                "kind": "video",
                "file": "queue-demo.mp4",
                "alt": "Animation of items entering and leaving a FIFO queue",
                "fallback": "Items leave from the **front**, as [the introduction](#intro) says.",
                "caption": "Watch which item leaves first."
            }
        ]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();

    // No file exists, and no root or repository is needed to build.
    let missing_root = root.path().join("no-such-root");
    for command in ["check", "build"] {
        output_success(
            Command::new(learnc())
                .arg(command)
                .arg("--root")
                .arg(&missing_root)
                .arg(&lesson),
        );
    }
    let artifact_path = root.path().join("lesson.learn");
    let frozen: serde_json::Value =
        serde_json::from_slice(&fs::read(&artifact_path).unwrap()).unwrap();
    assert_eq!(frozen["artifact_version"], "1.9.0");
    let node = &frozen["presentation"]["nodes"][1];
    assert_eq!(node["type"], "external_artifact");
    assert_eq!(node["file"], "queue-demo.mp4");
    assert!(node.get("provenance").is_none());
    assert_eq!(frozen["presentation"]["links"]["intro"]["target"], 0);
    assert!(!root.path().join("queue-demo.mp4").exists());

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
    let node = &state["lesson"]["nodes"][1];
    assert_eq!(node["type"], "external_artifact");
    assert_eq!(node["kind"], "video");
    assert_eq!(node["file"], "queue-demo.mp4");
    assert_eq!(
        node["alt"],
        "Animation of items entering and leaving a FIFO queue"
    );
    assert_eq!(
        node["fallback"],
        "Items leave from the **front**, as [the introduction](#intro) says."
    );
    assert_eq!(node["caption"], "Watch which item leaves first.");
    // Like a quiz, the block has no source resource to refer to.
    assert!(node.get("reference").is_none());
    assert_eq!(state["lesson"]["links"]["intro"]["target"], 0);
    // The sidecar directory holds nothing, so the block says its file is absent.
    assert_eq!(node["available"], false);
    assert!(node.get("version").is_none());
    server.stop();

    // Older schemas keep rejecting the new block.
    source_with_version(&lesson, &source, "2.4.0");
    let output = Command::new(learnc())
        .arg("check")
        .arg(&lesson)
        .output()
        .unwrap();
    assert!(!output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["diagnostics"][0]["code"], "source.deserialize");
}

/// A valid 1x1 PNG.
const TINY_PNG: [u8; 69] = [
    0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0x00, 0x00, 0x0d, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x00, 0x00, 0x00, 0x90, 0x77, 0x53,
    0xde, 0x00, 0x00, 0x00, 0x0c, 0x49, 0x44, 0x41, 0x54, 0x78, 0x9c, 0x63, 0xf8, 0xcf, 0xc0, 0x00,
    0x00, 0x03, 0x01, 0x01, 0x00, 0xc9, 0xfe, 0x92, 0xef, 0x00, 0x00, 0x00, 0x00, 0x49, 0x45, 0x4e,
    0x44, 0xae, 0x42, 0x60, 0x82,
];

fn header_value(response: &Http, name: &str) -> Option<String> {
    response.headers.lines().skip(1).find_map(|line| {
        let (key, value) = line.split_once(':')?;
        key.eq_ignore_ascii_case(name)
            .then(|| value.trim().to_owned())
    })
}

#[test]
fn sidecar_media_files_are_served_with_ranges_and_looked_up_on_every_request() {
    let root = TempDir::new("sidecar");
    let lesson = root.path().join("lesson.json");
    let media = |id: &str, kind: &str, file: &str| {
        serde_json::json!({
            "type": "external_artifact",
            "id": id,
            "kind": kind,
            "file": file,
            "alt": format!("Alt of {id}"),
            "fallback": format!("Fallback of {id}")
        })
    };
    let source = serde_json::json!({
        "schema_version": "2.5.0",
        "title": "Queues in motion",
        "blocks": [
            {
                "type": "markdown",
                "id": "intro",
                "source": {"kind": "inline", "content": "A queue releases its oldest item first."}
            },
            media("picture", "image", "picture.PNG"),
            media("bell", "audio", "bell.mp3"),
            media("demo", "video", "demo.mp4"),
            media("later", "video", "later.webm"),
            media("folder", "image", "folder.png"),
            media("escape", "image", "escape.png"),
            media("figure", "image", "figure.svg"),
        ]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    output_success(Command::new(learnc()).arg("build").arg(&lesson));

    // `lesson.learn` has its files in `lesson.assets`, a file at a time.
    let sidecar = root.path().join("lesson.assets");
    fs::create_dir(&sidecar).unwrap();
    fs::write(sidecar.join("picture.PNG"), TINY_PNG).unwrap();
    fs::write(sidecar.join("bell.mp3"), b"ID3-fake-audio").unwrap();
    fs::write(sidecar.join("demo.mp4"), b"0123456789").unwrap();
    fs::write(
        sidecar.join("figure.svg"),
        b"<svg xmlns='http://www.w3.org/2000/svg'/>",
    )
    .unwrap();
    // A directory with a file's name, and a link that leaves the directory.
    fs::create_dir(sidecar.join("folder.png")).unwrap();
    fs::write(root.path().join("secret.png"), b"secret").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.path().join("secret.png"), sidecar.join("escape.png")).unwrap();

    // Started from the lesson's directory with a relative path.
    let mut server = ChildGuard(Some(
        Command::new(learn())
            .current_dir(root.path())
            .args(["serve", "lesson.learn"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn learn server"),
    ));
    let (address, startup) = serve_address(&mut server);
    let mut fields: Vec<_> = startup.as_object().unwrap().keys().cloned().collect();
    fields.sort();
    assert_eq!(fields, ["artifact", "run_enabled", "status", "url"]);

    let availability = |address: &str| -> Vec<(String, bool, Option<String>)> {
        let state = request_json(address, "GET", "/api/v1/state", None);
        state["lesson"]["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|node| node["type"] == "external_artifact")
            .map(|node| {
                (
                    node["source_id"].as_str().unwrap().to_owned(),
                    node["available"].as_bool().unwrap(),
                    node["version"].as_str().map(str::to_owned),
                )
            })
            .collect()
    };
    let before = availability(&address);
    let flags: Vec<_> = before
        .iter()
        .map(|(id, available, _)| (id.as_str(), *available))
        .collect();
    assert_eq!(
        flags,
        [
            ("picture", true),
            ("bell", true),
            ("demo", true),
            ("later", false),
            ("folder", false),
            ("escape", false),
            ("figure", true),
        ]
    );
    // Only an available file has a version, which is its size and time.
    for (id, available, version) in &before {
        assert_eq!(version.is_some(), *available, "{id}");
    }
    assert!(before[2].2.as_ref().unwrap().starts_with("10-"));

    // The whole file, with the headers a player needs.
    let whole = http(&address, "GET", "/api/v1/artifacts/3/file", &[], "");
    assert_eq!(whole.status, 200, "{}", whole.headers);
    assert_eq!(whole.body, b"0123456789");
    assert_eq!(header_value(&whole, "content-type").unwrap(), "video/mp4");
    assert_eq!(header_value(&whole, "content-length").unwrap(), "10");
    assert_eq!(header_value(&whole, "accept-ranges").unwrap(), "bytes");
    assert_eq!(header_value(&whole, "cache-control").unwrap(), "no-cache");
    assert!(header_value(&whole, "content-range").is_none());
    // The cache-busting query is accepted and changes nothing.
    let versioned = http(&address, "GET", "/api/v1/artifacts/3/file?v=10-1", &[], "");
    assert_eq!(versioned.body, b"0123456789");

    // Each other kind has its own type and exact bytes.
    for (node, content_type, bytes) in [
        (1, "image/png", TINY_PNG.to_vec()),
        (2, "audio/mpeg", b"ID3-fake-audio".to_vec()),
        (
            7,
            "image/svg+xml",
            b"<svg xmlns='http://www.w3.org/2000/svg'/>".to_vec(),
        ),
    ] {
        let response = http(
            &address,
            "GET",
            &format!("/api/v1/artifacts/{node}/file"),
            &[],
            "",
        );
        assert_eq!(response.status, 200, "{node}");
        assert_eq!(
            header_value(&response, "content-type").unwrap(),
            content_type
        );
        assert_eq!(
            header_value(&response, "content-length").unwrap(),
            bytes.len().to_string()
        );
        assert_eq!(response.body, bytes, "{node}");
    }

    // Ranges: a slice, an open end, a suffix, and one past the end.
    for (range, bytes, content_range) in [
        ("bytes=2-4", "234", "bytes 2-4/10"),
        ("bytes=6-", "6789", "bytes 6-9/10"),
        ("bytes=-3", "789", "bytes 7-9/10"),
        ("bytes=8-100", "89", "bytes 8-9/10"),
    ] {
        let response = http(
            &address,
            "GET",
            "/api/v1/artifacts/3/file",
            &[("Range", range)],
            "",
        );
        assert_eq!(response.status, 206, "{range}: {}", response.headers);
        assert_eq!(response.body, bytes.as_bytes(), "{range}");
        assert_eq!(
            header_value(&response, "content-range").unwrap(),
            content_range
        );
        assert_eq!(
            header_value(&response, "content-length").unwrap(),
            bytes.len().to_string()
        );
        assert_eq!(
            header_value(&response, "content-type").unwrap(),
            "video/mp4"
        );
    }
    for range in ["bytes=10-", "bytes=50-60", "bytes=-0"] {
        let response = http(
            &address,
            "GET",
            "/api/v1/artifacts/3/file",
            &[("Range", range)],
            "",
        );
        assert_eq!(response.status, 416, "{range}");
        assert_eq!(
            header_value(&response, "content-range").unwrap(),
            "bytes */10"
        );
        assert_eq!(response.json()["code"], "range_not_satisfiable");
    }
    // Several ranges are not served as parts: the whole file comes back.
    let several = http(
        &address,
        "GET",
        "/api/v1/artifacts/3/file",
        &[("Range", "bytes=0-1,4-5")],
        "",
    );
    assert_eq!(
        (several.status, several.body.as_slice()),
        (200, &b"0123456789"[..])
    );

    // Nothing else is served: other blocks, other nodes, absent files, and a
    // directory or a link that leaves the sidecar directory.
    for (node, code) in [
        (0, "not_external_artifact"),
        (99, "unknown_node"),
        (4, "file_missing"),
        (5, "file_missing"),
        (6, "file_missing"),
    ] {
        let response = http(
            &address,
            "GET",
            &format!("/api/v1/artifacts/{node}/file"),
            &[],
            "",
        );
        assert_eq!(response.status, 404, "{node}");
        assert_eq!(response.json()["code"], code, "{node}");
        assert!(response.json()["message"].is_string());
    }
    // The route takes no file name: a path in its place is not a route.
    let by_name = http(
        &address,
        "GET",
        "/api/v1/artifacts/3/file/../../secret.png",
        &[],
        "",
    );
    assert_eq!(by_name.status, 404);
    assert!(!String::from_utf8_lossy(&by_name.body).contains("secret"));

    // A file produced while `learn` runs shows up on the next state request.
    fs::write(sidecar.join("later.webm"), b"webm-bytes").unwrap();
    let after = availability(&address);
    assert_eq!(after[3].0, "later");
    assert!(after[3].1 && after[3].2.as_ref().unwrap().starts_with("10-"));
    let later = http(&address, "GET", "/api/v1/artifacts/4/file", &[], "");
    assert_eq!(later.status, 200);
    assert_eq!(later.body, b"webm-bytes");
    assert_eq!(header_value(&later, "content-type").unwrap(), "video/webm");
    // Replacing a file changes its version; removing it flips it back.
    fs::write(sidecar.join("later.webm"), b"webm-bytes-longer").unwrap();
    let replaced = availability(&address);
    assert_ne!(replaced[3].2, after[3].2);
    fs::remove_file(sidecar.join("later.webm")).unwrap();
    assert!(!availability(&address)[3].1);
    assert_eq!(
        http(&address, "GET", "/api/v1/artifacts/4/file", &[], "").json()["code"],
        "file_missing"
    );

    // The warnings written at startup are exactly for the absent files.
    let mut stderr = String::new();
    let child = server.0.as_mut().unwrap();
    let mut pipe = child.stderr.take().unwrap();
    server.stop();
    pipe.read_to_string(&mut stderr).unwrap();
    let warnings: Vec<serde_json::Value> = stderr
        .lines()
        .map(|line| serde_json::from_str(line).unwrap_or_else(|_| panic!("not JSON: {line}")))
        .collect();
    let blocks: Vec<_> = warnings
        .iter()
        .map(|warning| {
            assert_eq!(warning["status"], "warning");
            assert_eq!(warning["warning"]["code"], "media_file_missing");
            warning["warning"]["message"].as_str().unwrap()
        })
        .collect();
    let named = |block: &str, file: &str| {
        blocks
            .iter()
            .filter(|message| {
                message.contains(&format!("`{block}`"))
                    && message.contains(&format!("`{file}`"))
                    && message.contains(&sidecar.display().to_string())
                    && message.contains("fallback")
            })
            .count()
    };
    assert_eq!(blocks.len(), 3, "{stderr}");
    assert_eq!(named("later", "later.webm"), 1, "{stderr}");
    assert_eq!(named("folder", "folder.png"), 1, "{stderr}");
    assert_eq!(named("escape", "escape.png"), 1, "{stderr}");
}

#[test]
fn a_lesson_whose_media_is_all_present_starts_without_warnings_and_text_mode_warns_in_text() {
    let root = TempDir::new("sidecar-present");
    let lesson = root.path().join("lesson.json");
    let source = serde_json::json!({
        "schema_version": "2.5.0",
        "title": "One picture",
        "blocks": [{
            "type": "external_artifact",
            "id": "picture",
            "kind": "image",
            "file": "picture.png",
            "alt": "A picture",
            "fallback": "A picture, in words."
        }]
    });
    fs::write(&lesson, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
    output_success(Command::new(learnc()).arg("build").arg(&lesson));
    let artifact = root.path().join("lesson.learn");

    // No sidecar directory at all: one warning, in the requested mode.
    let mut server = ChildGuard::spawn_with(&artifact, &["--text"]);
    let startup = server.0.as_mut().unwrap().stdout.take().unwrap();
    let mut line = String::new();
    BufReader::new(startup).read_line(&mut line).unwrap();
    assert!(line.starts_with("Serving "), "{line}");
    let mut pipe = server.0.as_mut().unwrap().stderr.take().unwrap();
    server.stop();
    let mut stderr = String::new();
    pipe.read_to_string(&mut stderr).unwrap();
    assert!(
        stderr.starts_with("warning[media_file_missing]: block `picture`"),
        "{stderr}"
    );
    assert_eq!(stderr.lines().count(), 1);

    // With the file in place there is nothing to warn about.
    let sidecar = root.path().join("lesson.assets");
    fs::create_dir(&sidecar).unwrap();
    fs::write(sidecar.join("picture.png"), TINY_PNG).unwrap();
    let mut server = ChildGuard::spawn(&artifact);
    let (address, _) = serve_address(&mut server);
    let state = request_json(&address, "GET", "/api/v1/state", None);
    assert_eq!(state["lesson"]["nodes"][0]["available"], true);
    let mut pipe = server.0.as_mut().unwrap().stderr.take().unwrap();
    server.stop();
    let mut stderr = String::new();
    pipe.read_to_string(&mut stderr).unwrap();
    assert_eq!(stderr, "");
}

fn source_with_version(path: &Path, source: &serde_json::Value, version: &str) {
    let mut source = source.clone();
    source["schema_version"] = version.into();
    fs::write(path, serde_json::to_vec_pretty(&source).unwrap()).unwrap();
}

#[test]
fn production_frontend_bundle_is_present_and_self_contained() {
    let dist = manifest_dir().join("web/dist");
    let index = fs::read_to_string(dist.join("index.html")).expect("prebuilt index.html");
    assert!(index.contains("id=\"root\""));
    assert!(index.contains("href=\"/favicon.svg\""));
    let favicon = fs::read(dist.join("favicon.svg")).expect("prebuilt SVG favicon");

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

    let directory = TempDir::new("frontend-assets");
    let lesson = directory.path().join("lesson.json");
    fs::copy(
        manifest_dir().join("tests/fixtures/smoke-lesson.json"),
        &lesson,
    )
    .unwrap();
    output_success(Command::new(learnc()).arg("build").arg(&lesson));
    let mut server = ChildGuard::spawn(&directory.path().join("lesson.learn"));
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

    let served_index = request_bytes(&address, "/");
    assert_eq!(served_index, index.as_bytes());
    assert_eq!(request_bytes(&address, "/favicon.svg"), favicon);

    let mut pending = referenced_assets(&index)
        .into_iter()
        .filter(|path| path.ends_with(".js"))
        .map(str::to_owned)
        .collect::<VecDeque<_>>();
    let mut visited = HashSet::new();
    let mut saw_dynamic_mermaid_import = false;
    while let Some(path) = pending.pop_front() {
        if !visited.insert(path.clone()) {
            continue;
        }

        let bundled_path = dist.join(&path);
        let bundled = fs::read(&bundled_path)
            .unwrap_or_else(|error| panic!("missing bundled JavaScript asset {path}: {error}"));
        let served = request_bytes(&address, &format!("/{path}"));
        assert_eq!(
            served, bundled,
            "learn did not serve the packaged JavaScript asset {path}"
        );

        let source = std::str::from_utf8(&bundled).unwrap_or_else(|error| {
            panic!("bundled JavaScript asset {path} is not UTF-8: {error}")
        });
        for dynamic_path in dynamic_javascript_imports(source, &path) {
            if dynamic_path
                .rsplit('/')
                .next()
                .is_some_and(|name| name.starts_with("mermaid.core-") && name.ends_with(".js"))
            {
                saw_dynamic_mermaid_import = true;
            }
            pending.push_back(dynamic_path);
        }
        for reference in referenced_javascript_assets(source, &path) {
            if reference.dynamic
                && reference
                    .path
                    .rsplit('/')
                    .next()
                    .is_some_and(|name| name.starts_with("mermaid.core-") && name.ends_with(".js"))
            {
                saw_dynamic_mermaid_import = true;
            }
            pending.push_back(reference.path);
        }
    }
    assert!(
        saw_dynamic_mermaid_import,
        "the production entrypoint did not dynamically import the hashed Mermaid bundle"
    );
    assert!(
        visited.len() > 1,
        "the production JavaScript module graph did not include any chunks"
    );
    server.stop();
}

/// Slow release hook: verifies `cargo install` produces all three usable binaries
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
    let installed_learnverify = installation
        .path()
        .join("bin")
        .join(format!("learnverify{suffix}"));
    assert!(installed_learnc.is_file());
    assert!(installed_learn.is_file());
    assert!(installed_learnverify.is_file());
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

fn sibling_repository_root(label: &str) -> TempDir {
    let root = TempDir::new(label);
    for (name, before, after) in [
        ("repo-a", "a-before\n", "a-after\n"),
        ("repo-b", "b-before\n", "b-after\n"),
    ] {
        let repository = root.path().join(name);
        fs::create_dir_all(repository.join("src")).unwrap();
        configure_repository(&repository);
        fs::write(repository.join("src/value.txt"), before).unwrap();
        git(&repository, &["add", "src/value.txt"]);
        git(&repository, &["commit", "-qm", "base"]);
        fs::write(repository.join("src/value.txt"), after).unwrap();
    }
    root
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
        Self::spawn_with(artifact, &[])
    }

    fn spawn_with(artifact: &Path, options: &[&str]) -> Self {
        let child = Command::new(learn())
            .arg("serve")
            .args(options)
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

    fn pid(&self) -> u32 {
        self.0.as_ref().expect("server is running").id()
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
    let response = request(address, method, path, body);
    serde_json::from_slice(&response).unwrap()
}

fn request_bytes(address: &str, path: &str) -> Vec<u8> {
    request(address, "GET", path, "")
}

fn request(address: &str, method: &str, path: &str, body: &str) -> Vec<u8> {
    let response = http(
        address,
        method,
        path,
        &[("Content-Type", "application/json")],
        body,
    );
    assert!(
        response.headers.starts_with("HTTP/1.1 200"),
        "unexpected response: {}",
        response.headers
    );
    response.body
}

struct Http {
    status: u16,
    headers: String,
    body: Vec<u8>,
}

impl Http {
    fn json(&self) -> serde_json::Value {
        serde_json::from_slice(&self.body)
            .unwrap_or_else(|error| panic!("not JSON ({error}): {:?}", self.headers))
    }
}

/// One request with any status. `Host` is the server's address unless given.
fn http(address: &str, method: &str, path: &str, headers: &[(&str, &str)], body: &str) -> Http {
    let mut stream = TcpStream::connect(address).expect("connect to local lesson server");
    let mut head = format!("{method} {path} HTTP/1.1\r\n");
    if !headers
        .iter()
        .any(|(name, _)| name.eq_ignore_ascii_case("host"))
    {
        head.push_str(&format!("Host: {address}\r\n"));
    }
    for (name, value) in headers {
        head.push_str(&format!("{name}: {value}\r\n"));
    }
    write!(
        stream,
        "{head}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
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
    let headers = String::from_utf8_lossy(&response[..split]).into_owned();
    let status = headers
        .split_whitespace()
        .nth(1)
        .and_then(|status| status.parse().ok())
        .expect("HTTP status line");
    Http {
        status,
        headers,
        body: response[split + 4..].to_vec(),
    }
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

#[derive(Debug, Eq, PartialEq)]
struct JavaScriptAssetReference {
    path: String,
    dynamic: bool,
}

fn referenced_javascript_assets(source: &str, current_path: &str) -> Vec<JavaScriptAssetReference> {
    let bytes = source.as_bytes();
    let mut references = Vec::new();
    let mut cursor = 0;
    while cursor < bytes.len() {
        let delimiter = bytes[cursor];
        if !matches!(delimiter, b'\'' | b'"' | b'`') {
            cursor += 1;
            continue;
        }

        let start = cursor;
        cursor += 1;
        let content_start = cursor;
        while cursor < bytes.len() {
            if bytes[cursor] == b'\\' {
                cursor = (cursor + 2).min(bytes.len());
                continue;
            }
            if bytes[cursor] == delimiter {
                break;
            }
            cursor += 1;
        }
        if cursor == bytes.len() {
            break;
        }

        let value = &source[content_start..cursor];
        if let Some(path) = resolve_javascript_asset(current_path, value) {
            references.push(JavaScriptAssetReference {
                path,
                dynamic: is_dynamic_import(source, start),
            });
        }
        cursor += 1;
    }
    references
}

fn resolve_javascript_asset(current_path: &str, reference: &str) -> Option<String> {
    if !reference.ends_with(".js") {
        return None;
    }

    let candidate = if reference.starts_with("assets/") {
        PathBuf::from(reference)
    } else if let Some(reference) = reference.strip_prefix('/') {
        PathBuf::from(reference)
    } else if reference.starts_with("./") || reference.starts_with("../") {
        Path::new(current_path).parent()?.join(reference)
    } else {
        return None;
    };

    let mut normalized = PathBuf::new();
    for component in candidate.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(component) => normalized.push(component),
            Component::ParentDir if normalized.pop() => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (normalized.starts_with("assets")
        && normalized.extension().and_then(|value| value.to_str()) == Some("js"))
    .then(|| normalized.to_string_lossy().replace('\\', "/"))
}

fn dynamic_javascript_imports(source: &str, current_path: &str) -> Vec<String> {
    let mut imports = Vec::new();
    let mut remainder = source;
    while let Some(import_start) = remainder.find("import(") {
        remainder = &remainder[import_start + "import(".len()..];
        let argument = remainder.trim_start();
        let Some(delimiter @ ('\'' | '"' | '`')) = argument.chars().next() else {
            continue;
        };
        let value = &argument[delimiter.len_utf8()..];
        let Some(end) = value.find(delimiter) else {
            continue;
        };
        if let Some(path) = resolve_javascript_asset(current_path, &value[..end]) {
            imports.push(path);
        }
        remainder = &value[end + delimiter.len_utf8()..];
    }
    imports
}

fn is_dynamic_import(source: &str, quote_start: usize) -> bool {
    let prefix = source[..quote_start].trim_end();
    let Some(prefix) = prefix.strip_suffix('(') else {
        return false;
    };
    prefix.trim_end().ends_with("import")
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

fn schema_code_block_has_language(schema: &serde_json::Value) -> bool {
    schema_block_has_property(schema, "code", "language")
}

fn schema_block<'a>(schema: &'a serde_json::Value, block_type: &str) -> &'a serde_json::Value {
    match schema {
        serde_json::Value::Object(object) => {
            let is_requested_block = object
                .get("properties")
                .and_then(|properties| properties.get("type"))
                .and_then(|kind| kind.get("const"))
                .is_some_and(|kind| kind == block_type);
            if is_requested_block {
                return schema;
            }
            object
                .values()
                .find_map(|value| find_schema_block(value, block_type))
                .unwrap_or_else(|| panic!("schema omitted {block_type} block"))
        }
        _ => panic!("schema root is not an object"),
    }
}

fn find_schema_block<'a>(
    schema: &'a serde_json::Value,
    block_type: &str,
) -> Option<&'a serde_json::Value> {
    match schema {
        serde_json::Value::Object(object) => {
            let is_requested_block = object
                .get("properties")
                .and_then(|properties| properties.get("type"))
                .and_then(|kind| kind.get("const"))
                .is_some_and(|kind| kind == block_type);
            is_requested_block.then_some(schema).or_else(|| {
                object
                    .values()
                    .find_map(|value| find_schema_block(value, block_type))
            })
        }
        serde_json::Value::Array(values) => values
            .iter()
            .find_map(|value| find_schema_block(value, block_type)),
        _ => None,
    }
}

fn schema_block_has_property(schema: &serde_json::Value, block_type: &str, property: &str) -> bool {
    find_schema_block(schema, block_type)
        .is_some_and(|block| block["properties"].get(property).is_some())
}

#[cfg(unix)]
#[test]
fn artifacts_record_blob_ids_heads_and_the_lesson_path_for_references() {
    use std::os::unix::fs::PermissionsExt;

    let root = TempDir::new("references");
    let repo = root.path();
    git(repo, &["init", "--quiet"]);
    git(repo, &["config", "user.name", "T"]);
    git(repo, &["config", "user.email", "t@example.invalid"]);
    git(repo, &["config", "commit.gpgsign", "false"]);
    fs::write(repo.join("tracked.rs"), "fn tracked() {}\n").unwrap();
    git(repo, &["add", "tracked.rs"]);
    git(repo, &["commit", "--quiet", "-m", "base"]);
    let head = git_stdout(repo, &["rev-parse", "HEAD"]);
    fs::write(repo.join("tracked.rs"), "fn tracked() {}\nfn dirty() {}\n").unwrap();
    fs::write(repo.join("untracked.md"), "# New\n").unwrap();
    fs::create_dir(repo.join("lessons")).unwrap();

    // A plain-file lesson must not run Git at all: a wrapper records any call.
    let wrapper_dir = repo.join("wrapper-bin");
    fs::create_dir(&wrapper_dir).unwrap();
    let marker = repo.join("git-was-called");
    let wrapper = wrapper_dir.join("git");
    fs::write(
        &wrapper,
        format!("#!/bin/sh\ntouch {}\nexit 97\n", shell_quote(&marker)),
    )
    .unwrap();
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let plain = repo.join("lessons/plain.json");
    fs::write(
        &plain,
        serde_json::to_vec(&serde_json::json!({"schema_version":"2.2.0","title":"Plain","blocks":[
            {"type":"markdown","id":"notes","source":{"kind":"file","path":"untracked.md"}},
            {"type":"code","id":"dirty","source":{"kind":"file","path":"tracked.rs","lines":{"start":2,"end":2}}}
        ]}))
        .unwrap(),
    )
    .unwrap();
    let path = std::env::join_paths(std::iter::once(wrapper_dir.clone()).chain(
        std::env::split_paths(&std::env::var_os("PATH").expect("PATH is set")),
    ))
    .unwrap();
    output_success(
        Command::new(learnc())
            .current_dir(repo)
            .args(["build", "lessons/plain.json"])
            .env("PATH", &path),
    );
    assert!(!marker.exists(), "a plain-file lesson invoked Git");
    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(repo.join("lessons/plain.learn")).unwrap()).unwrap();
    assert_eq!(artifact.artifact_version.as_str(), "1.9.0");
    assert_eq!(
        artifact.provenance.lesson_path.as_deref(),
        Some("lessons/plain.json")
    );
    let file_provenance =
        |artifact: &CompiledLesson, index: usize| match &artifact.presentation.nodes[index].content
        {
            CompiledNodeContent::Markdown { provenance, .. }
            | CompiledNodeContent::Code { provenance, .. } => match provenance {
                ResourceProvenance::File { blob_id, head, .. } => (blob_id.clone(), head.clone()),
                other => panic!("expected file provenance, found {other:?}"),
            },
            other => panic!("unexpected node {other:?}"),
        };
    // Blob IDs cover the whole file, not only the displayed lines, and match
    // what Git will record when the content is committed.
    let (untracked_blob, untracked_head) = file_provenance(&artifact, 0);
    assert_eq!(
        untracked_blob,
        Some(git_stdout(repo, &["hash-object", "untracked.md"]))
    );
    assert_eq!(untracked_head, None);
    let (dirty_blob, _) = file_provenance(&artifact, 1);
    assert_eq!(
        dirty_blob,
        Some(git_stdout(repo, &["hash-object", "tracked.rs"]))
    );

    // When the lesson already uses Git in that repository, HEAD is recorded
    // for worktree files and worktree diffs.
    let mixed = repo.join("lessons/mixed.json");
    fs::write(
        &mixed,
        serde_json::to_vec(&serde_json::json!({"schema_version":"2.2.0","title":"Mixed","blocks":[
            {"type":"code","id":"dirty","source":{"kind":"file","path":"tracked.rs"}},
            {"type":"code","id":"committed","source":{"kind":"git_blob","revision":"HEAD","path":"tracked.rs"}},
            {"type":"diff","id":"change","source":{"kind":"git","base":"HEAD","target":{"kind":"worktree"},
             "files":[{"path":"tracked.rs"}],"context_lines":1}}
        ]}))
        .unwrap(),
    )
    .unwrap();
    output_success(
        Command::new(learnc())
            .current_dir(repo)
            .args(["build", "lessons/mixed.json"]),
    );
    let artifact: CompiledLesson =
        serde_json::from_slice(&fs::read(repo.join("lessons/mixed.learn")).unwrap()).unwrap();
    let (blob, file_head) = file_provenance(&artifact, 0);
    assert_eq!(blob, Some(git_stdout(repo, &["hash-object", "tracked.rs"])));
    assert_eq!(file_head.as_deref(), Some(head.as_str()));
    let CompiledNodeContent::Diff { provenance, .. } = &artifact.presentation.nodes[2].content
    else {
        panic!("expected diff node")
    };
    let ResourceProvenance::GitDiff {
        worktree_blob_ids,
        head: diff_head,
        ..
    } = provenance
    else {
        panic!("expected Git diff provenance")
    };
    assert_eq!(
        worktree_blob_ids.get("tracked.rs"),
        Some(&git_stdout(repo, &["hash-object", "tracked.rs"]))
    );
    assert_eq!(diff_head.as_deref(), Some(head.as_str()));
}
