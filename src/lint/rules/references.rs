//! Lint rules about block links and about content that a link should replace:
//! large previews, forward and adjacent links, excerpts repeated far apart,
//! and names formatted as code that no nearby block shows.

use std::collections::HashSet;

use crate::artifact::{CompiledNode, CompiledNodeContent, ResourceProvenance, RunCodeSource};
use crate::compiler::links::{MarkdownField, find_links, markdown_fields};

use super::{
    Rules, TextPlace, code_reference_name, displayed_code, identifier_segments, identifier_words,
    inline_code_spans,
};
use crate::lint::{RelatedLintLocation, SourceLocation};

/// Where a character range of a Markdown field is in its editable source.
fn field_location(
    rules: &Rules<'_>,
    field: &MarkdownField,
    start_char: usize,
    end_char: usize,
) -> SourceLocation {
    match &field.file {
        None => rules
            .spans
            .string_range(&field.pointer, start_char, end_char),
        Some(path) => {
            let before = field.text.chars().take(start_char).collect::<String>();
            let line = before.matches('\n').count() + 1;
            let column = before.rsplit('\n').next().unwrap_or("").chars().count() + 1;
            SourceLocation::file_line(
                rules.root.join(path),
                line,
                column,
                column + end_char.saturating_sub(start_char),
            )
        }
    }
}

/// How many lines a link's preview shows.
fn preview_lines(node: &CompiledNode, lines: Option<(u32, u32)>) -> usize {
    match (&node.content, lines) {
        (CompiledNodeContent::Code { .. }, Some((start, end))) => (end - start + 1) as usize,
        (CompiledNodeContent::Code { content, .. }, None) => content.lines().count(),
        (CompiledNodeContent::Diff { diff, .. }, range) => diff
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .map(|hunk| match range {
                None => hunk.lines.len(),
                Some((start, end)) => {
                    let kept = hunk
                        .lines
                        .iter()
                        .map(|line| line.new_line.is_some_and(|n| n >= start && n <= end))
                        .collect::<Vec<_>>();
                    match (
                        kept.iter().position(|kept| *kept),
                        kept.iter().rposition(|kept| *kept),
                    ) {
                        (Some(first), Some(last)) => last - first + 1,
                        _ => 0,
                    }
                }
            })
            .sum(),
        (CompiledNodeContent::Markdown { content, .. }, _) => content.lines().count(),
        (CompiledNodeContent::MultipleChoice { prompt, .. }, _) => prompt.lines().count(),
        // A run block previews whole, since a link to one takes no line range:
        // its own code, or the pointer to the code it runs.
        (
            CompiledNodeContent::RunCode {
                code: RunCodeSource::Own { content, .. },
                ..
            },
            _,
        ) => content.lines().count(),
        (
            CompiledNodeContent::RunCode {
                code: RunCodeSource::Of { .. },
                ..
            },
            _,
        ) => 1,
        // An external artifact previews whole, as its alt text and fallback.
        (CompiledNodeContent::ExternalArtifact { fallback, .. }, _) => 1 + fallback.lines().count(),
    }
}

/// Highlight groups of a code node as source-file ranges, for suggestions.
fn highlight_ranges(node: &CompiledNode) -> Vec<(usize, u32, u32)> {
    let CompiledNodeContent::Code {
        highlights,
        first_line,
        ..
    } = &node.content
    else {
        return Vec::new();
    };
    let offset = first_line.unwrap_or(1) - 1;
    highlights
        .iter()
        .enumerate()
        .flat_map(|(group, highlight)| {
            highlight
                .lines
                .iter()
                .map(move |range| (group, range.start + offset, range.end + offset))
        })
        .collect()
}

fn span(start: u32, end: u32) -> String {
    if start == end {
        start.to_string()
    } else {
        format!("{start}-{end}")
    }
}

