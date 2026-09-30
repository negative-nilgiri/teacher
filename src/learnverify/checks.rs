//! Turns a compiled lesson into TypeSafe requests: one per multiple-choice
//! block and one per highlighted code block.
//!
//! Every request's `state` is an excerpt of the lesson in authored order, with
//! `subject` naming the block under review. The questions are yes/no, name
//! their exact target, and "yes" always means a problem.

use serde_json::{Value, json};

use crate::artifact::LinkedLines;
use crate::artifact::{
    CompiledCodeHighlight, CompiledNode, CompiledNodeContent, ResourceProvenance,
};
use crate::compiler::links::{
    LinkOccurrence, MarkdownField, find_links, markdown_fields, target_of,
};
use crate::lint::LoadedLesson;
use crate::repository::{DiffLineKind, ResolvedDiff};
use crate::source::{Block, MultipleChoiceBlock};

/// Version of the question wording and state shape. Changing either changes
/// answers, so bump it with any such change; it is part of every cache key.
pub(super) const CHECK_VERSION: u32 = 2;

const TRUNCATED: &str = "\n[… truncated]";

/// One semantic check, reported under its own `verify.` code.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Check {
    HintRevealsAnswer,
    ExplanationContradictsAnswer,
    MultipleDefensibleChoices,
    ImplausibleDistractor,
    AnnotationDoesNotExplain,
    AnnotationContradictsCode,
    HighlightUnexplained,
    ReferenceMismatch,
}

impl Check {
    pub(super) const ALL: [Self; 8] = [
        Self::HintRevealsAnswer,
        Self::ExplanationContradictsAnswer,
        Self::MultipleDefensibleChoices,
        Self::ImplausibleDistractor,
        Self::AnnotationDoesNotExplain,
        Self::AnnotationContradictsCode,
        Self::HighlightUnexplained,
        Self::ReferenceMismatch,
    ];

    pub(super) const fn code(self) -> &'static str {
        match self {
            Self::HintRevealsAnswer => "verify.hint_reveals_answer",
            Self::ExplanationContradictsAnswer => "verify.explanation_contradicts_answer",
            Self::MultipleDefensibleChoices => "verify.multiple_defensible_choices",
            Self::ImplausibleDistractor => "verify.implausible_distractor",
            Self::AnnotationDoesNotExplain => "verify.annotation_does_not_explain",
            Self::AnnotationContradictsCode => "verify.annotation_contradicts_code",
            Self::HighlightUnexplained => "verify.highlight_unexplained",
            Self::ReferenceMismatch => "verify.reference_mismatch",
        }
    }

    pub(super) fn from_code(code: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|check| check.code() == code)
    }

    pub(super) const fn suggestion(self) -> &'static str {
        match self {
            Self::HintRevealsAnswer => {
                "Rewrite the hint to point at the relevant code or reasoning instead of the answer."
            }
            Self::ExplanationContradictsAnswer => {
                "Check which choice is actually correct, then align the explanation or the `correct` marker."
            }
            Self::MultipleDefensibleChoices => {
                "Tighten the prompt or rewrite this choice so exactly one answer is defensible."
            }
            Self::ImplausibleDistractor => {
                "Replace it with a realistic misconception, nearby API, or believable consequence."
            }
            Self::AnnotationDoesNotExplain => {
                "Say what these lines do and why the learner should look at them."
            }
            Self::AnnotationContradictsCode => {
                "Correct the annotation, or move the highlight to the lines it describes."
            }
            Self::HighlightUnexplained => {
                "Add an `annotation`, or explain the lines in the Markdown next to the block."
            }
            Self::ReferenceMismatch => {
                "Point the link at the block or lines it describes, or reword its text to match them."
            }
        }
    }
}

/// One yes/no question inside a request.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Question {
    /// Key in the request's `questions` object, such as `choice_1_implausible`.
    pub(super) key: String,
    pub(super) check: Check,
    /// Hint, choice, or highlight-group index the question is about; `0` for
    /// the block explanation.
    pub(super) item: usize,
    pub(super) definition: Value,
}

/// One request: a block under review, its lesson excerpt, and its questions.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Job {
    /// Index of the reviewed block in `lesson.json`'s `blocks`.
    pub(super) block_index: usize,
    pub(super) state: Value,
    pub(super) questions: Vec<Question>,
    /// For link jobs: the links asked about, in question order.
    pub(super) links: Vec<JobLink>,
}

/// A block link a `link_N_mismatch` question is about, with where to report it.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct JobLink {
    pub(super) field: MarkdownField,
    pub(super) start_char: usize,
    pub(super) end_char: usize,
    pub(super) text: String,
    pub(super) target_index: usize,
}

impl Job {
    /// The exact JSON body sent to TypeSafe for `model`.
    pub(super) fn request(&self, model: &str) -> Value {
        let questions = self
            .questions
            .iter()
            .map(|question| (question.key.clone(), question.definition.clone()))
            .collect::<serde_json::Map<_, _>>();
        json!({ "model": model, "state": self.state, "questions": questions })
    }
}

