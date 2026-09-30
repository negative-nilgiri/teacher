import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { blockAnchor, useLinks } from "../links";
import type { DefinitionSite } from "../types";
import { TargetPreview } from "./ReferenceLink";

interface Open {
  name: string;
  sites: DefinitionSite[];
  top: number;
  left: number;
  /** Pinned by Cmd/Ctrl+click on an ambiguous name; hover changes it no more. */
  pinned: boolean;
}

const HOVER_DELAY = 350;

function linesLabel(site: DefinitionSite): string {
  if (!site.lines) return "";
  const { start, end } = site.lines;
  return start === end ? ` · line ${start}` : ` · lines ${start}–${end}`;
}

/** Jump to a block through the URL hash, so Back returns. */
export function jumpTo(sourceId: string): void {
  const hash = `#${blockAnchor(sourceId)}`;
  if (window.location.hash === hash) {
    document.getElementById(blockAnchor(sourceId))?.scrollIntoView?.();
    window.dispatchEvent(new HashChangeEvent("hashchange"));
  } else {
    window.location.hash = hash;
  }
}

/**
 * Go to definition for names in code. Hovering a linked name for a moment
 * previews its definition; Cmd/Ctrl+click jumps to it. A name with several
 * shown definitions offers a chooser instead of guessing.
 */
export function DefinitionLayer() {
  const { definitions, nodes } = useLinks();
  const [open, setOpen] = useState<Open | null>(null);
  const openTimer = useRef<number | undefined>(undefined);
  const closeTimer = useRef<number | undefined>(undefined);

  useEffect(() => {
    if (!definitions) return undefined;
    const sitesOf = (element: HTMLElement) => {
      const name = element.dataset.definition ?? "";
      const all = definitions[name] ?? [];
      const indexes = (element.dataset.sites ?? "").split(",").map(Number);
      return { name, sites: indexes.map((index) => all[index]).filter(Boolean) };
    };
    const place = (element: HTMLElement) => {
      const rect = element.getBoundingClientRect();
      const width = Math.min(544, window.innerWidth * 0.8);
      return {
        top: rect.bottom + window.scrollY + 6,
        left: Math.max(8, Math.min(rect.left + window.scrollX, window.innerWidth - width - 8)),
      };
    };
    const target = (event: Event) =>
      event.target instanceof Element
        ? event.target.closest<HTMLElement>(".definition-ref")
        : null;
    const onOver = (event: MouseEvent) => {
      const element = target(event);
      if (!element) return;
      window.clearTimeout(closeTimer.current);
      window.clearTimeout(openTimer.current);
      openTimer.current = window.setTimeout(() => {
        setOpen((current) =>
          current?.pinned ? current : { ...sitesOf(element), ...place(element), pinned: false },
        );
      }, HOVER_DELAY);
    };
    const onOut = (event: MouseEvent) => {
      if (!target(event)) return;
      window.clearTimeout(openTimer.current);
      closeTimer.current = window.setTimeout(
        () => setOpen((current) => (current?.pinned ? current : null)),
        150,
      );
    };
    const onClick = (event: MouseEvent) => {
      const element = target(event);
      if (!element || !(event.metaKey || event.ctrlKey)) return;
      event.preventDefault();
      window.clearTimeout(openTimer.current);
      const { name, sites } = sitesOf(element);
      if (sites.length === 1) {
        setOpen(null);
        jumpTo(nodes[sites[0].target].source_id);
      } else if (sites.length > 1) {
        setOpen({ name, sites, ...place(element), pinned: true });
      }
    };
    const onKey = (event: KeyboardEvent) => {
      if (event.key === "Escape") setOpen(null);
    };
    document.addEventListener("mouseover", onOver);
    document.addEventListener("mouseout", onOut);
    document.addEventListener("click", onClick);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mouseover", onOver);
      document.removeEventListener("mouseout", onOut);
      document.removeEventListener("click", onClick);
      document.removeEventListener("keydown", onKey);
      window.clearTimeout(openTimer.current);
      window.clearTimeout(closeTimer.current);
    };
  }, [definitions, nodes]);

  if (!open) return null;
  const several = open.sites.length > 1;
  return createPortal(
    <div
      aria-label={several ? `${open.sites.length} definitions of ${open.name}` : `Definition of ${open.name}`}
      className="reference-preview definition-preview"
      onMouseEnter={() => window.clearTimeout(closeTimer.current)}
      onMouseLeave={() => {
        if (!open.pinned) closeTimer.current = window.setTimeout(() => setOpen(null), 150);
      }}
      role={several ? "dialog" : "tooltip"}
      style={{ left: open.left, top: open.top }}
    >
      {several ? (
        <span className="reference-preview-label">
          {open.sites.length} definitions of <code>{open.name}</code>
        </span>
      ) : null}
      {open.sites.map((site, index) => {
        const node = nodes[site.target];
        if (!node) return null;
        return (
          <div className="definition-site" key={`${site.target}-${index}`}>
            {several ? (
              <button
                className="definition-go"
                onClick={() => {
                  setOpen(null);
                  jumpTo(node.source_id);
                }}
                type="button"
              >
                Go to {node.source_id}
                {linesLabel(site)}
              </button>
            ) : (
              <span className="reference-preview-label">
                {node.source_id}
                {linesLabel(site)} · ⌘/Ctrl+click to go
              </span>
            )}
            <TargetPreview lines={site.lines} node={node} />
          </div>
        );
      })}
    </div>,
    document.body,
  );
}