impl Rules<'_> {
    pub(super) fn reference_rules(&mut self) {
        let nodes = &self.artifact.presentation.nodes;
        let fields = markdown_fields(self.source, nodes);
        if self.source.schema_version.has_block_links() {
            for field in &fields {
                for link in find_links(&field.text) {
                    self.link_rules(field, &link);
                }
            }
        }
        self.repeated_excerpts();
        self.distant_code_references();
    }

    fn link_rules(&mut self, field: &MarkdownField, link: &crate::compiler::links::LinkOccurrence) {
        let Some(frozen) = self.artifact.presentation.links.get(&link.destination) else {
            return;
        };
        let target_index = frozen.target.get() as usize;
        let target = &self.artifact.presentation.nodes[target_index];
        let target_id = target.source_id.clone();
        let location = field_location(self, field, link.start_char, link.end_char);
        let lines = frozen.lines.map(|lines| (lines.start, lines.end));
        let shown = preview_lines(target, lines);
        if shown > self.config.max_reference_preview_lines {
            let groups = highlight_ranges(target)
                .into_iter()
                .map(|(group, start, end)| {
                    format!(
                        "highlight group {group} covers lines {}",
                        span(start, end).replace('-', "–")
                    )
                })
                .collect::<Vec<_>>();
            let example = highlight_ranges(target)
                .first()
                .map(|(_, start, end)| format!("#{target_id}:{}", span(*start, *end)))
                .unwrap_or_else(|| format!("#{target_id}:12-18"));
            let suggestion = if matches!(target.content, CompiledNodeContent::RunCode { .. }) {
                "A link to a run block previews all of its code and takes no line range; shorten the code, or link a shorter block.".to_owned()
            } else if matches!(target.content, CompiledNodeContent::ExternalArtifact { .. }) {
                "A link to an external artifact previews its alt text and fallback and takes no line range; shorten the fallback, or link a shorter block.".to_owned()
            } else if groups.is_empty() {
                format!("Link only the lines the reader needs, e.g. `{example}`.")
            } else {
                format!(
                    "Link only the lines the reader needs, e.g. `{example}`; in this block, {}.",
                    groups.join(", ")
                )
            };
            self.add(
                Some(field.block_index),
                "lint.reference.large_preview",
                &field.pointer,
                format!("link to `{target_id}` previews {shown} lines"),
                suggestion,
                Some(location.clone()),
            );
        }
        let distance = target_index.abs_diff(field.block_index);
        if distance == 1 {
            self.add(
                Some(field.block_index),
                "lint.reference.adjacent",
                &field.pointer,
                format!("link to `{target_id}` points at the block right next to it"),
                "The target is already in view; drop the link, or keep it only if the text is read far from the block.",
                Some(location),
            );
        } else if target_index > field.block_index {
            self.add(
                Some(field.block_index),
                "lint.reference.forward",
                &field.pointer,
                format!("link to `{target_id}` points at a block the learner has not reached yet"),
                "Consider showing the definition before the text that relies on it; keep the link if \"defined below\" is intended.",
                Some(location),
            );
        }
    }

    /// Two code blocks far apart that show the same lines of one file.
    fn repeated_excerpts(&mut self) {
        let nodes = &self.artifact.presentation.nodes;
        let excerpts =
            nodes
                .iter()
                .enumerate()
                .filter_map(|(index, node)| match &node.content {
                    CompiledNodeContent::Code {
                        content,
                        first_line: Some(first),
                        provenance:
                            ResourceProvenance::File { path, .. }
                            | ResourceProvenance::GitBlob { path, .. },
                        ..
                    } => Some((
                        index,
                        path.clone(),
                        *first,
                        content.lines().collect::<Vec<_>>(),
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>();
        let mut reported = HashSet::new();
        for (later_position, (later, path, later_first, later_lines)) in excerpts.iter().enumerate()
        {
            for (earlier, earlier_path, earlier_first, earlier_lines) in &excerpts[..later_position]
            {
                if path != earlier_path
                    || later - earlier - 1 < self.config.repeated_excerpt_gap
                    || reported.contains(later)
                {
                    continue;
                }
                // Longest run of identical lines at the same file line numbers.
                let (mut best, mut run_start, mut best_start) = (0usize, 0u32, 0u32);
                let mut run = 0usize;
                for (offset, text) in later_lines.iter().enumerate() {
                    let line = later_first + offset as u32;
                    let same = line
                        .checked_sub(*earlier_first)
                        .and_then(|index| earlier_lines.get(index as usize))
                        .is_some_and(|earlier_text| earlier_text == text);
                    if same {
                        if run == 0 {
                            run_start = line;
                        }
                        run += 1;
                        if run > best {
                            best = run;
                            best_start = run_start;
                        }
                    } else {
                        run = 0;
                    }
                }
                if best < self.config.repeated_excerpt_min_lines {
                    continue;
                }
                reported.insert(*later);
                let best_end = best_start + best as u32 - 1;
                let earlier_id = &nodes[*earlier].source_id;
                let pointer = match &self.source.blocks[*later] {
                    crate::source::Block::Code(block) => match &block.source {
                        crate::source::CodeSource::File { lines: Some(_), .. }
                        | crate::source::CodeSource::GitBlob { lines: Some(_), .. } => {
                            format!("/blocks/{later}/source/lines")
                        }
                        _ => format!("/blocks/{later}/source"),
                    },
                    _ => format!("/blocks/{later}/source"),
                };
                let related = self.spans.location(&format!("/blocks/{earlier}"));
                let schema = if self.source.schema_version.has_block_links() {
                    ""
                } else {
                    " (block links need source schema 2.3.0)"
                };
                self.add(
                    Some(*later),
                    "lint.code.repeated_excerpt",
                    &pointer,
                    format!(
                        "lines {best_start}–{best_end} of {path} are already shown in block `{earlier_id}`"
                    ),
                    format!(
                        "Show these lines once: link them where this block relies on them, e.g. `[…](#{earlier_id}:{best_start}-{best_end})`, and narrow this block to what is new{schema}."
                    ),
                    None,
                )
                .related
                .push(RelatedLintLocation {
                    message: format!("block `{earlier_id}` already shows these lines"),
                    location: related,
                });
            }
        }
    }

    /// Names formatted as code that some block shows, but none nearby, and
    /// that no link in the same text points to.
    fn distant_code_references(&mut self) {
        let nodes = &self.artifact.presentation.nodes;
        let shown_by = nodes
            .iter()
            .map(|node| match &node.content {
                CompiledNodeContent::Diff { diff, .. } => diff
                    .files
                    .iter()
                    .flat_map(|file| &file.hunks)
                    .flat_map(|hunk| &hunk.lines)
                    .flat_map(|line| identifier_words(&line.content))
                    .collect::<HashSet<_>>(),
                other => displayed_code(other)
                    .map(|code| identifier_words(code).collect())
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();
        let shows = |node: usize, name: &str| {
            identifier_segments(name).all(|segment| shown_by[node].contains(segment))
        };
        let gap = self.config.code_reference_gap;
        for (index, text, place) in self.scanned_texts() {
            let linked_targets = find_links(&text)
                .iter()
                .filter_map(|link| self.artifact.presentation.links.get(&link.destination))
                .map(|link| link.target.get() as usize)
                .collect::<Vec<_>>();
            let mut reported = HashSet::new();
            for code in inline_code_spans(&text) {
                let Some(name) = code_reference_name(code.content) else {
                    continue;
                };
                if !code_shaped(code.content, name) {
                    continue;
                }
                // Names shown nowhere belong to `lint.markdown.unshown_code_reference`.
                let showing = (0..nodes.len())
                    .filter(|node| shows(*node, name))
                    .collect::<Vec<_>>();
                if showing.is_empty()
                    || showing.iter().any(|node| node.abs_diff(index) <= gap)
                    || linked_targets.iter().any(|target| shows(*target, name))
                    || !reported.insert(name.to_owned())
                {
                    continue;
                }
                let nearest = *showing
                    .iter()
                    .min_by_key(|node| node.abs_diff(index))
                    .expect("at least one block shows the name");
                let target = &nodes[nearest];
                let last_segment = identifier_segments(name).last().unwrap_or(name);
                let destination = match name_line(target, last_segment) {
                    Some(line) => format!("{}:{line}", target.source_id),
                    None => target.source_id.clone(),
                };
                let (pointer, location) = match &place {
                    TextPlace::Json(pointer) => (
                        pointer.clone(),
                        self.spans.string_range(
                            pointer,
                            code.start_char,
                            code.start_char + code.content.chars().count(),
                        ),
                    ),
                    TextPlace::File(path) => (
                        format!("/blocks/{index}"),
                        SourceLocation::file_line(
                            path.clone(),
                            code.line,
                            code.column,
                            code.column + code.content.chars().count(),
                        ),
                    ),
                };
                let schema = if self.source.schema_version.has_block_links() {
                    ""
                } else {
                    " (block links need source schema 2.3.0)"
                };
                self.add(
                    Some(index),
                    "lint.markdown.distant_code_reference",
                    &pointer,
                    format!(
                        "`{name}` is shown in block `{}`, {} blocks away, but in no block within {gap}",
                        target.source_id,
                        nearest.abs_diff(index)
                    ),
                    format!(
                        "Link the block that shows it, e.g. [`{name}`](#{destination}), so the learner can peek at it{schema}."
                    ),
                    Some(location),
                );
            }
        }
    }
}

/// Whether a name looks like code rather than a word: a single all-lowercase
/// word such as `state` or `check` also matches prose, comments, and strings in
/// far-away blocks, so only qualified names, snake or screaming case, types,
/// and calls count for distance checks.
pub(super) fn code_shaped(span: &str, name: &str) -> bool {
    span.trim().ends_with("()")
        || name.contains(['_', '.', ':', '-', '>'])
        || name.chars().any(char::is_uppercase)
}

/// The first displayed line of `node` that contains `word` as a whole word,
/// in the numbers a block link uses. A run block has none: a link to it takes
/// no line range.
fn name_line(node: &CompiledNode, word: &str) -> Option<u32> {
    match &node.content {
        CompiledNodeContent::Code {
            content,
            first_line,
            ..
        } => content
            .lines()
            .position(|line| identifier_words(line).any(|found| found == word))
            .map(|index| first_line.unwrap_or(1) + index as u32),
        CompiledNodeContent::Diff { diff, .. } => diff
            .files
            .iter()
            .flat_map(|file| &file.hunks)
            .flat_map(|hunk| &hunk.lines)
            .filter(|line| identifier_words(&line.content).any(|found| found == word))
            .find_map(|line| line.new_line),
        _ => None,
    }
}