/// Plan every request for a compiled lesson, in block order.
pub(super) fn plan(lesson: &LoadedLesson, max_context_chars: usize) -> Vec<Job> {
    let nodes = &lesson.artifact.presentation.nodes;
    let title = &lesson.artifact.presentation.title;
    let mut jobs = Vec::new();
    for (index, (block, node)) in lesson.source.blocks.iter().zip(nodes).enumerate() {
        match (block, &node.content) {
            (Block::MultipleChoice(block), CompiledNodeContent::MultipleChoice { prompt, .. }) => {
                let (mut excerpt, unit, remaining) = unit_context(lesson, index, max_context_chars);
                excerpt.push(quiz_entry(block, prompt));
                let mut sources = unit;
                sources.push(index);
                add_linked_targets(lesson, &mut excerpt, &sources, remaining);
                jobs.push(Job {
                    block_index: index,
                    state: lesson_state(title, excerpt, block.id.as_str()),
                    questions: quiz_questions(block),
                    links: Vec::new(),
                });
            }
            (Block::Code(_), CompiledNodeContent::Code { highlights, .. })
                if !highlights.is_empty() =>
            {
                jobs.push(highlight_job(lesson, index, max_context_chars));
            }
            _ => {}
        }
        if let Some(job) = link_job(lesson, index, max_context_chars) {
            jobs.push(job);
        }
    }
    jobs
}

