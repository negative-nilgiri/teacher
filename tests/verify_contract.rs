//! `learnverify` command contract, against a local fake TypeSafe server.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use serde_json::{Value, json};

static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

struct TempRoot(PathBuf);

impl TempRoot {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "agent-teacher-verify-cli-{}-{}",
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

    /// Run `learnverify` with its own temporary directory (and therefore its
    /// own cache) and no inherited TypeSafe settings.
    fn command(&self, args: &[&str]) -> Command {
        let tmp = self.0.join("tmp");
        fs::create_dir_all(&tmp).unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_learnverify"));
        command
            .current_dir(&self.0)
            .args(args)
            .env("TMPDIR", tmp)
            .env_remove("TYPESAFE_API_KEY")
            .env_remove("TYPESAFE_BASE_URL")
            .env_remove("TYPESAFE_DEFAULT_MODEL");
        command
    }

    fn run_against(&self, server: &FakeJev, args: &[&str]) -> Output {
        self.command(args)
            .env("TYPESAFE_API_KEY", "test-key\n")
            .env("TYPESAFE_BASE_URL", &server.base_url)
            .output()
            .unwrap()
    }
}

impl Drop for TempRoot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn json_output(output: &Output) -> Value {
    serde_json::from_slice(&output.stdout).unwrap_or_else(|error| {
        panic!(
            "stdout is not JSON ({error}): {}",
            String::from_utf8_lossy(&output.stdout)
        )
    })
}

fn codes(report: &Value) -> Vec<String> {
    report["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|finding| finding["code"].as_str().unwrap().to_owned())
        .collect()
}

struct Recorded {
    authorization: Option<String>,
    body: Value,
}

type Handler = dyn Fn(&Value) -> (u16, Value) + Send + Sync;

/// A minimal HTTP/1.1 server that records each request and answers it with
/// `handler`.
struct FakeJev {
    base_url: String,
    requests: Arc<Mutex<Vec<Recorded>>>,
}

impl FakeJev {
    fn start(handler: impl Fn(&Value) -> (u16, Value) + Send + Sync + 'static) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let base_url = format!("http://{}/", listener.local_addr().unwrap());
        let requests = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        let handler: Arc<Handler> = Arc::new(handler);
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let recorded = Arc::clone(&recorded);
                let handler = Arc::clone(&handler);
                thread::spawn(move || serve(stream, &recorded, &*handler));
            }
        });
        Self { base_url, requests }
    }

    fn count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }

    fn bodies(&self) -> Vec<Value> {
        self.requests
            .lock()
            .unwrap()
            .iter()
            .map(|request| request.body.clone())
            .collect()
    }
}

fn serve(stream: TcpStream, recorded: &Mutex<Vec<Recorded>>, handler: &Handler) {
    let mut reader = BufReader::new(stream.try_clone().unwrap());
    let mut request_line = String::new();
    reader.read_line(&mut request_line).unwrap();
    let mut authorization = None;
    let mut length = 0;
    let mut chunked = false;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        let line = line.trim_end();
        if line.is_empty() {
            break;
        }
        let (name, value) = line.split_once(':').unwrap();
        match name.to_ascii_lowercase().as_str() {
            "authorization" => authorization = Some(value.trim().to_owned()),
            "content-length" => length = value.trim().parse().unwrap(),
            "transfer-encoding" => chunked = value.trim().eq_ignore_ascii_case("chunked"),
            _ => {}
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            let mut size = String::new();
            reader.read_line(&mut size).unwrap();
            let size = usize::from_str_radix(size.trim(), 16).unwrap();
            let mut chunk = vec![0; size + 2];
            reader.read_exact(&mut chunk).unwrap();
            if size == 0 {
                break;
            }
            body.extend_from_slice(&chunk[..size]);
        }
    } else {
        body.resize(length, 0);
        reader.read_exact(&mut body).unwrap();
    }
    let body: Value = serde_json::from_slice(&body).unwrap();
    let (status, response) = handler(&body);
    recorded.lock().unwrap().push(Recorded {
        authorization,
        body,
    });
    let payload = serde_json::to_vec(&response).unwrap();
    let mut stream = stream;
    write!(
        stream,
        "HTTP/1.1 {status} Status\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        payload.len()
    )
    .unwrap();
    stream.write_all(&payload).unwrap();
}

