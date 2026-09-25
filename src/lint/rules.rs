use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use crate::artifact::{CompiledLesson, CompiledNodeContent};
use crate::language::Language;
use crate::source::{Block, CodeSource, DiffSource, LessonSource, MarkdownSource, SchemaVersion};

use super::{LintConfig, LintDiagnostic, Severity, SourceLocation, SpanIndex};

pub(super) fn collect(
    source: &LessonSource,
    artifact: &CompiledLesson,
    spans: &SpanIndex,
    root: &Path,
    config: &LintConfig,
) -> Vec<LintDiagnostic> {
    let mut rules = Rules {
        source,
        artifact,
        spans,
        root,
        config,
        findings: Vec::new(),
    };
    for (index, (block, node)) in source
        .blocks
        .iter()
        .zip(&artifact.presentation.nodes)
        .enumerate()
    {
        match (block, &node.content) {
            (Block::Markdown(block), CompiledNodeContent::Markdown { .. }) => {
                if let MarkdownSource::Inline { content } = &block.source {
                    rules.inline_size(
                        index,
                        content,
                        config.max_inline_prose_chars,
                        "prose",
                        "lint.inline.prose.too_large",
                    );
                }
            }
            (
                Block::Code(block),
                CompiledNodeContent::Code {
                    content,
                    language,
                    highlights,
                    ..
                },
            ) => {
                if let CodeSource::Inline { content } = &block.source {
                    rules.inline_size(
                        index,
                        content,
                        config.max_inline_code_diff_chars,
                        "code",
                        "lint.inline.code_diff.too_large",
                    );
                }
                rules.code(index, block, content, *language, highlights);
            }
            (Block::Diff(block), CompiledNodeContent::Diff { diff, .. }) => {
                if let DiffSource::Inline { content } = &block.source {
                    rules.inline_size(
                        index,
                        content,
                        config.max_inline_code_diff_chars,
                        "diff",
                        "lint.inline.code_diff.too_large",
                    );
                }
                for file in &diff.files {
                    if file.is_new {
                        let path = file.display_path().unwrap_or("unnamed file");
                        let pointer = format!("/blocks/{index}/source");
                        rules.add(
                            Some(index),
                            "lint.diff.new_file",
                            &pointer,
                            format!("new file {path:?} is presented as a diff"),
                            "Explain the new file in Markdown and show a relevant sourced code range, or use a highlighted code block with a meaningful explanation.",
                            None,
                        );
                    }
                }
            }
            (Block::MultipleChoice(block), CompiledNodeContent::MultipleChoice { .. }) => {
                if let MarkdownSource::Inline { content } = &block.prompt {
                    let pointer = if matches!(
                        source.schema_version,
                        SchemaVersion::V1_0_0
                            | SchemaVersion::V1_1_0
                            | SchemaVersion::V1_2_0
                            | SchemaVersion::V1_3_0
                    ) {
                        format!("/blocks/{index}/prompt")
                    } else {
                        format!("/blocks/{index}/prompt/content")
                    };
                    rules.inline_size_at(
                        index,
                        content,
                        config.max_inline_prose_chars,
                        "quiz prompt",
                        &pointer,
                        "lint.inline.prose.too_large",
                    );
                }
                rules.choice_lengths(index, block);
                rules.missing_hints(index, block);
                rules.unexplained_distractors(index, block);
            }
            _ => unreachable!("compiled nodes retain the source block order and kind"),
        }
    }
    rules.question_ratio();
    rules.unshown_code_references();
    rules
        .findings
        .retain(|finding| !config.ignore_codes.contains(&finding.code));
    rules.findings
}

struct Rules<'a> {
    source: &'a LessonSource,
    artifact: &'a CompiledLesson,
    spans: &'a SpanIndex,
    root: &'a Path,
    config: &'a LintConfig,
    findings: Vec<LintDiagnostic>,
}

