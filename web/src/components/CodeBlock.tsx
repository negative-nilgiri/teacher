import { useId, useState } from "react";
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
  const annotationIdPrefix = useId();
  const [activeAnnotation, setActiveAnnotation] = useState<string | null>(null);
  const lineCount = sourceLineCount(content);
  const lineNumbers = Array.from({ length: lineCount }, (_, index) => index + 1).join("\n");
  const markerLanes = new Map<number, number>();
  const annotationAnchors = highlights.flatMap((highlight, groupIndex) => {
    if (!highlight.annotation) return [];
    const annotation = highlight.annotation;
    return highlight.lines.map((range, rangeIndex) => {
      const lane = markerLanes.get(range.start) ?? 0;
      markerLanes.set(range.start, lane + 1);
      return {
        annotation,
        groupIndex,
        highlight,
        key: annotationKey(groupIndex, rangeIndex),
        lane,
        range,
        rangeIndex,
      };
    });
  });

  return (
    <>
      <div className="code-listing-shell">
        <pre className="code-listing">
          <span className="code-listing-inner">
            {highlights.flatMap((highlight, groupIndex) =>
              highlight.lines.map((range, rangeIndex) => {
                const key = annotationKey(groupIndex, rangeIndex);
                return (
                  <span
                    aria-hidden="true"
                    className={`code-highlight code-highlight-${highlight.color}`}
                    data-end={range.end}
                    data-start={range.start}
                    key={`${key}-${range.start}-${range.end}`}
                    style={highlightStyle(range)}
                  />
                );
              }),
            )}
            <span aria-hidden="true" className="code-line-numbers">
              {lineNumbers}
            </span>
            <SyntaxCode className="code-listing-source" language={language}>
              {content}
            </SyntaxCode>
          </span>
        </pre>
        {annotationAnchors.map(
          ({ annotation, groupIndex, highlight, key, lane, range, rangeIndex }) => {
            const popoverId = `${annotationIdPrefix}-${groupIndex}-${rangeIndex}`;
            const active = activeAnnotation === key;
            return (
              <div
                className="code-annotation-anchor"
                key={key}
                style={annotationAnchorStyle(range, lane)}
              >
                <button
                  aria-controls={popoverId}
                  aria-expanded={active}
                  aria-label={`${active ? "Hide" : "Show"} annotation for ${rangeLabel(range).toLowerCase()}`}
                  className={`code-annotation-marker code-annotation-marker-${highlight.color}`}
                  onClick={() =>
                    setActiveAnnotation((current) => (current === key ? null : key))
                  }
                  type="button"
                >
                  <span aria-hidden="true">i</span>
                </button>
                {active ? (
                  <aside
                    className={`code-annotation-popover code-annotation-popover-${highlight.color}`}
                    id={popoverId}
                    role="note"
                    style={annotationPopoverStyle(range, lineCount)}
                  >
                    <Markdown className="highlight-annotation-content">
                      {annotation}
                    </Markdown>
                  </aside>
                ) : null}
              </div>
            );
          },
        )}
      </div>
      {highlights.length > 0 ? (
        <span className="visually-hidden">
          {highlights.map(highlightDescription).join("; ")}
        </span>
      ) : null}
    </>
  );
}

function annotationKey(groupIndex: number, rangeIndex: number): string {
  return `${groupIndex}-${rangeIndex}`;
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

function annotationAnchorStyle(range: { start: number }, lane: number): CSSProperties {
  const lineHeightRem = 0.86 * 1.55;
  return {
    right: `${0.55 + lane * 1.45}rem`,
    top: `calc(1rem + ${(range.start - 1) * lineHeightRem}rem)`,
  };
}

function annotationPopoverStyle(
  range: { start: number },
  lineCount: number,
): CSSProperties {
  const remainingLines = lineCount - range.start;
  if (remainingLines >= 4) return {};
  return {
    bottom: "-0.15rem",
    top: "auto",
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

function rangeLabel(range: { start: number; end: number }): string {
  return range.start === range.end
    ? `Line ${range.start}`
    : `Lines ${range.start}–${range.end}`;
}

function capitalize(value: string): string {
  return value.charAt(0).toUpperCase() + value.slice(1);
}
