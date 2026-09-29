//! Rendered diffs of Markdown documents.
//!
//! A diff hunk is a fragment of a document: it can cut a list, a code fence,
//! or a `$$` block in half, so hunk lines cannot be rendered as Markdown. This
//! module works on the complete before and after documents instead. It splits
//! each into blocks that render on their own, matches them, and keeps the
//! blocks that overlap the displayed hunks. The browser only renders the
//! resulting segments; it never compares text.

use std::collections::BTreeSet;

use pulldown_cmark::{Event, Options, Parser, Tag};
use serde::{Deserialize, Serialize};

use super::diff::{DiffLine, ResolvedDiffFile};

/// The displayed part of a Markdown diff, as complete blocks.
#[derive(Clone, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedMarkdownDiff {
    pub segments: Vec<RenderedSegment>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum RenderedSegment {
    Unchanged {
        markdown: String,
    },
    Removed {
        markdown: String,
    },
    Added {
        markdown: String,
    },
    /// Consecutive blocks outside the displayed hunks.
    Gap {
        blocks: u32,
    },
}

/// Parser options matching the browser's renderer: GFM (tables, task lists,
/// strikethrough, footnotes) and KaTeX math.
fn options() -> Options {
    Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM
}

/// One unit of change: a top-level block, or one item of a top-level list or
/// block quote, as complete source lines.
#[derive(Clone, Debug, Eq, PartialEq)]
struct Block {
    /// Source text of the block's lines, trailing whitespace removed.
    text: String,
    /// One-based inclusive source lines.
    first_line: u32,
    last_line: u32,
}

struct Document {
    blocks: Vec<Block>,
    /// Link reference and footnote definitions, appended to every segment so
    /// reference-style links still resolve when a block renders alone.
    definitions: String,
}

