//! The sidecar directory next to an artifact and the media files in it.
//!
//! A lesson names a media file only by a bare, validated file name. It is looked
//! up here on every request, never at load time, so a file generated while
//! `learn` runs is found without a restart.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

/// The directory a lesson's media files live in: the artifact path without its
/// extension plus `.assets`, so `dir/queue.learn` has `dir/queue.assets`. A path
/// without an extension just gets `.assets` appended.
///
/// The path is made absolute here, once, so a later change of the working
/// directory cannot move it. Symbolic links are left alone.
pub(crate) fn sidecar_dir(artifact: &Path) -> PathBuf {
    std::path::absolute(artifact)
        .unwrap_or_else(|_| artifact.to_owned())
        .with_extension("assets")
}

/// A media file that is in the sidecar directory right now.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct MediaFile {
    /// The resolved path, directly inside the sidecar directory.
    pub path: PathBuf,
    pub len: u64,
    /// Changes whenever the file's size or modification time does, so a page can
    /// ask for the new bytes instead of a cached copy.
    pub version: String,
}

/// Find `file` in the sidecar directory. Only a regular file whose resolved
/// location is directly inside the directory counts: a directory, a missing
/// name, and a symbolic link that leads anywhere else are all just absent.
pub(crate) fn find(sidecar: &Path, file: &str) -> Option<MediaFile> {
    let sidecar = sidecar.canonicalize().ok()?;
    let path = sidecar.join(file).canonicalize().ok()?;
    if path.parent() != Some(sidecar.as_path()) {
        return None;
    }
    let metadata = fs::metadata(&path).ok().filter(fs::Metadata::is_file)?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_nanos());
    Some(MediaFile {
        path,
        len: metadata.len(),
        version: format!("{}-{modified}", metadata.len()),
    })
}

/// The MIME type to serve a file as, from its extension.
pub(crate) fn content_type(file: &str) -> String {
    // The guess for `m4a` is the nonstandard `audio/m4a`.
    if file.to_ascii_lowercase().ends_with(".m4a") {
        return "audio/mp4".to_owned();
    }
    mime_guess::from_path(file)
        .first_or_octet_stream()
        .as_ref()
        .to_owned()
}

/// What a `Range` header asks of a file of a known length.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum ByteRange {
    /// No range, or one that is ignored: serve the whole file.
    Whole,
    /// These bytes, both ends included.
    Slice { start: u64, end: u64 },
    /// The range lies outside the file.
    Unsatisfiable,
}

/// Interpret one `Range: bytes=...` header: `a-b`, `a-`, or the last `-n` bytes.
/// A header that is not a single valid byte range, including a list of several
/// ranges, is ignored and the whole file is served, which the HTTP rules allow.
/// An end past the file is cut to the file's last byte.
pub(crate) fn parse_range(header: Option<&str>, len: u64) -> ByteRange {
    let Some(header) = header else {
        return ByteRange::Whole;
    };
    let Some((unit, spec)) = header.split_once('=') else {
        return ByteRange::Whole;
    };
    if !unit.trim().eq_ignore_ascii_case("bytes") || spec.contains(',') {
        return ByteRange::Whole;
    }
    let Some((first, last)) = spec.split_once('-') else {
        return ByteRange::Whole;
    };
    let (first, last) = (first.trim(), last.trim());
    match (number(first), number(last)) {
        // The last `n` bytes.
        (None, Some(count)) if first.is_empty() => {
            if count == 0 || len == 0 {
                ByteRange::Unsatisfiable
            } else {
                ByteRange::Slice {
                    start: len.saturating_sub(count),
                    end: len - 1,
                }
            }
        }
        (Some(start), None) if last.is_empty() => slice(start, len.saturating_sub(1), len),
        (Some(start), Some(end)) if end >= start => slice(start, end, len),
        _ => ByteRange::Whole,
    }
}

fn slice(start: u64, end: u64, len: u64) -> ByteRange {
    if start >= len {
        ByteRange::Unsatisfiable
    } else {
        ByteRange::Slice {
            start,
            end: end.min(len - 1),
        }
    }
}

