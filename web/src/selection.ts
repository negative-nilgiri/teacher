import type { LessonNode } from "./types";
import type { LineSelection } from "./reference";

function lineAt(source: Element, container: Node, offset: number): number {
  const before = document.createRange();
  before.setStart(source, 0);
  before.setEnd(container, offset);
  return before.toString().split("\n").length;
}

function diffRow(node: Node | null): HTMLElement | null {
  const element = node instanceof Element ? node : node?.parentElement ?? null;
  return element?.closest<HTMLElement>("tr[data-old-line], tr[data-new-line]") ?? null;
}

function sideRange(rows: HTMLElement[], attribute: string): [number, number] | undefined {
  const values = rows
    .map((row) => row.getAttribute(attribute))
    .filter((value): value is string => value !== null && value !== "")
    .map(Number);
  if (values.length === 0) return undefined;
  return [Math.min(...values), Math.max(...values)];
}

/** Every diff row from `start` to `end`, in document order. */
function rowsBetween(block: HTMLElement, start: HTMLElement, end: HTMLElement): HTMLElement[] {
  const rows = Array.from(
    block.querySelectorAll<HTMLElement>("tr[data-old-line], tr[data-new-line]"),
  );
  const first = rows.indexOf(start);
  const last = rows.indexOf(end);
  return rows.slice(Math.min(first, last), Math.max(first, last) + 1);
}

const BLOCK_ELEMENTS = "p, li, h1, h2, h3, h4, h5, h6, pre, blockquote, tr";

/** The tagged element that holds both ends of the range, if there is one. */
function holder(range: Range, selector: string): Element | null {
  const of = (node: Node) =>
    (node instanceof Element ? node : node.parentElement)?.closest(selector) ?? null;
  const start = of(range.startContainer);
  return start && start === of(range.endContainer) ? start : null;
}

/**
 * The rendered text of a range, with line breaks between blocks. Math is its
 * TeX source (`$...$`), because the rendered form selects as two copies.
 */
function renderedText(range: Range): string {
  const box = document.createElement("div");
  box.append(range.cloneContents());
  for (const math of box.querySelectorAll(".katex")) {
    const tex = math.querySelector('annotation[encoding="application/x-tex"]')?.textContent;
    const fence = math.parentElement?.classList.contains("katex-display") ? "$$" : "$";
    math.replaceWith(tex ? `${fence}${tex}${fence}` : "");
  }
  for (const element of box.querySelectorAll(BLOCK_ELEMENTS)) element.append("\n");
  return box.textContent.replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * The selection inside `block`: source lines of a code listing or diff, or the
 * rendered text of a Markdown block, a question prompt or its hints (never the
 * choices, which could show an answer). Null when none of those is selected.
 */
export function selectedLines(block: HTMLElement, node: LessonNode): LineSelection | null {
  const selection = window.getSelection();
  if (!selection || selection.isCollapsed || selection.rangeCount === 0) return null;
  const range = selection.getRangeAt(0);
  if (!block.contains(range.startContainer) || !block.contains(range.endContainer)) return null;

  if (node.type === "markdown" || node.type === "multiple_choice") {
    const selector = node.type === "markdown" ? ".prose-block" : ".question-prompt, .hints";
    if (!holder(range, selector)) return null;
    const text = renderedText(range);
    return text ? { kind: "text", text } : null;
  }

  if (node.type === "code") {
    const source = block.querySelector(".code-listing-source");
    if (!source || !source.contains(range.startContainer) || !source.contains(range.endContainer)) {
      return null;
    }
    const first = lineAt(source, range.startContainer, range.startOffset);
    let last = lineAt(source, range.endContainer, range.endOffset);
    // A selection that ends at the start of the next line does not include it.
    if (last > first && range.toString().endsWith("\n")) last -= 1;
    const offset = (node.first_line ?? 1) - 1;
    const group = (node.highlights ?? []).findIndex((highlight) =>
      highlight.lines.some((lines) => lines.start <= last && first <= lines.end),
    );
    return {
      kind: "code",
      start: first + offset,
      end: last + offset,
      ...(group >= 0 ? { group } : {}),
    };
  }

  if (node.type === "diff") {
    const start = diffRow(range.startContainer);
    const end = diffRow(range.endContainer);
    if (!start || !end) return null;
    const path = start.closest<HTMLElement>("[data-diff-path]")?.dataset.diffPath;
    const rows = rowsBetween(block, start, end);
    const old = sideRange(rows, "data-old-line");
    const added = sideRange(rows, "data-new-line");
    const selection: LineSelection = {
      kind: "diff",
      ...(path ? { path } : {}),
      ...(old ? { old } : {}),
      ...(added ? { new: added } : {}),
    };
    return selection.old || selection.new ? selection : null;
  }
  return null;
}
