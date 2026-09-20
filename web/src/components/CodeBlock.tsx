import type { CodeNode } from "../types";
import { MermaidDiagram } from "./MermaidDiagram";
import { SyntaxCode } from "./SyntaxCode";

export function CodeBlock({ node }: { node: CodeNode }) {
  if (node.language === "mermaid") {
    return (
      <section className="code-block lesson-block" aria-label={`Code: ${node.source_id}`}>
        <MermaidDiagram label={`Diagram: ${node.source_id}`} source={node.content} />
      </section>
    );
  }

  return (
    <section className="code-block lesson-block" aria-label={`Code: ${node.source_id}`}>
      <pre>
        <SyntaxCode language={node.language}>{node.content}</SyntaxCode>
      </pre>
    </section>
  );
}
