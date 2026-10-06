use std::collections::{HashMap, HashSet};

use serde::{Deserialize, de::DeserializeOwned};

use crate::diagnostics::{Diagnostic, DiagnosticBag};

use crate::language::{Language, RUN_FILE_PLACEHOLDER};

use super::{
    Block, CodeHighlight, CodeSource, DiffSource, ExternalArtifactBlock, GitDiffTarget,
    GitRevision, LessonSource, LineRange, MarkdownSource, OutputSource, RepoPath, RunCodeBlock,
    SourceId, SymbolTable, external_artifact_file_error,
    model::{
        LessonSourceV1_0_0, LessonSourceV1_1_0, LessonSourceV1_2_0, LessonSourceV1_3_0,
        LessonSourceV2_0_0, LessonSourceV2_1_0, LessonSourceV2_2_0, LessonSourceV2_3_0,
        LessonSourceV2_4_0, LessonSourceV2_5_0,
    },
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidatedLesson {
    source: LessonSource,
    symbols: SymbolTable,
}

impl ValidatedLesson {
    pub fn source(&self) -> &LessonSource {
        &self.source
    }

    pub fn symbols(&self) -> &SymbolTable {
        &self.symbols
    }

    pub fn into_parts(self) -> (LessonSource, SymbolTable) {
        (self.source, self.symbols)
    }
}

/// Parses one JSON source document and runs all non-I/O semantic checks.
pub fn parse_and_validate(input: &str) -> Result<ValidatedLesson, Vec<Diagnostic>> {
    // Report malformed JSON directly; otherwise a syntax error would surface
    // as a shape error from whichever decoder the fallback selected.
    // Trailing data is left for the strict decoder, which reports it with its
    // own code.
    let mut version_deserializer = serde_json::Deserializer::from_str(input);
    let document = serde_json::Value::deserialize(&mut version_deserializer).map_err(|error| {
        vec![Diagnostic::error(
            "source.deserialize",
            "",
            format!(
                "invalid JSON at line {}, column {}: {error}",
                error.line(),
                error.column()
            ),
        )]
    })?;
    let version = document
        .get("schema_version")
        .and_then(|value| value.as_str());
    let source = match version {
        Some("1.0.0") => deserialize_source::<LessonSourceV1_0_0>(input).map(Into::into),
        Some("1.1.0") => deserialize_source::<LessonSourceV1_1_0>(input).map(Into::into),
        Some("1.2.0") => deserialize_source::<LessonSourceV1_2_0>(input).map(Into::into),
        Some("1.3.0") => deserialize_source::<LessonSourceV1_3_0>(input).map(Into::into),
        Some("2.0.0") => deserialize_source::<LessonSourceV2_0_0>(input).map(Into::into),
        Some("2.1.0") => deserialize_source::<LessonSourceV2_1_0>(input).map(Into::into),
        Some("2.2.0") => deserialize_source::<LessonSourceV2_2_0>(input).map(Into::into),
        Some("2.3.0") => deserialize_source::<LessonSourceV2_3_0>(input).map(Into::into),
        Some("2.4.0") => deserialize_source::<LessonSourceV2_4_0>(input).map(Into::into),
        Some("2.5.0") => deserialize_source::<LessonSourceV2_5_0>(input).map(Into::into),
        _ => deserialize_source::<LessonSource>(input),
    }?;
    validate(source)
}

fn deserialize_source<T: DeserializeOwned>(input: &str) -> Result<T, Vec<Diagnostic>> {
    let mut deserializer = serde_json::Deserializer::from_str(input);
    let source: T = match serde_path_to_error::deserialize(&mut deserializer) {
        Ok(source) => source,
        Err(error) => {
            let pointer = serde_path_to_json_pointer(&error.path().to_string());
            let inner = error.inner();
            let diagnostic = Diagnostic::error(
                "source.deserialize",
                pointer,
                format!(
                    "invalid lesson source at line {}, column {}: {}",
                    inner.line(),
                    inner.column(),
                    inner
                ),
            );
            return Err(vec![diagnostic]);
        }
    };
    if let Err(error) = deserializer.end() {
        return Err(vec![Diagnostic::error(
            "source.json.trailing_data",
            "",
            format!(
                "unexpected data after the lesson document at line {}, column {}: {}",
                error.line(),
                error.column(),
                error
            ),
        )]);
    }
    Ok(source)
}

/// Checks source-only invariants and assigns deterministic dense node IDs.
///
/// File existence, Git ownership/revisions, patch parsing, and range/file
/// intersections are intentionally left to the resolution/compiler phase.
pub fn validate(source: LessonSource) -> Result<ValidatedLesson, Vec<Diagnostic>> {
    let mut diagnostics = DiagnosticBag::default();

    if source.title.trim().is_empty() {
        diagnostics.push(
            Diagnostic::error(
                "source.title.empty",
                "/title",
                "lesson title must not be empty or whitespace",
            )
            .with_suggestion("Provide a short title describing what the lesson teaches."),
        );
    }

    if source.blocks.len() > u32::MAX as usize {
        diagnostics.push(Diagnostic::error(
            "source.blocks.too_many",
            "/blocks",
            format!("a lesson may contain at most {} blocks", u32::MAX),
        ));
    }

    let blocks_by_id = source
        .blocks
        .iter()
        .rev()
        .map(|block| (block.id(), block))
        .collect::<HashMap<_, _>>();
    let mut first_ids: HashMap<&SourceId, usize> = HashMap::new();
    for (index, block) in source.blocks.iter().enumerate() {
        let base = format!("/blocks/{index}");
        let id = block.id();
        if let Err(error) = id.validate() {
            diagnostics.push(
                Diagnostic::error(
                    "source.id.invalid",
                    format!("{base}/id"),
                    format!("invalid source ID: {error}"),
                )
                .with_suggestion(
                    "Use a concise human-readable ID without surrounding whitespace or control characters.",
                ),
            );
        }
        match first_ids.entry(id) {
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(index);
            }
            std::collections::hash_map::Entry::Occupied(entry) => {
                diagnostics.push(
                    Diagnostic::error(
                        "source.id.duplicate",
                        format!("{base}/id"),
                        format!("source ID `{id}` is already used by another block"),
                    )
                    .with_related(
                        format!("/blocks/{}/id", entry.get()),
                        "the source ID was first declared here",
                    )
                    .with_suggestion("Give every block a document-unique source ID."),
                );
            }
        }

        match block {
            Block::Markdown(block) => validate_markdown_source(
                &block.source,
                &format!("{base}/source"),
                "Markdown",
                &mut diagnostics,
            ),
            Block::Code(block) => {
                if let Some(language) = &block.language {
                    nonempty(
                        language,
                        &format!("{base}/language"),
                        "code language",
                        &mut diagnostics,
                    );
                }
                if let Some(caption) = &block.caption {
                    nonempty(
                        caption,
                        &format!("{base}/caption"),
                        "caption",
                        &mut diagnostics,
                    );
                }
                validate_code(&block.source, &block.highlights, &base, &mut diagnostics);
            }
            Block::Diff(block) => {
                if let Some(caption) = &block.caption {
                    nonempty(
                        caption,
                        &format!("{base}/caption"),
                        "caption",
                        &mut diagnostics,
                    );
                }
                validate_diff(&block.source, &base, &mut diagnostics);
            }
            Block::MultipleChoice(block) => {
                validate_multiple_choice(block, &base, &mut diagnostics)
            }
            Block::RunCode(block) => {
                validate_run_code(block, &base, &blocks_by_id, &mut diagnostics)
            }
            Block::ExternalArtifact(block) => {
                validate_external_artifact(block, &base, &mut diagnostics)
            }
        }
    }

    if diagnostics.is_empty() {
        let symbols = SymbolTable::from_unique_ids(source.blocks.iter().map(Block::id));
        Ok(ValidatedLesson { source, symbols })
    } else {
        Err(diagnostics.into_vec())
    }
}

