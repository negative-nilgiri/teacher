import { act, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { LessonNodeView } from "../components/LessonNodeView";
import { formatReference } from "../reference";
import type { LessonNode, MarkdownNode, MultipleChoiceNode } from "../types";

const lesson = { artifact_path: "lessons/queue/lesson.learn", title: "Queues" };
const noop = async () => {};

const prose: MarkdownNode = {
  content: "The queue keeps order.\n\nIt grows as $x^2$ items arrive.\n\n- first\n- second\n",
  node_id: 0,
  source_id: "intro",
  type: "markdown",
};

const quiz: MultipleChoiceNode = {
  choices: [
    { choice_id: 0, content: "The oldest item" },
    { choice_id: 1, content: "The newest item" },
  ],
  hints: ["Think about FIFO."],
  node_id: 1,
  prompt: "Which item leaves first?",
  source_id: "order",
  type: "multiple_choice",
};

function textNode(root: Element, containing: string): Text {
  const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
  while (walker.nextNode()) {
    if ((walker.currentNode as Text).data.includes(containing)) return walker.currentNode as Text;
  }
  throw new Error(`no text containing ${containing}`);
}

function select(start: Node, startOffset: number, end: Node, endOffset: number) {
  const range = document.createRange();
  range.setStart(start, startOffset);
  range.setEnd(end, endOffset);
  act(() => {
    window.getSelection()!.removeAllRanges();
    window.getSelection()!.addRange(range);
    document.dispatchEvent(new Event("selectionchange"));
  });
}

function show(node: LessonNode) {
  return render(<LessonNodeView busy={false} lesson={lesson} node={node} onReveal={noop} onSubmit={noop} />);
}

describe("selecting text in Markdown blocks and questions", () => {
  it("copies the selected rendered text, with math as its TeX source", async () => {
    const writeText = vi.fn(async (_text: string) => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const { container } = show(prose);
    const first = textNode(container, "The queue");
    const last = textNode(container, "second");
    select(first, 4, last, 6);

    fireEvent.click(await screen.findByRole("button", { name: "Ask" }));
    expect(screen.getByText("Includes your selected text.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy reference" }));
    await vi.waitFor(() => expect(writeText).toHaveBeenCalled());

    const text = writeText.mock.calls[0][0];
    expect(text).toContain("block `intro` (Markdown)");
    expect(text).toContain("- selected text, as rendered:");
    expect(text).toContain("> queue keeps order.\n>\n> It grows as $x^2$ items arrive.");
    // The rendered math selects twice (visual and MathML); it must appear once.
    expect(text.match(/x\^2/g)).toHaveLength(1);
    expect(text).toContain("> second");
    expect(text).not.toContain("shown:");
  });

  it("offers no chip when nothing is selected or the selection is outside the prose", () => {
    const { container } = show(prose);
    expect(screen.queryByRole("button", { name: "Ask" })).not.toBeInTheDocument();
    const heading = container.querySelector(".lesson-block-header, header");
    if (heading?.firstChild) select(heading.firstChild, 0, heading.firstChild, 1);
    expect(screen.queryByRole("button", { name: "Ask" })).not.toBeInTheDocument();
  });

  it("allows the prompt and hints of a question but never its choices", async () => {
    const { container } = show(quiz);
    const prompt = textNode(container, "Which item");
    select(prompt, 0, prompt, 5);
    expect(await screen.findByRole("button", { name: "Ask" })).toBeInTheDocument();

    const choice = textNode(container, "The oldest item");
    select(choice, 0, choice, 3);
    expect(screen.queryByRole("button", { name: "Ask" })).not.toBeInTheDocument();

    // A selection from the prompt into a choice spans two regions.
    select(prompt, 0, choice, 3);
    expect(screen.queryByRole("button", { name: "Ask" })).not.toBeInTheDocument();

    fireEvent.click(screen.getByText(/Hints/));
    const hint = textNode(container, "FIFO");
    select(hint, 0, hint, 5);
    expect(await screen.findByRole("button", { name: "Ask" })).toBeInTheDocument();
  });

  it("quotes selected text in the reference and leaves other references alone", () => {
    expect(formatReference(lesson, prose, { kind: "text", text: "a\n\nb" }, "Why?")).toBe(
      [
        'Question about lesson "Queues", block `intro` (Markdown):',
        "- version: part of the lesson source",
        "- exact shown text: block `intro` in lessons/queue/lesson.learn",
        "- selected text, as rendered:",
        "> a",
        ">",
        "> b",
        "My question: Why?",
      ].join("\n"),
    );
    expect(formatReference(lesson, prose)).not.toContain("selected");
  });
});
