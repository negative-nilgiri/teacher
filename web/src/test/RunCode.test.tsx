import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { App } from "../App";
import { LessonNodeView } from "../components/LessonNodeView";
import { LinkContext } from "../links";
import { formatReference } from "../reference";
import type { CodeNode, LessonNode, RunCodeNode, StateResponse } from "../types";

vi.mock("mermaid", () => ({ default: { initialize: vi.fn(), render: vi.fn() } }));

const noop = async () => {};

const shown: CodeNode = {
  content: "print(1 + 1)\n",
  first_line: 3,
  language: "python",
  node_id: 0,
  reference: { kind: "file", path: "tools/sum.py", sha256: "aa", blob_id: "b10b" },
  source_id: "shown",
  type: "code",
};

const own: RunCodeNode = {
  caption: "Run it and **compare**.",
  content: "const total = 1 + 1;\nconsole.log(total);\n",
  expected_output: "2\n**not bold**\n",
  filename: "sum.js",
  first_line: 5,
  language: "javascript",
  node_id: 1,
  reference: { kind: "file", path: "web/sum.js", sha256: "bb" },
  source_id: "run-own",
  timeout_secs: 10,
  type: "run_code",
};

const ofShown: RunCodeNode = {
  expected_output: "2\n",
  language: "python",
  node_id: 2,
  of: 0,
  reference: shown.reference,
  source_id: "run-shown",
  timeout_secs: 10,
  type: "run_code",
};

const nodes: LessonNode[] = [shown, own, ofShown];

function view(node: LessonNode, lesson?: Parameters<typeof LessonNodeView>[0]["lesson"]) {
  return render(
    <LinkContext.Provider
      value={{ inPreview: false, links: { shown: { target: 0 } }, nodes }}
    >
      <LessonNodeView busy={false} lesson={lesson} node={node} onReveal={noop} onSubmit={noop} />
    </LinkContext.Provider>,
  );
}

describe("run code block", () => {
  it("shows its caption, highlighted code, and the frozen expected output", () => {
    const { container } = view(own);
    const block = screen.getByLabelText("Run: run-own");
    expect(block).toHaveTextContent("Run it and compare.");
    expect(block.querySelector("strong")).toHaveTextContent("compare");
    expect(screen.getByLabelText("Code: run-own")).toHaveTextContent("console.log(total);");
    expect(screen.getByText("sum.js")).toBeInTheDocument();
    expect(container.querySelector("code.language-javascript .hljs-keyword")).toHaveTextContent(
      "const",
    );
    // File-backed excerpts show the source-file line numbers.
    expect(container.querySelector(".code-line-numbers")).toHaveTextContent("5");

    const output = screen.getByLabelText("Expected output");
    expect(output).toHaveTextContent("Expected output");
    expect(output).toHaveTextContent("frozen when the lesson was built");
    // The output is literal text, never Markdown.
    expect(output.querySelector("pre")).toHaveTextContent("2 **not bold**");
    expect(output.querySelector("strong")).toHaveTextContent("Expected output");
    expect(output.querySelectorAll("strong")).toHaveLength(1);
    // Nothing runs in this version, so there is no Run control.
    expect(screen.queryByRole("button", { name: /^run/i })).not.toBeInTheDocument();
  });

  it("points at the block it runs instead of repeating its code", () => {
    const { container } = view(ofShown);
    const block = screen.getByLabelText("Run: run-shown");
    expect(block).toHaveTextContent("Runs shown");
    expect(block.querySelector(".code-block")).toBeNull();
    expect(container.querySelector("pre.code-listing")).toBeNull();
    const link = screen.getByRole("link", { name: "shown" });
    expect(link).toHaveAttribute("href", "#block-shown");
    expect(link.querySelector("code")).toHaveTextContent("shown");
    expect(screen.getByLabelText("Expected output")).toHaveTextContent("2");
  });

  it("previews the block it runs on hover", async () => {
    view(ofShown);
    act(() => {
      fireEvent.mouseEnter(screen.getByRole("link", { name: "shown" }));
    });
    const preview = await screen.findByRole("tooltip");
    expect(preview).toHaveTextContent("shown");
    expect(preview).toHaveTextContent("print(1 + 1)");
  });

  it("omits the output section when no output was recorded", () => {
    view({ ...own, expected_output: undefined });
    expect(screen.queryByLabelText("Expected output")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Code: run-own")).toBeInTheDocument();
  });

  it("is a block like the others: foldable, with a label and an ask affordance", async () => {
    const user = userEvent.setup();
    view(own, { title: "Sums" });
    expect(screen.getByLabelText("Run code block: run-own")).toBeInTheDocument();
    const toggle = screen.getByRole("button", { name: "Collapse run code block run-own" });
    await user.click(toggle);
    expect(screen.getByLabelText("Run: run-own")).not.toBeVisible();
    await user.click(screen.getByRole("button", { name: "Expand run code block run-own" }));
    expect(screen.getByLabelText("Run: run-own")).toBeVisible();
    expect(screen.getByRole("button", { name: "Ask about block run-own" })).toBeInTheDocument();
  });

  it("formats agent references like code blocks, naming the file it runs", () => {
    const lesson = { artifact_path: "sums.learn", lesson_path: "sums.json", title: "Sums" };
    expect(formatReference(lesson, own)).toBe(
      [
        'Question about lesson "Sums" (sums.json), block `run-own` (runnable code):',
        "- shown: web/sum.js, lines 5–6",
        "- version: worktree file at build time, sha256 bb",
        "- exact shown text: block `run-own` in sums.learn",
      ].join("\n"),
    );
    // A block of `of` has no lines of its own; it refers to the code's file.
    expect(formatReference(lesson, ofShown)).toContain("- shown: tools/sum.py\n");
    expect(
      formatReference(lesson, { ...own, reference: { kind: "inline", sha256: "cc" } }),
    ).toContain("- version: inline in the lesson source");
  });

  it("renders in a served lesson next to the block it runs", async () => {
    const state: StateResponse = {
      lesson: {
        links: { shown: { target: 0 } },
        nodes,
        title: "Sums",
      },
      progress: { completed_questions: 0, questions: {}, total_questions: 0 },
    };
    vi.stubGlobal("fetch", vi.fn().mockResolvedValue(Response.json(state)));
    render(<App />);
    expect(await screen.findByLabelText("Run: run-own")).toBeInTheDocument();
    expect(screen.getByLabelText("Run: run-shown")).toHaveTextContent("Runs shown");
    expect(document.getElementById("block-run-shown")).toBeInTheDocument();
    expect(document.getElementById("block-shown")).toBeInTheDocument();
    vi.unstubAllGlobals();
  });
});