impl Rules<'_> {
    fn add(
        &mut self,
        index: Option<usize>,
        code: &'static str,
        pointer: &str,
        message: impl Into<String>,
        suggestion: impl Into<String>,
        location: Option<SourceLocation>,
    ) -> &mut LintDiagnostic {
        let block_id = index.map(|index| self.source.blocks[index].id().as_str());
        self.findings.push(LintDiagnostic::new(
            code,
            severity_for_code(code),
            message,
            location.unwrap_or_else(|| self.spans.location(pointer)),
            block_id,
            pointer,
            suggestion,
        ));
        self.findings.last_mut().expect("finding was just pushed")
    }

    fn inline_size(
        &mut self,
        index: usize,
        content: &str,
        max: usize,
        kind: &str,
        code: &'static str,
    ) {
        self.inline_size_at(
            index,
            content,
            max,
            kind,
            &format!("/blocks/{index}/source/content"),
            code,
        );
    }

    fn inline_size_at(
        &mut self,
        index: usize,
        content: &str,
        max: usize,
        kind: &str,
        pointer: &str,
        code: &'static str,
    ) {
        let count = content.chars().count();
        if count > max {
            self.add(
                Some(index),
                code,
                pointer,
                format!("inline {kind} has {count} characters, exceeding the limit of {max}"),
                "Put this content in a file source; a temporary file is fine.",
                None,
            );
        }
    }

    fn code(
        &mut self,
        index: usize,
        block: &crate::source::CodeBlock,
        content: &str,
        language: Language,
        highlights: &[crate::artifact::CompiledCodeHighlight],
    ) {
        let base = format!("/blocks/{index}");
        let language_pointer = if block.language.is_some() {
            format!("{base}/language")
        } else {
            format!("{base}/source/path")
        };
        if language == Language::Markdown {
            self.add(
                Some(index),
                "lint.code.markdown_language",
                &language_pointer,
                "Markdown is presented as literal code",
                "Use a Markdown block for rendered prose. Keep a code block only when the literal Markdown syntax is the subject.",
                None,
            );
        } else if language == Language::Text {
            self.add(
                Some(index),
                "lint.code.plain_text",
                &language_pointer,
                "code block is presented as plain text",
                "If this is prose, use a Markdown block; if it is code, specify a supported language. Keep intentional plain text when appropriate.",
                None,
            );
        }

        let lines = content.lines().count();
        if lines > self.config.max_code_lines {
            let pointer = match block.source {
                CodeSource::Inline { .. } => format!("{base}/source/content"),
                _ => format!("{base}/source/lines"),
            };
            self.add(
                Some(index),
                "lint.code.too_many_lines",
                &pointer,
                format!(
                    "code block displays {lines} lines, exceeding the limit of {}",
                    self.config.max_code_lines
                ),
                "Split the excerpt into focused code blocks, with explanations where useful.",
                None,
            );
        }

        let highlight_capable =
            !matches!(block.source, CodeSource::Inline { .. }) && language != Language::Mermaid;
        if highlight_capable
            && highlights.is_empty()
            && lines >= self.config.suggest_highlights_min_lines
        {
            self.add(
                Some(index),
                "lint.code.no_highlights",
                &format!("{base}/source/lines"),
                format!("{lines}-line code fragment has no highlights"),
                "Consider highlighting the important lines if this excerpt has a focal point.",
                None,
            );
        }

        let highlighted: BTreeSet<u32> = highlights
            .iter()
            .flat_map(|group| &group.lines)
            .flat_map(|range| range.start..=range.end)
            .collect();
        if !highlighted.is_empty()
            && lines > 0
            && highlighted.len() >= self.config.highlight_coverage_min_lines
            && highlighted.len() as f64 / lines as f64 >= self.config.highlight_coverage_ratio
        {
            self.add(
                Some(index),
                "lint.code.highlight_coverage",
                &format!("{base}/highlights"),
                format!(
                    "highlights cover {} of {lines} displayed lines",
                    highlighted.len()
                ),
                "Split into focused code blocks when broad highlighting obscures the point.",
                None,
            );
        }
        let range_count: usize = highlights.iter().map(|group| group.lines.len()).sum();
        if range_count >= self.config.many_highlight_ranges {
            self.add(
                Some(index),
                "lint.code.many_highlight_ranges",
                &format!("{base}/highlights"),
                format!("code block has {range_count} highlighted line ranges"),
                "Consider separating the excerpt into focused code blocks.",
                None,
            );
        }

        self.filename_gap(index, block);
        if language == Language::Mermaid {
            self.mermaid_style(index, &block.source, content);
        }
    }

    fn filename_gap(&mut self, index: usize, block: &crate::source::CodeBlock) {
        let path = match &block.source {
            CodeSource::File { path, .. } | CodeSource::GitBlob { path, .. } => path.as_str(),
            CodeSource::Inline { .. } => return,
        };
        let Some(filename) = Path::new(path).file_name().and_then(|name| name.to_str()) else {
            return;
        };
        let nearest = self
            .artifact
            .presentation
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(other_index, node)| match &node.content {
                CompiledNodeContent::Markdown { content, .. }
                    if mentions_filename(content, filename) =>
                {
                    Some(other_index)
                }
                _ => None,
            })
            .min_by_key(|other_index| index.abs_diff(*other_index));
        if let Some(other_index) = nearest {
            let gap = index.abs_diff(other_index).saturating_sub(1);
            if gap >= self.config.filename_reference_gap {
                let pointer = format!("/blocks/{index}/source/path");
                let related = self.spans.location(&format!("/blocks/{other_index}"));
                self.add(
                    Some(index),
                    "lint.code.filename_reference_far",
                    &pointer,
                    format!("the nearest Markdown mention of {filename:?} is {gap} blocks away"),
                    "Consider adding nearby context if the distant filename mention does not explain this excerpt.",
                    None,
                )
                .related
                .push(super::RelatedLintLocation {
                    message: "nearest literal filename mention".to_owned(),
                    location: related,
                });
            }
        }
    }

    fn mermaid_style(&mut self, index: usize, source: &CodeSource, content: &str) {
        if !matches!(
            mermaid_svg::parse(content),
            Ok(mermaid_svg::Diagram::Flowchart(_) | mermaid_svg::Diagram::Class(_))
        ) {
            return;
        }
        let pointer = format!("/blocks/{index}/source/content");
        let source_pointer = format!("/blocks/{index}/source");
        for keyword in mermaid_style_keywords(content) {
            let (primary_pointer, location) = match source {
                CodeSource::Inline { .. } => {
                    let start_char = content[..keyword.byte_offset].chars().count();
                    (
                        pointer.as_str(),
                        self.spans.string_range(
                            &pointer,
                            start_char,
                            start_char + keyword.text.len(),
                        ),
                    )
                }
                CodeSource::File { path, lines } => (
                    source_pointer.as_str(),
                    SourceLocation::file_line(
                        self.root.join(path.as_str()),
                        lines.map_or(1, |range| range.start) as usize + keyword.line_index,
                        keyword.column,
                        keyword.column + keyword.text.len(),
                    ),
                ),
                CodeSource::GitBlob { .. } => (
                    source_pointer.as_str(),
                    self.spans.location(&source_pointer),
                ),
            };
            self.add(
                Some(index),
                "lint.mermaid.style_or_subgraph",
                primary_pointer,
                format!("diagram uses `{}`", keyword.text),
                "If this expresses a semantic distinction, consider reusable `classDef` and `class` assignments; keep `subgraph` when actual grouping is intended.",
                Some(location),
            );
        }
    }

    /// A wrong attempt reveals nothing, so without hints a stuck learner can
    /// only retry or reveal. Two-choice questions are skipped: after one wrong
    /// attempt a single option remains and a hint cannot help.
    fn missing_hints(&mut self, index: usize, block: &crate::source::MultipleChoiceBlock) {
        let choices = block.choices.len();
        if choices >= 3 && block.hints.is_empty() {
            self.add(
                Some(index),
                "lint.question.no_hints",
                &format!("/blocks/{index}"),
                format!(
                    "question has {choices} choices but no hints; after a wrong attempt the learner can only retry or reveal the answer"
                ),
                "Consider a hint that points toward the relevant code or reasoning without giving the answer away.",
                None,
            );
        }
    }

    /// Distractor explanations tell the learner why a tempting answer fails.
    /// Two-choice questions are skipped: the block explanation already covers
    /// the only distractor by contrast.
    fn unexplained_distractors(
        &mut self,
        index: usize,
        block: &crate::source::MultipleChoiceBlock,
    ) {
        if block.choices.len() < 3 {
            return;
        }
        let unexplained = block
            .choices
            .iter()
            .enumerate()
            .filter(|(_, choice)| !choice.correct && choice.explanation.is_none())
            .map(|(choice_index, _)| choice_index)
            .collect::<Vec<_>>();
        if unexplained.is_empty() {
            return;
        }
        let distractors = block.choices.len() - 1;
        let related = unexplained
            .iter()
            .map(|choice_index| super::RelatedLintLocation {
                message: "distractor without an explanation".to_owned(),
                location: self
                    .spans
                    .location(&format!("/blocks/{index}/choices/{choice_index}/content")),
            })
            .collect::<Vec<_>>();
        self.add(
            Some(index),
            "lint.question.unexplained_distractors",
            &format!("/blocks/{index}"),
            format!(
                "{} of {distractors} distractors have no explanation of why they are wrong",
                unexplained.len()
            ),
            "Consider adding an `explanation` to each distractor saying why it is wrong; it is shown after the question is answered or revealed.",
            None,
        )
        .related
        .extend(related);
    }

    fn choice_lengths(&mut self, index: usize, block: &crate::source::MultipleChoiceBlock) {
        let Some((shortest_index, shortest)) = block
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| (index, choice.content.chars().count()))
            .min_by_key(|(_, count)| *count)
        else {
            return;
        };
        let (longest_index, longest) = block
            .choices
            .iter()
            .enumerate()
            .map(|(index, choice)| (index, choice.content.chars().count()))
            .max_by_key(|(_, count)| *count)
            .expect("validated questions have choices");
        let gap = longest - shortest;
        if shortest > 0
            && gap >= self.config.min_choice_length_gap_chars
            && gap as f64 / shortest as f64 > self.config.max_choice_length_spread
        {
            let pointer = format!("/blocks/{index}/choices/{longest_index}/content");
            let related = self
                .spans
                .location(&format!("/blocks/{index}/choices/{shortest_index}/content"));
            self.add(
                Some(index),
                "lint.question.uneven_choice_lengths",
                &pointer,
                format!("answer choices range from {shortest} to {longest} characters"),
                "Make choices comparable in length without sacrificing clear answers; put fuller teaching detail in the explanation.",
                None,
            )
            .related
            .push(super::RelatedLintLocation {
                message: "shortest choice".to_owned(),
                location: related,
            });
        }
    }

    fn question_ratio(&mut self) {
        let total = self.source.blocks.len();
        if total == 0 {
            return;
        }
        let questions = self
            .source
            .blocks
            .iter()
            .filter(|block| matches!(block, Block::MultipleChoice(_)))
            .count();
        let ratio = questions as f64 / total as f64;
        if ratio < self.config.min_question_ratio {
            self.add(
                None,
                "lint.lesson.few_questions",
                "/blocks",
                format!(
                    "{questions} of {total} blocks are multiple-choice questions ({}); lint reports lessons under {} (min_question_ratio)",
                    percent(ratio),
                    percent(self.config.min_question_ratio),
                ),
                "If questions would serve the lesson's teaching goal, consider adding more.",
                None,
            );
        }
    }
}

