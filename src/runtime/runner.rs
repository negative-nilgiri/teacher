//! Runs the frozen code of a `run_code` block in a scratch directory.
//!
//! Everything here is blocking, so callers run it on a blocking thread. It
//! knows nothing about sessions, HTTP, or languages: the compiler froze the
//! command, so a run is "write this file, spawn this argv".

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use serde::Serialize;

/// Bytes kept from each of stdout and stderr.
pub(crate) const OUTPUT_CAP: usize = 64 * 1024;

/// The placeholder in a frozen argv for the scratch file's path.
const FILE_PLACEHOLDER: &str = "{file}";

/// How often a running process is checked against its deadline.
const POLL_INTERVAL: Duration = Duration::from_millis(5);

/// How long output readers get to finish once the process is gone. A process
/// that left its group can still hold the pipes open.
const READER_GRACE: Duration = Duration::from_millis(500);

/// What one run needs, all of it frozen in the artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct RunSpec {
    pub argv: Vec<String>,
    pub file_name: String,
    pub code: String,
    pub timeout: Duration,
}

/// The outcome of one run. A failing or killed program is a normal result.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    /// `None` when the program was killed or never started.
    pub exit_code: Option<i32>,
    pub timed_out: bool,
    /// Whether either stream was cut at the output cap.
    pub truncated: bool,
    pub duration_ms: u64,
    /// Why the program could not run, such as an interpreter that is missing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl RunResult {
    /// A run that never produced a program to observe.
    pub(crate) fn failed(error: impl Into<String>) -> Self {
        Self {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: None,
            timed_out: false,
            truncated: false,
            duration_ms: 0,
            error: Some(error.into()),
        }
    }
}

/// Run the code once and wait for it, or kill it at the timeout.
pub(crate) fn run(spec: &RunSpec) -> RunResult {
    let scratch = match Scratch::create() {
        Ok(scratch) => scratch,
        Err(error) => {
            return RunResult::failed(format!("could not create a scratch directory: {error}"));
        }
    };
    let file = scratch.path().join(&spec.file_name);
    if let Err(error) = fs::write(&file, &spec.code) {
        return RunResult::failed(format!("could not write the code to run: {error}"));
    }
    let argv = substitute_file(&spec.argv, &file);
    let Some((program, arguments)) = argv.split_first() else {
        return RunResult::failed("the block has no command to run");
    };

    let mut command = Command::new(program);
    command
        .args(arguments)
        .current_dir(scratch.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    isolate_process_group(&mut command);

    let started = Instant::now();
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return RunResult::failed(format!(
                "could not start `{program}`: it is not installed or not on PATH"
            ));
        }
        Err(error) => return RunResult::failed(format!("could not start `{program}`: {error}")),
    };
    let stdout = capture(child.stdout.take());
    let stderr = capture(child.stderr.take());

    let (status, timed_out) = wait_for(&mut child, spec.timeout);
    if !timed_out {
        // Whatever the program left running in its group dies with the run,
        // which also closes the pipes the readers are waiting on.
        kill_group(&mut child);
    }
    let duration_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);

    let (stdout, stdout_truncated) = stdout.finish();
    let (stderr, stderr_truncated) = stderr.finish();
    RunResult {
        stdout,
        stderr,
        exit_code: status.and_then(|status| status.code()),
        timed_out,
        truncated: stdout_truncated || stderr_truncated,
        duration_ms,
        error: None,
    }
}

/// Every `{file}` in the frozen argv becomes the scratch file's path.
fn substitute_file(argv: &[String], file: &Path) -> Vec<String> {
    let file = file.to_string_lossy();
    argv.iter()
        .map(|argument| argument.replace(FILE_PLACEHOLDER, &file))
        .collect()
}

/// Wait for the child, killing it at the deadline. The status is `None` only
/// when waiting itself failed.
fn wait_for(child: &mut Child, timeout: Duration) -> (Option<ExitStatus>, bool) {
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return (Some(status), false),
            Ok(None) => {}
            Err(_) => return (None, false),
        }
        if Instant::now() >= deadline {
            kill_group(child);
            return (child.wait().ok(), true);
        }
        thread::sleep(POLL_INTERVAL);
    }
}

/// A fresh, private directory that is removed when dropped.
struct Scratch(PathBuf);