/// Answer every question in `body`, with P(yes) chosen by `probability`.
fn answers(body: &Value, probability: impl Fn(&str) -> f64) -> Value {
    let answers = body["questions"]
        .as_object()
        .unwrap()
        .keys()
        .map(|key| {
            let yes = probability(key);
            (
                key.clone(),
                json!({"type": "choice", "choice": if yes >= 0.5 { "yes" } else { "no" },
                       "confidence": yes.max(1.0 - yes),
                       "probabilities": {"yes": yes, "no": 1.0 - yes}}),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"model": "jev-test-1", "answers": answers, "usage": {"input_tokens": 1, "output_tokens": 1}})
}

const LESSON: &str = r#"{"schema_version":"2.2.0","title":"Queues","blocks":[
    {"type":"markdown","id":"intro","source":{"kind":"inline","content":"A queue is first in, first out."}},
    {"type":"code","id":"impl","source":{"kind":"file","path":"queue.rs"},
     "highlights":[{"lines":[{"start":2,"end":2}],"annotation":"Removes the oldest item."}]},
    {"type":"multiple_choice","id":"order","prompt":{"kind":"inline","content":"Which item does `pop` return?"},
     "choices":[{"content":"The oldest","correct":true},{"content":"The newest"},{"content":"A banana"}],
     "hints":["It is the oldest one."],"explanation":"Queues remove the oldest item first."}
]}"#;

fn lesson_root() -> TempRoot {
    let root = TempRoot::new();
    root.write(
        "queue.rs",
        "fn pop(&mut self) -> Option<T> {\n    self.items.pop_front()\n}\n",
    );
    root.write("lesson.json", LESSON);
    root
}

fn problems(key: &str) -> f64 {
    match key {
        "hint_0_reveals_answer" => 0.95,
        "choice_2_implausible" => 0.70,
        _ => 0.05,
    }
}

#[test]
fn findings_come_from_the_api_and_are_cached() {
    let root = lesson_root();
    let server = FakeJev::start(|body| (200, answers(body, problems)));

    let output = root.run_against(&server, &["lesson.json"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let report = json_output(&output);
    assert_eq!(
        codes(&report),
        [
            "verify.hint_reveals_answer",
            "verify.implausible_distractor"
        ]
    );
    let hint = &report["diagnostics"][0];
    assert_eq!(hint["severity"], "warning");
    assert_eq!(hint["fatal"], false);
    assert_eq!(hint["probability"], 0.95);
    assert_eq!(hint["model"], "jev-test-1");
    assert_eq!(hint["block_id"], "order");
    assert_eq!(hint["pointer"], "/blocks/2/hints/0");
    assert_eq!(report["diagnostics"][1]["severity"], "info");
    assert_eq!(
        report["diagnostics"][1]["pointer"],
        "/blocks/2/choices/2/content"
    );

    {
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(
            requests
                .iter()
                .all(|request| request.authorization.as_deref() == Some("Bearer test-key"))
        );
    }
    let bodies = server.bodies();
    let quiz = bodies
        .iter()
        .find(|body| body["state"]["subject"] == "order")
        .unwrap();
    assert_eq!(quiz["model"], "jev-latest");
    assert_eq!(quiz["state"]["lesson"]["title"], "Queues");
    assert_eq!(
        quiz["state"]["lesson"]["excerpt"].as_array().unwrap().len(),
        3
    );
    assert!(quiz["questions"]["choice_1_defensible"].is_object());
    let code = bodies
        .iter()
        .find(|body| body["state"]["subject"] == "impl")
        .unwrap();
    assert_eq!(
        code["questions"]
            .as_object()
            .unwrap()
            .keys()
            .collect::<Vec<_>>(),
        [
            "group_0_annotation_contradicts_code",
            "group_0_annotation_does_not_explain"
        ]
    );

    // Unchanged lesson: every answer comes from the cache, even with a higher
    // threshold that re-grades it.
    let cached = root.run_against(
        &server,
        &["--min-warning-probability", "0.99", "lesson.json"],
    );
    assert_eq!(server.count(), 2);
    let cached = json_output(&cached);
    assert_eq!(cached["diagnostics"][0]["severity"], "info");

    let fresh = root.run_against(&server, &["--no-cache", "lesson.json"]);
    assert!(fresh.status.success());
    assert_eq!(server.count(), 4);

    let strict = root.run_against(&server, &["--warning-as-error", "warning", "lesson.json"]);
    assert!(!strict.status.success());
    assert_eq!(json_output(&strict)["diagnostics"][0]["fatal"], true);

    let text = root.run_against(&server, &["-t", "lesson.json"]);
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("warning[verify.hint_reveals_answer]"));
    assert!(text.contains("= note: probability 0.95 from jev-test-1"));
    assert!(text.contains("lesson.json:"));
}

#[test]
fn api_failures_are_reported_once_and_never_fail_the_run() {
    let root = lesson_root();
    let server = FakeJev::start(|body| {
        if body["state"]["subject"] == "impl" {
            (503, json!({"error": "overloaded"}))
        } else {
            (200, answers(body, problems))
        }
    });
    let output = root.run_against(
        &server,
        &[
            "--ignore-below",
            "warning",
            "--warning-as-error",
            "info",
            "lesson.json",
        ],
    );
    // The checked quiz's warning is fatal under this threshold; the skipped
    // block is not.
    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(
        codes(&report),
        ["verify.hint_reveals_answer", "verify.unavailable"]
    );
    assert_eq!(report["diagnostics"][0]["fatal"], true);
    let unavailable = &report["diagnostics"][1];
    assert_eq!(
        unavailable["message"],
        "semantic checks skipped for 1 of 2 blocks: TypeSafe API returned HTTP 503"
    );
    assert_eq!(unavailable["fatal"], false);
    assert!(unavailable.get("probability").is_none());
    assert_eq!(
        unavailable["related"][0]["message"],
        "`impl` was not checked: TypeSafe API returned HTTP 503"
    );

    let rejecting = FakeJev::start(|_| (401, json!({})));
    let output = root.run_against(
        &rejecting,
        &["--no-cache", "--warning-as-error", "info", "lesson.json"],
    );
    assert!(output.status.success());
    assert_eq!(
        json_output(&output)["diagnostics"][0]["message"],
        "semantic checks skipped: TypeSafe API returned HTTP 401"
    );
}

#[test]
fn missing_credentials_skip_the_checks_visibly() {
    let root = lesson_root();
    let output = root.command(&["lesson.json"]).output().unwrap();
    assert!(output.status.success());
    let report = json_output(&output);
    assert_eq!(codes(&report), ["verify.unavailable"]);
    assert_eq!(
        report["diagnostics"][0]["message"],
        "semantic checks skipped: TYPESAFE_API_KEY is not set or is empty"
    );
    assert_eq!(report["diagnostics"][0]["block_id"], Value::Null);

    let text = root.command(&["-t", "lesson.json"]).output().unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.starts_with("info[verify.unavailable]: semantic checks skipped"));
    assert!(text.contains("= help: Set TYPESAFE_API_KEY"));
}