/// Where a scanned piece of Markdown lives, so findings can point at it.
enum TextPlace {
    /// A decoded JSON string in `lesson.json`, addressed by pointer.
    Json(String),
    /// A whole Markdown file, addressed by root-joined path.
    File(PathBuf),
}

impl Rules<'_> {
    /// Report Markdown inline code that names something no code or diff block
    /// shows. This is a guess, hence `info`: the name may be a standard type or
    /// a concept. Choices and explanations are skipped because distractors
    /// deliberately name things that do not exist.
    fn unshown_code_references(&mut self) {
        let mut shown = HashSet::new();
        for node in &self.artifact.presentation.nodes {
            match &node.content {
                CompiledNodeContent::Code { content, .. } => {
                    shown.extend(identifier_words(content))
                }
                CompiledNodeContent::Diff { diff, .. } => {
                    for line in diff
                        .files
                        .iter()
                        .flat_map(|file| &file.hunks)
                        .flat_map(|hunk| &hunk.lines)
                    {
                        shown.extend(identifier_words(&line.content));
                    }
                }
                _ => {}
            }
        }

        // Choices and explanations are not scanned on their own, but when a
        // flagged name also appears there it should be fixed in the same pass.
        let mut quiz_texts = Vec::new();
        for (index, block) in self.source.blocks.iter().enumerate() {
            if let Block::MultipleChoice(block) = block {
                for (choice, value) in block.choices.iter().enumerate() {
                    quiz_texts.push((
                        format!("/blocks/{index}/choices/{choice}/content"),
                        "choice",
                        &value.content,
                    ));
                    if let Some(explanation) = &value.explanation {
                        quiz_texts.push((
                            format!("/blocks/{index}/choices/{choice}/explanation"),
                            "choice explanation",
                            explanation,
                        ));
                    }
                }
                quiz_texts.push((
                    format!("/blocks/{index}/explanation"),
                    "explanation",
                    &block.explanation,
                ));
            }
        }

        let texts = self.scanned_texts();
        for (index, text, place) in texts {
            let mut reported = HashSet::new();
            for span in inline_code_spans(&text) {
                let Some(name) = code_reference_name(span.content) else {
                    continue;
                };
                if identifier_segments(name).all(|segment| shown.contains(segment))
                    || !reported.insert(name.to_owned())
                {
                    continue;
                }
                let (pointer, location) = match &place {
                    TextPlace::Json(pointer) => (
                        pointer.clone(),
                        self.spans.string_range(
                            pointer,
                            span.start_char,
                            span.start_char + span.content.chars().count(),
                        ),
                    ),
                    TextPlace::File(path) => (
                        format!("/blocks/{index}"),
                        SourceLocation::file_line(
                            path.clone(),
                            span.line,
                            span.column,
                            span.column + span.content.chars().count(),
                        ),
                    ),
                };
                let related = quiz_texts
                    .iter()
                    .flat_map(|(quiz_pointer, kind, quiz_text)| {
                        inline_code_spans(quiz_text)
                            .into_iter()
                            .filter(|quiz_span| {
                                code_reference_name(quiz_span.content) == Some(name)
                            })
                            .map(|quiz_span| super::RelatedLintLocation {
                                message: format!("also used in a quiz {kind}"),
                                location: self.spans.string_range(
                                    quiz_pointer,
                                    quiz_span.start_char,
                                    quiz_span.start_char + quiz_span.content.chars().count(),
                                ),
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect::<Vec<_>>();
                self.add(
                    Some(index),
                    "lint.markdown.unshown_code_reference",
                    &pointer,
                    format!("`{name}` is formatted as code but appears in no code or diff block"),
                    "If the learner needs to see it, show the relevant code; otherwise check the name, or drop the code formatting if it names a concept.",
                    Some(location),
                )
                .related
                .extend(related);
            }
        }
    }

    /// Markdown the learner reads alongside code: Markdown blocks, captions,
    /// highlight annotations, quiz prompts, and hints.
    fn scanned_texts(&self) -> Vec<(usize, String, TextPlace)> {
        let legacy_prompt = matches!(
            self.source.schema_version,
            SchemaVersion::V1_0_0
                | SchemaVersion::V1_1_0
                | SchemaVersion::V1_2_0
                | SchemaVersion::V1_3_0
        );
        let mut texts = Vec::new();
        for (index, (block, node)) in self
            .source
            .blocks
            .iter()
            .zip(&self.artifact.presentation.nodes)
            .enumerate()
        {
            let base = format!("/blocks/{index}");
            let markdown_place = |source: &MarkdownSource, pointer: String| match source {
                MarkdownSource::Inline { .. } => TextPlace::Json(pointer),
                MarkdownSource::File { path } => TextPlace::File(self.root.join(path.as_str())),
            };
            match (block, &node.content) {
                (Block::Markdown(block), CompiledNodeContent::Markdown { content, .. }) => {
                    texts.push((
                        index,
                        content.clone(),
                        markdown_place(&block.source, format!("{base}/source/content")),
                    ));
                }
                (Block::Code(block), _) => {
                    if let Some(caption) = &block.caption {
                        texts.push((
                            index,
                            caption.clone(),
                            TextPlace::Json(format!("{base}/caption")),
                        ));
                    }
                    for (group, highlight) in block.highlights.iter().enumerate() {
                        if let Some(annotation) = &highlight.annotation {
                            texts.push((
                                index,
                                annotation.clone(),
                                TextPlace::Json(format!("{base}/highlights/{group}/annotation")),
                            ));
                        }
                    }
                }
                (Block::Diff(block), _) => {
                    if let Some(caption) = &block.caption {
                        texts.push((
                            index,
                            caption.clone(),
                            TextPlace::Json(format!("{base}/caption")),
                        ));
                    }
                }
                (
                    Block::MultipleChoice(block),
                    CompiledNodeContent::MultipleChoice { prompt, .. },
                ) => {
                    let pointer = if legacy_prompt {
                        format!("{base}/prompt")
                    } else {
                        format!("{base}/prompt/content")
                    };
                    texts.push((
                        index,
                        prompt.clone(),
                        markdown_place(&block.prompt, pointer),
                    ));
                    for (hint_index, hint) in block.hints.iter().enumerate() {
                        texts.push((
                            index,
                            hint.clone(),
                            TextPlace::Json(format!("{base}/hints/{hint_index}")),
                        ));
                    }
                }
                _ => {}
            }
        }
        texts
    }
}

/// Every identifier-like word (`[A-Za-z0-9_]+`) in displayed code.
fn identifier_words(content: &str) -> impl Iterator<Item = String> + '_ {
    content
        .split(|character: char| !(character.is_alphanumeric() || character == '_'))
        .filter(|word| !word.is_empty())
        .map(str::to_owned)
}

/// Segments of a qualified name such as `Queue::push`, `self.head`, or
/// `node->next`.
fn identifier_segments(name: &str) -> impl Iterator<Item = &str> {
    name.split("::")
        .flat_map(|part| part.split("->"))
        .flat_map(|part| part.split('.'))
}

/// The name an inline code span refers to, if it looks like a code
/// identifier. Commands, expressions, literals, paths, and filenames return
/// `None`.
fn code_reference_name(span: &str) -> Option<&str> {
    let name = span.trim().strip_suffix("()").unwrap_or(span.trim());
    let is_identifier = |segment: &str| {
        let mut characters = segment.chars();
        characters
            .next()
            .is_some_and(|first| first.is_alphabetic() || first == '_')
            && characters.all(|character| character.is_alphanumeric() || character == '_')
    };
    if !identifier_segments(name).all(is_identifier) {
        return None;
    }
    if matches!(
        name,
        "true" | "false" | "null" | "nil" | "None" | "undefined"
    ) {
        return None;
    }
    // `queue.rs` and `lesson.json` are filenames, not member access.
    if let Some((_, extension)) = name.rsplit_once('.') {
        let extension = extension.to_ascii_lowercase();
        if Language::from_authored(&extension) != Language::Text
            || matches!(
                extension.as_str(),
                "txt"
                    | "text"
                    | "lock"
                    | "log"
                    | "csv"
                    | "ini"
                    | "cfg"
                    | "conf"
                    | "env"
                    | "learn"
                    | "patch"
                    | "diff"
            )
        {
            return None;
        }
    }
    Some(name)
}

struct InlineCodeSpan<'a> {
    content: &'a str,
    /// Unicode-scalar offset of `content` in the scanned text.
    start_char: usize,
    /// One-based line and column of `content` in the scanned text.
    line: usize,
    column: usize,
}

/// Backtick code spans outside fenced code blocks. Spans are matched within a
/// line, which covers how agents write inline code.
fn inline_code_spans(text: &str) -> Vec<InlineCodeSpan<'_>> {
    let mut spans = Vec::new();
    let mut fence: Option<&str> = None;
    let mut line_start = 0;
    for (line_index, line) in text.split_inclusive('\n').enumerate() {
        let trimmed = line.trim_start();
        let marker = ["```", "~~~"]
            .into_iter()
            .find(|marker| trimmed.starts_with(marker));
        match (fence, marker) {
            (None, Some(marker)) => fence = Some(marker),
            (Some(open), Some(marker)) if open == marker => fence = None,
            (None, None) => {
                let bytes = line.as_bytes();
                let mut cursor = 0;
                while let Some(offset) = line[cursor..].find('`') {
                    let open = cursor + offset;
                    let run = bytes[open..]
                        .iter()
                        .take_while(|byte| **byte == b'`')
                        .count();
                    let content_start = open + run;
                    let delimiter = &line[open..content_start];
                    let Some(close) = find_closing_run(&line[content_start..], delimiter) else {
                        break;
                    };
                    let content_end = content_start + close;
                    let content = &line[content_start..content_end];
                    if !content.trim().is_empty() {
                        // Only the name matters; drop padding around it.
                        let leading = content.len() - content.trim_start().len();
                        let start = content_start + leading;
                        let content = content.trim();
                        spans.push(InlineCodeSpan {
                            content,
                            start_char: text[..line_start + start].chars().count(),
                            line: line_index + 1,
                            column: line[..start].chars().count() + 1,
                        });
                    }
                    cursor = content_end + run;
                }
            }
            (Some(_), _) => {}
        }
        line_start += line.len();
    }
    spans
}

