import { useId, useState, type ReactNode } from "react";

interface CollapsibleLessonBlockProps {
  children: ReactNode;
  kind: string;
  sourceId: string;
}

export function CollapsibleLessonBlock({
  children,
  kind,
  sourceId,
}: CollapsibleLessonBlockProps) {
  const [collapsed, setCollapsed] = useState(false);
  const contentId = useId();
  const action = collapsed ? "Expand" : "Collapse";

  return (
    <section
      aria-label={`${kind} block: ${sourceId}`}
      className={`lesson-block lesson-block-shell${collapsed ? " lesson-block-collapsed" : ""}`}
    >
      <header className="lesson-block-toolbar">
        <div className="lesson-block-identity">
          <strong className="lesson-block-kind">{kind}</strong>
          <code className="lesson-block-source">{sourceId}</code>
        </div>
        <button
          aria-controls={contentId}
          aria-expanded={!collapsed}
          aria-label={`${action} ${kind.toLowerCase()} block ${sourceId}`}
          className="lesson-block-toggle"
          onClick={() => setCollapsed((current) => !current)}
          type="button"
        >
          <span aria-hidden="true" className="lesson-block-toggle-icon">
            {collapsed ? "+" : "−"}
          </span>
          {action}
        </button>
      </header>
      <div className="lesson-block-content" hidden={collapsed} id={contentId}>
        {children}
      </div>
    </section>
  );
}