fn validate_markdown_source(
    source: &MarkdownSource,
    pointer: &str,
    label: &str,
    diagnostics: &mut DiagnosticBag,
) {
    match source {
        MarkdownSource::Inline { content } => {
            nonempty(content, &format!("{pointer}/content"), label, diagnostics)
        }
        MarkdownSource::File { path } => repo_path(path, &format!("{pointer}/path"), diagnostics),
    }
}

fn validate_code(
    source: &CodeSource,
    highlights: &[CodeHighlight],
    base: &str,
    diagnostics: &mut DiagnosticBag,
) {
    validate_code_highlights(source, highlights, base, diagnostics);
    match source {
        CodeSource::Inline { content } => nonempty(
            content,
            &format!("{base}/source/content"),
            "code",
            diagnostics,
        ),
        CodeSource::File { path, lines } => {
            repo_path(path, &format!("{base}/source/path"), diagnostics);
            if let Some(lines) = lines {
                line_range(lines, &format!("{base}/source/lines"), diagnostics);
            }
        }
        CodeSource::GitBlob {
            revision,
            path,
            lines,
        } => {
            git_revision(revision, &format!("{base}/source/revision"), diagnostics);
            repo_path(path, &format!("{base}/source/path"), diagnostics);
            if let Some(lines) = lines {
                line_range(lines, &format!("{base}/source/lines"), diagnostics);
            }
        }
    }
}

fn validate_code_highlights(
    source: &CodeSource,
    highlights: &[CodeHighlight],
    base: &str,
    diagnostics: &mut DiagnosticBag,
) {
    if highlights.is_empty() {
        return;
    }
    if matches!(source, CodeSource::Inline { .. }) {
        diagnostics.push(
            Diagnostic::error(
                "source.code.highlights.inline",
                format!("{base}/highlights"),
                "line highlights require a file-backed code source",
            )
            .with_suggestion(
                "Write generated code to a file and use a file source when line highlighting is needed.",
            ),
        );
    }

    let displayed_lines = match source {
        CodeSource::File { lines, .. } | CodeSource::GitBlob { lines, .. } => *lines,
        CodeSource::Inline { .. } => None,
    };
    for (highlight_index, highlight) in highlights.iter().enumerate() {
        if let Some(annotation) = &highlight.annotation {
            nonempty(
                annotation,
                &format!("{base}/highlights/{highlight_index}/annotation"),
                "highlight annotation",
                diagnostics,
            );
        }
        let highlight_base = format!("{base}/highlights/{highlight_index}/lines");
        if highlight.lines.is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    "source.code.highlights.empty",
                    &highlight_base,
                    "a highlight group must contain at least one line range",
                )
                .with_suggestion("Add a relevant range or remove the highlight group."),
            );
        }
        for (range_index, range) in highlight.lines.iter().enumerate() {
            let range_base = format!("{highlight_base}/{range_index}");
            line_range(range, &range_base, diagnostics);
            if let Some(displayed) = displayed_lines
                && (range.start < displayed.start || range.end > displayed.end)
            {
                diagnostics.push(
                    Diagnostic::error(
                        "source.code.highlight.outside_selection",
                        range_base,
                        format!(
                            "highlight lines {}-{} fall outside displayed lines {}-{}",
                            range.start, range.end, displayed.start, displayed.end
                        ),
                    )
                    .with_suggestion(
                        "Keep every highlight inside the code source's displayed range.",
                    ),
                );
            }
        }
    }

    for left_group in 0..highlights.len() {
        for right_group in (left_group + 1)..highlights.len() {
            if highlights[left_group].color == highlights[right_group].color {
                continue;
            }
            for (left_index, left) in highlights[left_group].lines.iter().enumerate() {
                for (right_index, right) in highlights[right_group].lines.iter().enumerate() {
                    if left.start <= right.end && right.start <= left.end {
                        diagnostics.push(
                            Diagnostic::error(
                                "source.code.highlight.color_overlap",
                                format!("{base}/highlights/{right_group}/lines/{right_index}"),
                                format!(
                                    "highlight lines {}-{} overlap differently colored lines {}-{}",
                                    right.start, right.end, left.start, left.end
                                ),
                            )
                            .with_related(
                                format!("{base}/highlights/{left_group}/lines/{left_index}"),
                                "the differently colored range is declared here",
                            )
                            .with_suggestion(
                                "Use one color for overlapping ranges or make the ranges disjoint.",
                            ),
                        );
                    }
                }
            }
        }
    }
}

fn validate_diff(source: &DiffSource, base: &str, diagnostics: &mut DiagnosticBag) {
    match source {
        DiffSource::Inline { content } => nonempty(
            content,
            &format!("{base}/source/content"),
            "diff",
            diagnostics,
        ),
        DiffSource::File { path } => repo_path(path, &format!("{base}/source/path"), diagnostics),
        DiffSource::Git {
            base: revision,
            target,
            files,
            ..
        } => {
            git_revision(revision, &format!("{base}/source/base"), diagnostics);
            if let GitDiffTarget::Revision { revision } = target {
                git_revision(
                    revision,
                    &format!("{base}/source/target/revision"),
                    diagnostics,
                );
            }
            if files.is_empty() {
                diagnostics.push(
                    Diagnostic::error(
                        "source.diff.files.empty",
                        format!("{base}/source/files"),
                        "a Git diff source must select at least one file",
                    )
                    .with_suggestion("Add a selected-root-relative file selection."),
                );
            }
            let mut selected_paths = HashSet::new();
            for (file_index, file) in files.iter().enumerate() {
                let file_base = format!("{base}/source/files/{file_index}");
                repo_path(&file.path, &format!("{file_base}/path"), diagnostics);
                if !selected_paths.insert(file.path.as_str()) {
                    diagnostics.push(
                        Diagnostic::error(
                            "source.diff.file.duplicate",
                            format!("{file_base}/path"),
                            format!("file `{}` is selected more than once", file.path.as_str()),
                        )
                        .with_suggestion("Combine the ranges into one selection for this path."),
                    );
                }
                if let Some(range) = file.before_lines {
                    line_range(&range, &format!("{file_base}/before_lines"), diagnostics);
                }
                if let Some(range) = file.after_lines {
                    line_range(&range, &format!("{file_base}/after_lines"), diagnostics);
                }
            }
        }
    }
}

