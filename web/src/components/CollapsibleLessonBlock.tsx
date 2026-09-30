import { useEffect, useId, useState, type ReactNode } from "react";
import { blockAnchor } from "../links";

interface CollapsibleLessonBlockProps {
  children: ReactNode;
  kind: string;
  sourceId: string;
  /** Extra toolbar controls, such as the ask button. */
  actions?: ReactNode;
}

export function CollapsibleLessonBlock({
  children,
  kind,
  sourceId,
  actions,
}: CollapsibleLessonBlockProps) {
  const [collapsed, setCollapsed] = useState(false);
  const [flashing, setFlashing] = useState(false);
  const contentId = useId();
  const anchor = blockAnchor(sourceId);

  // A jump to this block expands it and flashes it briefly.
  useEffect(() => {
    let timer: number | undefined;
    const onHash = () => {
      if (window.location.hash !== `#${anchor}`) return;
      setCollapsed(false);
      setFlashing(true);
      window.clearTimeout(timer);
      timer = window.setTimeout(() => setFlashing(false), 1600);
    };
    window.addEventListener("hashchange", onHash);
    return () => {
      window.removeEventListener("hashchange", onHash);
      window.clearTimeout(timer);
    };
  }, [anchor]);
  const action = collapsed ? "Expand" : "Collapse";

  return (
    <section
      aria-label={`${kind} block: ${sourceId}`}
      className={`lesson-block lesson-block-shell${collapsed ? " lesson-block-collapsed" : ""}${flashing ? " lesson-block-flash" : ""}`}
      id={anchor}
    >
      <header className="lesson-block-toolbar">
        <div className="lesson-block-identity">
          <strong className="lesson-block-kind">{kind}</strong>
          <code className="lesson-block-source">{sourceId}</code>
        </div>
        {actions}
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
