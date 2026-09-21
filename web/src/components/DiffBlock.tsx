import { useId, useState } from "react";
import type { DiffFile, DiffLine, DiffNode } from "../types";
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

function DiffFileSection({ collapsible, file }: { collapsible: boolean; file: DiffFile }) {
  const [collapsed, setCollapsed] = useState(false);
  const contentId = useId();
  const label = fileLabel(file.old_path, file.new_path);
  const action = collapsed ? "Expand" : "Collapse";

  return (
    <article className="diff-file">
      <header className="diff-file-header">
        <h2 className="diff-file-name">{label}</h2>
        <div className="diff-file-controls">
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
        {file.hunks.map((hunk, hunkIndex) => (
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
                  <tr className={`diff-line diff-line-${line.kind}`} key={lineIndex}>
                    <td className="line-number">{line.old_line ?? ""}</td>
                    <td className="line-number">{line.new_line ?? ""}</td>
                    <td className="diff-marker" aria-label={line.kind}>
                      {lineMarker(line)}
                    </td>
                    <td className="diff-content">
                      <SyntaxCode language={file.language}>{line.content}</SyntaxCode>
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        ))}
      </div>
    </article>
  );
}
