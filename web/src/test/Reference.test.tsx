import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { LessonNodeView } from "../components/LessonNodeView";
import { formatReference } from "../reference";
import type { CodeNode, DiffNode, LessonNode, MultipleChoiceNode } from "../types";

const lesson = {
  artifact_path: "lessons/queue/lesson.learn",
  lesson_path: "lessons/queue/lesson.json",
  title: "Queues",
};

const code: CodeNode = {
  content: "fn pop() {\n    self.items.pop_front()\n}\n",
  first_line: 40,
  highlights: [{ color: "yellow", lines: [{ end: 2, start: 2 }] }],
  language: "rust",
  node_id: 1,
  reference: {
    blob_id: "3f2a91c0",
    head: "5d11bde0",
    kind: "file",
    path: "src/queue.rs",
    sha256: "aa",
  },
  source_id: "pop",
  type: "code",
};

const noop = async () => {};

describe("lesson references", () => {
  it("formats every kind of source without content or answers", () => {
    expect(formatReference(lesson, code, { end: 41, group: 0, kind: "code", start: 41 }, "Why front?")).toBe(
      [
        'Question about lesson "Queues" (lessons/queue/lesson.json), block `pop` (code):',
        "- shown: src/queue.rs, lines 40–42; selected line 41 (highlight group 0)",
        "- version: worktree file at build time, blob 3f2a91c0, on top of commit 5d11bde0",
        "- exact shown text: block `pop` in lessons/queue/lesson.learn",
        "My question: Why front?",
      ].join("\n"),
    );

    const blob: LessonNode = {
      ...code,
      reference: {
        content_object_id: "b10b",
        kind: "git_blob",
        path: "src/queue.rs",
        repository: ".",
        revision: "HEAD~2",
        revision_object_id: "c0ffee",
        sha256: "aa",
      },
    };
    expect(formatReference(lesson, blob)).toContain(
      '- version: commit c0ffee (revision "HEAD~2"), blob b10b',
    );

    const diff: DiffNode = {
      files: [],
      node_id: 2,
      reference: {
        base_object_id: "base1",
        base_revision: "main",
        files: ["docs/SETUP.md"],
        head: "head1",
        kind: "git_diff",
        repository: ".",
        sha256: "aa",
        target: { kind: "worktree" },
        worktree_blob_ids: { "docs/SETUP.md": "blob1" },
      },
      source_id: "setup",
      type: "diff",
    };
    expect(
      formatReference(lesson, diff, { kind: "diff", new: [3, 5], path: "docs/SETUP.md" }),
    ).toBe(
      [
        'Question about lesson "Queues" (lessons/queue/lesson.json), block `setup` (diff):',
        "- shown: docs/SETUP.md; selected docs/SETUP.md new lines 3–5",
        '- version: diff of base base1 (revision "main") → worktree at build time (docs/SETUP.md blob blob1), on top of commit head1',
        "- exact shown text: block `setup` in lessons/queue/lesson.learn",
      ].join("\n"),
    );
    const committed: DiffNode = {
      ...diff,
      reference: {
        ...(diff.reference as Extract<DiffNode["reference"], { kind: "git_diff" }>),
        target: { kind: "revision", object_id: "t1", revision: "feature" },
      },
    };
    expect(formatReference(lesson, committed)).toContain(
      '→ target t1 (revision "feature")',
    );

    const quiz: MultipleChoiceNode = {
      choices: [{ choice_id: 0, content: "The oldest" }],
      hints: [],
      node_id: 3,
      prompt: "Which?",
      source_id: "order",
      type: "multiple_choice",
    };
    const quizText = formatReference(lesson, quiz, undefined, "  ");
    expect(quizText).toContain("block `order` (question)");
    expect(quizText).toContain("- version: part of the lesson source");
    expect(quizText).not.toContain("oldest");
    expect(quizText).not.toContain("My question");

    const inline: LessonNode = { ...code, first_line: undefined, reference: { kind: "inline", sha256: "aa" } };
    expect(formatReference({ title: "T" }, inline)).toBe(
      ['Question about lesson "T", block `pop` (code):', "- version: inline in the lesson source"].join("\n"),
    );
  });

  it("copies a reference from a discreet popover", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(
      <LessonNodeView busy={false} lesson={lesson} node={code} onReveal={noop} onSubmit={noop} />,
    );

    const trigger = screen.getByRole("button", { name: "Ask about block pop" });
    expect(trigger).toHaveClass("lesson-block-ask");
    expect(trigger).toHaveAttribute("aria-expanded", "false");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    await user.click(trigger);
    const field = screen.getByRole("textbox", { name: /Question/ });
    expect(field).toHaveFocus();
    await user.type(field, "Why the front?{Enter}");
    expect(writeText).toHaveBeenCalledWith(expect.stringContaining("My question: Why the front?"));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Copied");
    expect(trigger).toHaveFocus();

    await user.click(trigger);
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("falls back to a selected reference when copying is blocked", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn(async () => Promise.reject(new Error("denied"))) },
    });
    render(
      <LessonNodeView busy={false} lesson={lesson} node={code} onReveal={noop} onSubmit={noop} />,
    );
    await user.click(screen.getByRole("button", { name: "Ask about block pop" }));
    await user.click(screen.getByRole("button", { name: "Copy reference" }));
    const manual = screen.getByRole("textbox", { name: "Reference to copy" });
    expect(manual).toHaveAttribute("readonly");
    expect((manual as HTMLTextAreaElement).value).toContain("block `pop` (code)");
  });

  it("turns a selection of code lines into source lines and a highlight group", async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const { container } = render(
      <LessonNodeView busy={false} lesson={lesson} node={code} onReveal={noop} onSubmit={noop} />,
    );
    const source = container.querySelector(".code-listing-source")!;
    const walker = document.createTreeWalker(source, NodeFilter.SHOW_TEXT);
    const texts: Text[] = [];
    while (walker.nextNode()) texts.push(walker.currentNode as Text);
    const second = texts.find((text) => text.data.includes("pop_front"))!;
    const range = document.createRange();
    range.setStart(second, 0);
    range.setEnd(second, second.data.length);
    act(() => {
      window.getSelection()!.removeAllRanges();
      window.getSelection()!.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });

    fireEvent.click(await screen.findByRole("button", { name: "Ask" }));
    expect(screen.getByText("Includes your selected lines.")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Copy reference" }));
    await vi.waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining("selected line 41 (highlight group 0)"),
      ),
    );
  });

  it("turns a selection of diff rows into old and new line ranges", async () => {
    const writeText = vi.fn(async () => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    const diff: DiffNode = {
      files: [
        {
          hunks: [
            {
              header: "@@ -3,2 +3,2 @@",
              lines: [
                { content: "keep", kind: "context", new_line: 3, old_line: 3 },
                { content: "old value", kind: "deletion", new_line: null, old_line: 4 },
                { content: "new value", kind: "addition", new_line: 4, old_line: null },
              ],
            },
          ],
          language: "text",
          new_path: "notes.txt",
          old_path: "notes.txt",
        },
      ],
      node_id: 5,
      reference: {
        base_object_id: "b",
        base_revision: "HEAD",
        files: ["notes.txt"],
        kind: "git_diff",
        repository: ".",
        sha256: "aa",
        target: { kind: "revision", object_id: "t", revision: "main" },
      },
      source_id: "notes",
      type: "diff",
    };
    render(<LessonNodeView busy={false} lesson={lesson} node={diff} onReveal={noop} onSubmit={noop} />);
    const from = screen.getByText("keep").firstChild ?? screen.getByText("keep");
    const to = screen.getByText("new value").firstChild ?? screen.getByText("new value");
    const range = document.createRange();
    range.setStart(from, 0);
    range.setEnd(to, 3);
    act(() => {
      window.getSelection()!.removeAllRanges();
      window.getSelection()!.addRange(range);
      document.dispatchEvent(new Event("selectionchange"));
    });
    fireEvent.click(await screen.findByRole("button", { name: "Ask" }));
    fireEvent.click(screen.getByRole("button", { name: "Copy reference" }));
    await vi.waitFor(() =>
      expect(writeText).toHaveBeenCalledWith(
        expect.stringContaining("selected notes.txt old lines 3–4, new lines 3–4"),
      ),
    );
  });
});
