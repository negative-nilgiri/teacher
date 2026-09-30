import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { CollapsibleLessonBlock } from "../components/CollapsibleLessonBlock";
import { Markdown } from "../components/Markdown";
import { LinkContext, previewNode } from "../links";
import type { CodeNode, DiffNode, LessonNode, MultipleChoiceNode } from "../types";

vi.mock("mermaid", () => ({ default: { initialize: vi.fn(), render: vi.fn() } }));

const queue: CodeNode = {
  content: "// queue\nstruct Queue {\n    items: Vec<u32>,\n}\nfn other() {}",
  first_line: 10,
  highlights: [{ annotation: "The **buffer**.", color: "blue", lines: [{ end: 4, start: 2 }] }],
  language: "rust",
  node_id: 0,
  source_id: "queue-def",
  type: "code",
};

const quiz: MultipleChoiceNode = {
  choices: [{ choice_id: 0, content: "The oldest" }, { choice_id: 1, content: "The newest" }],
  hints: ["Think FIFO."],
  node_id: 1,
  prompt: "Which item leaves first?",
  source_id: "order",
  type: "multiple_choice",
};

const nodes: LessonNode[] = [queue, quiz];
const links = {
  "queue-def": { target: 0 },
  "queue-def:11-13": { lines: { end: 13, start: 11 }, target: 0 },
  order: { target: 1 },
};

function withLinks(markdown: string) {
  return render(
    <LinkContext.Provider value={{ inPreview: false, links, nodes }}>
      <Markdown>{markdown}</Markdown>
    </LinkContext.Provider>,
  );
}

describe("block links", () => {
  it("previews a ranged code target on hover without leaving the page", async () => {
    withLinks("It reads the [`Queue`](#queue-def:11-13) buffer.");
    const link = screen.getByRole("link", { name: "Queue" });
    expect(link).toHaveAttribute("href", "#block-queue-def");
    act(() => {
      fireEvent.mouseEnter(link);
    });
    const preview = await screen.findByRole("tooltip");
    expect(preview).toHaveTextContent("queue-def · lines 11–13");
    expect(preview).toHaveTextContent("struct Queue");
    expect(preview).not.toHaveTextContent("fn other");
    expect(preview).not.toHaveTextContent("// queue");
    expect(link).toHaveAttribute("aria-describedby", preview.id);
    act(() => {
      fireEvent.keyDown(link, { key: "Escape" });
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("previews only the prompt of a quiz", async () => {
    withLinks("As [the question](#order) asks.");
    act(() => {
      fireEvent.focus(screen.getByRole("link", { name: "the question" }));
    });
    const preview = await screen.findByRole("tooltip");
    expect(preview).toHaveTextContent("Which item leaves first?");
    expect(preview).not.toHaveTextContent("The oldest");
    expect(preview).not.toHaveTextContent("Think FIFO");
  });

  it("trims code and diff targets to their range", () => {
    const code = previewNode(queue, { end: 13, start: 11 }) as CodeNode;
    expect(code.content).toBe("struct Queue {\n    items: Vec<u32>,\n}");
    expect(code.first_line).toBe(11);
    expect(code.highlights).toEqual([{ annotation: "The **buffer**.", color: "blue", lines: [{ end: 3, start: 1 }] }]);

    const diff: DiffNode = {
      files: [
        {
          hunks: [
            {
              header: "@@",
              lines: [
                { content: "a", kind: "context", new_line: 1, old_line: 1 },
                { content: "b", kind: "deletion", new_line: null, old_line: 2 },
                { content: "c", kind: "addition", new_line: 2, old_line: null },
                { content: "d", kind: "context", new_line: 3, old_line: 3 },
              ],
            },
          ],
          language: "text",
          new_path: "x",
          old_path: "x",
        },
      ],
      node_id: 2,
      source_id: "d",
      type: "diff",
    };
    const trimmed = previewNode(diff, { end: 2, start: 1 }) as DiffNode;
    expect(trimmed.files[0].hunks[0].lines.map((line) => line.content)).toEqual(["a", "b", "c"]);
  });

  it("jumps through the URL hash and flashes the expanded target", () => {
    render(
      <CollapsibleLessonBlock kind="Code" sourceId="queue-def">
        <p>Body</p>
      </CollapsibleLessonBlock>,
    );
    const block = document.getElementById("block-queue-def")!;
    fireEvent.click(screen.getByRole("button", { name: "Collapse code block queue-def" }));
    expect(screen.getByText("Body")).not.toBeVisible();
    act(() => {
      window.location.hash = "#block-queue-def";
      window.dispatchEvent(new HashChangeEvent("hashchange"));
    });
    expect(screen.getByText("Body")).toBeVisible();
    expect(block).toHaveClass("lesson-block-flash");
  });

  it("renders links inside a preview plainly, without nested previews", async () => {
    const nested: CodeNode = { ...queue, node_id: 0, source_id: "queue-def" };
    render(
      <LinkContext.Provider value={{ inPreview: true, links, nodes: [nested, quiz] }}>
        <Markdown>{"See [the queue](#queue-def)."}</Markdown>
      </LinkContext.Provider>,
    );
    const link = screen.getByRole("link", { name: "the queue" });
    act(() => {
      fireEvent.mouseEnter(link);
    });
    expect(screen.queryByRole("tooltip")).not.toBeInTheDocument();
  });

  it("keeps ordinary links and external links working", () => {
    withLinks("An [external](https://example.com) link and [unknown](#missing).");
    expect(screen.getByRole("link", { name: "external" })).toHaveAttribute("target", "_blank");
    expect(screen.getByText("unknown")).toHaveClass("reference-link-broken");
  });
});
