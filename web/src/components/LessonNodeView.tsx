import type { ChoiceId, LessonNode, QuestionState } from "../types";
import { CodeBlock } from "./CodeBlock";
import { DiffBlock } from "./DiffBlock";
import { Markdown } from "./Markdown";
import { MultipleChoiceBlock } from "./MultipleChoiceBlock";

interface LessonNodeViewProps {
  node: LessonNode;
  questionState?: QuestionState;
  busy: boolean;
  onSubmit: (choiceId: ChoiceId) => Promise<void>;
  onReveal: () => Promise<void>;
}

export function LessonNodeView(props: LessonNodeViewProps) {
  const { node } = props;
  switch (node.type) {
    case "markdown":
      return (
        <section className="lesson-block prose-block">
          <Markdown>{node.content}</Markdown>
        </section>
      );
    case "code":
      return <CodeBlock node={node} />;
    case "diff":
      return <DiffBlock node={node} />;
    case "multiple_choice":
      return (
        <MultipleChoiceBlock
          busy={props.busy}
          node={node}
          onReveal={props.onReveal}
          onSubmit={props.onSubmit}
          state={props.questionState}
        />
      );
  }
}