#[test]
fn nothing_to_check_needs_no_credentials_and_compile_failures_make_no_request() {
    let root = TempRoot::new();
    root.write(
        "plain.json",
        r#"{"schema_version":"2.2.0","title":"Plain","blocks":[
            {"type":"markdown","id":"intro","source":{"kind":"inline","content":"Hello."}}]}"#,
    );
    let output = root.command(&["plain.json"]).output().unwrap();
    assert!(output.status.success());
    assert_eq!(json_output(&output), json!({"diagnostics": []}));

    let server = FakeJev::start(|body| (200, answers(body, |_| 0.0)));
    root.write(
        "broken.json",
        r#"{"schema_version":"2.2.0","title":"Broken","blocks":[
            {"type":"code","id":"missing","source":{"kind":"file","path":"nope.rs"}}]}"#,
    );
    let output = root.run_against(&server, &["broken.json"]);
    assert!(!output.status.success());
    let report = json_output(&output);
    assert_eq!(report["ok"], false);
    assert_eq!(server.count(), 0);
}

#[test]
fn unavailable_cannot_be_ignored_and_config_is_validated() {
    let root = lesson_root();
    let output = root
        .command(&["--ignore-code", "verify.unavailable", "lesson.json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert_eq!(
        json_output(&output)["diagnostics"][0]["code"],
        "verify.config.ignore_code.invalid"
    );

    root.write("verify.toml", "min_warning_probability = 0.5\n");
    let output = root
        .command(&["--config", "verify.toml", "lesson.json"])
        .output()
        .unwrap();
    assert_eq!(
        json_output(&output)["diagnostics"][0]["code"],
        "verify.config.threshold.invalid"
    );

    root.write("verify.toml", "max_context_char = 5\n");
    let output = root
        .command(&["--config", "verify.toml", "lesson.json"])
        .output()
        .unwrap();
    assert_eq!(
        json_output(&output)["diagnostics"][0]["code"],
        "verify.config.invalid"
    );
    assert!(root.path().join("lesson.json").exists());
}
