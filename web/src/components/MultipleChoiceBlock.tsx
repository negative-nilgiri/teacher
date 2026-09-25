import { useId, useState } from "react";
import type {
  ChoiceId,
  MultipleChoiceNode,
  QuestionState,
} from "../types";
import { Markdown } from "./Markdown";

interface MultipleChoiceBlockProps {
  node: MultipleChoiceNode;
  state?: QuestionState;
  busy: boolean;
  onSubmit: (choiceId: ChoiceId) => Promise<void>;
  onReveal: () => Promise<void>;
}

export function MultipleChoiceBlock({
  node,
  state,
  busy,
  onSubmit,
  onReveal,
}: MultipleChoiceBlockProps) {
  const groupName = `question-${useId()}`;
  const [selection, setSelection] = useState<ChoiceId | null>(null);
  const latestAttempt = state?.attempts.at(-1);
  const answer = state?.answer;
  const isResolved = state?.completed || state?.revealed;
  const choiceExplanations = new Map(
    (answer?.choice_explanations ?? []).map((entry) => [entry.choice_id, entry.explanation]),
  );

  async function submit(event: React.FormEvent) {
    event.preventDefault();
    if (selection === null) return;
    await onSubmit(selection);
  }

  return (
    <section className="question" aria-labelledby={`${groupName}-prompt`}>
      <Markdown className="question-prompt">{node.prompt}</Markdown>

      <form onSubmit={submit}>
        <fieldset disabled={busy || isResolved}>
          <legend className="visually-hidden" id={`${groupName}-prompt`}>
            Choose one answer
          </legend>
          <div className="choices">
            {node.choices.map((choice) => {
              const isAnswer = answer?.choice_id === choice.choice_id;
              const whyNot = choiceExplanations.get(choice.choice_id);
              return (
                <label
                  className={`choice${isAnswer ? " choice-answer" : ""}`}
                  key={choice.choice_id}
                >
                  <input
                    checked={selection === choice.choice_id}
                    name={groupName}
                    onChange={() => setSelection(choice.choice_id)}
                    type="radio"
                    value={choice.choice_id}
                  />
                  <div className="choice-body">
                    <Markdown>{choice.content}</Markdown>
                    {whyNot ? (
                      <div className="choice-explanation">
                        <span className="choice-explanation-label">Why not</span>
                        <Markdown>{whyNot}</Markdown>
                      </div>
                    ) : null}
                  </div>
                </label>
              );
            })}
          </div>

          <div className="question-actions">
            <button className="primary" disabled={selection === null || busy} type="submit">
              {busy ? "Checking…" : "Check answer"}
            </button>
            <button disabled={busy} onClick={onReveal} type="button">
              Reveal answer
            </button>
          </div>
        </fieldset>
      </form>

      {latestAttempt && !latestAttempt.correct && !answer ? (
        <p className="feedback feedback-incorrect" role="status">
          Not quite. Try another answer or use a hint.
        </p>
      ) : null}

      {answer ? (
        <div className="feedback feedback-answer" role="status">
          <strong>{state?.completed ? "Correct." : "Answer revealed."}</strong>
          <Markdown>{answer.explanation}</Markdown>
        </div>
      ) : null}

      {node.hints.length > 0 ? (
        <details className="hints">
          <summary>Hints ({node.hints.length})</summary>
          {node.hints.map((hint, index) => (
            <Markdown key={index}>{hint}</Markdown>
          ))}
        </details>
      ) : null}
    </section>
  );
}
