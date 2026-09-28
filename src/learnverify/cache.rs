//! Short-lived, per-user cache of TypeSafe answers.
//!
//! Agents rerun checks after every fix and model answers are not
//! deterministic, so unchanged requests reuse their previous answers. Entries
//! hold raw answers, not findings, so changing a threshold re-grades them
//! without a request.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::checks::CHECK_VERSION;
use super::client::Answers;

/// Entries older than this are ignored, so a moving model alias such as
/// `jev-latest` cannot serve old answers indefinitely.
const TTL: Duration = Duration::from_secs(24 * 60 * 60);

static TEMP_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// SHA-256 of the check version, the endpoint, and the exact request body
/// (which includes the requested model, the state, and every question).
pub(super) fn key(endpoint_url: &str, body: &Value) -> String {
    let body = serde_json::to_vec(body).expect("request bodies serialize");
    let mut hasher = Sha256::new();
    hasher.update(b"learnverify\0");
    hasher.update(CHECK_VERSION.to_le_bytes());
    for part in [endpoint_url.as_bytes(), &body] {
        // Length prefixes keep the parts unambiguous.
        hasher.update((part.len() as u64).to_le_bytes());
        hasher.update(part);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Debug)]
pub(super) struct Cache {
    directory: PathBuf,
}

impl Cache {
    /// Open `learnverify-<uid>` in the system temporary directory, or explain
    /// why the cache is disabled.
    pub(super) fn open() -> Result<Self, String> {
        Self::open_in(&std::env::temp_dir())
    }

    fn open_in(base: &Path) -> Result<Self, String> {
        let directory = base.join(directory_name());
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        if let Err(error) = builder.create(&directory)
            && error.kind() != std::io::ErrorKind::AlreadyExists
        {
            return Err(format!("could not create {}: {error}", directory.display()));
        }
        // Never follow a symlink: on a shared /tmp another user could plant one.
        let metadata = fs::symlink_metadata(&directory)
            .map_err(|error| format!("could not inspect {}: {error}", directory.display()))?;
        if !metadata.is_dir() {
            return Err(format!("{} is not a directory", directory.display()));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            check_private(metadata.uid(), metadata.mode(), current_uid())
                .map_err(|reason| format!("{} {reason}", directory.display()))?;
        }
        Ok(Self { directory })
    }

    pub(super) fn get(&self, key: &str) -> Option<Answers> {
        let path = self.entry(key);
        let modified = fs::metadata(&path)
            .and_then(|metadata| metadata.modified())
            .ok()?;
        if SystemTime::now()
            .duration_since(modified)
            .is_ok_and(|age| age > TTL)
        {
            return None;
        }
        serde_json::from_slice(&fs::read(&path).ok()?).ok()
    }

    /// Store answers atomically. Failures only cost a later request, so they
    /// are ignored.
    pub(super) fn put(&self, key: &str, answers: &Answers) {
        let encoded = serde_json::to_vec(answers).expect("answers serialize");
        let temporary = self.directory.join(format!(
            ".{key}.{}.{}.tmp",
            std::process::id(),
            TEMP_SEQUENCE.fetch_add(1, Ordering::Relaxed)
        ));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let written = options.open(&temporary).and_then(|mut file| {
            file.write_all(&encoded)?;
            file.sync_all()
        });
        if written
            .and_then(|()| fs::rename(&temporary, self.entry(key)))
            .is_err()
        {
            let _ = fs::remove_file(&temporary);
        }
    }

    fn entry(&self, key: &str) -> PathBuf {
        self.directory.join(format!("{key}.json"))
    }
}

#[cfg(unix)]
fn directory_name() -> String {
    format!("learnverify-{}", current_uid())
}

#[cfg(not(unix))]
fn directory_name() -> String {
    // Temporary directories are already per user on other platforms.
    "learnverify".to_owned()
}

#[cfg(unix)]
fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and cannot fail.
    unsafe { libc::geteuid() }
}

/// The cache holds lesson text and code, so it must belong to this user and
/// be closed to everyone else.
#[cfg(unix)]
fn check_private(owner: u32, mode: u32, uid: u32) -> Result<(), &'static str> {
    if owner != uid {
        return Err("is owned by another user");
    }
    if mode & 0o077 != 0 {
        return Err("is accessible to other users");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::AtomicU64;

    use serde_json::json;

    use super::*;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn base() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "agent-teacher-verify-cache-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn answers() -> Answers {
        Answers {
            model: "jev-1.13.0".into(),
            probabilities: BTreeMap::from([(
                "a".to_owned(),
                BTreeMap::from([("yes".to_owned(), 0.25), ("no".to_owned(), 0.75)]),
            )]),
        }
    }

    #[test]
    fn keys_change_with_the_endpoint_and_every_part_of_the_body() {
        let body = json!({"model": "jev-latest", "state": {"x": 1}, "questions": {"q": {}}});
        let same = key("https://a/v1/systemone", &body);
        assert_eq!(same, key("https://a/v1/systemone", &body.clone()));
        assert_eq!(same.len(), 64);
        assert_ne!(same, key("https://b/v1/systemone", &body));
        for changed in [
            json!({"model": "jev-2", "state": {"x": 1}, "questions": {"q": {}}}),
            json!({"model": "jev-latest", "state": {"x": 2}, "questions": {"q": {}}}),
            json!({"model": "jev-latest", "state": {"x": 1}, "questions": {"r": {}}}),
        ] {
            assert_ne!(same, key("https://a/v1/systemone", &changed));
        }
    }

    #[test]
    fn entries_round_trip_and_expire() {
        let base = base();
        let cache = Cache::open_in(&base).unwrap();
        assert_eq!(cache.get("k"), None);
        cache.put("k", &answers());
        assert_eq!(cache.get("k"), Some(answers()));

        let old = SystemTime::now() - TTL - Duration::from_secs(60);
        fs::File::options()
            .write(true)
            .open(cache.entry("k"))
            .unwrap()
            .set_modified(old)
            .unwrap();
        assert_eq!(cache.get("k"), None);

        fs::write(cache.entry("corrupt"), "{not json").unwrap();
        assert_eq!(cache.get("corrupt"), None);
        fs::remove_dir_all(base).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinks_and_directories_others_can_open() {
        use std::os::unix::fs::PermissionsExt;

        let base = base();
        let target = base.join("elsewhere");
        fs::create_dir(&target).unwrap();
        std::os::unix::fs::symlink(&target, base.join(directory_name())).unwrap();
        assert!(
            Cache::open_in(&base)
                .unwrap_err()
                .contains("not a directory")
        );
        fs::remove_file(base.join(directory_name())).unwrap();

        let cache = Cache::open_in(&base).unwrap();
        let mode = fs::metadata(&cache.directory).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o700);
        fs::set_permissions(&cache.directory, fs::Permissions::from_mode(0o770)).unwrap();
        assert!(
            Cache::open_in(&base)
                .unwrap_err()
                .contains("accessible to other users")
        );

        assert_eq!(check_private(1, 0o700, 2), Err("is owned by another user"));
        assert_eq!(check_private(2, 0o700, 2), Ok(()));
        fs::remove_dir_all(base).unwrap();
    }
}
