import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CodeBlock } from "../components/CodeBlock";
import { DefinitionLayer } from "../components/DefinitionLayer";
import { LinkContext } from "../links";
import type { CodeNode, DefinitionSite, LessonNode } from "../types";

vi.mock("mermaid", () => ({ default: { initialize: vi.fn(), render: vi.fn() } }));

const definitionBlock: CodeNode = {
  content: "struct Queue { items: Vec<u32> }\nfn key(q: &Queue) -> u32 { 0 }\nmacro_rules! square { ($x:expr) => { $x * $x }; }",
  first_line: 10,
  language: "rust",
  node_id: 0,
  source_id: "defs",
  type: "code",
};

const usingBlock: CodeNode = {
  content: "// key(queue) in a comment\nlet key = key(&queue);\nlet q: Queue = make(\"key\");\nlet n = square!(2);\nlet other = helper(1);",
  language: "rust",
  node_id: 1,
  source_id: "uses",
  type: "code",
};

const definitions: Record<string, DefinitionSite[]> = {
  Queue: [{ kind: "type", lines: { end: 10, start: 10 }, target: 0 }],
  key: [{ kind: "function", lines: { end: 11, start: 11 }, target: 0 }],
  square: [{ kind: "macro", lines: { end: 12, start: 12 }, target: 0 }],
  helper: [
    { kind: "function", lines: { end: 11, start: 11 }, target: 0 },
    { kind: "function", lines: { end: 12, start: 12 }, target: 0 },
  ],
};

const nodes: LessonNode[] = [definitionBlock, usingBlock];

function renderLesson() {
  return render(
    <LinkContext.Provider value={{ definitions, inPreview: false, links: {}, nodes }}>
      <DefinitionLayer />
      <CodeBlock node={definitionBlock} />
      <CodeBlock node={usingBlock} />
    </LinkContext.Provider>,
  );
}

function refs(container: HTMLElement, blockLabel: string) {
  return Array.from(
    container.querySelectorAll<HTMLElement>(`[aria-label="Code: ${blockLabel}"] .definition-ref`),
  ).map((element) => `${element.textContent}:${element.dataset.sites}`);
}

afterEach(() => {
  vi.useRealTimers();
});

describe("go to definition", () => {
  it("links calls to functions, any use of types, and never comments, strings or the definition itself", () => {
    const { container } = renderLesson();
    // `let key =` is a variable, not a call; `key(` is; the comment and the
    // string are skipped; `helper(` has two definitions.
    expect(refs(container, "uses")).toEqual(["key:0", "Queue:0", "square:0", "helper:0,1"]);
    // In the defining block, the names on their own definition lines stay plain,
    // while the `Queue` used in `key`'s signature links.
    expect(refs(container, "defs")).toEqual(["Queue:0"]);
  });

  it("previews on hover and jumps on Cmd/Ctrl+click", () => {
    vi.useFakeTimers();
    const { container } = renderLesson();
    const call = container.querySelector<HTMLElement>('[aria-label="Code: uses"] .definition-ref[data-definition="key"]')!;
    act(() => {
      fireEvent.mouseOver(call);
      vi.advanceTimersByTime(400);
    });
    const preview = screen.getByRole("tooltip", { name: "Definition of key" });
    expect(preview).toHaveTextContent("defs · line 11");
    expect(preview).toHaveTextContent("fn key");
    expect(preview).not.toHaveTextContent("macro_rules");

    act(() => {
      fireEvent.click(call, { ctrlKey: true });
    });
    expect(window.location.hash).toBe("#block-defs");
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();

    // A plain click does nothing, so text can still be selected.
    window.location.hash = "";
    act(() => {
      fireEvent.click(call);
    });
    expect(window.location.hash).toBe("");
  });

  it("offers a chooser for an ambiguous name", () => {
    const { container } = renderLesson();
    const ambiguous = container.querySelector<HTMLElement>('.definition-ref[data-definition="helper"]')!;
    act(() => {
      fireEvent.click(ambiguous, { metaKey: true });
    });
    const chooser = screen.getByRole("dialog", { name: "2 definitions of helper" });
    const options = screen.getAllByRole("button", { name: /^Go to defs/ });
    expect(options.map((option) => option.textContent)).toEqual([
      "Go to defs · line 11",
      "Go to defs · line 12",
    ]);
    expect(chooser).toBeInTheDocument();
    act(() => {
      fireEvent.click(options[1]);
    });
    expect(window.location.hash).toBe("#block-defs");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    act(() => {
      fireEvent.click(ambiguous, { metaKey: true });
    });
    act(() => {
      fireEvent.keyDown(document, { key: "Escape" });
    });
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("does not link inside previews, so hovering cannot nest", () => {
    const { container } = render(
      <LinkContext.Provider value={{ definitions, inPreview: true, links: {}, nodes }}>
        <CodeBlock node={usingBlock} />
      </LinkContext.Provider>,
    );
    expect(container.querySelector(".definition-ref")).toBeNull();
  });
});
