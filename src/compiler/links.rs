//! Block links: Markdown link destinations such as `#queue-def` or
//! `#queue-def:12-18` that point at another block of the lesson.
//!
//! Since source schema 2.3.0 the compiler finds these links in every
//! Markdown-bearing field, validates them against the compiled blocks, and
//! freezes one lesson-wide table in the artifact. The Markdown text itself is
//! unchanged; the browser looks each `#…` destination up in the table.

use std::collections::BTreeMap;
use std::path::PathBuf;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::artifact::{BlockLink, CompiledNode, CompiledNodeContent, LinkedLines};
use crate::diagnostics::Diagnostic;
use crate::source::{Block, LessonSource, MarkdownSource, RunCodeBlock};

/// One `#…` link inside a Markdown text.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct LinkOccurrence {
    /// Destination without the leading `#`, such as `queue-def:12-18`.
    pub(crate) destination: String,
    /// The link's visible text, as plain text.
    pub(crate) text: String,
    /// Unicode-scalar offsets of the whole link in the scanned text.
    pub(crate) start_char: usize,
    pub(crate) end_char: usize,
}

/// A Markdown-bearing field of the lesson and where its text lives.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MarkdownField {
    pub(crate) block_index: usize,
    /// Pointer to the editable value in `lesson.json`.
    pub(crate) pointer: String,
    pub(crate) text: String,
    /// Root-relative path when the text comes from a Markdown file.
    pub(crate) file: Option<PathBuf>,
}

/// Every `#…` link in `text`, in order. Code spans and fences are skipped by
/// the Markdown parser itself.
pub(crate) fn find_links(text: &str) -> Vec<LinkOccurrence> {
    let mut found = Vec::new();
    let mut open: Option<(String, usize, String)> = None;
    let options = Options::ENABLE_TABLES
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_MATH
        | Options::ENABLE_GFM;
    for (event, range) in Parser::new_ext(text, options).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { dest_url, .. }) => {
                if let Some(destination) = dest_url.strip_prefix('#') {
                    open = Some((destination.to_owned(), range.start, String::new()));
                }
            }
            Event::Text(value) | Event::Code(value) => {
                if let Some((_, _, visible)) = &mut open {
                    visible.push_str(&value);
                }
            }
            Event::End(TagEnd::Link) => {
                if let Some((destination, start, visible)) = open.take() {
                    found.push(LinkOccurrence {
                        destination,
                        text: visible,
                        start_char: text[..start].chars().count(),
                        end_char: text[..range.end].chars().count(),
                    });
                }
            }
            _ => {}
        }
    }
    found
}

/// The line part of a link destination.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LinkLines {
    /// No `:` part: the whole block.
    Whole,
    /// `:12` or `:12-18`.
    Range(u32, u32),
    /// A `:` part that is not a valid one-based range.
    Malformed,
}

/// Split `queue-def:12-18` into the block ID and its line part.
pub(crate) fn parse_destination(destination: &str) -> (&str, LinkLines) {
    let Some((id, lines)) = destination.split_once(':') else {
        return (destination, LinkLines::Whole);
    };
    let (start, end) = lines.split_once('-').unwrap_or((lines, lines));
    match (start.trim().parse::<u32>(), end.trim().parse::<u32>()) {
        (Ok(start), Ok(end)) if start > 0 && end >= start => (id, LinkLines::Range(start, end)),
        _ => (id, LinkLines::Malformed),
    }
}