/// Links whose targets resolve, found in the Markdown fields of `blocks`.
fn resolved_links(
    lesson: &LoadedLesson,
    blocks: &[usize],
) -> Vec<(MarkdownField, LinkOccurrence, usize, Option<LinkedLines>)> {
    let nodes = &lesson.artifact.presentation.nodes;
    markdown_fields(&lesson.source, nodes)
        .into_iter()
        .filter(|field| blocks.contains(&field.block_index))
        .flat_map(|field| {
            find_links(&field.text)
                .into_iter()
                .filter_map(|link| {
                    let (node, lines) = target_of(
                        &lesson.artifact.presentation.links,
                        nodes,
                        &link.destination,
                    )?;
                    Some((field.clone(), link, node.node_id.get() as usize, lines))
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

/// A linked block as context: the target with only its linked lines for a
/// ranged code link, and only the prompt of a quiz, capped at `budget`.
fn linked_entry(node: &CompiledNode, lines: Option<LinkedLines>, budget: usize) -> (Value, usize) {
    match (&node.content, lines) {
        (CompiledNodeContent::MultipleChoice { prompt, .. }, _) => {
            let (prompt, used) = truncate(prompt, budget);
            (
                json!({ "id": node.source_id, "kind": "multiple_choice", "prompt": prompt }),
                used,
            )
        }
        (
            CompiledNodeContent::Code {
                content,
                first_line,
                ..
            },
            Some(lines),
        ) => {
            let first = first_line.unwrap_or(1);
            let slice = content
                .lines()
                .skip((lines.start - first) as usize)
                .take((lines.end - lines.start + 1) as usize)
                .collect::<Vec<_>>()
                .join("\n");
            let (mut entry, _) = context_entry(node, budget);
            let (slice, used) = truncate(&slice, budget);
            entry["content"] = json!(slice);
            entry["first_line"] = json!(lines.start);
            (entry, used)
        }
        (_, lines) => {
            let (mut entry, used) = context_entry(node, budget);
            if let Some(lines) = lines {
                entry["linked_lines"] = json!({ "start": lines.start, "end": lines.end });
            }
            (entry, used)
        }
    }
}

/// Add the targets of links found in `sources` that the excerpt lacks.
fn add_linked_targets(
    lesson: &LoadedLesson,
    excerpt: &mut Vec<Value>,
    sources: &[usize],
    mut remaining: usize,
) {
    let nodes = &lesson.artifact.presentation.nodes;
    for (_, _, target, lines) in resolved_links(lesson, sources) {
        let id = &nodes[target].source_id;
        if remaining == 0 || excerpt.iter().any(|entry| entry["id"] == id.as_str()) {
            continue;
        }
        let (entry, used) = linked_entry(&nodes[target], lines, remaining);
        remaining -= used.min(remaining);
        excerpt.push(entry);
    }
}

/// One request per block whose text contains block links: the block, each
/// linked target, and one `link_N_mismatch` question per link.
fn link_job(lesson: &LoadedLesson, index: usize, max_chars: usize) -> Option<Job> {
    if !lesson.source.schema_version.has_block_links() {
        return None;
    }
    let found = resolved_links(lesson, &[index]);
    if found.is_empty() {
        return None;
    }
    let nodes = &lesson.artifact.presentation.nodes;
    let node = &nodes[index];
    let (block_entry, used) = match (&lesson.source.blocks[index], &node.content) {
        (Block::MultipleChoice(block), CompiledNodeContent::MultipleChoice { prompt, .. }) => {
            (quiz_entry(block, prompt), 0)
        }
        _ => context_entry(node, max_chars),
    };
    let mut excerpt = vec![block_entry];
    let mut remaining = max_chars.saturating_sub(used);
    let mut links = Vec::new();
    let mut described = Vec::new();
    for (position, (field, link, target, lines)) in found.into_iter().enumerate() {
        let target_id = nodes[target].source_id.clone();
        if !excerpt
            .iter()
            .any(|entry| entry["id"] == target_id.as_str())
            && remaining > 0
        {
            let (entry, used) = linked_entry(&nodes[target], lines, remaining);
            remaining -= used.min(remaining);
            excerpt.push(entry);
        }
        described.push(json!({
            "id": format!("link_{position}"),
            "text": link.text,
            "destination": format!("#{}", link.destination),
            "target": target_id,
        }));
        links.push(JobLink {
            field,
            start_char: link.start_char,
            end_char: link.end_char,
            text: link.text,
            target_index: target,
        });
    }
    let mut state = lesson_state(
        &lesson.artifact.presentation.title,
        excerpt,
        &node.source_id,
    );
    state["links"] = json!(described);
    Some(Job {
        block_index: index,
        state,
        questions: (0..links.len()).map(link_mismatch).collect(),
        links,
    })
}

fn lesson_state(title: &str, excerpt: Vec<Value>, subject: &str) -> Value {
    json!({
        "lesson": { "title": title, "excerpt": excerpt },
        "subject": subject,
    })
}

/// Blocks of the question's teaching unit, in lesson order: the non-question
/// blocks back to the previous question. A run of adjacent questions shares
/// one unit, so a question right after another still gets the unit before
/// both. Blocks are kept nearest-first within `max_chars`; the block that
/// crosses the limit is truncated and older ones are dropped.
fn unit_context(
    lesson: &LoadedLesson,
    question_index: usize,
    max_chars: usize,
) -> (Vec<Value>, Vec<usize>, usize) {
    let blocks = &lesson.source.blocks;
    let mut cursor = question_index;
    while cursor > 0 && matches!(blocks[cursor - 1], Block::MultipleChoice(_)) {
        cursor -= 1;
    }
    let mut unit = Vec::new();
    while cursor > 0 && !matches!(blocks[cursor - 1], Block::MultipleChoice(_)) {
        cursor -= 1;
        unit.push(cursor);
    }

    let mut remaining = max_chars;
    let mut entries = Vec::new();
    for &index in &unit {
        if remaining == 0 {
            break;
        }
        let node = &lesson.artifact.presentation.nodes[index];
        let (entry, used) = context_entry(node, remaining);
        remaining -= used;
        entries.push(entry);
    }
    entries.reverse();
    (entries, unit, remaining)
}

/// A Markdown, code, or diff block as quiz context, with its content capped
/// at `budget` characters. Returns the entry and the characters it used.
fn context_entry(node: &CompiledNode, budget: usize) -> (Value, usize) {
    let id = node.source_id.as_str();
    match &node.content {
        CompiledNodeContent::Markdown { content, .. } => {
            let (content, used) = truncate(content, budget);
            (
                json!({ "id": id, "kind": "markdown", "content": content }),
                used,
            )
        }
        CompiledNodeContent::Code {
            content,
            language,
            caption,
            first_line,
            provenance,
            ..
        } => {
            let (content, used) = truncate(content, budget);
            let mut entry = json!({
                "id": id,
                "kind": "code",
                "language": language.as_str(),
                "content": content,
            });
            add_source(&mut entry, provenance, *first_line);
            if let Some(caption) = caption {
                entry["caption"] = json!(caption);
            }
            (entry, used)
        }
        CompiledNodeContent::Diff { diff, caption, .. } => {
            let (content, used) = truncate(&unified_diff(diff), budget);
            let mut entry = json!({ "id": id, "kind": "diff", "content": content });
            if let Some(caption) = caption {
                entry["caption"] = json!(caption);
            }
            (entry, used)
        }
        CompiledNodeContent::MultipleChoice { .. } => {
            unreachable!("teaching units stop at questions")
        }
    }
}

fn add_source(entry: &mut Value, provenance: &ResourceProvenance, first_line: Option<u32>) {
    match provenance {
        ResourceProvenance::File { path, .. } => entry["path"] = json!(path),
        ResourceProvenance::GitBlob { path, revision, .. } => {
            entry["path"] = json!(path);
            entry["revision"] = json!(revision);
        }
        ResourceProvenance::Inline { .. } | ResourceProvenance::GitDiff { .. } => {}
    }
    if let Some(first_line) = first_line {
        entry["first_line"] = json!(first_line);
    }
}

/// The quiz as authored: choices in source order (the compiled artifact
/// shuffles them), with the resolved prompt text.
fn quiz_entry(block: &MultipleChoiceBlock, prompt: &str) -> Value {
    let choices = block
        .choices
        .iter()
        .enumerate()
        .map(|(index, choice)| {
            let mut entry = json!({
                "id": format!("choice_{index}"),
                "content": choice.content,
                "correct": choice.correct,
            });
            if let Some(explanation) = &choice.explanation {
                entry["explanation"] = json!(explanation);
            }
            entry
        })
        .collect::<Vec<_>>();
    let hints = block
        .hints
        .iter()
        .enumerate()
        .map(|(index, hint)| json!({ "id": format!("hint_{index}"), "content": hint }))
        .collect::<Vec<_>>();
    json!({
        "id": block.id.as_str(),
        "kind": "multiple_choice",
        "prompt": prompt,
        "choices": choices,
        "hints": hints,
        "explanation": block.explanation,
    })
}

fn quiz_questions(block: &MultipleChoiceBlock) -> Vec<Question> {
    let mut questions = Vec::new();
    for index in 0..block.hints.len() {
        questions.push(hint_reveals_answer(index));
    }
    questions.push(explanation_contradicts_answer());
    for (index, choice) in block.choices.iter().enumerate() {
        if !choice.correct {
            questions.push(choice_defensible(index));
            questions.push(choice_implausible(index));
        }
    }
    questions
}

fn highlight_job(lesson: &LoadedLesson, index: usize, max_chars: usize) -> Job {
    let nodes = &lesson.artifact.presentation.nodes;
    let title = &lesson.artifact.presentation.title;
    let node = &nodes[index];
    let CompiledNodeContent::Code {
        content,
        language,
        caption,
        highlights,
        first_line,
        provenance,
    } = &node.content
    else {
        unreachable!("highlight jobs are planned for code nodes only")
    };
    let first_line = first_line.unwrap_or(1);
    let lines = content.lines().collect::<Vec<_>>();
    let mut code = json!({
        "id": node.source_id,
        "kind": "code",
        "language": language.as_str(),
        "caption": caption,
        "numbered_content": numbered(content, first_line),
        "highlights": highlights
            .iter()
            .enumerate()
            .map(|(group, highlight)| group_state(group, highlight, &lines, first_line))
            .collect::<Vec<_>>(),
    });
    add_source(&mut code, provenance, Some(first_line));

    let mut remaining = max_chars;
    let mut excerpt = Vec::new();
    if let Some((id, text)) = adjacent_markdown(nodes, index, true) {
        let (content, used) = truncate(text, remaining);
        remaining -= used;
        excerpt.push(json!({ "id": id, "kind": "markdown", "content": content }));
    }
    excerpt.push(code);
    if let Some((id, text)) = adjacent_markdown(nodes, index, false)
        && remaining > 0
    {
        let (content, used) = truncate(text, remaining);
        remaining -= used;
        excerpt.push(json!({ "id": id, "kind": "markdown", "content": content }));
    }
    let mut sources = vec![index];
    sources.extend(index.checked_sub(1));
    sources.push(index + 1);
    add_linked_targets(lesson, &mut excerpt, &sources, remaining);

    let questions = highlights
        .iter()
        .enumerate()
        .flat_map(|(group, highlight)| {
            if highlight.annotation.is_some() {
                vec![
                    annotation_does_not_explain(group),
                    annotation_contradicts_code(group),
                ]
            } else {
                vec![highlight_unexplained(group)]
            }
        })
        .collect();
    Job {
        block_index: index,
        state: lesson_state(title, excerpt, &node.source_id),
        questions,
        links: Vec::new(),
    }
}

/// Code with its source-file line number on every line, so the model never
/// has to count lines.
pub(super) fn numbered(content: &str, first_line: u32) -> String {
    let first = first_line as usize;
    let width = (first + content.lines().count().saturating_sub(1))
        .to_string()
        .len();
    content
        .lines()
        .enumerate()
        .map(|(offset, line)| format!("{:>width$} | {line}", first + offset))
        .collect::<Vec<_>>()
        .join("\n")
}

/// One highlight group: ranges in source-file lines, each with its text
/// verbatim, and the annotation (`null` when absent).
fn group_state(
    group: usize,
    highlight: &CompiledCodeHighlight,
    lines: &[&str],
    first_line: u32,
) -> Value {
    let ranges = highlight
        .lines
        .iter()
        .map(|range| {
            // Compiled ranges are one-based positions inside the displayed fragment.
            let text = lines[range.start as usize - 1..range.end as usize].join("\n");
            json!({
                "start": first_line + range.start - 1,
                "end": first_line + range.end - 1,
                "text": text,
            })
        })
        .collect::<Vec<_>>();
    json!({
        "group": group,
        "color": highlight.color,
        "ranges": ranges,
        "annotation": highlight.annotation,
    })
}

/// The Markdown block directly before or after `index`, or `None` when that
/// neighbour is anything else.
fn adjacent_markdown(nodes: &[CompiledNode], index: usize, before: bool) -> Option<(&str, &str)> {
    let neighbour = if before {
        index.checked_sub(1)?
    } else {
        index + 1
    };
    let node = nodes.get(neighbour)?;
    match &node.content {
        CompiledNodeContent::Markdown { content, .. } => Some((&node.source_id, content)),
        _ => None,
    }
}

/// Cap `text` at `budget` Unicode scalar values, marking a cut. Returns the
/// text and the characters of the budget it consumed.
fn truncate(text: &str, budget: usize) -> (String, usize) {
    let length = text.chars().count();
    if length <= budget {
        return (text.to_owned(), length);
    }
    let kept = text.chars().take(budget).collect::<String>();
    (kept + TRUNCATED, budget)
}

/// Structured diff data rendered back to unified-diff text.
pub(super) fn unified_diff(diff: &ResolvedDiff) -> String {
    let mut output = String::new();
    for file in &diff.files {
        let old = file
            .old_path
            .as_deref()
            .map_or_else(|| "/dev/null".to_owned(), |path| format!("a/{path}"));
        let new = file
            .new_path
            .as_deref()
            .map_or_else(|| "/dev/null".to_owned(), |path| format!("b/{path}"));
        output.push_str(&format!("--- {old}\n+++ {new}\n"));
        for hunk in &file.hunks {
            output.push_str(&format!(
                "@@ -{},{} +{},{} @@",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            if !hunk.heading.is_empty() {
                output.push(' ');
                output.push_str(&hunk.heading);
            }
            output.push('\n');
            for line in &hunk.lines {
                let prefix = match line.kind {
                    DiffLineKind::Context => ' ',
                    DiffLineKind::Addition => '+',
                    DiffLineKind::Deletion => '-',
                };
                output.push(prefix);
                output.push_str(&line.content);
                output.push('\n');
            }
        }
    }
    output
}

/// A yes/no question in TypeSafe's Choice format. "Yes" always means the
/// target has the problem the question describes.
fn yes_no(
    key: String,
    check: Check,
    item: usize,
    question: String,
    focus: String,
    yes: [&str; 2],
    no: [&str; 2],
) -> Question {
    Question {
        key,
        check,
        item,
        definition: json!({
            "type": "choice",
            "instructions": { "question": question, "focus": focus },
            "criteria": {
                "yes": { "what": yes[0], "not_for": yes[1] },
                "no": { "what": no[0], "not_for": no[1] },
            },
        }),
    }
}

fn hint_reveals_answer(index: usize) -> Question {
    let hint = format!("hint_{index}");
    yes_no(
        format!("{hint}_reveals_answer"),
        Check::HintRevealsAnswer,
        index,
        format!(
            "In the multiple-choice block named by `subject`, does hint `{hint}` give away the correct answer?"
        ),
        format!(
            "Judge `{hint}` only and ignore the other hints. Answer yes if a learner could pick the choice marked `correct: true` from `{hint}` alone, without the reasoning the lesson teaches. Yes means `{hint}` is a problem."
        ),
        [
            "The hint names the correct choice, quotes or paraphrases it, or implies it so directly that no reasoning is left. This is a problem.",
            "A hint that points at the relevant code, concept, or line of reasoning while leaving the learner to work out the answer.",
        ],
        [
            "The hint nudges toward the reasoning, but the learner still has to work out which choice is correct.",
            "A hint from which the correct choice can be read off directly.",
        ],
    )
}

fn explanation_contradicts_answer() -> Question {
    yes_no(
        "explanation_contradicts_answer".to_owned(),
        Check::ExplanationContradictsAnswer,
        0,
        "In the multiple-choice block named by `subject`, does the block's `explanation` argue that a choice other than the one marked `correct: true` is the right answer?".to_owned(),
        "Compare the block's `explanation` field with the choice marked `correct: true`. The `explanation` fields inside individual choices are not the block's explanation; ignore them here. Yes means the explanation and the correct marker disagree, which is a problem.".to_owned(),
        [
            "The explanation supports, describes, or concludes in favour of a different choice than the one marked correct. This is a problem.",
            "An explanation that mentions another choice only to say why it is wrong.",
        ],
        [
            "The explanation argues for the choice marked correct.",
            "An explanation whose reasoning actually leads to a different choice than the marked one.",
        ],
    )
}

fn choice_defensible(index: usize) -> Question {
    let choice = format!("choice_{index}");
    yes_no(
        format!("{choice}_defensible"),
        Check::MultipleDefensibleChoices,
        index,
        format!(
            "In the multiple-choice block named by `subject`, could a careful learner who understood the lesson excerpt reasonably defend `{choice}` as a correct answer to the prompt?"
        ),
        format!(
            "`{choice}` is a distractor: the author marked another choice as correct. Judge `{choice}` only, reading the prompt as literally written, together with the lesson excerpt. Yes means the question has more than one defensible answer, which is a problem."
        ),
        [
            "The choice is also correct, or correct under a reasonable reading of the prompt, so more than one answer is defensible. This is a problem.",
            "A distractor that is tempting but clearly wrong once the lesson's reasoning is applied.",
        ],
        [
            "The choice is wrong for a reason the lesson excerpt supports, so only the marked choice remains defensible.",
            "A distractor that is right under an ordinary reading of the prompt.",
        ],
    )
}

fn choice_implausible(index: usize) -> Question {
    let choice = format!("choice_{index}");
    yes_no(
        format!("{choice}_implausible"),
        Check::ImplausibleDistractor,
        index,
        format!(
            "In the multiple-choice block named by `subject`, is `{choice}` obviously wrong to someone who has not understood the material?"
        ),
        format!(
            "`{choice}` is a distractor. Judge `{choice}` only, as it reads next to the prompt and the other choices. Yes means it is a giveaway that anyone could eliminate, which is a problem."
        ),
        [
            "The choice can be dismissed without understanding the lesson: a joke, absurd, off-topic, grammatically mismatched with the prompt, or clearly different in kind from the other choices. This is a problem.",
            "A wrong answer that still takes understanding to reject, such as a real misconception, a nearby API, or a believable consequence.",
        ],
        [
            "The choice is a plausible distractor that a learner has to reason about to reject.",
            "A choice that anyone reading the prompt would eliminate immediately.",
        ],
    )
}

fn annotation_does_not_explain(group: usize) -> Question {
    yes_no(
        format!("group_{group}_annotation_does_not_explain"),
        Check::AnnotationDoesNotExplain,
        group,
        format!(
            "In the code block named by `subject`, is the annotation of highlight group {group} vague, a restatement of the highlighted code, or only a reference to its color or line numbers?"
        ),
        format!(
            "Judge the `annotation` of highlight group {group} against the `text` of its ranges. Yes means the annotation does not tell the learner what these lines do or why they matter, which is a problem."
        ),
        [
            "The annotation says nothing a learner could not read directly from the lines, stays vague (for example “the important part” or “see here”), or only names the color or line numbers. This is a problem.",
            "A short annotation that does say what the lines do or why they matter.",
        ],
        [
            "The annotation explains what the highlighted lines do or why the learner should look at them.",
            "An annotation that only repeats the code in words, or points at the lines without saying why.",
        ],
    )
}

fn annotation_contradicts_code(group: usize) -> Question {
    yes_no(
        format!("group_{group}_annotation_contradicts_code"),
        Check::AnnotationContradictsCode,
        group,
        format!(
            "In the code block named by `subject`, does the annotation of highlight group {group} describe behavior that its highlighted lines do not have?"
        ),
        format!(
            "Check every factual claim in the `annotation` of highlight group {group} against the `text` of its ranges and the surrounding `numbered_content`. Yes means the annotation is wrong about that code, which is a problem."
        ),
        [
            "The annotation is factually wrong about the highlighted lines: for example it names another function or variable, states the opposite condition, or describes an effect these lines do not produce. This is a problem.",
            "An annotation that is correct but incomplete or simplified, or that explains why the lines matter rather than what they do.",
        ],
        [
            "Every claim the annotation makes about the highlighted lines is true of that code.",
            "An annotation that describes lines other than the ones highlighted.",
        ],
    )
}

fn link_mismatch(index: usize) -> Question {
    let link = format!("link_{index}");
    yes_no(
        format!("{link}_mismatch"),
        Check::ReferenceMismatch,
        index,
        format!(
            "In the block named by `subject`, does link `{link}` point at content that fails to show what its link text says?"
        ),
        format!(
            "`{link}` is listed in `links`, with its text and `target`. Compare the text with the target block in the excerpt, or with its `content` when only some lines are linked. Yes means the link is wrong or stale, which is a problem."
        ),
        [
            "The linked block or lines do not contain what the link text names or describes: for example another function, a different struct, or lines that moved after a code change. This is a problem.",
            "A link whose target does show what the text names, even if it shows more around it.",
        ],
        [
            "The linked block or lines show what the link text names or describes.",
            "A target that only mentions the named thing in passing while the text promises its definition.",
        ],
    )
}

fn highlight_unexplained(group: usize) -> Question {
    yes_no(
        format!("group_{group}_highlight_unexplained"),
        Check::HighlightUnexplained,
        group,
        format!(
            "In the code block named by `subject`, is it unclear why the lines of highlight group {group} are highlighted, given the block's `caption` and the Markdown blocks right before and after it in the excerpt?"
        ),
        format!(
            "Highlight group {group} has no annotation. Look for the reason in the code block's `caption` and in the adjacent Markdown blocks of the excerpt. Yes means nothing there says why these lines matter, which is a problem."
        ),
        [
            "Neither the caption nor the adjacent Markdown explains what these lines do or why they matter, so the highlight directs attention without a reason. This is a problem.",
            "Lines whose role is stated in the caption or the adjacent Markdown, even without naming line numbers.",
        ],
        [
            "The caption or the adjacent Markdown makes clear why these lines are highlighted.",
            "Prose that only mentions the file or the block without explaining these lines.",
        ],
    )
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;
    use crate::compiler::CompileOptions;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct Root(PathBuf);

    impl Root {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "agent-teacher-verify-checks-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            Self(path)
        }

        fn load(&self, lesson: Value) -> LoadedLesson {
            let path = self.0.join("lesson.json");
            fs::write(&path, serde_json::to_string_pretty(&lesson).unwrap()).unwrap();
            crate::lint::load_lesson(&path, &CompileOptions::new(&self.0), "learnverify").unwrap()
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn markdown(id: &str, content: &str) -> Value {
        json!({"type":"markdown","id":id,"source":{"kind":"inline","content":content}})
    }

    fn quiz(id: &str) -> Value {
        json!({"type":"multiple_choice","id":id,
            "prompt":{"kind":"inline","content":"Which item leaves first?"},
            "choices":[
                {"content":"The oldest","correct":true},
                {"content":"The newest","explanation":"That is a stack."},
                {"content":"A random one"}
            ],
            "hints":["Think FIFO."],
            "explanation":"Queues remove the oldest item."})
    }

    fn excerpt_ids(job: &Job) -> Vec<&str> {
        job.state["lesson"]["excerpt"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| entry["id"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn quiz_state_is_a_lesson_excerpt_with_authored_choices() {
        let root = Root::new();
        let lesson = root.load(json!({"schema_version":"2.2.0","title":"Queues","blocks":[
            markdown("intro", "A queue is FIFO."),
            {"type":"code","id":"impl","language":"rust","source":{"kind":"inline","content":"q.pop_front()"}},
            quiz("order")
        ]}));
        let jobs = plan(&lesson, 6000);
        assert_eq!(jobs.len(), 1);
        let job = &jobs[0];
        assert_eq!(job.block_index, 2);
        assert_eq!(job.state["subject"], "order");
        assert_eq!(job.state["lesson"]["title"], "Queues");
        assert_eq!(excerpt_ids(job), ["intro", "impl", "order"]);
        let quiz = &job.state["lesson"]["excerpt"][2];
        assert_eq!(
            quiz["choices"][0],
            json!({"id":"choice_0","content":"The oldest","correct":true})
        );
        assert_eq!(quiz["choices"][1]["explanation"], "That is a stack.");
        assert_eq!(
            quiz["hints"],
            json!([{"id":"hint_0","content":"Think FIFO."}])
        );
        let keys = job
            .questions
            .iter()
            .map(|q| q.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "hint_0_reveals_answer",
                "explanation_contradicts_answer",
                "choice_1_defensible",
                "choice_1_implausible",
                "choice_2_defensible",
                "choice_2_implausible"
            ]
        );
        let request = job.request("jev-latest");
        assert_eq!(request["model"], "jev-latest");
        assert_eq!(
            request["questions"]["choice_2_implausible"]["criteria"]
                .as_object()
                .unwrap()
                .keys()
                .collect::<Vec<_>>(),
            ["no", "yes"]
        );
    }

    #[test]
    fn adjacent_questions_share_their_teaching_unit_and_hintless_quizzes_skip_hint_questions() {
        let root = Root::new();
        let mut hintless = quiz("second");
        hintless.as_object_mut().unwrap().remove("hints");
        let lesson = root.load(json!({"schema_version":"2.2.0","title":"T","blocks":[
            markdown("old", "Earlier unit."),
            quiz("first-unit"),
            markdown("new", "Current unit."),
            quiz("first"),
            hintless,
            markdown("later", "Next unit.")
        ]}));
        let jobs = plan(&lesson, 6000);
        assert_eq!(excerpt_ids(&jobs[1]), ["new", "first"]);
        assert_eq!(excerpt_ids(&jobs[2]), ["new", "second"]);
        assert!(
            jobs[2]
                .questions
                .iter()
                .all(|q| q.check != Check::HintRevealsAnswer)
        );
    }

    #[test]
    fn context_keeps_the_nearest_blocks_within_the_budget() {
        let root = Root::new();
        let lesson = root.load(json!({"schema_version":"2.2.0","title":"T","blocks":[
            markdown("far", "0123456789"),
            markdown("middle", "abcdefghij"),
            markdown("near", "ABCDEFGHIJ"),
            quiz("q")
        ]}));
        let job = &plan(&lesson, 15)[0];
        assert_eq!(excerpt_ids(job), ["middle", "near", "q"]);
        let excerpt = &job.state["lesson"]["excerpt"];
        assert_eq!(excerpt[1]["content"], "ABCDEFGHIJ");
        assert_eq!(excerpt[0]["content"], format!("abcde{TRUNCATED}"));
        let none = &plan(&lesson, 0)[0];
        assert_eq!(excerpt_ids(none), ["q"]);
    }

    #[test]
    fn highlight_state_numbers_lines_and_uses_strictly_adjacent_markdown() {
        let root = Root::new();
        fs::write(
            root.0.join("queue.rs"),
            (1..=12).map(|n| format!("line {n}\n")).collect::<String>(),
        )
        .unwrap();
        let lesson = root.load(json!({"schema_version":"2.2.0","title":"T","blocks":[
            markdown("before", "Look at the removal."),
            {"type":"code","id":"impl","caption":"Removal path.",
             "source":{"kind":"file","path":"queue.rs","lines":{"start":9,"end":11}},
             "highlights":[
                {"lines":[{"start":9,"end":9},{"start":11,"end":11}],"annotation":"Pops the **oldest** item."},
                {"lines":[{"start":10,"end":10}],"color":"blue"}
             ]},
            quiz("after-is-a-quiz")
        ]}));
        let jobs = plan(&lesson, 6000);
        let job = jobs.iter().find(|job| job.block_index == 1).unwrap();
        assert_eq!(job.state["subject"], "impl");
        assert_eq!(excerpt_ids(job), ["before", "impl"]);
        let code = &job.state["lesson"]["excerpt"][1];
        assert_eq!(code["path"], "queue.rs");
        assert_eq!(code["first_line"], 9);
        assert_eq!(code["caption"], "Removal path.");
        assert_eq!(
            code["numbered_content"],
            " 9 | line 9\n10 | line 10\n11 | line 11"
        );
        assert_eq!(
            code["highlights"][0]["ranges"],
            json!([{"start":9,"end":9,"text":"line 9"},{"start":11,"end":11,"text":"line 11"}])
        );
        assert_eq!(code["highlights"][1]["annotation"], Value::Null);
        assert_eq!(code["highlights"][1]["color"], "blue");
        let keys = job
            .questions
            .iter()
            .map(|q| q.key.as_str())
            .collect::<Vec<_>>();
        assert_eq!(
            keys,
            [
                "group_0_annotation_does_not_explain",
                "group_0_annotation_contradicts_code",
                "group_1_highlight_unexplained"
            ]
        );
    }

    #[test]
    fn links_get_their_own_request_and_join_quiz_context() {
        let root = Root::new();
        let lesson = root.load(json!({"schema_version":"2.3.0","title":"Links","blocks":[
            {"type":"code","id":"queue-def","language":"rust","source":{"kind":"inline","content":"// queue\nstruct Queue {\n    items: Vec<u32>,\n}"}},
            quiz("earlier"),
            markdown("far", "Far away."),
            markdown("uses", "It stores a [`Queue`](#queue-def:2-4)."),
            quiz("order")
        ]}));
        let jobs = plan(&lesson, 6000);
        let link_job = jobs
            .iter()
            .find(|job| !job.links.is_empty())
            .expect("a link job");
        assert_eq!(link_job.block_index, 3);
        assert_eq!(link_job.state["subject"], "uses");
        assert_eq!(
            link_job.state["links"],
            json!([{"id":"link_0","text":"Queue","destination":"#queue-def:2-4","target":"queue-def"}])
        );
        assert_eq!(excerpt_ids(link_job), ["uses", "queue-def"]);
        let target = &link_job.state["lesson"]["excerpt"][1];
        assert_eq!(target["content"], "struct Queue {\n    items: Vec<u32>,\n}");
        assert_eq!(target["first_line"], 2);
        assert_eq!(link_job.questions[0].key, "link_0_mismatch");
        assert_eq!(link_job.questions[0].check, Check::ReferenceMismatch);

        // The quiz's unit is `far` and `uses`; the linked definition joins it.
        let quiz_job = jobs
            .iter()
            .find(|job| job.state["subject"] == "order")
            .unwrap();
        assert_eq!(excerpt_ids(quiz_job), ["far", "uses", "order", "queue-def"]);

        // Older schemas plan no link jobs.
        let older = root.load(json!({"schema_version":"2.2.0","title":"Links","blocks":[
            markdown("uses", "[x](#anything)")
        ]}));
        assert!(plan(&older, 6000).is_empty());
    }

    #[test]
    fn numbering_pads_to_the_widest_line_number() {
        assert_eq!(numbered("a\nb", 99), " 99 | a\n100 | b");
        assert_eq!(numbered("only", 1), "1 | only");
    }

    #[test]
    fn diffs_render_back_to_unified_text() {
        let diff = crate::repository::parse_unified_diff(
            "diff --git a/q.rs b/q.rs\n--- a/q.rs\n+++ b/q.rs\n@@ -1,2 +1,2 @@ fn main\n keep\n-old\n+new\n",
        )
        .unwrap();
        assert_eq!(
            unified_diff(&diff),
            "--- a/q.rs\n+++ b/q.rs\n@@ -1,2 +1,2 @@ fn main\n keep\n-old\n+new\n"
        );
    }

    #[test]
    fn every_question_says_that_yes_is_the_problem() {
        let questions = [
            hint_reveals_answer(0),
            explanation_contradicts_answer(),
            choice_defensible(1),
            choice_implausible(1),
            annotation_does_not_explain(0),
            annotation_contradicts_code(0),
            highlight_unexplained(0),
            link_mismatch(0),
        ];
        for question in questions {
            let definition = &question.definition;
            assert!(
                definition["instructions"]["focus"]
                    .as_str()
                    .unwrap()
                    .contains("Yes means")
            );
            assert!(
                definition["criteria"]["yes"]["what"]
                    .as_str()
                    .unwrap()
                    .ends_with("This is a problem."),
                "{}",
                question.key
            );
        }
        for check in Check::ALL {
            assert_eq!(Check::from_code(check.code()), Some(check));
        }
    }
}
