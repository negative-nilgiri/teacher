//! Turns answers into findings located at the editable source span.

use serde::Serialize;

use super::checks::{Check, Job, Question};
use super::client::{Answers, VerifyError};
use super::config::VerifyConfig;
use crate::lint::{
    LintDiagnostic, LoadedLesson, RelatedLintLocation, Severity, SourceLocation, prompt_pointer,
    write_finding_text,
};
use crate::source::{Block, CodeBlock, LineRange, MarkdownSource, MultipleChoiceBlock};

pub(super) const UNAVAILABLE: &str = "verify.unavailable";

/// A lint-shaped finding plus the probability and model behind it. Both extra
/// fields are omitted on `verify.unavailable`, which is a status report.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VerifyDiagnostic {
    #[serde(flatten)]
    pub finding: LintDiagnostic,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probability: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
}

impl VerifyDiagnostic {
    fn is_unavailable(&self) -> bool {
        self.finding.code == UNAVAILABLE
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct VerifyReport {
    pub diagnostics: Vec<VerifyDiagnostic>,
}

impl VerifyReport {
    /// Filter, then compute fatality, as lint does. `verify.unavailable` is
    /// never filtered and never fatal: a skipped run must stay visible and
    /// must not fail the caller.
    pub fn from_findings(
        findings: Vec<VerifyDiagnostic>,
        ignore_below: Option<Severity>,
        warning_as_error: Severity,
    ) -> Self {
        let diagnostics = findings
            .into_iter()
            .filter(|finding| {
                finding.is_unavailable()
                    || ignore_below
                        .is_none_or(|floor| finding.finding.severity.rank() >= floor.rank())
            })
            .map(|mut finding| {
                finding.finding.fatal = !finding.is_unavailable()
                    && finding.finding.severity.rank() >= warning_as_error.rank();
                finding
            })
            .collect();
        Self { diagnostics }
    }

    pub fn is_fatal(&self) -> bool {
        self.diagnostics.iter().any(|finding| finding.finding.fatal)
    }

    pub fn text(&self) -> String {
        let mut output = String::new();
        for finding in &self.diagnostics {
            let note = match (finding.probability, &finding.model) {
                (Some(probability), Some(model)) => {
                    Some(format!("probability {probability:.2} from {model}"))
                }
                _ => None,
            };
            write_finding_text(&mut output, &finding.finding, note.as_deref());
        }
        output
    }
}

/// Grade every job's outcome, in block order, then add one
/// `verify.unavailable` for the blocks that could not be checked.
pub(super) fn findings(
    lesson: &LoadedLesson,
    jobs: &[Job],
    outcomes: &[Result<Answers, VerifyError>],
    config: &VerifyConfig,
) -> Vec<VerifyDiagnostic> {
    let mut findings = Vec::new();
    let mut failures = Vec::new();
    for (job, outcome) in jobs.iter().zip(outcomes) {
        let answers = match outcome {
            Ok(answers) => answers,
            Err(error) => {
                failures.push((job.block_index, error));
                continue;
            }
        };
        for question in &job.questions {
            let probability = answers.yes(&question.key);
            let Some(severity) = severity(question.check, probability, config) else {
                continue;
            };
            if config
                .ignore_codes
                .iter()
                .any(|code| code == question.check.code())
            {
                continue;
            }
            findings.push(VerifyDiagnostic {
                finding: locate(lesson, job.block_index, question, severity),
                probability: Some(probability),
                model: Some(answers.model.clone()),
            });
        }
    }
    if !failures.is_empty() {
        findings.push(unavailable(lesson, &failures, jobs.len()));
    }
    findings
}

fn severity(check: Check, probability: f64, config: &VerifyConfig) -> Option<Severity> {
    let warning = if check == Check::AnnotationContradictsCode {
        config.min_contradiction_warning_probability
    } else {
        config.min_warning_probability
    };
    if probability >= warning {
        Some(Severity::Warning)
    } else if probability >= config.min_info_probability {
        Some(Severity::Info)
    } else {
        None
    }
}

fn locate(
    lesson: &LoadedLesson,
    index: usize,
    question: &Question,
    severity: Severity,
) -> LintDiagnostic {
    let base = format!("/blocks/{index}");
    let item = question.item;
    let block = &lesson.source.blocks[index];
    let (pointer, message, related) = match (block, question.check) {
        (Block::MultipleChoice(quiz), Check::HintRevealsAnswer) => (
            format!("{base}/hints/{item}"),
            format!(
                "hint {} may give away the correct answer",
                quote(&quiz.hints[item])
            ),
            vec![correct_choice(lesson, index, quiz)],
        ),
        (Block::MultipleChoice(quiz), Check::ExplanationContradictsAnswer) => (
            format!("{base}/explanation"),
            "the explanation may argue for a choice other than the one marked correct".to_owned(),
            vec![correct_choice(lesson, index, quiz)],
        ),
        (Block::MultipleChoice(quiz), Check::MultipleDefensibleChoices) => (
            format!("{base}/choices/{item}/content"),
            format!(
                "distractor {} could be defended as a correct answer",
                quote(&quiz.choices[item].content)
            ),
            vec![prompt(lesson, index, quiz)],
        ),
        (Block::MultipleChoice(quiz), Check::ImplausibleDistractor) => (
            format!("{base}/choices/{item}/content"),
            format!(
                "distractor {} may be obviously wrong without understanding the material",
                quote(&quiz.choices[item].content)
            ),
            Vec::new(),
        ),
        (Block::Code(code), Check::AnnotationDoesNotExplain) => (
            format!("{base}/highlights/{item}/annotation"),
            format!(
                "the annotation of highlight group {item} (lines {}) may not explain what those lines do or why they matter",
                ranges(code, item)
            ),
            Vec::new(),
        ),
        (Block::Code(code), Check::AnnotationContradictsCode) => (
            format!("{base}/highlights/{item}/annotation"),
            format!(
                "the annotation of highlight group {item} may describe behavior that lines {} do not have",
                ranges(code, item)
            ),
            vec![RelatedLintLocation {
                message: "the highlighted lines".to_owned(),
                location: lesson
                    .spans
                    .location(&format!("{base}/highlights/{item}/lines")),
            }],
        ),
        (Block::Code(code), Check::HighlightUnexplained) => (
            format!("{base}/highlights/{item}/lines"),
            format!(
                "nothing near this code block says why lines {} (highlight group {item}) matter",
                ranges(code, item)
            ),
            Vec::new(),
        ),
        _ => unreachable!("questions are planned for their own block kind"),
    };
    let mut finding = LintDiagnostic::new(
        question.check.code(),
        severity,
        message,
        lesson.spans.location(&pointer),
        Some(block.id().as_str()),
        pointer,
        question.check.suggestion(),
    );
    finding.related = related;
    finding
}

fn correct_choice(
    lesson: &LoadedLesson,
    index: usize,
    quiz: &MultipleChoiceBlock,
) -> RelatedLintLocation {
    let correct = quiz
        .choices
        .iter()
        .position(|choice| choice.correct)
        .expect("validated questions have one correct choice");
    RelatedLintLocation {
        message: "the choice marked correct".to_owned(),
        location: lesson
            .spans
            .location(&format!("/blocks/{index}/choices/{correct}/content")),
    }
}

fn prompt(lesson: &LoadedLesson, index: usize, quiz: &MultipleChoiceBlock) -> RelatedLintLocation {
    let location = match &quiz.prompt {
        MarkdownSource::Inline { .. } => lesson
            .spans
            .location(&prompt_pointer(lesson.source.schema_version, index)),
        MarkdownSource::File { path } => {
            SourceLocation::file_line(lesson.root.join(path.as_str()), 1, 1, 1)
        }
    };
    RelatedLintLocation {
        message: "the prompt".to_owned(),
        location,
    }
}

/// The group's ranges as authored, in source-file lines: `41–43, 47`.
fn ranges(code: &CodeBlock, group: usize) -> String {
    code.highlights[group]
        .lines
        .iter()
        .map(|LineRange { start, end }| {
            if start == end {
                start.to_string()
            } else {
                format!("{start}–{end}")
            }
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// A short, single-line quotation of Markdown for a message.
fn quote(text: &str) -> String {
    const MAX: usize = 60;
    let flat = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= MAX {
        format!("“{flat}”")
    } else {
        format!("“{}…”", flat.chars().take(MAX).collect::<String>())
    }
}

fn unavailable(
    lesson: &LoadedLesson,
    failures: &[(usize, &VerifyError)],
    total: usize,
) -> VerifyDiagnostic {
    let mut reasons = Vec::<String>::new();
    for (_, error) in failures {
        let reason = error.to_string();
        if !reasons.contains(&reason) {
            reasons.push(reason);
        }
    }
    let reasons = reasons.join("; ");
    let message = if failures.len() == total {
        format!("semantic checks skipped: {reasons}")
    } else {
        format!(
            "semantic checks skipped for {} of {total} blocks: {reasons}",
            failures.len()
        )
    };
    let suggestion = if failures
        .iter()
        .any(|(_, error)| matches!(error, VerifyError::MissingApiKey))
    {
        "Set TYPESAFE_API_KEY to run the semantic checks; lint results are unaffected."
    } else {
        "Continue without these semantic checks, or rerun learnverify later."
    };
    let mut finding = LintDiagnostic::new(
        UNAVAILABLE,
        Severity::Info,
        message,
        lesson.spans.location(""),
        None,
        "",
        suggestion,
    );
    finding.related = failures
        .iter()
        .map(|(index, error)| RelatedLintLocation {
            message: format!(
                "`{}` was not checked: {error}",
                lesson.source.blocks[*index].id().as_str()
            ),
            location: lesson.spans.location(&format!("/blocks/{index}")),
        })
        .collect();
    VerifyDiagnostic {
        finding,
        probability: None,
        model: None,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    use serde_json::json;

    use super::*;
    use crate::compiler::CompileOptions;
    use crate::learnverify::checks::plan;

    static NEXT_DIR: AtomicU64 = AtomicU64::new(0);

    struct Fixture {
        root: PathBuf,
        lesson: LoadedLesson,
        jobs: Vec<Job>,
    }

    impl Fixture {
        fn new(schema: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "agent-teacher-verify-grade-{}-{}",
                std::process::id(),
                NEXT_DIR.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            fs::write(root.join("q.rs"), "fn a() {}\nfn b() {}\nfn c() {}\n").unwrap();
            let prompt = if schema == "1.0.0" {
                json!("Which comes first?")
            } else {
                json!({"kind":"inline","content":"Which comes first?"})
            };
            let mut blocks = vec![json!({"type":"multiple_choice","id":"quiz","prompt":prompt,
                "choices":[{"content":"`a`","correct":true},{"content":"`b`"}],
                "hints":["It is `a`."],"explanation":"`a` is defined first."})];
            if schema != "1.0.0" {
                blocks.push(json!({"type":"code","id":"code","source":{"kind":"file","path":"q.rs"},
                    "highlights":[{"lines":[{"start":1,"end":2}],"annotation":"Defines `a` and `b`."},
                                  {"lines":[{"start":3,"end":3}],"color":"blue"}]}));
            }
            let lesson_json = json!({"schema_version":schema,"title":"T","blocks":blocks});
            let path = root.join("lesson.json");
            fs::write(&path, serde_json::to_string_pretty(&lesson_json).unwrap()).unwrap();
            let lesson =
                crate::lint::load_lesson(&path, &CompileOptions::new(&root), "learnverify")
                    .unwrap();
            let jobs = plan(&lesson, 6000);
            Self { root, lesson, jobs }
        }

        /// Answer every question with the same P(yes), except overrides.
        fn answers(&self, job: usize, yes: f64, overrides: &[(&str, f64)]) -> Answers {
            let probabilities = self.jobs[job]
                .questions
                .iter()
                .map(|question| {
                    let p = overrides
                        .iter()
                        .find(|(key, _)| *key == question.key)
                        .map_or(yes, |(_, p)| *p);
                    (
                        question.key.clone(),
                        BTreeMap::from([("yes".to_owned(), p), ("no".to_owned(), 1.0 - p)]),
                    )
                })
                .collect();
            Answers {
                model: "jev-1.13.0".into(),
                probabilities,
            }
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    fn codes(findings: &[VerifyDiagnostic]) -> Vec<(&str, Severity)> {
        findings
            .iter()
            .map(|f| (f.finding.code.as_str(), f.finding.severity))
            .collect()
    }

    #[test]
    fn probabilities_map_to_severities_with_a_lower_contradiction_threshold() {
        let fixture = Fixture::new("2.2.0");
        let outcomes = vec![
            Ok(fixture.answers(
                0,
                0.10,
                &[
                    ("hint_0_reveals_answer", 0.85),
                    ("choice_1_implausible", 0.60),
                ],
            )),
            Ok(fixture.answers(
                1,
                0.59,
                &[
                    ("group_0_annotation_contradicts_code", 0.70),
                    ("group_1_highlight_unexplained", 0.84),
                ],
            )),
        ];
        let found = findings(
            &fixture.lesson,
            &fixture.jobs,
            &outcomes,
            &VerifyConfig::default(),
        );
        assert_eq!(
            codes(&found),
            [
                ("verify.hint_reveals_answer", Severity::Warning),
                ("verify.implausible_distractor", Severity::Info),
                ("verify.annotation_contradicts_code", Severity::Warning),
                ("verify.highlight_unexplained", Severity::Info),
            ]
        );
        let hint = &found[0];
        assert_eq!(hint.finding.pointer, "/blocks/0/hints/0");
        assert_eq!(hint.finding.block_id.as_deref(), Some("quiz"));
        assert_eq!(
            hint.finding.message,
            "hint “It is `a`.” may give away the correct answer"
        );
        assert_eq!(hint.finding.related[0].message, "the choice marked correct");
        assert_eq!(hint.probability, Some(0.85));
        assert_eq!(hint.model.as_deref(), Some("jev-1.13.0"));
        assert_eq!(found[1].finding.pointer, "/blocks/0/choices/1/content");
        assert_eq!(
            found[2].finding.pointer,
            "/blocks/1/highlights/0/annotation"
        );
        assert_eq!(found[2].finding.related[0].message, "the highlighted lines");
        assert_eq!(found[3].finding.pointer, "/blocks/1/highlights/1/lines");
        assert!(
            found[3]
                .finding
                .message
                .contains("lines 3 (highlight group 1)")
        );

        let value = serde_json::to_value(hint).unwrap();
        for field in [
            "code",
            "severity",
            "fatal",
            "message",
            "location",
            "block_id",
            "pointer",
            "suggestion",
            "related",
            "probability",
            "model",
        ] {
            assert!(value.get(field).is_some(), "{field}");
        }
    }

    #[test]
    fn legacy_prompts_and_ignore_codes() {
        let fixture = Fixture::new("1.0.0");
        let outcomes = vec![Ok(fixture.answers(0, 0.0, &[("choice_1_defensible", 0.9)]))];
        let found = findings(
            &fixture.lesson,
            &fixture.jobs,
            &outcomes,
            &VerifyConfig::default(),
        );
        assert_eq!(
            codes(&found),
            [("verify.multiple_defensible_choices", Severity::Warning)]
        );
        let prompt = &found[0].finding.related[0];
        assert_eq!(prompt.message, "the prompt");
        assert_eq!(
            prompt.location,
            fixture.lesson.spans.location("/blocks/0/prompt")
        );

        let ignoring = VerifyConfig {
            ignore_codes: vec!["verify.multiple_defensible_choices".into()],
            ..VerifyConfig::default()
        };
        assert!(findings(&fixture.lesson, &fixture.jobs, &outcomes, &ignoring).is_empty());
    }

    #[test]
    fn failures_become_one_unfilterable_unavailable_finding() {
        let fixture = Fixture::new("2.2.0");
        let outcomes = vec![
            Ok(fixture.answers(0, 0.9, &[])),
            Err(VerifyError::Api { status: 503 }),
        ];
        let found = findings(
            &fixture.lesson,
            &fixture.jobs,
            &outcomes,
            &VerifyConfig::default(),
        );
        let unavailable = found.last().unwrap();
        assert_eq!(unavailable.finding.code, UNAVAILABLE);
        assert_eq!(
            unavailable.finding.message,
            "semantic checks skipped for 1 of 2 blocks: TypeSafe API returned HTTP 503"
        );
        assert_eq!(unavailable.finding.block_id, None);
        assert_eq!(unavailable.finding.pointer, "");
        assert_eq!(
            unavailable.finding.related[0].message,
            "`code` was not checked: TypeSafe API returned HTTP 503"
        );
        let value = serde_json::to_value(unavailable).unwrap();
        assert!(value.get("probability").is_none() && value.get("model").is_none());

        let report = VerifyReport::from_findings(found, Some(Severity::Warning), Severity::Info);
        assert!(
            report
                .diagnostics
                .iter()
                .all(|f| f.finding.severity == Severity::Warning || f.finding.code == UNAVAILABLE)
        );
        assert!(
            report
                .diagnostics
                .iter()
                .any(|f| f.finding.code == UNAVAILABLE && !f.finding.fatal)
        );
        assert!(report.is_fatal());

        let all_failed = vec![
            Err(VerifyError::MissingApiKey),
            Err(VerifyError::MissingApiKey),
        ];
        let found = findings(
            &fixture.lesson,
            &fixture.jobs,
            &all_failed,
            &VerifyConfig::default(),
        );
        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].finding.message,
            "semantic checks skipped: TYPESAFE_API_KEY is not set or is empty"
        );
        assert!(found[0].finding.suggestion.contains("TYPESAFE_API_KEY"));
        let report = VerifyReport::from_findings(found, None, Severity::Info);
        assert!(!report.is_fatal());
        let text = report.text();
        assert!(text.starts_with("info[verify.unavailable]: semantic checks skipped"));
        assert!(!text.contains("= note:"));
    }

    #[test]
    fn text_output_carries_the_probability_note() {
        let fixture = Fixture::new("2.2.0");
        let outcomes = vec![
            Ok(fixture.answers(0, 0.0, &[("explanation_contradicts_answer", 0.912)])),
            Ok(fixture.answers(1, 0.0, &[])),
        ];
        let found = findings(
            &fixture.lesson,
            &fixture.jobs,
            &outcomes,
            &VerifyConfig::default(),
        );
        let text = VerifyReport::from_findings(found, None, Severity::Error).text();
        assert!(text.contains("warning[verify.explanation_contradicts_answer]"));
        assert!(text.contains("= note: probability 0.91 from jev-1.13.0"));
    }

    #[test]
    fn quotes_are_single_line_and_short() {
        assert_eq!(quote("a\n  b"), "“a b”");
        assert_eq!(quote(&"x".repeat(70)), format!("“{}…”", "x".repeat(60)));
    }
}