/// A non-negative decimal number. One too large for 64 bits is as good as the
/// largest.
fn number(text: &str) -> Option<u64> {
    (!text.is_empty() && text.bytes().all(|byte| byte.is_ascii_digit()))
        .then(|| text.parse().unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sidecar_directory_replaces_the_extension_with_assets() {
        let cwd = std::env::current_dir().unwrap();
        assert_eq!(
            sidecar_dir(Path::new("/lessons/queue.learn")),
            Path::new("/lessons/queue.assets")
        );
        assert_eq!(
            sidecar_dir(Path::new("/lessons/queue")),
            Path::new("/lessons/queue.assets")
        );
        assert_eq!(
            sidecar_dir(Path::new("/lessons/queue.v2.learn")),
            Path::new("/lessons/queue.v2.assets")
        );
        assert_eq!(
            sidecar_dir(Path::new("/lessons.d/queue")),
            Path::new("/lessons.d/queue.assets")
        );
        // A relative path is fixed against the current directory right away.
        assert_eq!(
            sidecar_dir(Path::new("out/queue.learn")),
            cwd.join("out/queue.assets")
        );
        assert_eq!(sidecar_dir(Path::new("queue")), cwd.join("queue.assets"));
    }

    #[test]
    fn mime_types_follow_the_extension_for_every_allowed_kind() {
        for (file, expected) in [
            ("a.png", "image/png"),
            ("a.jpg", "image/jpeg"),
            ("a.JPEG", "image/jpeg"),
            ("a.gif", "image/gif"),
            ("a.webp", "image/webp"),
            ("a.svg", "image/svg+xml"),
            ("a.avif", "image/avif"),
            ("a.mp3", "audio/mpeg"),
            ("a.wav", "audio/wav"),
            ("a.ogg", "audio/ogg"),
            ("a.m4a", "audio/mp4"),
            ("a.aac", "audio/aac"),
            ("a.flac", "audio/flac"),
            ("a.mp4", "video/mp4"),
            ("a.webm", "video/webm"),
            ("a.mov", "video/quicktime"),
            ("a.ogv", "video/ogg"),
        ] {
            assert_eq!(content_type(file), expected, "{file}");
        }
    }

    #[test]
    fn ranges_cover_the_forms_a_browser_sends() {
        let slice = |start, end| ByteRange::Slice { start, end };
        // A file of 10 bytes.
        for (header, expected) in [
            ("bytes=0-3", slice(0, 3)),
            ("bytes=4-4", slice(4, 4)),
            ("bytes=0-", slice(0, 9)),
            ("bytes=7-", slice(7, 9)),
            ("bytes=-3", slice(7, 9)),
            ("bytes=-10", slice(0, 9)),
            ("bytes=-99", slice(0, 9)),
            ("bytes=5-99", slice(5, 9)),
            ("bytes=5-99999999999999999999999", slice(5, 9)),
            (" Bytes = 2 - 3 ", slice(2, 3)),
        ] {
            assert_eq!(parse_range(Some(header), 10), expected, "{header}");
        }
    }

    #[test]
    fn unsatisfiable_ranges_are_told_apart_from_ignored_ones() {
        for header in [
            "bytes=10-",
            "bytes=10-12",
            "bytes=99-",
            "bytes=-0",
            "bytes=99999999999999999999999-",
        ] {
            assert_eq!(
                parse_range(Some(header), 10),
                ByteRange::Unsatisfiable,
                "{header}"
            );
        }
        // Nothing in an empty file can be asked for.
        for header in ["bytes=0-", "bytes=0-0", "bytes=-1"] {
            assert_eq!(parse_range(Some(header), 0), ByteRange::Unsatisfiable);
        }
        // Not a single valid byte range: the whole file is served.
        for header in [
            "bytes=0-1,4-5",
            "bytes=5-2",
            "bytes=",
            "bytes=-",
            "bytes=a-b",
            "bytes=+1-2",
            "bytes=1",
            "items=0-1",
            "0-1",
            "",
        ] {
            assert_eq!(parse_range(Some(header), 10), ByteRange::Whole, "{header}");
        }
        assert_eq!(parse_range(None, 10), ByteRange::Whole);
    }

    struct Dir(PathBuf);

    impl Dir {
        fn new(label: &str) -> Self {
            let path = std::env::temp_dir().join(format!(
                "agent-teacher-media-{label}-{}-{:?}",
                std::process::id(),
                std::thread::current().id()
            ));
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn only_a_regular_file_in_the_directory_is_found() {
        let root = Dir::new("find");
        let sidecar = root.0.join("queue.assets");
        fs::create_dir(&sidecar).unwrap();
        fs::write(sidecar.join("demo.mp4"), b"0123456789").unwrap();
        fs::create_dir(sidecar.join("folder.png")).unwrap();
        fs::write(root.0.join("secret.png"), b"secret").unwrap();

        let found = find(&sidecar, "demo.mp4").unwrap();
        assert_eq!(found.len, 10);
        assert!(found.version.starts_with("10-"));
        assert_eq!(found.path.file_name().unwrap(), "demo.mp4");

        assert_eq!(find(&sidecar, "absent.mp4"), None);
        assert_eq!(find(&sidecar, "folder.png"), None, "a directory");
        assert_eq!(find(&root.0.join("none.assets"), "demo.mp4"), None);
        // Not even a name that were to escape the directory.
        assert_eq!(find(&sidecar, "../secret.png"), None);
        assert_eq!(find(&sidecar, ".."), None);

        // The version follows the content.
        fs::write(sidecar.join("demo.mp4"), b"012345678901").unwrap();
        assert!(
            find(&sidecar, "demo.mp4")
                .unwrap()
                .version
                .starts_with("12-")
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_leading_out_of_the_directory_is_not_found() {
        use std::os::unix::fs::symlink;

        let root = Dir::new("links");
        let sidecar = root.0.join("queue.assets");
        fs::create_dir(&sidecar).unwrap();
        fs::write(root.0.join("outside.png"), b"outside").unwrap();
        fs::write(sidecar.join("inside.png"), b"inside").unwrap();
        symlink(root.0.join("outside.png"), sidecar.join("out.png")).unwrap();
        symlink(&root.0, sidecar.join("up.png")).unwrap();
        symlink("inside.png", sidecar.join("alias.png")).unwrap();
        symlink(root.0.join("gone.png"), sidecar.join("dangling.png")).unwrap();

        assert_eq!(find(&sidecar, "out.png"), None);
        assert_eq!(find(&sidecar, "up.png"), None);
        assert_eq!(find(&sidecar, "dangling.png"), None);
        // A link to a file in the same directory stays inside it.
        assert_eq!(find(&sidecar, "alias.png").unwrap().len, 6);
        // The directory itself may be reached through a link.
        symlink(&sidecar, root.0.join("linked.assets")).unwrap();
        assert!(find(&root.0.join("linked.assets"), "inside.png").is_some());
    }
}