fn validate_run_code(
    block: &RunCodeBlock,
    base: &str,
    blocks_by_id: &HashMap<&SourceId, &Block>,
    diagnostics: &mut DiagnosticBag,
) {
    if let Some(language) = &block.language {
        nonempty(
            language,
            &format!("{base}/language"),
            "code language",
            diagnostics,
        );
    }
    if let Some(caption) = &block.caption {
        nonempty(caption, &format!("{base}/caption"), "caption", diagnostics);
    }

    // The language the block runs, when its code is unambiguous, and the
    // place to report it.
    let (language, language_pointer) = match (&block.source, &block.of) {
        (Some(_), Some(_)) => {
            diagnostics.push(
                Diagnostic::error(
                    "source.run_code.source_and_of",
                    format!("{base}/of"),
                    "a run block takes its own `source` or the code of another block with `of`, not both",
                )
                .with_suggestion(
                    "Remove `source` to run the code another block shows, or remove `of` to run this block's own code.",
                ),
            );
            (None, base.to_owned())
        }
        (None, None) => {
            diagnostics.push(
                Diagnostic::error(
                    "source.run_code.no_source",
                    base,
                    "a run block needs its own `source` or the ID of a code block in `of`",
                )
                .with_suggestion(
                    "Add `source` (inline, file, or git_blob) or `of` naming a code block.",
                ),
            );
            (None, base.to_owned())
        }
        (Some(source), None) => {
            validate_code(source, &[], base, diagnostics);
            let pointer = if block.language.is_some() {
                format!("{base}/language")
            } else {
                format!("{base}/source")
            };
            (Some(source.language(block.language.as_deref())), pointer)
        }
        (None, Some(of)) => {
            let pointer = format!("{base}/of");
            if block.language.is_some() {
                diagnostics.push(
                    Diagnostic::error(
                        "source.run_code.of_with_language",
                        format!("{base}/language"),
                        "a run block with `of` runs the language of the code block it names",
                    )
                    .with_suggestion("Remove `language`."),
                );
            }
            let language = match blocks_by_id.get(of) {
                None => {
                    diagnostics.push(
                        Diagnostic::error(
                            "source.run_code.unknown_block",
                            &pointer,
                            format!("`of` names no block with ID `{of}`"),
                        )
                        .with_suggestion("Use the `id` of an existing code block."),
                    );
                    None
                }
                Some(Block::Code(code)) => Some(code.resolved_language()),
                Some(_) => {
                    diagnostics.push(
                        Diagnostic::error(
                            "source.run_code.of_not_code",
                            &pointer,
                            format!("`of` names block `{of}`, which is not a code block"),
                        )
                        .with_suggestion(
                            "Point `of` at a code block, or give this block its own `source`.",
                        ),
                    );
                    None
                }
            };
            (language, pointer)
        }
    };
    if let Some(language) = language {
        if language == Language::Mermaid {
            diagnostics.push(
                Diagnostic::error(
                    "source.run_code.not_runnable",
                    &language_pointer,
                    "mermaid diagrams cannot be run",
                )
                .with_suggestion("Use a code block for the diagram."),
            );
        } else if block.argv.is_none() && language.default_runner().is_none() {
            diagnostics.push(
                Diagnostic::error(
                    "source.run_code.no_runner",
                    &language_pointer,
                    format!(
                        "there is no default runner for language `{language}`; only python, javascript, and shell have one"
                    ),
                )
                .with_suggestion(format!(
                    "Add `argv`, such as [\"ruby\", \"{RUN_FILE_PLACEHOLDER}\"], where {RUN_FILE_PLACEHOLDER} stands for the code's scratch file."
                )),
            );
        }
    }

    if let Some(argv) = &block.argv {
        let pointer = format!("{base}/argv");
        if argv.is_empty() {
            diagnostics.push(
                Diagnostic::error(
                    "source.run_code.invalid_argv",
                    &pointer,
                    "`argv` must not be empty",
                )
                .with_suggestion(format!(
                    "List the program and its arguments, with {RUN_FILE_PLACEHOLDER} for the scratch file."
                )),
            );
        }
        for (index, argument) in argv.iter().enumerate() {
            if argument.trim().is_empty() {
                diagnostics.push(Diagnostic::error(
                    "source.run_code.invalid_argv",
                    format!("{pointer}/{index}"),
                    "`argv` entries must not be empty or whitespace",
                ));
            }
        }
        if !argv.is_empty()
            && !argv
                .iter()
                .any(|argument| argument.contains(RUN_FILE_PLACEHOLDER))
        {
            diagnostics.push(
                Diagnostic::error(
                    "source.run_code.invalid_argv",
                    &pointer,
                    format!("`argv` must contain {RUN_FILE_PLACEHOLDER}, the scratch file to run"),
                )
                .with_suggestion(format!(
                    "Pass the file to the program, e.g. [\"python3\", \"{RUN_FILE_PLACEHOLDER}\"]."
                )),
            );
        }
    }

    if let Some(timeout) = block.timeout_secs
        && !(1..=60).contains(&timeout)
    {
        diagnostics.push(
            Diagnostic::error(
                "source.run_code.invalid_timeout",
                format!("{base}/timeout_secs"),
                format!("`timeout_secs` must be between 1 and 60, found {timeout}"),
            )
            .with_suggestion("Omit it for the default of 10 seconds."),
        );
    }

    match &block.expected_output {
        Some(OutputSource::Inline { content }) => nonempty(
            content,
            &format!("{base}/expected_output/content"),
            "expected output",
            diagnostics,
        ),
        Some(OutputSource::File { path }) => {
            repo_path(path, &format!("{base}/expected_output/path"), diagnostics)
        }
        None => {}
    }
}

fn validate_external_artifact(
    block: &ExternalArtifactBlock,
    base: &str,
    diagnostics: &mut DiagnosticBag,
) {
    let pointer = format!("{base}/file");
    if let Some(message) = external_artifact_file_error(&block.file) {
        diagnostics.push(
            Diagnostic::error(
                "source.external_artifact.invalid_file_name",
                &pointer,
                format!("invalid file name: {message}"),
            )
            .with_suggestion("Use a bare file name such as `queue-demo.mp4`, not a path."),
        );
    } else if !block.kind.allows_file(&block.file) {
        diagnostics.push(
            Diagnostic::error(
                "source.external_artifact.extension_not_allowed",
                &pointer,
                format!(
                    "`{}` is not a {} file this block can show; the extension must be one of: {}",
                    block.file,
                    block.kind.as_str(),
                    block.kind.extensions().join(", ")
                ),
            )
            .with_suggestion(
                "Use a file with an allowed extension, or change `kind` to match the file.",
            ),
        );
    }
    nonempty(&block.alt, &format!("{base}/alt"), "alt text", diagnostics);
    nonempty(
        &block.fallback,
        &format!("{base}/fallback"),
        "fallback",
        diagnostics,
    );
    if let Some(caption) = &block.caption {
        nonempty(caption, &format!("{base}/caption"), "caption", diagnostics);
    }
}

