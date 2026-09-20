import type { ChoiceId, LessonNode, QuestionState } from "../types";
import { CodeBlock } from "./CodeBlock";
import { CollapsibleLessonBlock } from "./CollapsibleLessonBlock";
import { DiffBlock } from "./DiffBlock";
import { specificLanguageDisplayName } from "./LanguageLabel";
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
  let content;
  let kind: string;

  switch (node.type) {
    case "markdown":
      kind = "Markdown";
      content = (
        <section className="prose-block">
          <Markdown>{node.content}</Markdown>
        </section>
      );
      break;
    case "code":
      kind = specificLanguageDisplayName(node.language) ?? "Code";
      content = <CodeBlock node={node} />;
      break;
    case "diff":
      kind = "Diff";
      content = <DiffBlock node={node} />;
      break;
    case "multiple_choice":
      kind = "Question";
      content = (
        <MultipleChoiceBlock
          busy={props.busy}
          node={node}
          onReveal={props.onReveal}
          onSubmit={props.onSubmit}
          state={props.questionState}
        />
      );
      break;
  }

  return (
    <CollapsibleLessonBlock kind={kind} sourceId={node.source_id}>
      {content}
    </CollapsibleLessonBlock>
  );
}
