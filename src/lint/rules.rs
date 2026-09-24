use std::collections::BTreeSet;
use std::path::Path;

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
            }
            _ => unreachable!("compiled nodes retain the source block order and kind"),
        }
    }
    rules.question_ratio();
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
        if questions as f64 / (total as f64) < self.config.min_question_ratio {
            self.add(
                None,
                "lint.lesson.few_questions",
                "/blocks",
                format!("{questions} of {total} blocks are multiple-choice questions"),
                "If questions would serve the lesson's teaching goal, consider adding more.",
                None,
            );
        }
    }
}

fn severity_for_code(code: &str) -> Severity {
    match code {
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
        | "lint.lesson.few_questions" => Severity::Info,
        _ => unreachable!("every lint rule has an intrinsic severity"),
    }
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
        fs::remove_dir_all(root).unwrap();
    }
}