/// The Markdown-bearing fields of a compiled lesson: Markdown blocks (after
/// resolving files), captions, highlight annotations, external-artifact
/// fallbacks, quiz prompts, choices, choice explanations, hints, and
/// explanations.
pub(crate) fn markdown_fields(source: &LessonSource, nodes: &[CompiledNode]) -> Vec<MarkdownField> {
    let mut fields = Vec::new();
    for (index, (block, node)) in source.blocks.iter().zip(nodes).enumerate() {
        let base = format!("/blocks/{index}");
        let mut push = |pointer: String, text: &str, file: Option<PathBuf>| {
            fields.push(MarkdownField {
                block_index: index,
                pointer,
                text: text.to_owned(),
                file,
            });
        };
        match (block, &node.content) {
            (Block::Markdown(block), CompiledNodeContent::Markdown { content, .. }) => {
                match &block.source {
                    MarkdownSource::Inline { .. } => {
                        push(format!("{base}/source/content"), content, None)
                    }
                    MarkdownSource::File { path } => push(
                        format!("{base}/source"),
                        content,
                        Some(PathBuf::from(path.as_str())),
                    ),
                }
            }
            (Block::Code(block), _) => {
                if let Some(caption) = &block.caption {
                    push(format!("{base}/caption"), caption, None);
                }
                for (group, highlight) in block.highlights.iter().enumerate() {
                    if let Some(annotation) = &highlight.annotation {
                        push(
                            format!("{base}/highlights/{group}/annotation"),
                            annotation,
                            None,
                        );
                    }
                }
            }
            (Block::Diff(block), _) => {
                if let Some(caption) = &block.caption {
                    push(format!("{base}/caption"), caption, None);
                }
            }
            (Block::RunCode(block), _) => {
                if let Some(caption) = &block.caption {
                    push(format!("{base}/caption"), caption, None);
                }
            }
            (Block::ExternalArtifact(block), _) => {
                push(format!("{base}/fallback"), &block.fallback, None);
                if let Some(caption) = &block.caption {
                    push(format!("{base}/caption"), caption, None);
                }
            }
            (Block::MultipleChoice(block), CompiledNodeContent::MultipleChoice { prompt, .. }) => {
                let file = match &block.prompt {
                    MarkdownSource::File { path } => Some(PathBuf::from(path.as_str())),
                    MarkdownSource::Inline { .. } => None,
                };
                push(
                    crate::lint::prompt_pointer(source.schema_version, index),
                    prompt,
                    file,
                );
                for (choice_index, choice) in block.choices.iter().enumerate() {
                    push(
                        format!("{base}/choices/{choice_index}/content"),
                        &choice.content,
                        None,
                    );
                    if let Some(explanation) = &choice.explanation {
                        push(
                            format!("{base}/choices/{choice_index}/explanation"),
                            explanation,
                            None,
                        );
                    }
                }
                for (hint_index, hint) in block.hints.iter().enumerate() {
                    push(format!("{base}/hints/{hint_index}"), hint, None);
                }
                push(format!("{base}/explanation"), &block.explanation, None);
            }
            _ => {}
        }
    }
    fields
}

/// The lines a node displays, as the numbers its gutter shows: source-file
/// lines for code (fragment positions for inline code) and new-side lines for
/// diffs. `None` for nodes that cannot take a line range.
pub(crate) fn displayed_lines(node: &CompiledNode) -> Option<(u32, u32)> {
    match &node.content {
        CompiledNodeContent::Code {
            content,
            first_line,
            ..
        } => {
            let count = content.lines().count().max(1) as u32;
            let first = first_line.unwrap_or(1);
            Some((first, first + count - 1))
        }
        CompiledNodeContent::Diff { diff, .. } => {
            let lines = diff
                .files
                .iter()
                .flat_map(|file| &file.hunks)
                .flat_map(|hunk| &hunk.lines)
                .filter_map(|line| line.new_line);
            let (mut low, mut high) = (u32::MAX, 0);
            for line in lines {
                low = low.min(line);
                high = high.max(line);
            }
            (high > 0).then_some((low, high))
        }
        _ => None,
    }
}

