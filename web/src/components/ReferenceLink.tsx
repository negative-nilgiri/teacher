import { useEffect, useId, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { blockAnchor, LinkContext, previewNode, useLinks } from "../links";
import type { LessonNode } from "../types";
import { CodeBlock } from "./CodeBlock";
import { DiffBlock } from "./DiffBlock";
import { ExternalArtifactPreview } from "./ExternalArtifactBlock";
import { Markdown } from "./Markdown";
import { RunCodeBlock } from "./RunCodeBlock";

interface ReferenceLinkProps {
  /** The destination without `#`, such as `queue-def:12-18`. */
  destination: string;
  children: ReactNode;
}

/**
 * A link to another block of the lesson. Hover or focus previews the target
 * in place, so the learner keeps their position; clicking jumps to it through
 * the URL hash, so the browser's Back button returns.
 */
export function ReferenceLink({ destination, children }: ReferenceLinkProps) {
  const context = useLinks();
  const previewId = useId();
  const linkRef = useRef<HTMLAnchorElement>(null);
  const closeTimer = useRef<number | undefined>(undefined);
  const [position, setPosition] = useState<{ top: number; left: number } | null>(null);
  const open = position !== null;
  useEffect(() => () => window.clearTimeout(closeTimer.current), []);
  const link = context.links[destination];
  const target = link ? context.nodes[link.target] : undefined;
  if (!link || !target) return <span className="reference-link-broken">{children}</span>;

  const href = `#${blockAnchor(target.source_id)}`;
  if (context.inPreview) {
    return (
      <a className="reference-link" href={href}>
        {children}
      </a>
    );
  }

  // The preview lives in a portal: it holds block content that cannot nest
  // inside a paragraph, and no scrolling container may clip it.
  const show = () => {
    window.clearTimeout(closeTimer.current);
    const rect = linkRef.current?.getBoundingClientRect();
    const width = Math.min(544, window.innerWidth * 0.8);
    setPosition({
      top: (rect?.bottom ?? 0) + window.scrollY + 6,
      left: Math.max(8, Math.min((rect?.left ?? 0) + window.scrollX, window.innerWidth - width - 8)),
    });
  };
  // A short delay lets the pointer move from the link into the preview.
  const hide = () => {
    window.clearTimeout(closeTimer.current);
    closeTimer.current = window.setTimeout(() => setPosition(null), 150);
  };

  return (
    <>
      <a
        aria-describedby={open ? previewId : undefined}
        className="reference-link"
        href={href}
        onBlur={hide}
        onFocus={show}
        onKeyDown={(event) => {
          if (event.key === "Escape") setPosition(null);
        }}
        onMouseEnter={show}
        onMouseLeave={hide}
        ref={linkRef}
      >
        {children}
      </a>
      {position
        ? createPortal(
        <div
          className="reference-preview"
          id={previewId}
          onMouseEnter={show}
          onMouseLeave={hide}
          role="tooltip"
          style={{ left: position.left, top: position.top }}
        >
          <span className="reference-preview-label">
            {target.source_id}
            {link.lines
              ? link.lines.start === link.lines.end
                ? ` · line ${link.lines.start}`
                : ` · lines ${link.lines.start}–${link.lines.end}`
              : null}
          </span>
          <TargetPreview lines={link.lines} node={target} />
        </div>,
            document.body,
          )
        : null}
    </>
  );
}

/**
 * A block rendered for a preview: trimmed to `lines`, questions as prompt
 * only, and inside a context that renders links and definitions plainly.
 */
export function TargetPreview({
  node: target,
  lines,
}: {
  node: LessonNode;
  lines?: { start: number; end: number };
}) {
  const context = useLinks();
  const node = previewNode(target, lines);
  let preview: ReactNode;
  switch (node.type) {
    case "code":
      preview = <CodeBlock node={node} />;
      break;
    case "diff":
      preview = <DiffBlock node={node} />;
      break;
    case "markdown":
      preview = <Markdown>{node.content}</Markdown>;
      break;
    case "multiple_choice":
      preview = <Markdown>{node.prompt}</Markdown>;
      break;
    case "run_code":
      preview = <RunCodeBlock node={node} />;
      break;
    case "external_artifact":
      preview = <ExternalArtifactPreview node={node} />;
      break;
  }
  return <LinkContext.Provider value={{ ...context, inPreview: true }}>{preview}</LinkContext.Provider>;
}
