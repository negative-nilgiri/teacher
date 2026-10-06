import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import { LessonNodeView } from "../components/LessonNodeView";
import type { RunControls } from "../components/RunCodeBlock";
import { LinkContext } from "../links";
import { formatReference } from "../reference";
import type {
  CodeNode,
  LessonNode,
  RunCodeNode,
  RunResult,
  StateResponse,
} from "../types";

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

function view(
  node: LessonNode,
  lesson?: Parameters<typeof LessonNodeView>[0]["lesson"],
  run?: RunControls,
) {
  return render(
    <LinkContext.Provider
      value={{ inPreview: false, links: { shown: { target: 0 } }, nodes }}
    >
      <LessonNodeView
        busy={false}
        lesson={lesson}
        node={node}
        onReveal={noop}
        onSubmit={noop}
        run={run}
      />
    </LinkContext.Provider>,
  );
}

const result: RunResult = {
  duration_ms: 42,
  exit_code: 0,
  stderr: "",
  stdout: "2\n",
  timed_out: false,
  truncated: false,
};

function controls(overrides: Partial<RunControls> = {}): RunControls {
  return { busy: false, enabled: true, onRun: vi.fn(), ...overrides };
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
    // Without run controls there is nothing to press, only a hint.
    expect(screen.queryByRole("button", { name: /^run/i })).not.toBeInTheDocument();
    expect(screen.getByText(/to run this code/)).toBeInTheDocument();
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
      run: { enabled: false, token: null },
      runs: {},
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

describe("running a run block", () => {
  it("shows a hint instead of a button when running is off", () => {
    view(own, undefined, controls({ enabled: false }));
    expect(screen.queryByRole("button", { name: /^run/i })).not.toBeInTheDocument();
    const hint = screen.getByText(/to run this code/);
    expect(hint).toHaveTextContent("Start learn serve --allow-run to run this code.");
    expect(hint.querySelector("code")).toHaveTextContent("learn serve --allow-run");
    // The frozen output is all there is, and it says so.
    expect(screen.getByLabelText("Expected output")).toHaveTextContent("frozen when the lesson was built");
    expect(screen.queryByLabelText("Run output")).not.toBeInTheDocument();
  });

  it("offers an enabled Run button and calls back when pressed", async () => {
    const user = userEvent.setup();
    const onRun = vi.fn();
    view(own, undefined, controls({ onRun }));
    expect(screen.queryByText(/to run this code/)).not.toBeInTheDocument();
    const button = screen.getByRole("button", { name: "Run" });
    expect(button).toBeEnabled();
    await user.click(button);
    expect(onRun).toHaveBeenCalledTimes(1);
    expect(screen.queryByLabelText("Run output")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Expected output")).toBeInTheDocument();
  });

  it("is busy while a run is under way", () => {
    view(own, undefined, controls({ busy: true }));
    const button = screen.getByRole("button", { name: "Running…" });
    expect(button).toBeDisabled();
  });

  it("shows stdout, the exit code and duration, beside the frozen output", () => {
    view(own, undefined, controls({ result }));
    const output = screen.getByLabelText("Run output");
    expect(output).toHaveTextContent("exit code 0");
    expect(output).toHaveTextContent("42 ms");
    expect(screen.getByLabelText("stdout")).toHaveTextContent("2");
    expect(screen.queryByLabelText("stderr")).not.toBeInTheDocument();
    expect(output).not.toHaveTextContent("Timed out");
    expect(output).not.toHaveTextContent("cut at");
    // Both the run and the frozen text are available, as literal text.
    expect(screen.getByLabelText("Expected output")).toHaveTextContent("2 **not bold**");
    expect(screen.getByRole("button", { name: "Run again" })).toBeEnabled();
  });

  it("shows a failing run with stderr set apart from stdout", () => {
    view(
      own,
      undefined,
      controls({
        result: { ...result, duration_ms: 1500, exit_code: 3, stderr: "boom <b>\n", stdout: "before\n" },
      }),
    );
    const output = screen.getByLabelText("Run output");
    expect(output).toHaveTextContent("exit code 3");
    expect(output).toHaveTextContent("1.5 s");
    const stderr = screen.getByLabelText("stderr");
    expect(stderr).toHaveTextContent("boom <b>");
    expect(stderr.closest(".run-stream")).toHaveClass("run-stream-stderr");
    expect(screen.getByLabelText("stdout").closest(".run-stream")).not.toHaveClass(
      "run-stream-stderr",
    );
    expect(output.querySelector("b")).toBeNull();
  });

  it("says when a run timed out and has no exit code", () => {
    view(
      own,
      undefined,
      controls({
        result: { ...result, duration_ms: 10000, exit_code: null, stdout: "", timed_out: true },
      }),
    );
    const output = screen.getByLabelText("Run output");
    expect(output).toHaveTextContent("Timed out after 10 s and was stopped.");
    expect(output).toHaveTextContent("no exit code");
    expect(output).toHaveTextContent("10.0 s");
  });

  it("says when output was truncated", () => {
    view(own, undefined, controls({ result: { ...result, truncated: true } }));
    expect(screen.getByLabelText("Run output")).toHaveTextContent(
      "Output was cut at 64 KiB per stream.",
    );
  });

  it("reports a program that could not start, and a silent one", () => {
    const { unmount } = view(
      own,
      undefined,
      controls({
        result: {
          ...result,
          duration_ms: 0,
          error: "could not start `node`: it is not installed or not on PATH",
          exit_code: null,
          stdout: "",
        },
      }),
    );
    expect(screen.getByRole("alert")).toHaveTextContent("could not start `node`");
    unmount();

    view(own, undefined, controls({ result: { ...result, stdout: "" } }));
    expect(screen.getByLabelText("Run output")).toHaveTextContent("The program printed nothing.");
  });

  it("runs a block of `of` the same way", () => {
    view(ofShown, undefined, controls({ result }));
    expect(screen.getByLabelText("Run: run-shown")).toHaveTextContent("Runs shown");
    expect(screen.getByLabelText("stdout")).toHaveTextContent("2");
    expect(screen.getByRole("button", { name: "Run again" })).toBeInTheDocument();
  });
});

describe("running through the app", () => {
  const lesson = { links: { shown: { target: 0 } }, nodes, title: "Sums" };
  const progress = { completed_questions: 0, questions: {}, total_questions: 0 };
  const enabled = { enabled: true, token: "t0k3n" };

  function state(overrides: Partial<StateResponse> = {}): StateResponse {
    return { lesson, progress, run: enabled, runs: {}, ...overrides };
  }

  afterEach(() => vi.unstubAllGlobals());

  it("posts the token to the node's run route and shows the result", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(Response.json(state()))
      .mockResolvedValueOnce(Response.json({ run: result }));
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    const block = await screen.findByLabelText("Run: run-own");
    await user.click(within(block).getByRole("button", { name: "Run" }));

    expect(await screen.findByLabelText("Run output")).toHaveTextContent("exit code 0");
    expect(fetchMock).toHaveBeenLastCalledWith(
      "/api/v1/runs/1",
      expect.objectContaining({ method: "POST", headers: { "x-learn-token": "t0k3n" } }),
    );
    // Only that block has a result.
    expect(within(screen.getByLabelText("Run: run-shown")).queryByLabelText("Run output")).toBeNull();
    expect(within(block).getByRole("button", { name: "Run again" })).toBeEnabled();
  });

  it("is busy while the request is pending and leaves other blocks alone", async () => {
    let finish: (response: Response) => void = () => {};
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(Response.json(state()))
      .mockReturnValueOnce(new Promise<Response>((resolve) => (finish = resolve)));
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    const own = await screen.findByLabelText("Run: run-own");
    await user.click(within(own).getByRole("button", { name: "Run" }));
    expect(within(own).getByRole("button", { name: "Running…" })).toBeDisabled();
    expect(within(screen.getByLabelText("Run: run-shown")).getByRole("button", { name: "Run" })).toBeEnabled();

    await act(async () => finish(Response.json({ run: result })));
    await waitFor(() =>
      expect(within(own).getByRole("button", { name: "Run again" })).toBeEnabled(),
    );
  });

  it("shows a run from an earlier load of the page", async () => {
    vi.stubGlobal(
      "fetch",
      vi.fn().mockResolvedValue(
        Response.json(state({ runs: { "1": { ...result, stdout: "kept\n" } } })),
      ),
    );
    render(<App />);
    const block = await screen.findByLabelText("Run: run-own");
    expect(within(block).getByLabelText("stdout")).toHaveTextContent("kept");
  });

  it("shows only the hint and the frozen output when running is off", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValue(Response.json(state({ run: { enabled: false, token: null } })));
    vi.stubGlobal("fetch", fetchMock);
    render(<App />);
    await screen.findByLabelText("Run: run-own");
    expect(screen.queryByRole("button", { name: /^run/i })).not.toBeInTheDocument();
    expect(screen.getAllByText(/to run this code/)).toHaveLength(2);
    expect(screen.getAllByLabelText("Expected output").length).toBeGreaterThan(0);
  });

  it("reports a refused run without losing the lesson", async () => {
    vi.stubGlobal(
      "fetch",
      vi
        .fn()
        .mockResolvedValueOnce(Response.json(state()))
        .mockResolvedValueOnce(
          Response.json({ code: "invalid_token", message: "Bad token" }, { status: 403 }),
        ),
    );
    const user = userEvent.setup();
    render(<App />);
    const block = await screen.findByLabelText("Run: run-own");
    await user.click(within(block).getByRole("button", { name: "Run" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Bad token");
    expect(within(block).getByRole("button", { name: "Run" })).toBeEnabled();
    expect(within(block).queryByLabelText("Run output")).toBeNull();
  });
});