impl Scratch {
    fn create() -> std::io::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let mut suffix = [0u8; 8];
        // The name only has to be unguessable enough that no one pre-creates
        // it, and `create_dir` refuses one that exists.
        getrandom::getrandom(&mut suffix).map_err(std::io::Error::other)?;
        let path = std::env::temp_dir().join(format!(
            "learn-run-{}-{}-{:016x}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed),
            u64::from_le_bytes(suffix)
        ));
        fs::create_dir(&path)?;
        let scratch = Self(path);
        restrict_to_owner(&scratch.0)?;
        Ok(scratch)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[cfg(unix)]
fn restrict_to_owner(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}

#[cfg(not(unix))]
fn restrict_to_owner(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Put the program in a process group of its own, so a timeout can stop it and
/// everything it started.
#[cfg(unix)]
fn isolate_process_group(command: &mut Command) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(not(unix))]
fn isolate_process_group(_command: &mut Command) {}

/// Kill the child's whole process group. Elsewhere there is no group to kill,
/// so only the child dies.
#[cfg(unix)]
fn kill_group(child: &mut Child) {
    let Ok(group) = i32::try_from(child.id()) else {
        return;
    };
    // SAFETY: `kill` only signals. The child leads a group of its own (see
    // `isolate_process_group`), so `-group` reaches that group and nothing
    // else. A group that is already gone is not an error here.
    unsafe {
        libc::kill(-group, libc::SIGKILL);
    }
}

#[cfg(not(unix))]
fn kill_group(child: &mut Child) {
    let _ = child.kill();
}

/// One stream being read on its own thread, keeping at most `OUTPUT_CAP`
/// bytes and discarding the rest so the program never blocks on a full pipe.
struct Capture {
    shared: Arc<Mutex<Captured>>,
    reader: Option<JoinHandle<()>>,
}

#[derive(Default)]
struct Captured {
    bytes: Vec<u8>,
    truncated: bool,
}

fn capture(stream: Option<impl Read + Send + 'static>) -> Capture {
    let shared = Arc::new(Mutex::new(Captured::default()));
    let reader = stream.map(|mut stream| {
        let shared = Arc::clone(&shared);
        thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(length) => {
                        let Ok(mut captured) = shared.lock() else {
                            break;
                        };
                        let room = OUTPUT_CAP - captured.bytes.len();
                        if length > room {
                            captured.truncated = true;
                        }
                        captured.bytes.extend_from_slice(&chunk[..length.min(room)]);
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        })
    });
    Capture { shared, reader }
}

