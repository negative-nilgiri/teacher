import type { CodeNode } from "../types";

export function CodeBlock({ node }: { node: CodeNode }) {
  return (
    <section className="code-block lesson-block" aria-label={`Code: ${node.source_id}`}>
      <pre>
        <code>{node.content}</code>
      </pre>
    </section>
  );
}
