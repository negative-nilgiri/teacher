import type { ChoiceId, LessonNode, QuestionState } from "../types";
import type { LessonContext } from "../reference";
import { AskAboutBlock } from "./AskAboutBlock";
import { specificLanguageDisplayName } from "../languages";
import { CodeBlock } from "./CodeBlock";
import { CollapsibleLessonBlock } from "./CollapsibleLessonBlock";
import { DiffBlock } from "./DiffBlock";
import { ExternalArtifactBlock } from "./ExternalArtifactBlock";
import { Markdown } from "./Markdown";
import { MultipleChoiceBlock } from "./MultipleChoiceBlock";
import { RunCodeBlock, type RunControls } from "./RunCodeBlock";

interface LessonNodeViewProps {
  node: LessonNode;
  /** When present, the block offers a copyable reference for an agent. */
  lesson?: LessonContext;
  questionState?: QuestionState;
  /** What a run block needs to run; without it, a run block cannot run. */
  run?: RunControls;
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
    case "run_code":
      kind = "Run code";
      content = <RunCodeBlock node={node} run={props.run} />;
      break;
    case "external_artifact":
      kind = node.kind[0].toUpperCase() + node.kind.slice(1);
      content = <ExternalArtifactBlock node={node} />;
      break;
  }

  return (
    <CollapsibleLessonBlock
      actions={props.lesson ? <AskAboutBlock lesson={props.lesson} node={node} /> : null}
      kind={kind}
      sourceId={node.source_id}
    >
      {content}
    </CollapsibleLessonBlock>
  );
}
