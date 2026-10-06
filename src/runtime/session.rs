use std::collections::{BTreeMap, BTreeSet};

use crate::artifact::ChoiceId;
use crate::source::NodeId;

use super::model::{Attempt, LessonProgress, QuestionState, RunStatus, RuntimeLesson};
use super::runner::{RunResult, RunSpec};

#[derive(Clone, Debug)]
pub(crate) struct Session {
    questions: BTreeMap<NodeId, QuestionState>,
    total_questions: usize,
    /// The secret a run request must carry; `None` when running is not enabled.
    run_token: Option<String>,
    /// Run blocks with a run under way, so each runs once at a time.
    running: BTreeSet<NodeId>,
    /// The last result of each run block. Never part of quiz progress.
    runs: BTreeMap<NodeId, RunResult>,
}

impl Session {
    pub fn new(lesson: &RuntimeLesson, run_token: Option<String>) -> Self {
        Self {
            questions: BTreeMap::new(),
            total_questions: lesson.answers.len(),
            run_token,
            running: BTreeSet::new(),
            runs: BTreeMap::new(),
        }
    }

    pub fn progress(&self) -> LessonProgress {
        LessonProgress {
            completed_questions: self
                .questions
                .values()
                .filter(|question| question.completed)
                .count(),
            total_questions: self.total_questions,
            questions: self.questions.clone(),
        }
    }

    pub fn submit(
        &mut self,
        lesson: &RuntimeLesson,
        node_id: NodeId,
        choice_id: ChoiceId,
    ) -> Result<QuestionState, SessionError> {
        let answer = lesson
            .answers
            .get(&node_id)
            .ok_or(SessionError::QuestionNotFound { node_id })?;
        if !answer.valid_choices.contains(&choice_id) {
            return Err(SessionError::ChoiceNotFound { node_id, choice_id });
        }

        let correct = choice_id == answer.correct_choice_id;
        let question = self.questions.entry(node_id).or_default();
        question.attempts.push(Attempt { choice_id, correct });
        if correct {
            question.completed = true;
            question.answer = Some(answer.revealed());
        }
        Ok(question.clone())
    }

    pub fn reveal(
        &mut self,
        lesson: &RuntimeLesson,
        node_id: NodeId,
    ) -> Result<QuestionState, SessionError> {
        let answer = lesson
            .answers
            .get(&node_id)
            .ok_or(SessionError::QuestionNotFound { node_id })?;
        let question = self.questions.entry(node_id).or_default();
        question.revealed = true;
        question.answer = Some(answer.revealed());
        Ok(question.clone())
    }

    pub fn run_status(&self) -> RunStatus {
        RunStatus {
            enabled: self.run_token.is_some(),
            token: self.run_token.clone(),
        }
    }

    pub fn run_results(&self) -> BTreeMap<NodeId, RunResult> {
        self.runs.clone()
    }

    /// Admit one run of a block and hand back what to execute. The caller runs
    /// it without holding the session and then calls `finish_run`.
    pub fn begin_run(
        &mut self,
        lesson: &RuntimeLesson,
        node_id: NodeId,
        token: Option<&str>,
    ) -> Result<RunSpec, SessionError> {
        let expected = self.run_token.as_deref().ok_or(SessionError::RunDisabled)?;
        if !token.is_some_and(|token| same_token(token, expected)) {
            return Err(SessionError::InvalidToken);
        }
        let spec = match lesson.runs.get(&node_id) {
            Some(spec) => spec,
            None if lesson
                .public
                .nodes
                .iter()
                .any(|node| node.node_id == node_id) =>
            {
                return Err(SessionError::NotRunnable { node_id });
            }
            None => return Err(SessionError::UnknownNode { node_id }),
        };
        if !self.running.insert(node_id) {
            return Err(SessionError::RunInProgress { node_id });
        }
        Ok(spec.clone())
    }

    /// Record the outcome of an admitted run, replacing the block's last one.
    pub fn finish_run(&mut self, node_id: NodeId, result: RunResult) {
        self.running.remove(&node_id);
        self.runs.insert(node_id, result);
    }
}

