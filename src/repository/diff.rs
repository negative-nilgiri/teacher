use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::language::Language;

use super::{RepoPath, RepositoryError, RepositoryErrorKind};

/// A one-based, inclusive range used to select changed lines.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LineRange {
    pub start: u32,
    pub end: u32,
}

impl LineRange {
    pub fn new(start: u32, end: u32) -> Result<Self, RepositoryError> {
        if start == 0 || end < start {
            return Err(RepositoryError::new(
                RepositoryErrorKind::LineRange,
                "line ranges must be one-based, inclusive, and non-empty",
            ));
        }
        Ok(Self { start, end })
    }

    fn contains(self, line: u32) -> bool {
        self.start <= line && line <= self.end
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum DiffTarget {
    Revision(String),
    Worktree,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffFileRequest {
    pub path: RepoPath,
    pub before_lines: Option<LineRange>,
    pub after_lines: Option<LineRange>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DiffRequest {
    pub base: String,
    pub target: DiffTarget,
    pub files: Vec<DiffFileRequest>,
    pub context_lines: u16,
}

#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedDiff {
    pub files: Vec<ResolvedDiffFile>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedDiffFile {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    /// Language inferred from the displayed path and frozen for rendering.
    #[serde(default)]
    pub language: Language,
    pub old_object_id: Option<String>,
    pub new_object_id: Option<String>,
    pub is_new: bool,
    pub is_deleted: bool,
    pub hunks: Vec<ResolvedDiffHunk>,
}

impl ResolvedDiffFile {
    pub fn display_path(&self) -> Option<&str> {
        self.new_path.as_deref().or(self.old_path.as_deref())
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct ResolvedDiffHunk {
    pub old_start: u32,
    pub old_lines: u32,
    pub new_start: u32,
    pub new_lines: u32,
    pub heading: String,
    pub lines: Vec<DiffLine>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiffLineKind {
    Context,
    Addition,
    Deletion,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct DiffLine {
    pub kind: DiffLineKind,
    pub content: String,
    pub old_line: Option<u32>,
    pub new_line: Option<u32>,
}

/// Parse a unified Git patch into renderer-ready data.
pub fn parse_unified_diff(patch: &str) -> Result<ResolvedDiff, RepositoryError> {
    let mut files = Vec::new();
    let mut current_file: Option<ResolvedDiffFile> = None;
    let mut current_hunk: Option<ResolvedDiffHunk> = None;
    let mut old_cursor = 0;
    let mut new_cursor = 0;

    let flush_hunk = |file: &mut Option<ResolvedDiffFile>, hunk: &mut Option<ResolvedDiffHunk>| {
        if let Some(hunk) = hunk.take()
            && let Some(file) = file.as_mut()
        {
            file.hunks.push(hunk);
        }
    };
    let flush_file = |files: &mut Vec<ResolvedDiffFile>, file: &mut Option<ResolvedDiffFile>| {
        if let Some(mut file) = file.take() {
            file.language = file
                .display_path()
                .map(Language::from_path)
                .unwrap_or_default();
            files.push(file);
        }
    };

    // Lines still owed to the current hunk by its header. While either side is
    // nonzero, every line is hunk body, even one that resembles a file header
    // (for example a deleted SQL comment `-- note` renders as `--- note`).
    let mut old_remaining = 0u32;
    let mut new_remaining = 0u32;

    for (index, line) in patch.lines().enumerate() {
        let line_number = index + 1;
        if let Some(hunk) = current_hunk.as_mut()
            && (old_remaining > 0 || new_remaining > 0)
        {
            let (kind, old_line, new_line) = match line.as_bytes().first().copied() {
                // `diff.suppressBlankEmpty` emits blank context lines without a space.
                Some(b' ') | None if old_remaining > 0 && new_remaining > 0 => {
                    old_remaining -= 1;
                    new_remaining -= 1;
                    old_cursor += 1;
                    new_cursor += 1;
                    (
                        DiffLineKind::Context,
                        Some(old_cursor - 1),
                        Some(new_cursor - 1),
                    )
                }
                Some(b'+') if new_remaining > 0 => {
                    new_remaining -= 1;
                    new_cursor += 1;
                    (DiffLineKind::Addition, None, Some(new_cursor - 1))
                }
                Some(b'-') if old_remaining > 0 => {
                    old_remaining -= 1;
                    old_cursor += 1;
                    (DiffLineKind::Deletion, Some(old_cursor - 1), None)
                }
                Some(b'\\') => continue,
                Some(b' ' | b'+' | b'-') | None => {
                    return Err(invalid_patch(
                        line_number,
                        "hunk body line counts do not match its header",
                    ));
                }
                _ => {
                    return Err(invalid_patch(
                        line_number,
                        "hunk line has no context/addition/deletion prefix",
                    ));
                }
            };
            hunk.lines.push(DiffLine {
                kind,
                content: line.get(1..).unwrap_or_default().to_owned(),
                old_line,
                new_line,
            });
            continue;
        }

        if line.starts_with("diff --git ") {
            flush_hunk(&mut current_file, &mut current_hunk);
            flush_file(&mut files, &mut current_file);
            let (old_path, new_path) = parse_diff_git_paths(line)
                .ok_or_else(|| invalid_patch(line_number, "invalid diff --git header"))?;
            current_file = Some(ResolvedDiffFile {
                old_path: Some(old_path),
                new_path: Some(new_path),
                language: Language::Text,
                old_object_id: None,
                new_object_id: None,
                is_new: false,
                is_deleted: false,
                hunks: Vec::new(),
            });
        } else if let Some(rest) = line.strip_prefix("index ") {
            let file = current_file.as_mut().ok_or_else(|| {
                invalid_patch(line_number, "index header appeared before a file header")
            })?;
            let ids = rest.split_whitespace().next().unwrap_or_default();
            if let Some((old_id, new_id)) = ids.split_once("..") {
                file.old_object_id = Some(old_id.to_owned());
                file.new_object_id = Some(new_id.to_owned());
            }
        } else if line.starts_with("new file mode ") {
            let file = current_file.as_mut().ok_or_else(|| {
                invalid_patch(line_number, "new-file marker appeared before a file header")
            })?;
            file.is_new = true;
        } else if line.starts_with("deleted file mode ") {
            let file = current_file.as_mut().ok_or_else(|| {
                invalid_patch(
                    line_number,
                    "deleted-file marker appeared before a file header",
                )
            })?;
            file.is_deleted = true;
        } else if let Some(value) = line.strip_prefix("--- ") {
            // Plain unified patches need not have a `diff --git` header.
            if current_hunk.is_some()
                || current_file
                    .as_ref()
                    .is_some_and(|file| !file.hunks.is_empty())
            {
                flush_hunk(&mut current_file, &mut current_hunk);
                flush_file(&mut files, &mut current_file);
            }
            let old_path = patch_path(value)
                .ok_or_else(|| invalid_patch(line_number, "invalid old path header"))?;
            let file = current_file.get_or_insert_with(|| ResolvedDiffFile {
                old_path: None,
                new_path: None,
                language: Language::Text,
                old_object_id: None,
                new_object_id: None,
                is_new: false,
                is_deleted: false,
                hunks: Vec::new(),
            });
            file.old_path = old_path;
            file.is_new = file.old_path.is_none();
        } else if let Some(value) = line.strip_prefix("+++ ") {
            let file = current_file.as_mut().ok_or_else(|| {
                invalid_patch(line_number, "new path appeared before a file header")
            })?;
            file.new_path = patch_path(value)
                .ok_or_else(|| invalid_patch(line_number, "invalid new path header"))?;
            file.is_deleted = file.new_path.is_none();
        } else if line.starts_with("@@ ") {
            flush_hunk(&mut current_file, &mut current_hunk);
            if current_file.is_none() {
                return Err(invalid_patch(
                    line_number,
                    "hunk appeared before a file header",
                ));
            }
            let (old_start, old_lines, new_start, new_lines, heading) = parse_hunk_header(line)
                .ok_or_else(|| invalid_patch(line_number, "invalid unified-diff hunk header"))?;
            old_cursor = old_start;
            new_cursor = new_start;
            old_remaining = old_lines;
            new_remaining = new_lines;
            current_hunk = Some(ResolvedDiffHunk {
                old_start,
                old_lines,
                new_start,
                new_lines,
                heading,
                lines: Vec::new(),
            });
        } else if current_hunk.is_some()
            && line != "-- "
            && matches!(line.as_bytes().first(), Some(b' ' | b'+' | b'-'))
        {
            // The header's counts are exhausted, so this line cannot belong to
            // the hunk. `-- ` alone is the `git format-patch` signature trailer;
            // other unprefixed text (such as commit messages) is ignored.
            return Err(invalid_patch(
                line_number,
                "hunk body line counts do not match its header",
            ));
        }
    }

    flush_hunk(&mut current_file, &mut current_hunk);
    flush_file(&mut files, &mut current_file);
    if files.is_empty() && !patch.trim().is_empty() {
        return Err(invalid_patch(1, "content is not a unified diff"));
    }
    for file in &files {
        if file.old_path.is_none() && file.new_path.is_none() {
            return Err(invalid_patch(
                1,
                "diff file has neither an old nor a new path",
            ));
        }
        for hunk in &file.hunks {
            let actual_old = u32::try_from(
                hunk.lines
                    .iter()
                    .filter(|line| line.kind != DiffLineKind::Addition)
                    .count(),
            )
            .map_err(|_| invalid_patch(1, "hunk contains too many old-side lines"))?;
            let actual_new = u32::try_from(
                hunk.lines
                    .iter()
                    .filter(|line| line.kind != DiffLineKind::Deletion)
                    .count(),
            )
            .map_err(|_| invalid_patch(1, "hunk contains too many new-side lines"))?;
            if actual_old != hunk.old_lines || actual_new != hunk.new_lines {
                return Err(invalid_patch(
                    1,
                    "hunk body line counts do not match its header",
                ));
            }
        }
    }
    Ok(ResolvedDiff { files })
}

/// Retain changed regions that intersect the authored before/after ranges and
/// rebuild hunks with the requested amount of surrounding context.
pub fn select_diff_ranges(
    diff: ResolvedDiff,
    requests: &[DiffFileRequest],
    context_lines: u16,
) -> Result<ResolvedDiff, RepositoryError> {
    let mut by_path = BTreeMap::new();
    for request in requests {
        if by_path.insert(request.path.as_str(), request).is_some() {
            return Err(RepositoryError::at_path(
                RepositoryErrorKind::InvalidPath,
                "a Git diff cannot select the same path more than once",
                request.path.to_path_buf(),
            ));
        }
    }
    let mut files = Vec::new();

    for mut file in diff.files {
        let Some(path) = file.display_path() else {
            continue;
        };
        let Some(request) = by_path.get(path) else {
            continue;
        };
        if request.before_lines.is_some() || request.after_lines.is_some() {
            file.hunks = file
                .hunks
                .into_iter()
                .flat_map(|hunk| select_hunk_regions(hunk, request, context_lines))
                .collect();
        }
        if !file.hunks.is_empty() {
            files.push(file);
        }
    }

    for request in requests {
        if files
            .iter()
            .any(|file| file.display_path() == Some(request.path.as_str()))
        {
            continue;
        }
        return Err(
            if request.before_lines.is_some() || request.after_lines.is_some() {
                RepositoryError::at_path(
                    RepositoryErrorKind::EmptySelection,
                    "selected line range intersects no changed lines",
                    request.path.to_path_buf(),
                )
            } else {
                RepositoryError::at_path(
                    RepositoryErrorKind::UnchangedPath,
                    "selected path has no changes between the compared states",
                    request.path.to_path_buf(),
                )
            },
        );
    }

    Ok(ResolvedDiff { files })
}

fn select_hunk_regions(
    hunk: ResolvedDiffHunk,
    request: &DiffFileRequest,
    context_lines: u16,
) -> Vec<ResolvedDiffHunk> {
    let mut slices = Vec::<(usize, usize)>::new();
    let mut index = 0;
    while index < hunk.lines.len() {
        if hunk.lines[index].kind == DiffLineKind::Context {
            index += 1;
            continue;
        }

        let changed_start = index;
        while index < hunk.lines.len() && hunk.lines[index].kind != DiffLineKind::Context {
            index += 1;
        }
        let changed_end = index;
        if !hunk.lines[changed_start..changed_end]
            .iter()
            .any(|line| changed_line_selected(line, request))
        {
            continue;
        }

        let mut start = changed_start;
        for _ in 0..context_lines {
            if start == 0 || hunk.lines[start - 1].kind != DiffLineKind::Context {
                break;
            }
            start -= 1;
        }
        let mut end = changed_end;
        for _ in 0..context_lines {
            if end == hunk.lines.len() || hunk.lines[end].kind != DiffLineKind::Context {
                break;
            }
            end += 1;
        }

        if let Some((_, previous_end)) = slices.last_mut()
            && start <= *previous_end
        {
            *previous_end = (*previous_end).max(end);
        } else {
            slices.push((start, end));
        }
    }

    slices
        .into_iter()
        .map(|(start, end)| rebuild_hunk(&hunk, start, end))
        .collect()
}

fn changed_line_selected(line: &DiffLine, request: &DiffFileRequest) -> bool {
    match line.kind {
        DiffLineKind::Deletion => request
            .before_lines
            .zip(line.old_line)
            .is_some_and(|(range, number)| range.contains(number)),
        DiffLineKind::Addition => request
            .after_lines
            .zip(line.new_line)
            .is_some_and(|(range, number)| range.contains(number)),
        DiffLineKind::Context => false,
    }
}

fn rebuild_hunk(hunk: &ResolvedDiffHunk, start: usize, end: usize) -> ResolvedDiffHunk {
    let preceding = &hunk.lines[..start];
    let lines = hunk.lines[start..end].to_vec();
    let preceding_old = preceding
        .iter()
        .filter(|line| line.kind != DiffLineKind::Addition)
        .count() as u32;
    let preceding_new = preceding
        .iter()
        .filter(|line| line.kind != DiffLineKind::Deletion)
        .count() as u32;
    let mut old_start = hunk.old_start + preceding_old;
    let mut new_start = hunk.new_start + preceding_new;
    let old_lines = lines
        .iter()
        .filter(|line| line.kind != DiffLineKind::Addition)
        .count() as u32;
    let new_lines = lines
        .iter()
        .filter(|line| line.kind != DiffLineKind::Deletion)
        .count() as u32;
    // In a zero-count unified range, the start names the position immediately
    // before the first line on the opposite side.
    if old_lines == 0 && preceding_old > 0 {
        old_start -= 1;
    }
    if new_lines == 0 && preceding_new > 0 {
        new_start -= 1;
    }
    ResolvedDiffHunk {
        old_start,
        old_lines,
        new_start,
        new_lines,
        heading: hunk.heading.clone(),
        lines,
    }
}

pub(crate) fn complete_addition(path: &RepoPath, content: &str) -> ResolvedDiffFile {
    let lines: Vec<_> = content.lines().collect();
    let line_count = u32::try_from(lines.len()).unwrap_or(u32::MAX);
    ResolvedDiffFile {
        old_path: None,
        new_path: Some(path.as_str().to_owned()),
        language: Language::from_path(path.as_str()),
        old_object_id: None,
        new_object_id: None,
        is_new: true,
        is_deleted: false,
        hunks: vec![ResolvedDiffHunk {
            old_start: 0,
            old_lines: 0,
            new_start: if line_count == 0 { 0 } else { 1 },
            new_lines: line_count,
            heading: String::new(),
            lines: lines
                .into_iter()
                .enumerate()
                .map(|(index, content)| DiffLine {
                    kind: DiffLineKind::Addition,
                    content: content.to_owned(),
                    old_line: None,
                    new_line: Some(index as u32 + 1),
                })
                .collect(),
        }],
    }
}

fn invalid_patch(line: usize, message: &str) -> RepositoryError {
    RepositoryError::new(
        RepositoryErrorKind::InvalidPatch,
        format!("{message} at patch line {line}"),
    )
}

fn patch_path(value: &str) -> Option<Option<String>> {
    let value = if value.starts_with('"') {
        take_git_token(value)?.0
    } else {
        value.split('\t').next().unwrap_or(value).to_owned()
    };
    if value == "/dev/null" {
        Some(None)
    } else {
        let normalized = value
            .strip_prefix("a/")
            .or_else(|| value.strip_prefix("b/"))
            .unwrap_or(&value);
        RepoPath::parse(normalized).ok()?;
        Some(Some(normalized.to_owned()))
    }
}

fn parse_diff_git_paths(line: &str) -> Option<(String, String)> {
    let rest = line.strip_prefix("diff --git ")?;
    let (old, new) = if rest.starts_with('"') {
        let (old, rest) = take_git_token(rest)?;
        let (new, trailing) = take_git_token(rest.trim_start())?;
        if !trailing.trim().is_empty() {
            return None;
        }
        (old, new)
    } else {
        // Git leaves ordinary spaces unquoted. Prefer a delimiter that yields
        // the same old/new path (the common no-rename case).
        let first = rest.find(" b/")?;
        let mut split = first;
        for (index, _) in rest.match_indices(" b/") {
            let old = rest.get(2..index)?;
            let new = rest.get(index + 3..)?;
            if old == new {
                split = index;
                break;
            }
        }
        (rest[..split].to_owned(), rest[split + 1..].to_owned())
    };
    let old = old.strip_prefix("a/")?;
    let new = new.strip_prefix("b/")?;
    RepoPath::parse(old).ok()?;
    RepoPath::parse(new).ok()?;
    Some((old.to_owned(), new.to_owned()))
}

/// Decode one token using Git's C-style path quoting.
fn take_git_token(value: &str) -> Option<(String, &str)> {
    if !value.starts_with('"') {
        let split = value.find(char::is_whitespace).unwrap_or(value.len());
        return Some((value[..split].to_owned(), &value[split..]));
    }

    let bytes = value.as_bytes();
    let mut decoded = Vec::new();
    let mut index = 1;
    while index < bytes.len() {
        match bytes[index] {
            b'"' => {
                let decoded = String::from_utf8(decoded).ok()?;
                return Some((decoded, &value[index + 1..]));
            }
            b'\\' => {
                index += 1;
                let escaped = *bytes.get(index)?;
                match escaped {
                    b'a' => decoded.push(0x07),
                    b'b' => decoded.push(0x08),
                    b't' => decoded.push(b'\t'),
                    b'n' => decoded.push(b'\n'),
                    b'v' => decoded.push(0x0b),
                    b'f' => decoded.push(0x0c),
                    b'r' => decoded.push(b'\r'),
                    b'\\' | b'"' => decoded.push(escaped),
                    b'0'..=b'7' => {
                        let mut octal = (escaped - b'0') as u16;
                        for _ in 0..2 {
                            let Some(next @ b'0'..=b'7') = bytes.get(index + 1).copied() else {
                                break;
                            };
                            index += 1;
                            octal = octal * 8 + (next - b'0') as u16;
                        }
                        decoded.push(u8::try_from(octal).ok()?);
                    }
                    _ => return None,
                }
            }
            byte if byte.is_ascii() => decoded.push(byte),
            _ => {
                let character = value[index..].chars().next()?;
                let mut encoded = [0; 4];
                decoded.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
                index += character.len_utf8() - 1;
            }
        }
        index += 1;
    }
    None
}

fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32, String)> {
    let rest = line.strip_prefix("@@ -")?;
    let (ranges, heading) = rest.split_once(" @@")?;
    let (old, new) = ranges.split_once(" +")?;
    let (old_start, old_lines) = parse_range(old)?;
    let (new_start, new_lines) = parse_range(new)?;
    Some((
        old_start,
        old_lines,
        new_start,
        new_lines,
        heading.trim_start().to_owned(),
    ))
}

fn parse_range(value: &str) -> Option<(u32, u32)> {
    if let Some((start, count)) = value.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((value.parse().ok()?, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PATCH: &str = "diff --git a/src/lib.rs b/src/lib.rs\nindex 1111111..2222222 100644\n--- a/src/lib.rs\n+++ b/src/lib.rs\n@@ -1,3 +1,4 @@ fn main() {\n unchanged\n-old\n+new\n+extra\n tail\n";

    #[test]
    fn parses_paths_hunks_kinds_and_line_numbers() {
        let parsed = parse_unified_diff(PATCH).unwrap();
        let file = &parsed.files[0];
        assert_eq!(file.language, Language::Rust);
        assert_eq!(file.old_path.as_deref(), Some("src/lib.rs"));
        assert_eq!(file.new_object_id.as_deref(), Some("2222222"));
        assert_eq!(file.hunks[0].heading, "fn main() {");
        assert_eq!(file.hunks[0].lines[1].old_line, Some(2));
        assert_eq!(file.hunks[0].lines[2].new_line, Some(2));
        assert_eq!(file.hunks[0].lines[3].new_line, Some(3));
    }

    #[test]
    fn selects_changed_lines_by_the_correct_side() {
        let parsed = parse_unified_diff(PATCH).unwrap();
        let selected = select_diff_ranges(
            parsed,
            &[DiffFileRequest {
                path: RepoPath::parse("src/lib.rs").unwrap(),
                before_lines: Some(LineRange::new(2, 2).unwrap()),
                after_lines: None,
            }],
            1,
        )
        .unwrap();
        assert_eq!(selected.files.len(), 1);

        let parsed = parse_unified_diff(PATCH).unwrap();
        let error = select_diff_ranges(
            parsed,
            &[DiffFileRequest {
                path: RepoPath::parse("src/lib.rs").unwrap(),
                before_lines: Some(LineRange::new(20, 30).unwrap()),
                after_lines: None,
            }],
            1,
        )
        .unwrap_err();
        assert_eq!(error.kind(), RepositoryErrorKind::EmptySelection);
    }

    #[test]
    fn splits_merged_hunk_around_unselected_changed_regions() {
        let patch = "--- a/file.txt\n+++ b/file.txt\n@@ -1,6 +1,6 @@\n one\n-two\n+TWO\n three\n-four\n+FOUR\n five\n six\n";
        let selected = select_diff_ranges(
            parse_unified_diff(patch).unwrap(),
            &[DiffFileRequest {
                path: RepoPath::parse("file.txt").unwrap(),
                before_lines: None,
                after_lines: Some(LineRange::new(2, 2).unwrap()),
            }],
            1,
        )
        .unwrap();

        let hunks = &selected.files[0].hunks;
        assert_eq!(hunks.len(), 1);
        assert_eq!(hunks[0].old_start, 1);
        assert_eq!(hunks[0].old_lines, 3);
        assert_eq!(hunks[0].new_start, 1);
        assert_eq!(hunks[0].new_lines, 3);
        assert_eq!(
            hunks[0]
                .lines
                .iter()
                .map(|line| (line.kind, line.content.as_str()))
                .collect::<Vec<_>>(),
            vec![
                (DiffLineKind::Context, "one"),
                (DiffLineKind::Deletion, "two"),
                (DiffLineKind::Addition, "TWO"),
                (DiffLineKind::Context, "three"),
            ]
        );
    }

    #[test]
    fn rebuilds_zero_count_side_using_unified_diff_coordinates() {
        let patch =
            "--- a/file.txt\n+++ b/file.txt\n@@ -1,3 +1,4 @@\n one\n+inserted\n two\n three\n";
        let selected = select_diff_ranges(
            parse_unified_diff(patch).unwrap(),
            &[DiffFileRequest {
                path: RepoPath::parse("file.txt").unwrap(),
                before_lines: None,
                after_lines: Some(LineRange::new(2, 2).unwrap()),
            }],
            0,
        )
        .unwrap();

        let hunk = &selected.files[0].hunks[0];
        assert_eq!((hunk.old_start, hunk.old_lines), (1, 0));
        assert_eq!((hunk.new_start, hunk.new_lines), (2, 1));
        assert_eq!(hunk.lines[0].old_line, None);
        assert_eq!(hunk.lines[0].new_line, Some(2));
    }

    #[test]
    fn parses_plain_unified_patch_without_git_headers() {
        let patch = "--- src/old.rs\n+++ src/new.rs\n@@ -1 +1 @@\n-old\n+new\n";
        let parsed = parse_unified_diff(patch).unwrap();
        assert_eq!(parsed.files.len(), 1);
        assert_eq!(parsed.files[0].old_path.as_deref(), Some("src/old.rs"));
        assert_eq!(parsed.files[0].new_path.as_deref(), Some("src/new.rs"));
        assert_eq!(parsed.files[0].language, Language::Rust);
        assert_eq!(parsed.files[0].hunks[0].lines.len(), 2);
    }

    #[test]
    fn infers_diff_language_from_the_displayed_path() {
        let patch = "--- diagram.txt\n+++ diagram.mmd\n@@ -1 +1 @@\n-old\n+flowchart LR\n";
        let parsed = parse_unified_diff(patch).unwrap();
        assert_eq!(parsed.files[0].language, Language::Mermaid);
    }

    #[test]
    fn decodes_git_quoted_paths() {
        let patch = "diff --git \"a/quo\\\"te.rs\" \"b/quo\\\"te.rs\"\n--- \"a/quo\\\"te.rs\"\n+++ \"b/quo\\\"te.rs\"\n@@ -1 +1 @@\n-old\n+new\n";
        let parsed = parse_unified_diff(patch).unwrap();
        assert_eq!(parsed.files[0].old_path.as_deref(), Some("quo\"te.rs"));
        assert_eq!(parsed.files[0].new_path.as_deref(), Some("quo\"te.rs"));
    }

    #[test]
    fn rejects_malformed_hunk_counts_and_non_patch_content() {
        let bad_count = "--- file.rs\n+++ file.rs\n@@ -1,2 +1 @@\n-old\n+new\n";
        assert_eq!(
            parse_unified_diff(bad_count).unwrap_err().kind(),
            RepositoryErrorKind::InvalidPatch
        );
        assert_eq!(
            parse_unified_diff("this is not a patch")
                .unwrap_err()
                .kind(),
            RepositoryErrorKind::InvalidPatch
        );
    }

    #[test]
    fn header_like_content_lines_stay_in_their_hunk() {
        // Deleting `-- note` and adding `++ counter` produce body lines that
        // start with `--- ` and `+++ `.
        let patch = "--- a/q.sql\n+++ b/q.sql\n@@ -1,3 +1,3 @@\n select 1;\n--- note\n+++ counter\n select 2;\n";
        let parsed = parse_unified_diff(patch).unwrap();
        assert_eq!(parsed.files.len(), 1);
        let lines = &parsed.files[0].hunks[0].lines;
        assert_eq!(lines[1].kind, DiffLineKind::Deletion);
        assert_eq!(lines[1].content, "-- note");
        assert_eq!(lines[2].kind, DiffLineKind::Addition);
        assert_eq!(lines[2].content, "++ counter");
    }

    #[test]
    fn accepts_blank_context_and_format_patch_trailer() {
        let patch = "Subject: [PATCH] demo\n\n---\ndiff --git a/f.txt b/f.txt\n--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n a\n\n-b\n+B\n\\ No newline at end of file\n-- \n2.45.0\n";
        let parsed = parse_unified_diff(patch).unwrap();
        let lines = &parsed.files[0].hunks[0].lines;
        assert_eq!(lines.len(), 4);
        assert_eq!(lines[1].kind, DiffLineKind::Context);
        assert_eq!(lines[1].content, "");
    }

    #[test]
    fn rejects_hunk_bodies_that_disagree_with_their_header() {
        for patch in [
            "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n",
            "--- a/f\n+++ b/f\n@@ -1 +1 @@\n-a\n+b\n+extra\n",
            "--- a/f\n+++ b/f\n@@ -1,2 +1,1 @@\n a\n+b\n",
        ] {
            assert_eq!(
                parse_unified_diff(patch).unwrap_err().kind(),
                RepositoryErrorKind::InvalidPatch,
                "{patch:?}"
            );
        }
    }

    #[test]
    fn rejects_selected_path_without_changes() {
        let error = select_diff_ranges(
            parse_unified_diff(PATCH).unwrap(),
            &[
                DiffFileRequest {
                    path: RepoPath::parse("src/lib.rs").unwrap(),
                    before_lines: None,
                    after_lines: None,
                },
                DiffFileRequest {
                    path: RepoPath::parse("src/typo.rs").unwrap(),
                    before_lines: None,
                    after_lines: None,
                },
            ],
            3,
        )
        .unwrap_err();
        assert_eq!(error.kind(), RepositoryErrorKind::UnchangedPath);
    }
}
