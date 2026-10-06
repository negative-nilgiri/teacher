import { useLinks } from "../links";
import type { CodeNode, RunCodeNode } from "../types";
import { CodeBlock } from "./CodeBlock";
import { Markdown } from "./Markdown";
import { ReferenceLink } from "./ReferenceLink";

/**
 * Code that can be run, with the output it produced when the lesson was built.
 * A block of its own code shows that code; a block of `of` points at the code
 * block it runs instead of repeating the code.
 */
export function RunCodeBlock({ node }: { node: RunCodeNode }) {
  const { nodes } = useLinks();
  const target = node.of === undefined ? undefined : nodes[node.of];

  return (
    <section className="run-block" aria-label={`Run: ${node.source_id}`}>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      {node.content !== undefined ? (
        <CodeBlock node={ownCode(node, node.content)} />
      ) : target ? (
        <p className="run-target">
          Runs{" "}
          <ReferenceLink destination={target.source_id}>
            <code>{target.source_id}</code>
          </ReferenceLink>
        </p>
      ) : null}
      {node.expected_output !== undefined ? (
        <section className="run-output" aria-label="Expected output">
          <header className="source-block-header run-output-header">
            <strong className="run-output-title">Expected output</strong>
            <span className="run-output-note">frozen when the lesson was built</span>
          </header>
          <pre className="run-output-text">
            <code>{node.expected_output}</code>
          </pre>
        </section>
      ) : null}
    </section>
  );
}

/** The block's own code shown as an ordinary code block, without its caption. */
function ownCode(node: RunCodeNode, content: string): CodeNode {
  return {
    content,
    filename: node.filename,
    first_line: node.first_line,
    language: node.language,
    node_id: node.node_id,
    reference: node.reference,
    source_id: node.source_id,
    type: "code",
  };
}
