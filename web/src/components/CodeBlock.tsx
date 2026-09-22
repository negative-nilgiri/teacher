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
      <HighlightAnnotations highlights={highlights} />
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
          {highlights.flatMap((highlight, groupIndex) =>
            highlight.lines.map((range, rangeIndex) => (
              <span
                aria-hidden="true"
                className={`code-highlight code-highlight-${highlight.color}`}
                data-end={range.end}
                data-start={range.start}
                key={`${groupIndex}-${rangeIndex}-${range.start}-${range.end}`}
                style={highlightStyle(range)}
              />
            )),
          )}
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

function HighlightAnnotations({ highlights }: { highlights: CodeHighlight[] }) {
  const annotated = highlights.filter(
    (highlight): highlight is CodeHighlight & { annotation: string } =>
      Boolean(highlight.annotation),
  );
  if (annotated.length === 0) return null;

  return (
    <aside className="highlight-annotations" aria-label="Highlighted code explanations">
      <ul>
        {annotated.map((highlight, index) => (
          <li
            className={`highlight-annotation highlight-annotation-${highlight.color}`}
            key={`${highlight.color}-${lineReferences(highlight)}-${index}`}
          >
            <span className="highlight-annotation-label">
              {capitalize(highlight.color)} · {lineReferences(highlight)}
            </span>
            <Markdown className="highlight-annotation-content">{highlight.annotation}</Markdown>
          </li>
        ))}
      </ul>
    </aside>
  );
}

function highlightStyle(highlight: { start: number; end: number }): CSSProperties {
  const lineHeight = 1.55;
  return {
    height: `${(highlight.end - highlight.start + 1) * lineHeight}em`,
    top: `calc(1rem + ${(highlight.start - 1) * lineHeight}em)`,
  };
}

function highlightDescription(highlight: CodeHighlight): string {
  return `Highlighted ${lineReferences(highlight).toLowerCase()} in ${highlight.color}`;
}

function lineReferences(highlight: CodeHighlight): string {
  const ranges = highlight.lines.map(({ start, end }) =>
    start === end ? `${start}` : `${start}–${end}`,
  );
  return `${ranges.length === 1 && highlight.lines[0].start === highlight.lines[0].end ? "Line" : "Lines"} ${ranges.join(", ")}`;
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