/// Offset of the next backtick run exactly as long as `delimiter`.
fn find_closing_run(rest: &str, delimiter: &str) -> Option<usize> {
    let bytes = rest.as_bytes();
    let mut cursor = 0;
    while let Some(offset) = rest[cursor..].find(delimiter) {
        let start = cursor + offset;
        let run = bytes[start..]
            .iter()
            .take_while(|byte| **byte == b'`')
            .count();
        if run == delimiter.len() {
            return Some(start);
        }
        cursor = start + run;
    }
    None
}

/// Format a ratio as a percentage with at most one decimal (`0.2` -> `20%`,
/// `1.0 / 6.0` -> `16.7%`).
fn percent(ratio: f64) -> String {
    let formatted = format!("{:.1}", ratio * 100.0);
    format!("{}%", formatted.strip_suffix(".0").unwrap_or(&formatted))
}

fn severity_for_code(code: &str) -> Severity {
    known_severity(code).expect("every lint rule has an intrinsic severity")
}

/// The intrinsic severity of a lint code, or `None` for an unknown code.
pub(super) fn known_severity(code: &str) -> Option<Severity> {
    Some(match code {
        "lint.inline.prose.too_large"
        | "lint.inline.code_diff.too_large"
        | "lint.diff.new_file" => Severity::Error,
        "lint.code.markdown_language" => Severity::Critical,
        "lint.code.plain_text"
        | "lint.code.too_many_lines"
        | "lint.code.highlight_coverage"
        | "lint.question.uneven_choice_lengths"
        | "lint.mermaid.style_or_subgraph" => Severity::Warning,
        "lint.code.no_highlights"
        | "lint.code.many_highlight_ranges"
        | "lint.code.filename_reference_far"
        | "lint.lesson.few_questions"
        | "lint.question.no_hints"
        | "lint.question.unexplained_distractors"
        | "lint.markdown.unshown_code_reference" => Severity::Info,
        _ => return None,
    })
}