fn validate_multiple_choice(
    block: &super::MultipleChoiceBlock,
    base: &str,
    diagnostics: &mut DiagnosticBag,
) {
    validate_markdown_source(
        &block.prompt,
        &format!("{base}/prompt"),
        "question prompt",
        diagnostics,
    );
    if block.choices.len() < 2 {
        diagnostics.push(
            Diagnostic::error(
                "source.quiz.choices.too_few",
                format!("{base}/choices"),
                format!(
                    "a multiple-choice question needs at least 2 choices, found {}",
                    block.choices.len()
                ),
            )
            .with_suggestion("Add at least two plausible Markdown choices."),
        );
    }
    let correct_count = block.choices.iter().filter(|choice| choice.correct).count();
    if correct_count != 1 {
        diagnostics.push(
            Diagnostic::error(
                "source.quiz.correct.invalid_count",
                format!("{base}/choices"),
                format!(
                    "a multiple-choice question needs exactly 1 correct choice, found {correct_count}"
                ),
            )
            .with_suggestion("Set `correct: true` on exactly one choice; omission means false."),
        );
    }
    for (index, choice) in block.choices.iter().enumerate() {
        nonempty(
            &choice.content,
            &format!("{base}/choices/{index}/content"),
            "choice content",
            diagnostics,
        );
        if let Some(explanation) = &choice.explanation {
            let pointer = format!("{base}/choices/{index}/explanation");
            if choice.correct {
                diagnostics.push(
                    Diagnostic::error(
                        "source.quiz.choice_explanation.on_correct",
                        &pointer,
                        "the correct choice cannot have its own explanation",
                    )
                    .with_suggestion(
                        "Move this text into the question's `explanation`; choice explanations say why a distractor is wrong.",
                    ),
                );
            } else {
                nonempty(explanation, &pointer, "choice explanation", diagnostics);
            }
        }
    }
    for (index, hint) in block.hints.iter().enumerate() {
        nonempty(hint, &format!("{base}/hints/{index}"), "hint", diagnostics);
    }
    nonempty(
        &block.explanation,
        &format!("{base}/explanation"),
        "answer explanation",
        diagnostics,
    );
}

fn nonempty(value: &str, pointer: &str, label: &str, diagnostics: &mut DiagnosticBag) {
    if value.trim().is_empty() {
        diagnostics.push(Diagnostic::error(
            "source.content.empty",
            pointer,
            format!("{label} must not be empty or whitespace"),
        ));
    }
}

fn repo_path(path: &RepoPath, pointer: &str, diagnostics: &mut DiagnosticBag) {
    if let Some(message) = path.validation_error() {
        diagnostics.push(
            Diagnostic::error(
                "source.path.invalid",
                pointer,
                format!("invalid selected-root-relative path: {message}"),
            )
            .with_suggestion(
                "Use a forward-slash path below the selected filesystem root without `.` or `..` components.",
            ),
        );
    }
}

fn git_revision(revision: &GitRevision, pointer: &str, diagnostics: &mut DiagnosticBag) {
    if let Some(message) = revision.validation_error() {
        diagnostics.push(
            Diagnostic::error(
                "source.git.revision.invalid",
                pointer,
                format!("invalid Git revision expression: {message}"),
            )
            .with_suggestion("Use a symbolic revision such as `HEAD`, `main`, or `HEAD~2`."),
        );
    }
}

fn line_range(range: &LineRange, pointer: &str, diagnostics: &mut DiagnosticBag) {
    if range.start == 0 {
        diagnostics.push(
            Diagnostic::error(
                "source.lines.start.zero",
                format!("{pointer}/start"),
                "line ranges are one-based; start must be at least 1",
            )
            .with_suggestion("Use 1 for the first line."),
        );
    }
    if range.end < range.start {
        diagnostics.push(
            Diagnostic::error(
                "source.lines.reversed",
                pointer,
                format!(
                    "inclusive line range end ({}) is before start ({})",
                    range.end, range.start
                ),
            )
            .with_suggestion(
                "Set `end` to the last included line, greater than or equal to `start`.",
            ),
        );
    }
}

