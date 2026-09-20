import type { DiffLine, DiffNode } from "../types";
import { LanguageLabel } from "./LanguageLabel";
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
  return (
    <section className="diff-block" aria-label={`Diff: ${node.source_id}`}>
      {node.files.map((file, fileIndex) => (
        <article className="diff-file" key={`${file.old_path}:${file.new_path}:${fileIndex}`}>
          <header className="diff-file-header">
            <h2 className="diff-file-name">{fileLabel(file.old_path, file.new_path)}</h2>
            <LanguageLabel language={file.language} />
          </header>
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
        </article>
      ))}
    </section>
  );
}
