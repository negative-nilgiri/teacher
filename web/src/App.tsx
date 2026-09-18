import { useEffect, useState } from "react";
import { loadState, revealAnswer, submitChoice } from "./api";
import { LessonNodeView } from "./components/LessonNodeView";
import type {
  ChoiceId,
  LessonProgress,
  MutationResponse,
  NodeId,
  StateResponse,
} from "./types";

function isFullState(response: MutationResponse): response is StateResponse {
  return "lesson" in response;
}

export function App() {
  const [state, setState] = useState<StateResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyNode, setBusyNode] = useState<NodeId | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    loadState(controller.signal)
      .then(setState)
      .catch((cause: unknown) => {
        if (cause instanceof DOMException && cause.name === "AbortError") return;
        setError(cause instanceof Error ? cause.message : "The lesson could not be loaded.");
      });
    return () => controller.abort();
  }, []);

  function applyMutation(nodeId: NodeId, response: MutationResponse) {
    setState((current) => {
      if (isFullState(response)) return response;
      if (!current) return current;
      const questions = {
        ...response.progress.questions,
        [String(nodeId)]: response.question,
      };
      const progress: LessonProgress = { ...response.progress, questions };
      return { ...current, progress };
    });
  }

  async function mutate(
    nodeId: NodeId,
    operation: () => Promise<MutationResponse>,
  ) {
    setBusyNode(nodeId);
    setError(null);
    try {
      applyMutation(nodeId, await operation());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "The request failed.");
    } finally {
      setBusyNode(null);
    }
  }

  if (error && !state) {
    return (
      <main className="shell status-page">
        <h1>Lesson unavailable</h1>
        <p role="alert">{error}</p>
      </main>
    );
  }

  if (!state) {
    return (
      <main className="shell status-page" aria-busy="true">
        <p>Loading lesson…</p>
      </main>
    );
  }

  const { lesson, progress } = state;
  const completion =
    progress.total_questions === 0
      ? 0
      : (progress.completed_questions / progress.total_questions) * 100;

  return (
    <>
      <header className="lesson-header">
        <div className="shell">
          <p className="eyebrow">Interactive lesson</p>
          <h1>{lesson.title}</h1>
          {progress.total_questions > 0 ? (
            <div className="progress-summary" aria-label="Lesson progress">
              <span>
                {progress.completed_questions} of {progress.total_questions} questions completed
              </span>
              <progress max={100} value={completion} />
            </div>
          ) : null}
        </div>
      </header>

      <main className="shell lesson">
        {error ? <p className="request-error" role="alert">{error}</p> : null}
        {lesson.nodes.map((node) => (
          <LessonNodeView
            busy={busyNode === node.node_id}
            key={node.node_id}
            node={node}
            onReveal={() => mutate(node.node_id, () => revealAnswer(node.node_id))}
            onSubmit={(choiceId: ChoiceId) =>
              mutate(node.node_id, () => submitChoice(node.node_id, choiceId))
            }
            questionState={progress.questions[String(node.node_id)]}
          />
        ))}
      </main>
    </>
  );
}