/// Validate every block link and build the artifact's link table.
pub(crate) fn resolve(
    source: &LessonSource,
    nodes: &[CompiledNode],
) -> Result<BTreeMap<String, BlockLink>, Vec<Diagnostic>> {
    let mut table = BTreeMap::new();
    let mut diagnostics = Vec::new();
    if !source.schema_version.has_block_links() {
        return Ok(table);
    }
    let by_id = nodes
        .iter()
        .map(|node| (node.source_id.as_str(), node))
        .collect::<BTreeMap<_, _>>();
    // A run block's `of` is a link to the code block it runs, so the browser
    // can preview and jump to it like any other block link.
    for block in &source.blocks {
        if let Block::RunCode(RunCodeBlock { of: Some(of), .. }) = block
            && let Some(target) = by_id.get(of.as_str())
        {
            table.insert(
                of.as_str().to_owned(),
                BlockLink {
                    target: target.node_id,
                    lines: None,
                },
            );
        }
    }
    for field in markdown_fields(source, nodes) {
        for link in find_links(&field.text) {
            let (id, lines) = parse_destination(&link.destination);
            let Some(target) = by_id.get(id) else {
                diagnostics.push(
                    Diagnostic::error(
                        "source.reference.unknown_block",
                        field.pointer.clone(),
                        format!("link `#{}` names no block with ID `{id}`", link.destination),
                    )
                    .with_suggestion("Use the `id` of an existing block, such as `#queue-def`."),
                );
                continue;
            };
            let lines = match lines {
                LinkLines::Whole => None,
                LinkLines::Malformed => {
                    diagnostics.push(
                        Diagnostic::error(
                            "source.reference.invalid_lines",
                            field.pointer.clone(),
                            format!(
                                "link `#{}` has a malformed line range",
                                link.destination
                            ),
                        )
                        .with_suggestion("Write `#block-id:12-18` or `#block-id:12`, with the start not after the end."),
                    );
                    continue;
                }
                LinkLines::Range(start, end) => {
                    let Some((low, high)) = displayed_lines(target) else {
                        diagnostics.push(
                            Diagnostic::error(
                                "source.reference.lines_on_non_code",
                                field.pointer.clone(),
                                format!(
                                    "link `#{}` gives lines, but block `{id}` is not a code or diff block",
                                    link.destination
                                ),
                            )
                            .with_suggestion(format!("Link the whole block with `#{id}`.")),
                        );
                        continue;
                    };
                    if start < low || end > high {
                        diagnostics.push(
                            Diagnostic::error(
                                "source.reference.invalid_lines",
                                field.pointer.clone(),
                                format!(
                                    "link `#{}` asks for lines {start}–{end}, but block `{id}` displays lines {low}–{high}",
                                    link.destination
                                ),
                            )
                            .with_suggestion("Use the line numbers the target block shows in its gutter."),
                        );
                        continue;
                    }
                    Some(LinkedLines { start, end })
                }
            };
            table.insert(
                link.destination.clone(),
                BlockLink {
                    target: target.node_id,
                    lines,
                },
            );
        }
    }
    if diagnostics.is_empty() {
        Ok(table)
    } else {
        Err(diagnostics)
    }
}

