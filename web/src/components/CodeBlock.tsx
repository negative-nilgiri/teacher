import type { CodeNode } from "../types";
import { LanguageLabel } from "./LanguageLabel";
import { Markdown } from "./Markdown";
import { MermaidDiagram } from "./MermaidDiagram";
import { SyntaxCode } from "./SyntaxCode";

export function CodeBlock({ node }: { node: CodeNode }) {
  return (
    <section className="code-block" aria-label={`Code: ${node.source_id}`}>
      <header className="source-block-header">
        <LanguageLabel language={node.language} />
      </header>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      {node.language === "mermaid" ? (
        <MermaidDiagram label={`Diagram: ${node.source_id}`} source={node.content} />
      ) : (
        <pre>
          <SyntaxCode language={node.language}>{node.content}</SyntaxCode>
        </pre>
      )}
    </section>
  );
}