impl Capture {
    /// The text read so far, once the stream ends or the grace period does.
    fn finish(self) -> (String, bool) {
        let deadline = Instant::now() + READER_GRACE;
        if let Some(reader) = &self.reader {
            while !reader.is_finished() && Instant::now() < deadline {
                thread::sleep(POLL_INTERVAL);
            }
        }
        let captured = self.shared.lock().map(|captured| {
            (
                String::from_utf8_lossy(&captured.bytes).into_owned(),
                captured.truncated,
            )
        });
        captured.unwrap_or_default()
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    fn shell(code: &str, timeout_secs: u64) -> RunSpec {
        RunSpec {
            argv: vec!["sh".into(), "{file}".into()],
            file_name: "main.sh".into(),
            code: code.into(),
            timeout: Duration::from_secs(timeout_secs),
        }
    }

    #[test]
    fn file_placeholder_is_replaced_in_every_argument() {
        let argv = [
            "sh".to_owned(),
            "-c".into(),
            "cat {file}; echo {file}".into(),
        ];
        assert_eq!(
            substitute_file(&argv, Path::new("/scratch/main.sh")),
            ["sh", "-c", "cat /scratch/main.sh; echo /scratch/main.sh"]
        );
        // Nothing else about the command is rewritten.
        assert_eq!(substitute_file(&["true".into()], Path::new("/x")), ["true"]);
    }

    #[test]
    fn a_run_returns_both_streams_and_the_exit_code() {
        let result = run(&shell("echo out\necho err >&2\nexit 3\n", 5));
        assert_eq!(result.stdout, "out\n");
        assert_eq!(result.stderr, "err\n");
        assert_eq!(result.exit_code, Some(3));
        assert!(!result.timed_out && !result.truncated);
        assert_eq!(result.error, None);
    }

    #[test]
    fn the_code_runs_from_its_file_in_a_scratch_directory_that_is_removed() {
        // `$0` is the script path the argv substituted; the cwd is its directory.
        let result = run(&shell("pwd -P\necho \"$0\"\ncat main.sh | wc -l\n", 5));
        let lines = result.stdout.lines().collect::<Vec<_>>();
        let scratch = Path::new(lines[0]);
        // `$0` may spell the temporary directory through a symlink.
        let directory = scratch.file_name().unwrap().to_string_lossy();
        assert!(
            lines[1].ends_with(&format!("/{directory}/main.sh")),
            "{}",
            lines[1]
        );
        assert_eq!(lines[2].trim(), "3");
        assert!(scratch.starts_with(fs::canonicalize(std::env::temp_dir()).unwrap()));
        assert!(!scratch.exists(), "scratch directory was not removed");
    }

    #[test]
    fn scratch_directories_are_private_and_distinct() {
        let first = Scratch::create().unwrap();
        let second = Scratch::create().unwrap();
        assert_ne!(first.path(), second.path());
        use std::os::unix::fs::PermissionsExt;
        let mode = fs::metadata(first.path()).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        let path = first.path().to_owned();
        drop(first);
        assert!(!path.exists());
    }

    #[test]
    fn stdin_is_closed() {
        let result = run(&shell("cat\necho done\n", 5));
        assert_eq!(result.stdout, "done\n");
        assert!(!result.timed_out);
    }

    #[test]
    fn the_learners_environment_is_inherited() {
        let result = run(&shell("echo \"$PATH\"\n", 5));
        assert_eq!(result.stdout.trim(), std::env::var("PATH").unwrap());
    }

    #[test]
    fn output_is_capped_per_stream_and_marked_truncated() {
        // 100 KiB on each stream, with the pipe still drained to the end.
        let code =
            "head -c 102400 /dev/zero | tr '\\0' a\nhead -c 102400 /dev/zero | tr '\\0' b >&2\n";
        let result = run(&shell(code, 10));
        assert_eq!(result.stdout.len(), OUTPUT_CAP);
        assert_eq!(result.stderr.len(), OUTPUT_CAP);
        assert!(result.stdout.bytes().all(|byte| byte == b'a'));
        assert!(result.stderr.bytes().all(|byte| byte == b'b'));
        assert!(result.truncated);
        assert_eq!(result.exit_code, Some(0));

        let exact = run(&shell("head -c 65536 /dev/zero | tr '\\0' a\n", 10));
        assert_eq!(exact.stdout.len(), OUTPUT_CAP);
        assert!(!exact.truncated);
    }

    #[test]
    fn a_timeout_kills_the_program_and_what_it_started() {
        // The child writes its pid where the test can look for it afterwards.
        let code = "sleep 30 &\necho $! > child.pid\ncat child.pid\nsleep 30\n";
        let started = Instant::now();
        let result = run(&shell(code, 1));
        assert!(result.timed_out);
        assert_eq!(result.exit_code, None);
        assert_eq!(result.error, None);
        assert!(started.elapsed() < Duration::from_secs(10));
        assert!(result.duration_ms >= 1000);

        let child = result.stdout.trim().parse::<i32>().unwrap();
        // `kill -0` only probes. The pid is gone once the group was killed
        // and the orphan reaped by init, which can take a moment.
        let deadline = Instant::now() + Duration::from_secs(5);
        while unsafe { libc::kill(child, 0) } == 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(20));
        }
        assert_ne!(unsafe { libc::kill(child, 0) }, 0, "grandchild survived");
    }

    #[test]
    fn a_program_that_backgrounds_work_does_not_outlive_its_run() {
        // The script exits at once; the background process holds the pipes.
        let started = Instant::now();
        let result = run(&shell("sleep 30 &\necho started\n", 5));
        assert_eq!(result.stdout, "started\n");
        assert!(!result.timed_out);
        assert_eq!(result.exit_code, Some(0));
        assert!(started.elapsed() < Duration::from_secs(4));
    }

    #[test]
    fn a_missing_interpreter_is_a_result_with_an_error() {
        let spec = RunSpec {
            argv: vec!["learn-no-such-interpreter".into(), "{file}".into()],
            ..shell("", 5)
        };
        let result = run(&spec);
        assert_eq!(result.exit_code, None);
        assert!(!result.timed_out);
        assert!(
            result
                .error
                .as_deref()
                .unwrap()
                .contains("`learn-no-such-interpreter`: it is not installed")
        );
    }

    #[test]
    fn a_program_killed_by_a_signal_has_no_exit_code_but_did_not_time_out() {
        let result = run(&shell("kill -9 $$\n", 5));
        assert_eq!(result.exit_code, None);
        assert!(!result.timed_out);
        assert_eq!(result.error, None);
    }

    #[test]
    fn invalid_utf8_is_replaced_rather_than_failing_the_run() {
        let result = run(&shell("printf 'a\\377b'\n", 5));
        assert_eq!(result.stdout, "a\u{fffd}b");
    }
}
