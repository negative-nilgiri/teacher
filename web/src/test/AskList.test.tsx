import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AskListPanel, AskListProvider } from "../askList";
import { LessonNodeView } from "../components/LessonNodeView";
import type { CodeNode, LessonNode, MultipleChoiceNode } from "../types";

const lesson = {
  artifact_path: "lessons/queue/lesson.learn",
  lesson_path: "lessons/queue/lesson.json",
  title: "Queues",
};

const code: CodeNode = {
  content: "fn pop() {}\n",
  first_line: 40,
  highlights: [],
  language: "rust",
  node_id: 1,
  reference: { blob_id: "3f2a91c0", head: "5d11bde0", kind: "file", path: "src/queue.rs", sha256: "aa" },
  source_id: "pop",
  type: "code",
};

const quiz: MultipleChoiceNode = {
  choices: [{ choice_id: 0, content: "The oldest" }],
  hints: [],
  node_id: 2,
  prompt: "Which?",
  source_id: "order",
  type: "multiple_choice",
};

const nodes: LessonNode[] = [code, quiz];
const noop = async () => {};

function page() {
  return (
    <AskListProvider lesson={lesson}>
      {nodes.map((node) => (
        <LessonNodeView busy={false} key={node.node_id} lesson={lesson} node={node} onReveal={noop} onSubmit={noop} />
      ))}
      <AskListPanel lesson={lesson} nodes={nodes} />
    </AskListProvider>
  );
}

async function addAsk(user: ReturnType<typeof userEvent.setup>, block: string, question: string) {
  await user.click(screen.getByRole("button", { name: `Ask about block ${block}` }));
  if (question) await user.type(screen.getByRole("textbox", { name: /Question/ }), question);
  await user.click(screen.getByRole("button", { name: "Add to list" }));
}

afterEach(() => window.sessionStorage.clear());

describe("collected questions", () => {
  it("shows nothing until a question is added, and keeps Copy reference as it was", async () => {
    const user = userEvent.setup();
    render(page());
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Ask about block pop" }));
    expect(screen.getByRole("button", { name: "Copy reference" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add to list" })).toBeInTheDocument();
  });

  it("adds, edits and removes questions, then copies them all", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async (_text: string) => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(page());

    await addAsk(user, "pop", "Why the front?");
    expect(screen.getByRole("status")).toHaveTextContent("Added to list");
    await addAsk(user, "order", "Which one is it?");
    await addAsk(user, "pop", "Dropped later");

    await user.click(screen.getByRole("button", { name: "Asks (3)" }));
    const panel = screen.getByRole("complementary", { name: "Collected questions" });
    const fields = within(panel).getAllByRole("textbox");
    expect(fields.map((field) => (field as HTMLTextAreaElement).value)).toEqual([
      "Why the front?",
      "Which one is it?",
      "Dropped later",
    ]);

    await user.type(fields[0], " Really?");
    expect(within(panel).getAllByRole("textbox")[0]).toHaveValue("Why the front? Really?");
  });

  it("numbers every entry in the copied text, without content or answers", async () => {
    const user = userEvent.setup();
    const writeText = vi.fn(async (_text: string) => {});
    Object.defineProperty(navigator, "clipboard", { configurable: true, value: { writeText } });
    render(page());
    await addAsk(user, "pop", "Why the front?");
    await addAsk(user, "order", "");

    await user.click(screen.getByRole("button", { name: "Asks (2)" }));
    await user.click(screen.getByRole("button", { name: "Copy all" }));

    const text = writeText.mock.calls[0][0];
    expect(text).toContain('[1/2] Question about lesson "Queues" (lessons/queue/lesson.json), block `pop` (code):');
    expect(text).toContain("My question: Why the front?");
    expect(text).toContain("[2/2] Question about lesson \"Queues\"");
    expect(text).toContain("block `order` (question)");
    expect(text).not.toContain("fn pop");
    expect(text).not.toContain("oldest");
    expect(text.split("\n\n")).toHaveLength(2);
    expect(screen.getByRole("button", { name: /Asks \(2\)/ })).toHaveTextContent("copied");
  });

  it("removes one question, clears the rest, and survives a remount", async () => {
    const user = userEvent.setup();
    const first = render(page());
    await addAsk(user, "pop", "One");
    await addAsk(user, "order", "Two");
    first.unmount();

    render(page());
    await user.click(screen.getByRole("button", { name: "Asks (2)" }));
    await user.click(screen.getByRole("button", { name: "Remove question about block pop" }));
    expect(screen.getByRole("button", { name: "Asks (1)" })).toBeInTheDocument();
    expect(screen.getByRole("textbox")).toHaveValue("Two");

    await user.click(screen.getByRole("button", { name: "Clear" }));
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("selects the text to copy by hand when the clipboard is blocked", async () => {
    const user = userEvent.setup();
    Object.defineProperty(navigator, "clipboard", {
      configurable: true,
      value: { writeText: vi.fn(async () => Promise.reject(new Error("denied"))) },
    });
    render(page());
    await addAsk(user, "pop", "Why?");
    await user.click(screen.getByRole("button", { name: "Asks (1)" }));
    await user.click(screen.getByRole("button", { name: "Copy all" }));

    const manual = screen.getByRole("textbox", { name: "Questions to copy" }) as HTMLTextAreaElement;
    expect(manual.value).toContain("[1/1] Question about lesson");
  });
});
