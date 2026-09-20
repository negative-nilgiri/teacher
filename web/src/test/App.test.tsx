import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import type { StateResponse } from "../types";

const initialState: StateResponse = {
  lesson: {
    title: "Queue changes",
    nodes: [
      { node_id: 0, source_id: "intro", type: "markdown", content: "## Why this changed" },
      { node_id: 2, source_id: "queue-code", type: "code", content: "queue.push_back(item);", language: "rust" },
      {
        node_id: 3,
        source_id: "queue-diff",
        type: "diff",
        files: [
          {
            old_path: "src/queue.rs",
            new_path: "src/queue.rs",
            language: "rust",
            hunks: [
              {
                header: "@@ -1 +1 @@",
                lines: [
                  { kind: "deletion", content: "stack.push(item);", old_line: 1, new_line: null },
                  { kind: "addition", content: "queue.push_back(item);", old_line: null, new_line: 1 },
                ],
              },
            ],
          },
        ],
      },
      {
        node_id: 1,
        source_id: "quiz",
        type: "multiple_choice",
        prompt: "Which order is **FIFO**?",
        choices: [
          { choice_id: 0, content: "First in, first out" },
          { choice_id: 1, content: "Last in, first out" },
        ],
        hints: ["Think about a real queue."],
      },
    ],
  },
  progress: {
    completed_questions: 0,
    total_questions: 1,
    questions: {
      "1": { attempts: [], completed: false, revealed: false },
    },
  },
};

afterEach(() => vi.unstubAllGlobals());

describe("App", () => {
  it("bootstraps and renders the public lesson", async () => {
    const fetchMock = vi.fn().mockResolvedValue(Response.json(initialState));
    vi.stubGlobal("fetch", fetchMock);

    render(<App />);

    expect(await screen.findByRole("heading", { name: "Queue changes" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Why this changed" })).toBeInTheDocument();
    expect(screen.getByLabelText("Code: queue-code")).toHaveTextContent("queue.push_back(item);");
    expect(screen.getByRole("button", { name: "Collapse rust block queue-code" })).toBeInTheDocument();
    expect(screen.getByLabelText("Diff: queue-diff")).toHaveTextContent("stack.push(item);");
    expect(screen.getByText("0 of 1 questions completed")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenCalledWith("/api/v1/state", expect.objectContaining({ signal: expect.any(AbortSignal) }));
  });

  it("folds blocks independently without discarding their local state", async () => {
    const fetchMock = vi.fn().mockResolvedValue(Response.json(initialState));
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    const markdownHeading = await screen.findByRole("heading", { name: "Why this changed" });
    const code = screen.getByLabelText("Code: queue-code");
    const markdownToggle = screen.getByRole("button", {
      name: "Collapse markdown block intro",
    });

    await user.click(markdownToggle);
    expect(markdownHeading).not.toBeVisible();
    expect(code).toBeVisible();
    expect(markdownToggle).toHaveAttribute("aria-expanded", "false");

    await user.click(screen.getByRole("button", { name: "Expand markdown block intro" }));
    expect(markdownHeading).toBeVisible();

    const choice = screen.getByRole("radio", { name: "First in, first out" });
    await user.click(choice);
    await user.click(screen.getByRole("button", { name: "Collapse question block quiz" }));
    expect(choice).not.toBeVisible();
    await user.click(screen.getByRole("button", { name: "Expand question block quiz" }));
    expect(choice).toBeChecked();
  });

  it("submits a generated choice id and adopts server-owned progress", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(Response.json(initialState))
      .mockResolvedValueOnce(
        Response.json({
          progress: {
            completed_questions: 1,
            total_questions: 1,
            questions: {},
          },
          question: {
            attempts: [{ choice_id: 0, correct: true }],
            completed: true,
            revealed: false,
            answer: { choice_id: 0, explanation: "A queue removes the oldest item first." },
          },
        }),
      );
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("radio", { name: "First in, first out" }));
    await user.click(screen.getByRole("button", { name: "Check answer" }));

    await waitFor(() => expect(screen.getByText("1 of 1 questions completed")).toBeInTheDocument());
    expect(screen.getByText("Correct.")).toBeInTheDocument();
    expect(screen.getByText("A queue removes the oldest item first.")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenLastCalledWith(
      "/api/v1/questions/1/submit",
      expect.objectContaining({ method: "POST", body: JSON.stringify({ choice_id: 0 }) }),
    );
  });

  it("does not receive or infer an answer after an incorrect attempt", async () => {
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(Response.json(initialState))
      .mockResolvedValueOnce(
        Response.json({
          progress: { ...initialState.progress, questions: {} },
          question: {
            attempts: [{ choice_id: 1, correct: false }],
            completed: false,
            revealed: false,
          },
        }),
      );
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("radio", { name: "Last in, first out" }));
    await user.click(screen.getByRole("button", { name: "Check answer" }));

    expect(await screen.findByText("Not quite. Try another answer or use a hint.")).toBeInTheDocument();
    expect(screen.queryByText("Answer revealed.")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Check answer" })).toBeEnabled();
  });

  it("explicitly reveals an answer and accepts a full-state replacement", async () => {
    const revealedState: StateResponse = {
      ...initialState,
      progress: {
        ...initialState.progress,
        questions: {
          "1": {
            attempts: [],
            completed: false,
            revealed: true,
            answer: { choice_id: 0, explanation: "The oldest item leaves first." },
          },
        },
      },
    };
    const fetchMock = vi
      .fn()
      .mockResolvedValueOnce(Response.json(initialState))
      .mockResolvedValueOnce(Response.json(revealedState));
    vi.stubGlobal("fetch", fetchMock);
    const user = userEvent.setup();
    render(<App />);

    await user.click(await screen.findByRole("button", { name: "Reveal answer" }));

    expect(await screen.findByText("Answer revealed.")).toBeInTheDocument();
    expect(screen.getByText("The oldest item leaves first.")).toBeInTheDocument();
    expect(fetchMock).toHaveBeenLastCalledWith(
      "/api/v1/questions/1/reveal",
      expect.objectContaining({ method: "POST" }),
    );
  });
});