/// The node a link destination resolves to in a compiled lesson.
pub(crate) fn target_of<'a>(
    links: &BTreeMap<String, BlockLink>,
    nodes: &'a [CompiledNode],
    destination: &str,
) -> Option<(&'a CompiledNode, Option<LinkedLines>)> {
    let link = links.get(destination)?;
    let node = nodes.get(link.target.get() as usize)?;
    Some((node, link.lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_hash_links_with_their_text_and_skips_code() {
        let text = "See [the `Queue` struct](#queue-def:12-18) and [docs](https://x.example).\n\n`[not](#link)`\n\n```\n[fenced](#nope)\n```\n";
        let links = find_links(text);
        assert_eq!(links.len(), 1);
        assert_eq!(links[0].destination, "queue-def:12-18");
        assert_eq!(links[0].text, "the Queue struct");
        assert_eq!(
            &text[links[0].start_char..links[0].end_char],
            "[the `Queue` struct](#queue-def:12-18)"
        );
    }

    fn lesson(schema: &str, text: &str) -> String {
        serde_json::json!({"schema_version": schema, "title": "Links", "blocks": [
            {"type":"code","id":"queue-def","language":"rust","source":{"kind":"inline","content":"struct Queue {\n    items: Vec<u32>,\n}"}},
            {"type":"markdown","id":"notes","source":{"kind":"inline","content":"Notes."}},
            {"type":"markdown","id":"use","source":{"kind":"inline","content":text}}
        ]})
        .to_string()
    }

    fn compile(input: &str) -> Result<crate::artifact::CompiledLesson, Vec<Diagnostic>> {
        crate::compiler::compile(input, &crate::compiler::CompileOptions::new("."))
    }

    #[test]
    fn valid_links_are_frozen_in_the_artifact() {
        let artifact = compile(&lesson(
            "2.3.0",
            "It holds a [`Queue`](#queue-def:1-3) and [the notes](#notes).",
        ))
        .unwrap();
        let links = &artifact.presentation.links;
        assert_eq!(links.len(), 2);
        assert_eq!(links["queue-def:1-3"].target.get(), 0);
        assert_eq!(
            links["queue-def:1-3"].lines,
            Some(LinkedLines { start: 1, end: 3 })
        );
        assert_eq!(links["notes"].lines, None);
        // The Markdown itself is unchanged.
        let CompiledNodeContent::Markdown { content, .. } = &artifact.presentation.nodes[2].content
        else {
            panic!("expected Markdown")
        };
        assert!(content.contains("(#queue-def:1-3)"));
    }

    #[test]
    fn broken_links_fail_compilation_with_precise_codes() {
        for (text, code) in [
            ("[x](#missing)", "source.reference.unknown_block"),
            ("[x](#queue-def:3-1)", "source.reference.invalid_lines"),
            ("[x](#queue-def:2-9)", "source.reference.invalid_lines"),
            ("[x](#notes:1)", "source.reference.lines_on_non_code"),
        ] {
            let diagnostics = compile(&lesson("2.3.0", text)).unwrap_err();
            assert_eq!(diagnostics[0].code, code, "{text}");
            assert_eq!(diagnostics[0].pointer, "/blocks/2/source/content");
        }
    }

    #[test]
    fn older_schemas_keep_hash_links_as_ordinary_links() {
        let artifact = compile(&lesson("2.2.0", "[x](#missing)")).unwrap();
        assert!(artifact.presentation.links.is_empty());
    }

    #[test]
    fn links_in_quizzes_and_highlight_annotations_are_checked() {
        let input = serde_json::json!({"schema_version":"2.3.0","title":"Q","blocks":[
            {"type":"code","id":"def","language":"rust","source":{"kind":"inline","content":"fn a() {}"}},
            {"type":"multiple_choice","id":"q","prompt":{"kind":"inline","content":"What does [it](#def) do?"},
             "choices":[{"content":"A","correct":true},{"content":"B","explanation":"See [gone](#gone)."}],
             "explanation":"Because."}
        ]})
        .to_string();
        let diagnostics = compile(&input).unwrap_err();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].pointer, "/blocks/1/choices/1/explanation");
    }

    #[test]
    fn destinations_split_into_id_and_lines() {
        assert_eq!(
            parse_destination("queue-def"),
            ("queue-def", LinkLines::Whole)
        );
        assert_eq!(
            parse_destination("queue-def:12"),
            ("queue-def", LinkLines::Range(12, 12))
        );
        assert_eq!(
            parse_destination("queue-def:12-18"),
            ("queue-def", LinkLines::Range(12, 18))
        );
        for bad in ["q:18-12", "q:0", "q:a-b", "q:"] {
            assert_eq!(parse_destination(bad).1, LinkLines::Malformed, "{bad}");
        }
    }
}