fn split(text: &str) -> Document {
    let line_starts = std::iter::once(0)
        .chain(text.match_indices('\n').map(|(offset, _)| offset + 1))
        .collect::<Vec<_>>();
    let line_of = |byte: usize| line_starts.partition_point(|start| *start <= byte);
    let lines = text.split('\n').collect::<Vec<_>>();
    let block = |start: usize, end: usize| {
        let first = line_of(start);
        let last = line_of(end.saturating_sub(1).max(start));
        let source = lines[first - 1..last]
            .iter()
            .map(|line| line.trim_end())
            .collect::<Vec<_>>()
            .join("\n");
        Block {
            text: source.trim_end().to_owned(),
            first_line: first as u32,
            last_line: last as u32,
        }
    };

    let parser = Parser::new_ext(text, options()).into_offset_iter();
    let mut definition_lines = BTreeSet::new();
    for (_, definition) in parser.reference_definitions().iter() {
        definition_lines.extend(
            line_of(definition.span.start)..=line_of(definition.span.end.saturating_sub(1)),
        );
    }

    let mut blocks = Vec::new();
    let mut depth = 0usize;
    // Inside a top-level list or block quote, whose children are the units.
    let mut container = false;
    for (event, range) in parser {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    container = matches!(tag, Tag::List(_) | Tag::BlockQuote(_));
                    if !container {
                        blocks.push(block(range.start, range.end));
                    }
                } else if depth == 1 && container {
                    blocks.push(block(range.start, range.end));
                }
                if matches!(tag, Tag::FootnoteDefinition(_)) && depth == 0 {
                    let last = line_of(range.end.saturating_sub(1));
                    definition_lines.extend(line_of(range.start)..=last);
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            Event::Rule if depth == 0 || (depth == 1 && container) => {
                blocks.push(block(range.start, range.end));
            }
            _ => {}
        }
    }
    let definitions = definition_lines
        .iter()
        .map(|line| lines[line - 1].trim_end())
        .collect::<Vec<_>>()
        .join("\n");
    Document {
        blocks: blocks
            .into_iter()
            .filter(|block| !block.text.is_empty())
            .collect(),
        definitions,
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Step {
    Same(usize, usize),
    Removed(usize),
    Added(usize),
}

/// Longest common subsequence over block text; removals come before the
/// additions that replace them.
fn matches(before: &[Block], after: &[Block]) -> Vec<Step> {
    let (n, m) = (before.len(), after.len());
    let mut table = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            table[i][j] = if before[i].text == after[j].text {
                table[i + 1][j + 1] + 1
            } else {
                table[i + 1][j].max(table[i][j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut steps = Vec::new();
    while i < n || j < m {
        if i < n && j < m && before[i].text == after[j].text {
            steps.push(Step::Same(i, j));
            i += 1;
            j += 1;
        } else if i < n && (j == m || table[i + 1][j] >= table[i][j + 1]) {
            steps.push(Step::Removed(i));
            i += 1;
        } else {
            steps.push(Step::Added(j));
            j += 1;
        }
    }
    pair_replacements(steps)
}

/// Within each run of changes between unchanged blocks, put every removed
/// block next to the added block that replaces it, so a changed block reads
/// as "old, new" rather than all removals followed by all additions.
fn pair_replacements(steps: Vec<Step>) -> Vec<Step> {
    let mut paired = Vec::with_capacity(steps.len());
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let flush = |paired: &mut Vec<Step>, removed: &mut Vec<Step>, added: &mut Vec<Step>| {
        let (mut removed, mut added) = (removed.drain(..), added.drain(..));
        loop {
            match (removed.next(), added.next()) {
                (None, None) => break,
                (old, new) => paired.extend(old.into_iter().chain(new)),
            }
        }
    };
    for step in steps {
        match step {
            Step::Removed(_) => removed.push(step),
            Step::Added(_) => added.push(step),
            Step::Same(..) => {
                flush(&mut paired, &mut removed, &mut added);
                paired.push(step);
            }
        }
    }
    flush(&mut paired, &mut removed, &mut added);
    paired
}

/// Render the part of a Markdown change that the line diff of `file`
/// displays, as complete blocks. `before` and `after` are the complete
/// documents; a new file has an empty `before`, a deleted one an empty `after`.
pub fn render_markdown_diff(
    file: &ResolvedDiffFile,
    before: &str,
    after: &str,
) -> RenderedMarkdownDiff {
    let lines = file.hunks.iter().flat_map(|hunk| &hunk.lines);
    let old_shown = lines
        .clone()
        .filter_map(|line: &DiffLine| line.old_line)
        .collect::<BTreeSet<_>>();
    let new_shown = lines
        .filter_map(|line: &DiffLine| line.new_line)
        .collect::<BTreeSet<_>>();
    let overlaps = |shown: &BTreeSet<u32>, block: &Block| {
        shown
            .range(block.first_line..=block.last_line)
            .next()
            .is_some()
    };

    let before = split(before);
    let after = split(after);
    let with = |block: &Block, definitions: &str| {
        if definitions.is_empty() {
            block.text.clone()
        } else {
            format!("{}\n\n{definitions}", block.text)
        }
    };

    let mut segments = Vec::new();
    let mut hidden = 0u32;
    for step in matches(&before.blocks, &after.blocks) {
        let segment = match step {
            Step::Same(i, j) => (overlaps(&old_shown, &before.blocks[i])
                || overlaps(&new_shown, &after.blocks[j]))
            .then(|| RenderedSegment::Unchanged {
                markdown: with(&after.blocks[j], &after.definitions),
            }),
            Step::Removed(i) => {
                overlaps(&old_shown, &before.blocks[i]).then(|| RenderedSegment::Removed {
                    markdown: with(&before.blocks[i], &before.definitions),
                })
            }
            Step::Added(j) => {
                overlaps(&new_shown, &after.blocks[j]).then(|| RenderedSegment::Added {
                    markdown: with(&after.blocks[j], &after.definitions),
                })
            }
        };
        match segment {
            Some(segment) => {
                if hidden > 0 {
                    segments.push(RenderedSegment::Gap { blocks: hidden });
                    hidden = 0;
                }
                segments.push(segment);
            }
            None => hidden += 1,
        }
    }
    if hidden > 0 && !segments.is_empty() {
        segments.push(RenderedSegment::Gap { blocks: hidden });
    }
    RenderedMarkdownDiff { segments }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repository::diff::parse_unified_diff;

    fn texts(document: &str) -> Vec<String> {
        split(document)
            .blocks
            .into_iter()
            .map(|block| block.text)
            .collect()
    }

    #[test]
    fn blocks_are_complete_and_lists_split_into_items() {
        let document = "# Setup\n\nInstall the tools.\nRun it.\n\n- Node\n- Rust\n  nested line\n\n3. third\n4. fourth\n\n```sh\njust install\n\nmore\n```\n\n> quoted one\n>\n> quoted two\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n$$\nx^2\n$$\n\n---\n";
        assert_eq!(
            texts(document),
            [
                "# Setup",
                "Install the tools.\nRun it.",
                "- Node",
                "- Rust\n  nested line",
                "3. third",
                "4. fourth",
                "```sh\njust install\n\nmore\n```",
                "> quoted one",
                "> quoted two",
                "| a | b |\n|---|---|\n| 1 | 2 |",
                "$$\nx^2\n$$",
                "---",
            ]
        );
    }

    #[test]
    fn definitions_are_collected_for_every_segment() {
        let document = split(
            "See [the guide][g] and a note.[^n]\n\n[g]: https://example.com\n\n[^n]: The note.\n",
        );
        assert_eq!(
            document.definitions,
            "[g]: https://example.com\n[^n]: The note."
        );
    }

    #[test]
    fn matching_keeps_unchanged_blocks_and_pairs_replacements() {
        let before = split("A\n\nB\n\nC\n").blocks;
        let after = split("A\n\nB2\n\nC\n\nD\n").blocks;
        assert_eq!(
            matches(&before, &after),
            [
                Step::Same(0, 0),
                Step::Removed(1),
                Step::Added(1),
                Step::Same(2, 2),
                Step::Added(3)
            ]
        );
    }

    fn file(patch: &str) -> ResolvedDiffFile {
        parse_unified_diff(patch).unwrap().files.remove(0)
    }

    #[test]
    fn only_blocks_overlapping_the_hunks_render_with_gaps_for_the_rest() {
        let before = "# Title\n\nOne.\n\nTwo.\n\nThree.\n\nFour.\n\n- a\n- b\n";
        let after = "# Title\n\nOne.\n\nTwo.\n\nThree.\n\nFour.\n\n- a\n- new\n- b\n";
        let diff = file(
            "diff --git a/d.md b/d.md\n--- a/d.md\n+++ b/d.md\n@@ -11,2 +11,3 @@\n - a\n+- new\n - b\n",
        );
        let rendered = render_markdown_diff(&diff, before, after);
        assert_eq!(
            rendered.segments,
            [
                RenderedSegment::Gap { blocks: 5 },
                RenderedSegment::Unchanged {
                    markdown: "- a".into()
                },
                RenderedSegment::Added {
                    markdown: "- new".into()
                },
                RenderedSegment::Unchanged {
                    markdown: "- b".into()
                },
            ]
        );
    }

    #[test]
    fn reworded_paragraphs_show_removed_then_added_with_definitions() {
        let before = "Install [tools][t].\n\n[t]: https://t.example\n";
        let after = "Install [tools][t] once.\n\n[t]: https://t.example\n";
        let diff = file(
            "diff --git a/d.md b/d.md\n--- a/d.md\n+++ b/d.md\n@@ -1 +1 @@\n-Install [tools][t].\n+Install [tools][t] once.\n",
        );
        assert_eq!(
            render_markdown_diff(&diff, before, after).segments,
            [
                RenderedSegment::Removed {
                    markdown: "Install [tools][t].\n\n[t]: https://t.example".into()
                },
                RenderedSegment::Added {
                    markdown: "Install [tools][t] once.\n\n[t]: https://t.example".into()
                },
            ]
        );
    }

    #[test]
    fn new_and_deleted_documents_are_entirely_added_or_removed() {
        let added = file(
            "diff --git a/n.md b/n.md\nnew file mode 100644\n--- /dev/null\n+++ b/n.md\n@@ -0,0 +1,3 @@\n+# New\n+\n+Text.\n",
        );
        assert_eq!(
            render_markdown_diff(&added, "", "# New\n\nText.\n").segments,
            [
                RenderedSegment::Added {
                    markdown: "# New".into()
                },
                RenderedSegment::Added {
                    markdown: "Text.".into()
                },
            ]
        );
        let deleted = file(
            "diff --git a/o.md b/o.md\ndeleted file mode 100644\n--- a/o.md\n+++ /dev/null\n@@ -1 +0,0 @@\n-Gone.\n",
        );
        assert_eq!(
            render_markdown_diff(&deleted, "Gone.\n", "").segments,
            [RenderedSegment::Removed {
                markdown: "Gone.".into()
            }]
        );
    }
}