/// Whether `content` names `filename` as a whole name, so `data.rs` does not
/// count as a mention of `a.rs`. Path separators and punctuation may surround
/// the name.
fn mentions_filename(content: &str, filename: &str) -> bool {
    let is_name_char = |character: char| character.is_alphanumeric() || "_-.".contains(character);
    content.match_indices(filename).any(|(start, _)| {
        let boundary_before = !content[..start]
            .chars()
            .next_back()
            .is_some_and(is_name_char);
        let mut after = content[start + filename.len()..].chars();
        let boundary_after = match after.next() {
            None => true,
            // A sentence-ending period still ends the name; `a.rs.bak` does not.
            Some('.') => !after.next().is_some_and(is_name_char),
            Some(character) => !is_name_char(character),
        };
        boundary_before && boundary_after
    })
}

/// A `subgraph` or `style` keyword that begins a Mermaid statement.
struct MermaidKeyword {
    text: &'static str,
    byte_offset: usize,
    line_index: usize,
    /// One-based Unicode-scalar column.
    column: usize,
}

/// Find `subgraph`/`style` as the first word of a statement. Statements end at
/// a newline or an unquoted `;`; quoted text, bracketed labels and class
/// bodies, and `%%` comments are skipped.
fn mermaid_style_keywords(content: &str) -> Vec<MermaidKeyword> {
    let mut found = Vec::new();
    let mut line_start = 0;
    let mut depth = 0usize;
    for (line_index, line) in content.split_inclusive('\n').enumerate() {
        let mut in_quote = false;
        let mut statement_start = depth == 0;
        for (offset, character) in line.char_indices() {
            if in_quote {
                in_quote = character != '"';
                continue;
            }
            if statement_start && matches!(character, ' ' | '\t') {
                continue;
            }
            if statement_start {
                statement_start = false;
                let rest = &line[offset..];
                if rest.starts_with("%%") {
                    break;
                }
                if let Some(text) = ["subgraph", "style"].into_iter().find(|keyword| {
                    rest.starts_with(keyword)
                        && rest[keyword.len()..]
                            .chars()
                            .next()
                            .is_none_or(|next| next.is_whitespace() || next == ';')
                }) {
                    found.push(MermaidKeyword {
                        text,
                        byte_offset: line_start + offset,
                        line_index,
                        column: line[..offset].chars().count() + 1,
                    });
                }
            }
            match character {
                '"' => in_quote = true,
                '[' | '(' | '{' => depth += 1,
                ']' | ')' | '}' => depth = depth.saturating_sub(1),
                ';' if depth == 0 => statement_start = true,
                _ => {}
            }
        }
        line_start += line.len();
    }
    found
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::{Value, json};

    use super::*;
    use crate::compiler::{CompileOptions, compile};

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    fn temp_root() -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!(
            "agent-teacher-lint-{}-{}",
            std::process::id(),
            NEXT_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).unwrap();
        path
    }

    fn findings(value: Value, root: &Path, config: &LintConfig) -> Vec<LintDiagnostic> {
        let input = serde_json::to_string_pretty(&value).unwrap();
        let artifact = compile(&input, &CompileOptions::new(root)).unwrap();
        let validated = crate::source::parse_and_validate(&input).unwrap();
        let spans = SpanIndex::new(&input, root.join("lesson.json")).unwrap();
        collect(validated.source(), &artifact, &spans, root, config)
    }

    #[test]
    fn counts_unicode_inline_content_and_full_choice_markdown() {
        let root = temp_root();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Inline sizes", "blocks":[
                {"type":"markdown","id":"prose","source":{"kind":"inline","content":"éééé"}},
                {"type":"code","id":"code","language":"rust","source":{"kind":"inline","content":"fn main() {}"}},
                {"type":"diff","id":"patch","source":{"kind":"inline","content":"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n"}},
                {"type":"multiple_choice","id":"quiz","prompt":{"kind":"inline","content":"What?"},
                 "choices":[{"content":"a","correct":true},{"content":"**a much longer answer**"}],"explanation":"Because."}
            ]
        });
        let config = LintConfig {
            max_inline_prose_chars: 3,
            max_inline_code_diff_chars: 10,
            ..LintConfig::default()
        };
        let found = findings(lesson, &root, &config);
        assert_eq!(
            found
                .iter()
                .filter(|f| f.code == "lint.inline.prose.too_large")
                .count(),
            2
        );
        assert_eq!(
            found
                .iter()
                .filter(|f| f.code == "lint.inline.code_diff.too_large")
                .count(),
            2
        );
        assert!(
            found
                .iter()
                .any(|f| f.code == "lint.question.uneven_choice_lengths")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn uses_distinct_highlight_lines_and_source_selectors() {
        let root = temp_root();
        fs::write(
            root.join("sample.rs"),
            (1..=10)
                .map(|line| format!("let n{line} = {line};\n"))
                .collect::<String>(),
        )
        .unwrap();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Highlights", "blocks":[
                {"type":"markdown","id":"mention","source":{"kind":"inline","content":"Read sample.rs."}},
                {"type":"markdown","id":"one","source":{"kind":"inline","content":"One."}},
                {"type":"markdown","id":"two","source":{"kind":"inline","content":"Two."}},
                {"type":"markdown","id":"three","source":{"kind":"inline","content":"Three."}},
                {"type":"code","id":"focused","language":"rust","source":{"kind":"file","path":"sample.rs"},
                 "highlights":[{"lines":[{"start":1,"end":3},{"start":5,"end":6}]},
                               {"lines":[{"start":8,"end":10}],"color":"blue"}]},
                {"type":"code","id":"bare","language":"rust","source":{"kind":"file","path":"sample.rs"}}
            ]
        });
        let config = LintConfig {
            max_code_lines: 6,
            suggest_highlights_min_lines: 10,
            highlight_coverage_min_lines: 8,
            highlight_coverage_ratio: 0.8,
            ..LintConfig::default()
        };
        let found = findings(lesson, &root, &config);
        let codes = found.iter().map(|f| f.code.as_str()).collect::<Vec<_>>();
        assert_eq!(
            codes
                .iter()
                .filter(|code| **code == "lint.code.too_many_lines")
                .count(),
            2
        );
        assert!(codes.contains(&"lint.code.highlight_coverage"));
        assert!(codes.contains(&"lint.code.many_highlight_ranges"));
        assert!(codes.contains(&"lint.code.no_highlights"));
        let far = found
            .iter()
            .find(|f| f.code == "lint.code.filename_reference_far")
            .unwrap();
        assert_eq!(far.block_id.as_deref(), Some("focused"));
        assert_eq!(far.related.len(), 1);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn identifies_new_files_but_not_additions_to_existing_files() {
        let root = temp_root();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Diffs", "blocks":[
                {"type":"diff","id":"new","source":{"kind":"inline","content":"diff --git a/new.rs b/new.rs\nnew file mode 100644\n--- /dev/null\n+++ b/new.rs\n@@ -0,0 +1 @@\n+fn main() {}\n"}},
                {"type":"diff","id":"existing","source":{"kind":"inline","content":"diff --git a/old.rs b/old.rs\n--- a/old.rs\n+++ b/old.rs\n@@ -1 +1,2 @@\n old\n+new\n"}}
            ]
        });
        let found = findings(lesson, &root, &LintConfig::default());
        let new_files = found
            .iter()
            .filter(|f| f.code == "lint.diff.new_file")
            .collect::<Vec<_>>();
        assert_eq!(new_files.len(), 1);
        assert_eq!(new_files[0].block_id.as_deref(), Some("new"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_flowchart_constructs_at_editable_file_lines() {
        let root = temp_root();
        fs::write(
            root.join("diagram.mmd"),
            "ignored\nflowchart LR\n  subgraph Group\n    A --> B\n  end\n  style A fill:#f00\n",
        )
        .unwrap();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Diagram", "blocks":[
                {"type":"code","id":"flow","source":{"kind":"file","path":"diagram.mmd","lines":{"start":2,"end":6}}},
                {"type":"code","id":"sequence","language":"mermaid","source":{"kind":"inline","content":"sequenceDiagram\n  A->>B: Message"}}
            ]
        });
        let found = findings(lesson, &root, &LintConfig::default());
        let style = found
            .iter()
            .filter(|f| f.code == "lint.mermaid.style_or_subgraph")
            .collect::<Vec<_>>();
        assert_eq!(style.len(), 2);
        assert_eq!(style[0].location.start.line, 3);
        assert_eq!(style[0].location.start.column, 3);
        assert_eq!(style[1].location.start.line, 6);
        assert_eq!(
            style[0].location.path,
            root.join("diagram.mmd").to_string_lossy()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn filename_mentions_match_whole_names_only() {
        assert!(mentions_filename("See `src/a.rs`.", "a.rs"));
        assert!(mentions_filename("Open a.rs.", "a.rs"));
        assert!(mentions_filename("a.rs: the entry point", "a.rs"));
        assert!(!mentions_filename("See data.rs for details", "a.rs"));
        assert!(!mentions_filename("See a.rs.bak instead", "a.rs"));
        assert!(!mentions_filename("my_a.rs", "a.rs"));
    }

    #[test]
    fn finds_style_keywords_as_first_word_of_each_statement() {
        let content = "flowchart LR\n  A-->C; style C fill:#0f0\n  B[\"label; style x\"]\n  %% style in a comment\n  stylish --> D\n  subgraph;\n";
        let found = mermaid_style_keywords(content)
            .into_iter()
            .map(|keyword| (keyword.text, keyword.line_index, keyword.column))
            .collect::<Vec<_>>();
        assert_eq!(found, [("style", 1, 10), ("subgraph", 5, 3)]);
    }

    #[test]
    fn skips_class_members_named_style() {
        let content = "classDiagram\n  class Theme {\n    style\n  }\n  style Theme fill:#f9f\n";
        let found = mermaid_style_keywords(content)
            .into_iter()
            .map(|keyword| keyword.line_index)
            .collect::<Vec<_>>();
        assert_eq!(found, [4]);
    }

    #[test]
    fn reports_style_in_inline_class_diagram() {
        let root = temp_root();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Classes", "blocks":[
                {"type":"code","id":"classes","language":"mermaid","source":{"kind":"inline","content":"classDiagram\n  class A\n  class B\n  A <|-- B; style B fill:#f9f"}}
            ]
        });
        let found = findings(lesson, &root, &LintConfig::default());
        let style = found
            .iter()
            .filter(|f| f.code == "lint.mermaid.style_or_subgraph")
            .collect::<Vec<_>>();
        assert_eq!(style.len(), 1);
        assert_eq!(style[0].block_id.as_deref(), Some("classes"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn flags_markdown_and_text_code_and_one_lesson_ratio() {
        let root = temp_root();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Kinds", "blocks":[
                {"type":"code","id":"md","language":"markdown","source":{"kind":"inline","content":"# Heading"}},
                {"type":"code","id":"text","source":{"kind":"inline","content":"literal"}},
                {"type":"markdown","id":"prose","source":{"kind":"inline","content":"Description."}}
            ]
        });
        let found = findings(lesson, &root, &LintConfig::default());
        assert!(
            found
                .iter()
                .any(|f| f.code == "lint.code.markdown_language")
        );
        assert!(found.iter().any(|f| f.code == "lint.code.plain_text"));
        let ratio = found
            .iter()
            .find(|f| f.code == "lint.lesson.few_questions")
            .unwrap();
        assert_eq!(ratio.block_id, None);
        assert_eq!(ratio.pointer, "/blocks");
        assert_eq!(
            ratio.message,
            "0 of 3 blocks are multiple-choice questions (0%); lint reports lessons under 20% (min_question_ratio)"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn inline_code_spans_skip_fences_and_trim_padding() {
        let text = "Use `head` and `` a`b ``.\n```rust\nlet `x` = 1;\n```\nThen ` tail `.";
        let spans = inline_code_spans(text)
            .into_iter()
            .map(|span| (span.content, span.line, span.column, span.start_char))
            .collect::<Vec<_>>();
        assert_eq!(
            spans,
            [("head", 1, 6, 5), ("a`b", 1, 19, 18), ("tail", 5, 8, 58)]
        );
    }

    #[test]
    fn code_reference_names_are_identifier_like() {
        for (span, expected) in [
            ("count", Some("count")),
            ("pop_front()", Some("pop_front")),
            ("Queue::push", Some("Queue::push")),
            ("self.head", Some("self.head")),
            ("node->next", Some("node->next")),
            ("learnc check", None),
            ("queue.rs", None),
            ("lesson.json", None),
            ("src/lib", None),
            ("42", None),
            ("a + b", None),
            ("true", None),
        ] {
            assert_eq!(code_reference_name(span), expected, "{span}");
        }
    }

    #[test]
    fn reports_names_missing_from_code_with_related_quiz_mentions() {
        let root = temp_root();
        let lesson = json!({
            "schema_version":"2.1.0", "title":"Queue", "blocks":[
                {"type":"markdown","id":"intro","source":{"kind":"inline","content":"We `push` onto `head_ptr`; see `queue.rs`."}},
                {"type":"code","id":"impl","language":"rust","source":{"kind":"inline","content":"fn push(queue: &mut Queue) {}"}},
                {"type":"multiple_choice","id":"quiz",
                 "prompt":{"kind":"inline","content":"What does `tail_ptr` track?"},
                 "choices":[
                     {"content":"The newest item","correct":true},
                     {"content":"The same thing as `head_ptr`"},
                     {"content":"A `ghost` pointer"}
                 ],
                 "explanation":"Unlike `head_ptr`, it tracks the newest item."}
            ]
        });
        let found = findings(lesson, &root, &LintConfig::default());
        let unshown = found
            .iter()
            .filter(|f| f.code == "lint.markdown.unshown_code_reference")
            .collect::<Vec<_>>();
        let names = unshown
            .iter()
            .map(|f| {
                (
                    f.block_id.as_deref().unwrap(),
                    f.message.split('`').nth(1).unwrap(),
                )
            })
            .collect::<Vec<_>>();
        // `push` is shown, `queue.rs` is a filename, and `ghost` appears only
        // in a choice.
        assert_eq!(names, [("intro", "head_ptr"), ("quiz", "tail_ptr")]);
        assert_eq!(unshown[0].severity, Severity::Info);
        let related = unshown[0]
            .related
            .iter()
            .map(|related| related.message.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            related,
            [
                "also used in a quiz choice",
                "also used in a quiz explanation"
            ]
        );

        let ignored = findings(
            json!({"schema_version":"2.1.0","title":"T","blocks":[
                {"type":"markdown","id":"m","source":{"kind":"inline","content":"`missing`"}}
            ]}),
            &root,
            &LintConfig {
                ignore_codes: vec!["lint.markdown.unshown_code_reference".to_owned()],
                ..LintConfig::default()
            },
        );
        assert!(
            ignored
                .iter()
                .all(|f| f.code != "lint.markdown.unshown_code_reference")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_questions_with_three_or_more_choices_and_no_hints() {
        let root = temp_root();
        let question = |id: &str, choices: &[&str], hints: &[&str]| {
            json!({"type":"multiple_choice","id":id,
                "prompt":{"kind":"inline","content":"Pick one."},
                "choices":choices.iter().enumerate()
                    .map(|(i, c)| json!({"content":c,"correct":i == 0})).collect::<Vec<_>>(),
                "hints":hints,
                "explanation":"Because."})
        };
        let lesson = json!({"schema_version":"2.1.0","title":"Hints","blocks":[
            question("two", &["Yes", "No"], &[]),
            question("three", &["Red", "Green", "Blue"], &[]),
            question("hinted", &["Red", "Green", "Blue"], &["Think about **light**."])
        ]});
        let found = findings(lesson, &root, &LintConfig::default());
        let flagged = found
            .iter()
            .filter(|f| f.code == "lint.question.no_hints")
            .collect::<Vec<_>>();
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].block_id.as_deref(), Some("three"));
        assert_eq!(flagged[0].severity, Severity::Info);
        assert_eq!(flagged[0].pointer, "/blocks/1");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reports_questions_with_unexplained_distractors() {
        let root = temp_root();
        let question = |id: &str, choices: Value| {
            json!({"type":"multiple_choice","id":id,
                "prompt":{"kind":"inline","content":"Pick one."},
                "choices":choices,"hints":["Think."],"explanation":"Because."})
        };
        let lesson = json!({"schema_version":"2.2.0","title":"Distractors","blocks":[
            question("two", json!([{"content":"Yes","correct":true},{"content":"No"}])),
            question("partial", json!([
                {"content":"Red","correct":true},
                {"content":"Green","explanation":"Green is `wavelength` 530."},
                {"content":"Blue"}
            ])),
            question("complete", json!([
                {"content":"Red","correct":true},
                {"content":"Green","explanation":"Too short."},
                {"content":"Blue","explanation":"Shorter still."}
            ]))
        ]});
        let found = findings(lesson, &root, &LintConfig::default());
        let flagged = found
            .iter()
            .filter(|f| f.code == "lint.question.unexplained_distractors")
            .collect::<Vec<_>>();
        assert_eq!(flagged.len(), 1);
        assert_eq!(flagged[0].block_id.as_deref(), Some("partial"));
        assert_eq!(
            flagged[0].message,
            "1 of 2 distractors have no explanation of why they are wrong"
        );
        assert_eq!(flagged[0].related.len(), 1);
        assert_eq!(flagged[0].severity, Severity::Info);
        // Choice explanations are not scanned for unshown code on their own.
        assert!(
            found
                .iter()
                .all(|f| f.code != "lint.markdown.unshown_code_reference")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn percent_keeps_at_most_one_decimal() {
        assert_eq!(percent(0.2), "20%");
        assert_eq!(percent(1.0 / 6.0), "16.7%");
        assert_eq!(percent(0.125), "12.5%");
        assert_eq!(percent(0.0), "0%");
    }
}
