import type { CSSProperties } from "react";
import type { CodeHighlight, CodeNode } from "../types";
import { LanguageLabel } from "./LanguageLabel";
import { Markdown } from "./Markdown";
import { MermaidDiagram } from "./MermaidDiagram";
import { SyntaxCode } from "./SyntaxCode";

export function CodeBlock({ node }: { node: CodeNode }) {
  const highlights = node.highlights ?? [];
  return (
    <section className="code-block" aria-label={`Code: ${node.source_id}`}>
      <header
        className={`source-block-header${node.filename ? " source-block-header-with-file" : ""}`}
      >
        {node.filename ? <code className="source-file-name">{node.filename}</code> : null}
        <LanguageLabel language={node.language} />
      </header>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      {node.language === "mermaid" ? (
        <MermaidDiagram label={`Diagram: ${node.source_id}`} source={node.content} />
      ) : (
        <CodeListing content={node.content} highlights={highlights} language={node.language} />
      )}
    </section>
  );
}

function CodeListing({
  content,
  highlights,
  language,
}: {
  content: string;
  highlights: CodeHighlight[];
  language: string;
}) {
  const lineCount = sourceLineCount(content);
  const lineNumbers = Array.from({ length: lineCount }, (_, index) => index + 1).join("\n");

  return (
    <>
      <pre className="code-listing">
        <span className="code-listing-inner">
          {highlights.map((highlight, index) => (
            <span
              aria-hidden="true"
              className={`code-highlight code-highlight-${highlight.color}`}
              data-end={highlight.end}
              data-start={highlight.start}
              key={`${highlight.start}-${highlight.end}-${highlight.color}-${index}`}
              style={highlightStyle(highlight)}
            />
          ))}
          <span aria-hidden="true" className="code-line-numbers">
            {lineNumbers}
          </span>
          <SyntaxCode className="code-listing-source" language={language}>
            {content}
          </SyntaxCode>
        </span>
      </pre>
      {highlights.length > 0 ? (
        <span className="visually-hidden">
          {highlights.map(highlightDescription).join("; ")}
        </span>
      ) : null}
    </>
  );
}

function sourceLineCount(content: string): number {
  if (content.length === 0) return 0;
  const withoutFinalNewline = content.endsWith("\n") ? content.slice(0, -1) : content;
  return withoutFinalNewline.split("\n").length;
}

function highlightStyle(highlight: CodeHighlight): CSSProperties {
  const lineHeight = 1.55;
  return {
    height: `${(highlight.end - highlight.start + 1) * lineHeight}em`,
    top: `calc(1rem + ${(highlight.start - 1) * lineHeight}em)`,
  };
}

function highlightDescription(highlight: CodeHighlight): string {
  const lines =
    highlight.start === highlight.end
      ? `Highlighted line ${highlight.start}`
      : `Highlighted lines ${highlight.start} through ${highlight.end}`;
  return `${lines} in ${highlight.color}`;
}