/// Compares in time that depends only on the length, not on where the first
/// difference is.
fn same_token(given: &str, expected: &str) -> bool {
    given.len() == expected.len()
        && given
            .bytes()
            .zip(expected.bytes())
            .fold(0, |difference, (given, expected)| {
                difference | (given ^ expected)
            })
            == 0
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum SessionError {
    QuestionNotFound {
        node_id: NodeId,
    },
    ChoiceNotFound {
        node_id: NodeId,
        choice_id: ChoiceId,
    },
    RunDisabled,
    InvalidToken,
    UnknownNode {
        node_id: NodeId,
    },
    NotRunnable {
        node_id: NodeId,
    },
    RunInProgress {
        node_id: NodeId,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::model::{
        project_runtime_lesson,
        tests::{quiz_artifact, run_artifact},
    };

    #[test]
    fn incorrect_attempts_allow_a_retry_without_revealing_the_answer() {
        let lesson = project_runtime_lesson(&quiz_artifact());
        let mut session = Session::new(&lesson, None);

        let wrong = session
            .submit(&lesson, NodeId::new(0), ChoiceId::new(0))
            .unwrap();
        assert!(!wrong.completed);
        assert!(wrong.answer.is_none());
        assert_eq!(session.progress().completed_questions, 0);

        let correct = session
            .submit(&lesson, NodeId::new(0), ChoiceId::new(1))
            .unwrap();
        assert!(correct.completed);
        assert_eq!(correct.attempts.len(), 2);
        let answer = correct.answer.unwrap();
        assert_eq!(answer.choice_id, ChoiceId::new(1));
        // Distractor explanations arrive with the answer, never after a miss.
        assert_eq!(answer.choice_explanations[0].choice_id, ChoiceId::new(0));
        assert_eq!(session.progress().completed_questions, 1);
    }

    #[test]
    fn reveal_is_recorded_but_does_not_claim_a_correct_completion() {
        let lesson = project_runtime_lesson(&quiz_artifact());
        let mut session = Session::new(&lesson, None);
        let revealed = session.reveal(&lesson, NodeId::new(0)).unwrap();
        assert!(revealed.revealed);
        assert!(!revealed.completed);
        assert_eq!(
            revealed.answer.unwrap().choice_explanations[0].explanation,
            "No ignores the question"
        );
        assert_eq!(session.progress().completed_questions, 0);
    }

    #[test]
    fn invalid_choices_do_not_create_attempts() {
        let lesson = project_runtime_lesson(&quiz_artifact());
        let mut session = Session::new(&lesson, None);
        assert!(matches!(
            session.submit(&lesson, NodeId::new(0), ChoiceId::new(99)),
            Err(SessionError::ChoiceNotFound { .. })
        ));
        assert!(session.progress().questions.is_empty());
    }

    fn run_session(token: Option<&str>) -> (RuntimeLesson, Session) {
        let lesson = project_runtime_lesson(&run_artifact());
        let session = Session::new(&lesson, token.map(str::to_owned));
        (lesson, session)
    }

    fn finished() -> RunResult {
        RunResult {
            stdout: "hi\n".into(),
            ..RunResult::failed("")
        }
    }

    #[test]
    fn running_is_off_without_a_token() {
        let (lesson, mut session) = run_session(None);
        assert_eq!(
            session.run_status(),
            RunStatus {
                enabled: false,
                token: None
            }
        );
        // Even a request that guesses a token gets nothing.
        for token in [None, Some(""), Some("anything")] {
            assert_eq!(
                session.begin_run(&lesson, NodeId::new(2), token),
                Err(SessionError::RunDisabled)
            );
        }
        assert!(session.run_results().is_empty());
    }

    #[test]
    fn a_run_needs_the_exact_token() {
        let (lesson, mut session) = run_session(Some("secret"));
        assert!(session.run_status().enabled);
        assert_eq!(session.run_status().token.as_deref(), Some("secret"));
        for token in [
            None,
            Some(""),
            Some("secre"),
            Some("secrets"),
            Some("Secret"),
        ] {
            assert_eq!(
                session.begin_run(&lesson, NodeId::new(2), token),
                Err(SessionError::InvalidToken),
                "{token:?}"
            );
        }
        assert!(
            session
                .begin_run(&lesson, NodeId::new(2), Some("secret"))
                .is_ok()
        );
    }

    #[test]
    fn only_run_blocks_can_be_admitted() {
        let (lesson, mut session) = run_session(Some("secret"));
        let begin = |session: &mut Session, node: u32| {
            session.begin_run(&lesson, NodeId::new(node), Some("secret"))
        };
        assert_eq!(
            begin(&mut session, 0),
            Err(SessionError::NotRunnable {
                node_id: NodeId::new(0)
            })
        );
        assert_eq!(
            begin(&mut session, 1),
            Err(SessionError::NotRunnable {
                node_id: NodeId::new(1)
            })
        );
        assert_eq!(
            begin(&mut session, 99),
            Err(SessionError::UnknownNode {
                node_id: NodeId::new(99)
            })
        );
        let spec = begin(&mut session, 3).unwrap();
        assert_eq!(spec.code, "echo own\n");
    }

    #[test]
    fn a_block_runs_once_at_a_time_and_keeps_its_last_result() {
        let (lesson, mut session) = run_session(Some("secret"));
        let node = NodeId::new(2);
        session.begin_run(&lesson, node, Some("secret")).unwrap();
        assert_eq!(
            session.begin_run(&lesson, node, Some("secret")),
            Err(SessionError::RunInProgress { node_id: node })
        );
        // Another block is not held up by it.
        assert!(
            session
                .begin_run(&lesson, NodeId::new(3), Some("secret"))
                .is_ok()
        );
        // Nothing is recorded until the run finishes.
        assert!(session.run_results().is_empty());

        session.finish_run(node, finished());
        assert_eq!(session.run_results()[&node].stdout, "hi\n");
        session.begin_run(&lesson, node, Some("secret")).unwrap();
        session.finish_run(
            node,
            RunResult {
                stdout: "again\n".into(),
                ..finished()
            },
        );
        assert_eq!(session.run_results()[&node].stdout, "again\n");
    }

    #[test]
    fn running_never_touches_quiz_progress() {
        let (lesson, mut session) = run_session(Some("secret"));
        let before = session.progress();
        session
            .begin_run(&lesson, NodeId::new(2), Some("secret"))
            .unwrap();
        session.finish_run(NodeId::new(2), finished());
        assert_eq!(session.progress(), before);
    }
}
