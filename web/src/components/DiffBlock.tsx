import { useId, useState } from "react";
import type { DiffFile, DiffHunk, DiffLine, DiffNode, RenderedSegment } from "../types";
import { LanguageLabel } from "./LanguageLabel";
import { Markdown } from "./Markdown";
import { SyntaxCode } from "./SyntaxCode";

function lineMarker(line: DiffLine): string {
  if (line.kind === "addition") return "+";
  if (line.kind === "deletion") return "−";
  return " ";
}

function fileLabel(oldPath: string | null, newPath: string | null): string {
  if (oldPath === null) return newPath ?? "new file";
  if (newPath === null) return oldPath;
  return oldPath === newPath ? oldPath : `${oldPath} → ${newPath}`;
}

export function DiffBlock({ node }: { node: DiffNode }) {
  const filesAreCollapsible = node.files.length > 1;

  return (
    <section className="diff-block" aria-label={`Diff: ${node.source_id}`}>
      {node.caption ? <Markdown className="source-caption">{node.caption}</Markdown> : null}
      {node.files.map((file, fileIndex) => (
        <DiffFileSection
          collapsible={filesAreCollapsible}
          file={file}
          key={`${file.old_path}:${file.new_path}:${fileIndex}`}
        />
      ))}
    </section>
  );
}

type DiffView = "rendered" | "source";

function DiffFileSection({ collapsible, file }: { collapsible: boolean; file: DiffFile }) {
  const [collapsed, setCollapsed] = useState(false);
  // Markdown files the compiler prepared start rendered; the line diff stays one click away.
  const [view, setView] = useState<DiffView>(file.rendered ? "rendered" : "source");
  const contentId = useId();
  const label = fileLabel(file.old_path, file.new_path);
  const action = collapsed ? "Expand" : "Collapse";

  return (
    <article className="diff-file" data-diff-path={file.new_path ?? file.old_path ?? undefined}>
      <header className="diff-file-header">
        <h2 className="diff-file-name">{label}</h2>
        <div className="diff-file-controls">
          {file.rendered ? (
            <div aria-label={`View of ${label}`} className="diff-view-switch" role="group">
              {(["rendered", "source"] as const).map((option) => (
                <button
                  aria-pressed={view === option}
                  className="diff-view-option"
                  key={option}
                  onClick={() => setView(option)}
                  type="button"
                >
                  {option === "rendered" ? "Rendered" : "Source"}
                </button>
              ))}
            </div>
          ) : null}
          <LanguageLabel language={file.language} />
          {collapsible ? (
            <button
              aria-controls={contentId}
              aria-expanded={!collapsed}
              aria-label={`${action} diff file ${label}`}
              className="diff-file-toggle"
              onClick={() => setCollapsed((current) => !current)}
              type="button"
            >
              <span aria-hidden="true" className="diff-file-toggle-icon">
                {collapsed ? "+" : "−"}
              </span>
              {action}
            </button>
          ) : null}
        </div>
      </header>
      <div hidden={collapsed} id={contentId}>
        {file.rendered && view === "rendered" ? (
          <RenderedMarkdown segments={file.rendered.segments} />
        ) : (
          <SourceHunks hunks={file.hunks} language={file.language} />
        )}
      </div>
    </article>
  );
}

const SEGMENT_LABELS = { added: "Added", removed: "Removed" } as const;

/** Each segment is a complete Markdown block, so it renders on its own. */
function RenderedMarkdown({ segments }: { segments: RenderedSegment[] }) {
  return (
    <div className="markdown-diff">
      {segments.map((segment, index) => {
        if (segment.kind === "gap") {
          const noun = segment.blocks === 1 ? "block" : "blocks";
          return (
            <div className="markdown-diff-gap" key={index}>
              ⋯ {segment.blocks} {noun} not shown
            </div>
          );
        }
        return (
          <div className={`markdown-diff-segment markdown-diff-${segment.kind}`} key={index}>
            {segment.kind === "unchanged" ? null : (
              <span className="visually-hidden">{SEGMENT_LABELS[segment.kind]}: </span>
            )}
            <Markdown>{segment.markdown}</Markdown>
          </div>
        );
      })}
    </div>
  );
}

function SourceHunks({ hunks, language }: { hunks: DiffHunk[]; language: string }) {
  return (
    <>
        {hunks.map((hunk, hunkIndex) => (
          <div className="diff-hunk" key={`${hunk.header}:${hunkIndex}`}>
            <div className="diff-hunk-header">{hunk.header}</div>
            <table>
              <thead className="visually-hidden">
                <tr>
                  <th>Old line</th>
                  <th>New line</th>
                  <th>Change</th>
                  <th>Content</th>
                </tr>
              </thead>
              <tbody>
                {hunk.lines.map((line, lineIndex) => (
                  <tr
                    className={`diff-line diff-line-${line.kind}`}
                    data-new-line={line.new_line ?? undefined}
                    data-old-line={line.old_line ?? undefined}
                    key={lineIndex}
                  >
                    <td className="line-number">{line.old_line ?? ""}</td>
                    <td className="line-number">{line.new_line ?? ""}</td>
                    <td className="diff-marker" aria-label={line.kind}>
                      {lineMarker(line)}
                    </td>
                    <td className="diff-content">
                      <SyntaxCode language={language}>{line.content}</SyntaxCode>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
    </>
  );
}
