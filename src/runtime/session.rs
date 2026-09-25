use std::collections::BTreeMap;

use crate::artifact::ChoiceId;
use crate::source::NodeId;

use super::model::{Attempt, LessonProgress, QuestionState, RuntimeLesson};

#[derive(Clone, Debug)]
pub(crate) struct Session {
    questions: BTreeMap<NodeId, QuestionState>,
    total_questions: usize,
}

impl Session {
    pub fn new(lesson: &RuntimeLesson) -> Self {
        Self {
            questions: BTreeMap::new(),
            total_questions: lesson.answers.len(),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::model::{project_runtime_lesson, tests::quiz_artifact};

    #[test]
    fn incorrect_attempts_allow_a_retry_without_revealing_the_answer() {
        let lesson = project_runtime_lesson(&quiz_artifact());
        let mut session = Session::new(&lesson);

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
        let mut session = Session::new(&lesson);
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
        let mut session = Session::new(&lesson);
        assert!(matches!(
            session.submit(&lesson, NodeId::new(0), ChoiceId::new(99)),
            Err(SessionError::ChoiceNotFound { .. })
        ));
        assert!(session.progress().questions.is_empty());
    }
}