/// Converts serde_path_to_error's display syntax into an RFC 6901 pointer.
fn serde_path_to_json_pointer(path: &str) -> String {
    if path.is_empty() || path == "." {
        return String::new();
    }
    let mut segments = Vec::new();
    let mut current = String::new();
    let mut chars = path.trim_start_matches('.').chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '.' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
            }
            '[' => {
                if !current.is_empty() {
                    segments.push(std::mem::take(&mut current));
                }
                let mut index = String::new();
                for character in chars.by_ref() {
                    if character == ']' {
                        break;
                    }
                    index.push(character);
                }
                if !index.is_empty() {
                    segments.push(index);
                }
            }
            other => current.push(other),
        }
    }
    if !current.is_empty() {
        segments.push(current);
    }
    segments
        .into_iter()
        .map(|segment| crate::diagnostics::escape_json_pointer_segment(&segment))
        .fold(String::new(), |mut pointer, segment| {
            pointer.push('/');
            pointer.push_str(&segment);
            pointer
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_lesson() -> &'static str {
        r##"{
            "schema_version":"1.0.0",
            "title":"Queues",
            "blocks":[
                {
                    "type":"markdown",
                    "id":"intro",
                    "source":{"kind":"inline","content":"# Queues"}
                },
                {
                    "type":"multiple_choice",
                    "id":"check-order",
                    "prompt":"Which item leaves first?",
                    "choices":[
                        {"content":"The oldest", "correct":true},
                        {"content":"The newest"}
                    ],
                    "hints":["Think **FIFO**."],
                    "explanation":"A queue is first-in, first-out."
                }
            ]
        }"##
    }

    #[test]
    fn valid_source_gets_dense_node_ids() {
        let lesson = parse_and_validate(valid_lesson()).expect("valid lesson");
        let intro = SourceId::new("intro").unwrap();
        let question = SourceId::new("check-order").unwrap();
        assert_eq!(lesson.symbols().node_id(&intro).unwrap().get(), 0);
        assert_eq!(lesson.symbols().node_id(&question).unwrap().get(), 1);
    }

    #[test]
    fn collects_independent_semantic_errors() {
        let json = r#"{
            "schema_version":"1.0.0",
            "title":" ",
            "blocks":[
                {"type":"code","id":"same","source":{"kind":"file","path":"../x"}},
                {
                    "type":"multiple_choice",
                    "id":"same",
                    "prompt":"",
                    "choices":[{"content":"", "correct":true}],
                    "hints":[""],
                    "explanation":""
                }
            ]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("source is invalid");
        let codes: HashSet<_> = diagnostics
            .iter()
            .map(|error| error.code.as_str())
            .collect();
        assert!(codes.contains("source.title.empty"));
        assert!(codes.contains("source.path.invalid"));
        assert!(codes.contains("source.id.duplicate"));
        assert!(codes.contains("source.quiz.choices.too_few"));
        assert!(codes.contains("source.content.empty"));
        assert_eq!(
            diagnostics
                .iter()
                .find(|error| error.code == "source.id.duplicate")
                .unwrap()
                .pointer,
            "/blocks/1/id"
        );
    }

    #[test]
    fn structural_error_points_to_containing_tagged_block() {
        let json = r#"{
            "schema_version":"1.0.0",
            "title":"Queues",
            "blocks":[{"type":"code","id":"sample","source":{"kind":"file","path":4}}]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("path has wrong type");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "source.deserialize");
        // Serde buffers internally tagged enum contents, so nested type errors
        // resolve to the containing block. Semantic diagnostics identify the
        // exact nested field whenever the value can be decoded.
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
    }

    #[test]
    fn line_ranges_are_one_based_and_inclusive() {
        let json = r#"{
            "schema_version":"1.0.0",
            "title":"Ranges",
            "blocks":[{
                "type":"code",
                "id":"bad-range",
                "source":{"kind":"file","path":"src/lib.rs","lines":{"start":0,"end":0}}
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("range starts at zero");
        assert_eq!(diagnostics[0].pointer, "/blocks/0/source/lines/start");
    }

    #[test]
    fn rejects_an_explicit_blank_code_language() {
        let json = r#"{
            "schema_version":"1.1.0",
            "title":"Language",
            "blocks":[{
                "type":"code",
                "id":"sample",
                "language":"   ",
                "source":{"kind":"inline","content":"some code"}
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("blank language is invalid");
        assert_eq!(diagnostics[0].code, "source.content.empty");
        assert_eq!(diagnostics[0].pointer, "/blocks/0/language");
    }

    #[test]
    fn source_1_0_rejects_the_language_field_added_in_1_1() {
        let json = r#"{
            "schema_version":"1.0.0",
            "title":"Legacy contract",
            "blocks":[{
                "type":"code",
                "id":"sample",
                "language":"rust",
                "source":{"kind":"inline","content":"fn main() {}"}
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("1.0 must remain a closed shape");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
        assert!(diagnostics[0].message.contains("unknown field `language`"));
    }

    #[test]
    fn source_1_1_rejects_the_caption_field_added_in_1_2() {
        let json = r#"{
            "schema_version":"1.1.0",
            "title":"Legacy contract",
            "blocks":[{
                "type":"code",
                "id":"diagram",
                "language":"mermaid",
                "caption":"A hidden assumption.",
                "source":{"kind":"inline","content":"flowchart LR\nA --> B"}
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("1.1 must remain a closed shape");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
        assert!(diagnostics[0].message.contains("unknown field `caption`"));
    }

    #[test]
    fn source_1_2_rejects_blank_code_and_diff_captions() {
        let json = r#"{
            "schema_version":"1.2.0",
            "title":"Captions",
            "blocks":[
                {
                    "type":"code",
                    "id":"diagram",
                    "caption":"   ",
                    "source":{"kind":"inline","content":"flowchart LR\nA --> B"}
                },
                {
                    "type":"diff",
                    "id":"change",
                    "caption":"\n",
                    "source":{"kind":"inline","content":"diff --git a/a b/a\n--- a/a\n+++ b/a\n@@ -1 +1 @@\n-old\n+new\n"}
                }
            ]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("blank captions are invalid");
        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].pointer, "/blocks/0/caption");
        assert_eq!(diagnostics[1].pointer, "/blocks/1/caption");
    }

    #[test]
    fn source_1_2_rejects_highlights_added_in_1_3() {
        let json = r#"{
            "schema_version":"1.2.0",
            "title":"Legacy contract",
            "blocks":[{
                "type":"code",
                "id":"sample",
                "highlights":[{"lines":[{"start":1,"end":1}]}],
                "source":{"kind":"file","path":"src/lib.rs"}
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("1.2 must remain a closed shape");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
        assert!(
            diagnostics[0]
                .message
                .contains("unknown field `highlights`")
        );
    }

    #[test]
    fn source_1_3_rejects_prompt_sources_added_in_2_0() {
        let json = r#"{
            "schema_version":"1.3.0",
            "title":"Legacy prompt",
            "blocks":[{
                "type":"multiple_choice",
                "id":"question",
                "prompt":{"kind":"inline","content":"Which answer is correct?"},
                "choices":[
                    {"content":"First","correct":true},
                    {"content":"Second"}
                ],
                "explanation":"The first answer is correct."
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("1.3 keeps string prompts");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
        assert!(diagnostics[0].message.contains("expected a string"));
    }

    #[test]
    fn source_2_0_requires_prompt_sources_and_validates_them() {
        let legacy = r#"{
            "schema_version":"2.0.0",
            "title":"New prompt contract",
            "blocks":[{
                "type":"multiple_choice",
                "id":"legacy",
                "prompt":"This shape is no longer current.",
                "choices":[
                    {"content":"First","correct":true},
                    {"content":"Second"}
                ],
                "explanation":"The first answer is correct."
            }]
        }"#;
        let diagnostics = parse_and_validate(legacy).expect_err("2.0 rejects string prompts");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");

        let invalid_source = r#"{
            "schema_version":"2.0.0",
            "title":"New prompt contract",
            "blocks":[{
                "type":"multiple_choice",
                "id":"question",
                "prompt":{"kind":"file","path":"../outside.md"},
                "choices":[
                    {"content":"First","correct":true},
                    {"content":"Second"}
                ],
                "explanation":"The first answer is correct."
            }]
        }"#;
        let diagnostics = parse_and_validate(invalid_source).expect_err("prompt path escapes root");
        assert_eq!(diagnostics[0].code, "source.path.invalid");
        assert_eq!(diagnostics[0].pointer, "/blocks/0/prompt/path");
    }

    #[test]
    fn choice_explanations_begin_in_source_2_2_and_only_explain_distractors() {
        let quiz = |version: &str, choices: &str| {
            format!(
                r#"{{"schema_version":"{version}","title":"Quiz","blocks":[{{
                    "type":"multiple_choice","id":"q",
                    "prompt":{{"kind":"inline","content":"Pick one."}},
                    "choices":{choices},
                    "explanation":"A is right."
                }}]}}"#
            )
        };
        let legacy = quiz(
            "2.1.0",
            r#"[{"content":"A","correct":true},{"content":"B","explanation":"B is wrong."}]"#,
        );
        let diagnostics = parse_and_validate(&legacy).expect_err("2.1 rejects choice explanations");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert!(
            diagnostics[0]
                .message
                .contains("unknown field `explanation`")
        );

        let valid = quiz(
            "2.2.0",
            r#"[{"content":"A","correct":true},{"content":"B","explanation":"B is wrong."}]"#,
        );
        let lesson = parse_and_validate(&valid).expect("distractor explanations are valid");
        let Block::MultipleChoice(block) = &lesson.source().blocks[0] else {
            panic!("expected a question")
        };
        assert_eq!(block.choices[1].explanation.as_deref(), Some("B is wrong."));

        let invalid = quiz(
            "2.2.0",
            r#"[{"content":"A","correct":true,"explanation":"Duplicate."},{"content":"B","explanation":"  "}]"#,
        );
        let diagnostics = parse_and_validate(&invalid).expect_err("invalid explanations");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "source.quiz.choice_explanation.on_correct"
                && diagnostic.pointer == "/blocks/0/choices/0/explanation"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "source.content.empty"
                && diagnostic.pointer == "/blocks/0/choices/1/explanation"
        }));
    }

    #[test]
    fn highlight_annotations_begin_in_source_2_1_and_must_not_be_blank() {
        let legacy = r#"{
            "schema_version":"2.0.0",
            "title":"Legacy highlights",
            "blocks":[{
                "type":"code",
                "id":"code",
                "highlights":[{
                    "lines":[{"start":1,"end":1}],
                    "annotation":"This explanation is too new for 2.0.0."
                }],
                "source":{"kind":"file","path":"sample.rs"}
            }]
        }"#;
        let diagnostics = parse_and_validate(legacy).expect_err("2.0 rejects annotations");
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
        assert!(
            diagnostics[0]
                .message
                .contains("unknown field `annotation`")
        );

        let current = r#"{
            "schema_version":"2.1.0",
            "title":"Annotated highlights",
            "blocks":[{
                "type":"code",
                "id":"code",
                "highlights":[{
                    "lines":[{"start":1,"end":1}],
                    "annotation":"   "
                }],
                "source":{"kind":"file","path":"sample.rs"}
            }]
        }"#;
        let diagnostics = parse_and_validate(current).expect_err("blank annotation is invalid");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "source.content.empty"
                && diagnostic.pointer == "/blocks/0/highlights/0/annotation"
        }));
    }

    #[test]
    fn highlights_require_file_backing_and_disjoint_colors() {
        let json = r#"{
            "schema_version":"1.3.0",
            "title":"Highlights",
            "blocks":[
                {
                    "type":"code",
                    "id":"inline",
                    "highlights":[{"lines":[{"start":1,"end":1}]}],
                    "source":{"kind":"inline","content":"let value = 1;"}
                },
                {
                    "type":"code",
                    "id":"overlap",
                    "highlights":[
                        {"lines":[{"start":12,"end":14}],"color":"blue"},
                        {"lines":[{"start":14,"end":15}],"color":"green"}
                    ],
                    "source":{
                        "kind":"file",
                        "path":"src/lib.rs",
                        "lines":{"start":10,"end":20}
                    }
                }
            ]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("invalid highlights");
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "source.code.highlights.inline"
                && diagnostic.pointer == "/blocks/0/highlights"
        }));
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "source.code.highlight.color_overlap"
                && diagnostic.pointer == "/blocks/1/highlights/1/lines/0"
        }));
    }

    #[test]
    fn highlight_ranges_must_stay_inside_the_displayed_file_selection() {
        let json = r#"{
            "schema_version":"1.3.0",
            "title":"Highlights",
            "blocks":[{
                "type":"code",
                "id":"sample",
                "highlights":[{"lines":[{"start":9,"end":12}]}],
                "source":{
                    "kind":"git_blob",
                    "revision":"HEAD",
                    "path":"src/lib.rs",
                    "lines":{"start":10,"end":20}
                }
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("highlight escapes selection");
        assert_eq!(
            diagnostics[0].code,
            "source.code.highlight.outside_selection"
        );
        assert_eq!(diagnostics[0].pointer, "/blocks/0/highlights/0/lines/0");
    }

    #[test]
    fn git_diff_requires_unique_files() {
        let json = r#"{
            "schema_version":"1.0.0",
            "title":"Diff",
            "blocks":[{
                "type":"diff",
                "id":"changes",
                "source":{
                    "kind":"git",
                    "base":"HEAD~1",
                    "target":{"kind":"worktree"},
                    "files":[{"path":"src/lib.rs"},{"path":"src/lib.rs"}],
                    "context_lines":3
                }
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("duplicate path");
        assert_eq!(diagnostics[0].code, "source.diff.file.duplicate");
        assert_eq!(diagnostics[0].pointer, "/blocks/0/source/files/1/path");
    }

    #[test]
    fn serde_paths_convert_to_json_pointers() {
        assert_eq!(
            serde_path_to_json_pointer("blocks[2].source.path"),
            "/blocks/2/source/path"
        );
        assert_eq!(serde_path_to_json_pointer("."), "");
    }

    #[test]
    fn reports_json_syntax_errors_before_shape_errors() {
        let input = r#"{"schema_version":"1.0.0","title":"T","blocks":[{"id":"q","type":"multiple_choice","prompt":"p","choices":[{"content":"a","correct":true},{"content":"b"}],}]}"#;
        let diagnostics = parse_and_validate(input).unwrap_err();
        assert_eq!(diagnostics.len(), 1);
        assert!(
            diagnostics[0].message.starts_with("invalid JSON at line 1"),
            "{}",
            diagnostics[0].message
        );
    }

    #[test]
    fn rejects_trailing_json_data() {
        let json = format!("{} true", valid_lesson());
        let diagnostics = parse_and_validate(&json).expect_err("trailing value is invalid");
        assert_eq!(diagnostics[0].code, "source.json.trailing_data");
        assert_eq!(diagnostics[0].pointer, "");
    }

    fn run_lesson(version: &str, blocks: &str) -> String {
        format!(r#"{{"schema_version":"{version}","title":"Run","blocks":[{blocks}]}}"#)
    }

    const SHOWN_CODE: &str = r#"{"type":"code","id":"shown","language":"python","source":{"kind":"inline","content":"print(1)"}}"#;

    fn run_codes(blocks: &str) -> Vec<(String, String)> {
        parse_and_validate(&run_lesson("2.4.0", blocks))
            .expect_err("run block is invalid")
            .into_iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.pointer))
            .collect()
    }

    #[test]
    fn run_blocks_begin_in_source_2_4_and_older_schemas_reject_them() {
        let block = r#"{"type":"run_code","id":"run","language":"python","source":{"kind":"inline","content":"print(1)"}}"#;
        for version in ["2.0.0", "2.2.0", "2.3.0"] {
            let diagnostics = parse_and_validate(&run_lesson(version, block))
                .expect_err("older schemas are closed shapes");
            assert_eq!(diagnostics[0].code, "source.deserialize", "{version}");
            assert_eq!(diagnostics[0].pointer, "/blocks/0/type", "{version}");
            assert!(
                diagnostics[0]
                    .message
                    .contains("unknown variant `run_code`"),
                "{version}: {}",
                diagnostics[0].message
            );
        }
        let lesson = parse_and_validate(&run_lesson("2.4.0", block)).expect("2.4.0 accepts it");
        assert!(matches!(lesson.source().blocks[0], Block::RunCode(_)));
        assert_eq!(
            lesson.source().schema_version,
            super::super::SchemaVersion::V2_4_0
        );
    }

    #[test]
    fn run_blocks_accept_every_documented_shape() {
        let blocks = [
            SHOWN_CODE,
            r#"{"type":"run_code","id":"of","of":"shown","caption":"Runs it.","timeout_secs":60,
                "expected_output":{"kind":"inline","content":"1\n"}}"#,
            r#"{"type":"run_code","id":"inline","language":"js","timeout_secs":1,
                "source":{"kind":"inline","content":"console.log(1)"}}"#,
            r#"{"type":"run_code","id":"inferred","source":{"kind":"file","path":"tool.sh","lines":{"start":1,"end":3}},
                "expected_output":{"kind":"file","path":"tool.out"}}"#,
            r#"{"type":"run_code","id":"blob","source":{"kind":"git_blob","revision":"HEAD","path":"tool.py"}}"#,
            r#"{"type":"run_code","id":"argv","language":"rust","argv":["rustc","--out-dir=.","{file}"],
                "source":{"kind":"inline","content":"fn main() {}"}}"#,
            r#"{"type":"run_code","id":"argv-over-default","language":"python","argv":["python3","-u","{file}"],
                "source":{"kind":"inline","content":"print(1)"}}"#,
        ]
        .join(",");
        parse_and_validate(&run_lesson("2.4.0", &blocks)).expect("valid run blocks");
    }

    #[test]
    fn run_blocks_need_exactly_one_of_source_and_of() {
        assert_eq!(
            run_codes(&format!(
                r#"{SHOWN_CODE},{{"type":"run_code","id":"r","of":"shown",
                    "source":{{"kind":"inline","content":"print(2)"}}}}"#
            )),
            [(
                "source.run_code.source_and_of".into(),
                "/blocks/1/of".into()
            )]
        );
        assert_eq!(
            run_codes(r#"{"type":"run_code","id":"r"}"#),
            [("source.run_code.no_source".into(), "/blocks/0".into())]
        );
    }

    #[test]
    fn of_must_name_a_runnable_code_block_and_forbids_language() {
        let run = |extra: &str| format!(r#"{{"type":"run_code","id":"r",{extra}}}"#);
        assert_eq!(
            run_codes(&run(r#""of":"nowhere""#)),
            [(
                "source.run_code.unknown_block".into(),
                "/blocks/0/of".into()
            )]
        );
        let markdown =
            r#"{"type":"markdown","id":"text","source":{"kind":"inline","content":"Hi"}}"#;
        assert_eq!(
            run_codes(&format!("{markdown},{}", run(r#""of":"text""#))),
            [("source.run_code.of_not_code".into(), "/blocks/1/of".into())]
        );
        assert_eq!(
            run_codes(&format!(
                "{SHOWN_CODE},{}",
                run(r#""of":"shown","language":"python""#)
            )),
            [(
                "source.run_code.of_with_language".into(),
                "/blocks/1/language".into()
            )]
        );
        // The language comes from the referenced block, so its problems are
        // reported on `of`: a diagram is not runnable and Rust has no runner.
        let diagram = r#"{"type":"code","id":"diagram","language":"mermaid","source":{"kind":"inline","content":"flowchart LR\n A --> B"}}"#;
        assert_eq!(
            run_codes(&format!("{diagram},{}", run(r#""of":"diagram""#))),
            [("source.run_code.not_runnable".into(), "/blocks/1/of".into())]
        );
        let rust = r#"{"type":"code","id":"rust","source":{"kind":"file","path":"src/lib.rs"}}"#;
        assert_eq!(
            run_codes(&format!("{rust},{}", run(r#""of":"rust""#))),
            [("source.run_code.no_runner".into(), "/blocks/1/of".into())]
        );
    }

    #[test]
    fn languages_without_a_default_runner_need_argv() {
        let run = |extra: &str| {
            format!(
                r#"{{"type":"run_code","id":"r",{extra},"source":{{"kind":"inline","content":"x"}}}}"#
            )
        };
        assert_eq!(
            run_codes(&run(r#""language":"ruby""#)),
            [(
                "source.run_code.no_runner".into(),
                "/blocks/0/language".into()
            )]
        );
        assert_eq!(
            run_codes(&run(r#""language":"mermaid","argv":["mmdc","{file}"]"#)),
            [(
                "source.run_code.not_runnable".into(),
                "/blocks/0/language".into()
            )]
        );
        // Inline code without a language is plain text, which has no runner.
        let unspecified =
            r#"{"type":"run_code","id":"r","source":{"kind":"inline","content":"x"}}"#;
        assert_eq!(
            run_codes(unspecified),
            [(
                "source.run_code.no_runner".into(),
                "/blocks/0/source".into()
            )]
        );
    }

    #[test]
    fn argv_must_be_a_nonblank_command_with_the_scratch_file() {
        let run = |argv: &str| {
            format!(
                r#"{{"type":"run_code","id":"r","language":"python","argv":{argv},
                    "source":{{"kind":"inline","content":"x"}}}}"#
            )
        };
        for (argv, pointer) in [
            ("[]", "/blocks/0/argv"),
            (r#"["python3"]"#, "/blocks/0/argv"),
            (r#"["python3","  ","{file}"]"#, "/blocks/0/argv/1"),
        ] {
            assert_eq!(
                run_codes(&run(argv)),
                [("source.run_code.invalid_argv".into(), pointer.into())],
                "{argv}"
            );
        }
    }

    #[test]
    fn timeouts_are_whole_seconds_from_one_to_sixty() {
        let run = |timeout: &str| {
            format!(
                r#"{{"type":"run_code","id":"r","language":"python","timeout_secs":{timeout},
                    "source":{{"kind":"inline","content":"x"}}}}"#
            )
        };
        for timeout in ["0", "61", "4294967295"] {
            assert_eq!(
                run_codes(&run(timeout)),
                [(
                    "source.run_code.invalid_timeout".into(),
                    "/blocks/0/timeout_secs".into()
                )],
                "{timeout}"
            );
        }
        for timeout in ["-1", "1.5", "\"10\""] {
            let diagnostics = parse_and_validate(&run_lesson("2.4.0", &run(timeout)))
                .expect_err("not an unsigned integer");
            assert_eq!(diagnostics[0].code, "source.deserialize", "{timeout}");
        }
    }

    #[test]
    fn run_block_sources_captions_and_outputs_are_checked_like_code_blocks() {
        let invalid = r#"{"type":"run_code","id":"r","language":" ","caption":"\n",
            "source":{"kind":"file","path":"../outside.py","lines":{"start":4,"end":2}},
            "expected_output":{"kind":"file","path":"/abs.out"}},
            {"type":"run_code","id":"s","language":"python","source":{"kind":"inline","content":" "},
             "expected_output":{"kind":"inline","content":"  "}}"#;
        let codes = run_codes(invalid);
        for expected in [
            ("source.content.empty", "/blocks/0/language"),
            ("source.content.empty", "/blocks/0/caption"),
            ("source.path.invalid", "/blocks/0/source/path"),
            ("source.lines.reversed", "/blocks/0/source/lines"),
            ("source.path.invalid", "/blocks/0/expected_output/path"),
            ("source.content.empty", "/blocks/1/source/content"),
            ("source.content.empty", "/blocks/1/expected_output/content"),
        ] {
            assert!(
                codes.contains(&(expected.0.to_owned(), expected.1.to_owned())),
                "missing {expected:?} in {codes:?}"
            );
        }
        // Highlights are not part of a run block.
        let diagnostics = parse_and_validate(&run_lesson(
            "2.4.0",
            r#"{"type":"run_code","id":"r","highlights":[{"lines":[{"start":1,"end":1}]}],
                "source":{"kind":"file","path":"a.py"}}"#,
        ))
        .expect_err("highlights are unsupported");
        assert!(
            diagnostics[0]
                .message
                .contains("unknown field `highlights`")
        );
    }

    const EXTERNAL: &str = r#"{"type":"external_artifact","id":"demo","kind":"video","file":"queue-demo.mp4",
        "alt":"A queue animation","fallback":"Items leave from the **front**."}"#;

    fn external_codes(block: &str) -> Vec<(String, String)> {
        parse_and_validate(&run_lesson("2.5.0", block))
            .expect_err("external artifact is invalid")
            .into_iter()
            .map(|diagnostic| (diagnostic.code, diagnostic.pointer))
            .collect()
    }

    /// The block with one field replaced, as JSON.
    fn external_with(field: &str, value: serde_json::Value) -> String {
        let mut block: serde_json::Value = serde_json::from_str(EXTERNAL).unwrap();
        block[field] = value;
        block.to_string()
    }

    #[test]
    fn external_artifacts_begin_in_source_2_5_and_older_schemas_reject_them() {
        for version in ["2.0.0", "2.2.0", "2.3.0", "2.4.0"] {
            let diagnostics = parse_and_validate(&run_lesson(version, EXTERNAL))
                .expect_err("older schemas are closed shapes");
            assert_eq!(diagnostics[0].code, "source.deserialize", "{version}");
            assert_eq!(diagnostics[0].pointer, "/blocks/0/type", "{version}");
            assert!(
                diagnostics[0]
                    .message
                    .contains("unknown variant `external_artifact`"),
                "{version}: {}",
                diagnostics[0].message
            );
        }
        let lesson = parse_and_validate(&run_lesson("2.5.0", EXTERNAL)).expect("2.5.0 accepts it");
        assert!(matches!(
            lesson.source().blocks[0],
            Block::ExternalArtifact(_)
        ));
        assert_eq!(
            lesson.source().schema_version,
            super::super::SchemaVersion::CURRENT
        );
    }

    #[test]
    fn external_artifacts_accept_every_kind_extension_in_any_case() {
        let mut blocks = Vec::new();
        for kind in [
            super::super::ExternalArtifactKind::Image,
            super::super::ExternalArtifactKind::Audio,
            super::super::ExternalArtifactKind::Video,
        ] {
            for (index, extension) in kind.extensions().iter().enumerate() {
                for file in [
                    format!("clip.{extension}"),
                    format!("Clip.{}", extension.to_uppercase()),
                ] {
                    blocks.push(
                        serde_json::json!({
                            "type": "external_artifact",
                            "id": format!("{}-{index}-{file}", kind.as_str()),
                            "kind": kind.as_str(),
                            "file": file,
                            "alt": "Alt text",
                            "fallback": "Fallback *text*.",
                            "caption": "A [caption](#demo)."
                        })
                        .to_string(),
                    );
                }
            }
        }
        blocks.push(EXTERNAL.to_owned());
        parse_and_validate(&run_lesson("2.5.0", &blocks.join(","))).expect("valid blocks");
    }

    #[test]
    fn external_artifact_file_names_are_bare_names() {
        for file in [
            "",
            "a/b.mp4",
            "a\\b.mp4",
            ".",
            "..",
            ".hidden.mp4",
            "a\n.mp4",
            "a\u{7f}.mp4",
        ] {
            assert_eq!(
                external_codes(&external_with("file", file.into())),
                [(
                    "source.external_artifact.invalid_file_name".into(),
                    "/blocks/0/file".into()
                )],
                "{file:?}"
            );
        }
    }

    #[test]
    fn external_artifact_extensions_must_suit_the_kind() {
        for (kind, file) in [
            ("video", "clip.png"),
            ("image", "clip.mp4"),
            ("audio", "clip.webm"),
            ("video", "clip"),
            ("video", "clip."),
            ("video", "mp4"),
            ("video", "clip.mp4.exe"),
            ("image", "clip.mp3"),
        ] {
            let block =
                external_with("file", file.into()).replace("\"video\"", &format!("\"{kind}\""));
            assert_eq!(
                external_codes(&block),
                [(
                    "source.external_artifact.extension_not_allowed".into(),
                    "/blocks/0/file".into()
                )],
                "{kind} {file}"
            );
        }
    }

    #[test]
    fn external_artifact_text_fields_are_checked_like_other_prose() {
        for field in ["alt", "fallback", "caption"] {
            assert_eq!(
                external_codes(&external_with(field, " \n".into())),
                [("source.content.empty".into(), format!("/blocks/0/{field}"))],
                "{field}"
            );
        }
    }

    #[test]
    fn external_artifacts_require_every_field_but_caption_and_reject_strays() {
        for field in ["kind", "file", "alt", "fallback"] {
            let mut block: serde_json::Value = serde_json::from_str(EXTERNAL).unwrap();
            block.as_object_mut().unwrap().remove(field);
            let diagnostics = parse_and_validate(&run_lesson("2.5.0", &block.to_string()))
                .expect_err("a required field is missing");
            assert_eq!(diagnostics[0].code, "source.deserialize", "{field}");
            assert!(
                diagnostics[0]
                    .message
                    .contains(&format!("missing field `{field}`")),
                "{field}: {}",
                diagnostics[0].message
            );
        }
        for (field, value) in [
            ("kind", serde_json::json!("document")),
            (
                "source",
                serde_json::json!({"kind": "inline", "content": "x"}),
            ),
        ] {
            let diagnostics =
                parse_and_validate(&run_lesson("2.5.0", &external_with(field, value)))
                    .expect_err("closed shape");
            assert_eq!(diagnostics[0].code, "source.deserialize", "{field}");
        }
    }

    #[test]
    fn rejects_stray_fields_nested_inside_a_block() {
        let json = r#"{
            "schema_version":"1.0.0",
            "title":"Strict sources",
            "blocks":[{
                "type":"code",
                "id":"sample",
                "source":{
                    "kind":"file",
                    "path":"src/lib.rs",
                    "revision":"HEAD"
                }
            }]
        }"#;
        let diagnostics = parse_and_validate(json).expect_err("file source has a stray revision");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, "source.deserialize");
        assert_eq!(diagnostics[0].pointer, "/blocks/0");
        assert!(diagnostics[0].message.contains("unknown field `revision`"));
    }
}
