import { act, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import { LessonNodeView } from "../components/LessonNodeView";
import { LinkContext } from "../links";
import { formatReference } from "../reference";
import type { ExternalArtifactNode, LessonNode, MarkdownNode, StateResponse } from "../types";

vi.mock("mermaid", () => ({ default: { initialize: vi.fn(), render: vi.fn() } }));

const noop = async () => {};

const intro: MarkdownNode = {
  content: "A queue releases its oldest item first.",
  node_id: 0,
  reference: { kind: "inline", sha256: "aa" },
  source_id: "intro",
  type: "markdown",
};

const demo: ExternalArtifactNode = {
  alt: "Animation of items entering and leaving a FIFO queue",
  caption: "Watch **which** item leaves first.",
  fallback: "Items leave from the **front**, as [the introduction](#intro) says. Cost: $n^2$.",
  file: "queue-demo.mp4",
  kind: "video",
  node_id: 1,
  source_id: "queue-demo",
  type: "external_artifact",
};

const diagram: ExternalArtifactNode = {
  alt: "Boxes in a row",
  fallback: "Four boxes in a row.",
  file: "queue-diagram.png",
  kind: "image",
  node_id: 2,
  source_id: "queue-diagram",
  type: "external_artifact",
};

const nodes: LessonNode[] = [intro, demo, diagram];

function view(node: LessonNode, lesson?: Parameters<typeof LessonNodeView>[0]["lesson"]) {
  return render(
    <LinkContext.Provider value={{ inPreview: false, links: { intro: { target: 0 }, "queue-demo": { target: 1 } }, nodes }}>
      <LessonNodeView busy={false} lesson={lesson} node={node} onReveal={noop} onSubmit={noop} />
    </LinkContext.Provider>,
  );
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe("external artifact block", () => {
  it("shows the caption and the fallback Markdown, labelled as a text version", () => {
    const { container } = view(demo);
    const block = screen.getByLabelText("Media: queue-demo");
    expect(block.querySelector(".source-caption strong")).toHaveTextContent("which");
    expect(block).toHaveTextContent("The video queue-demo.mp4 is not available here");
    expect(block).toHaveTextContent("text description");
    expect(block).toHaveTextContent("Animation of items entering and leaving a FIFO queue");
    expect(block.querySelector(".external-fallback .markdown strong")).toHaveTextContent("front");
    // KaTeX works in the fallback, like in other Markdown fields.
    expect(container.querySelector(".katex")).toBeInTheDocument();
    // This slice shows no media element at all.
    expect(container.querySelector("video, audio, img, source")).toBeNull();
  });

  it("omits the caption when there is none and names the media kind", () => {
    view(diagram);
    const block = screen.getByLabelText("Media: queue-diagram");
    expect(block.querySelector(".source-caption")).toBeNull();
    expect(block).toHaveTextContent("The image queue-diagram.png is not available here");
    expect(block).toHaveTextContent("Four boxes in a row.");
    expect(screen.getByLabelText("Image block: queue-diagram")).toBeInTheDocument();
  });

  it("links to other blocks from its fallback and previews them", async () => {
    view(demo);
    const link = screen.getByRole("link", { name: "the introduction" });
    expect(link).toHaveAttribute("href", "#block-intro");
    act(() => {
      fireEvent.mouseEnter(link);
    });
    const preview = await screen.findByRole("tooltip");
    expect(preview).toHaveTextContent("A queue releases its oldest item first.");
  });

  it("is previewed as its alt text and fallback when another block links to it", async () => {
    const linking: MarkdownNode = {
      ...intro,
      content: "See [the demo](#queue-demo).",
      node_id: 3,
      source_id: "uses",
    };
    view(linking);
    act(() => {
      fireEvent.mouseEnter(screen.getByRole("link", { name: "the demo" }));
    });
    const preview = await screen.findByRole("tooltip");
    expect(preview).toHaveTextContent("Animation of items entering and leaving a FIFO queue");
    expect(preview).toHaveTextContent("Items leave from the front");
    // The preview carries no notice, caption, or nested link.
    expect(preview).not.toHaveTextContent("is not available here");
    expect(preview).not.toHaveTextContent("Watch which item");
  });

  it("is a block like the others: foldable, with a label and an ask affordance", async () => {
    const user = userEvent.setup();
    view(demo, { title: "Queues" });
    expect(screen.getByLabelText("Video block: queue-demo")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Collapse video block queue-demo" }));
    expect(screen.getByLabelText("Media: queue-demo")).not.toBeVisible();
    await user.click(screen.getByRole("button", { name: "Expand video block queue-demo" }));
    expect(screen.getByLabelText("Media: queue-demo")).toBeVisible();
    expect(screen.getByRole("button", { name: "Ask about block queue-demo" })).toBeInTheDocument();
  });

  it("formats agent references that name the block and its fallback text", () => {
    const lesson = { artifact_path: "queues.learn", lesson_path: "queues.json", title: "Queues" };
    expect(formatReference(lesson, demo)).toBe(
      [
        'Question about lesson "Queues" (queues.json), block `queue-demo` (external media):',
        "- shown: fallback text of video queue-demo.mp4",
        "- version: part of the lesson source",
        "- exact shown text: block `queue-demo` in queues.learn",
      ].join("\n"),
    );
  });

  it("renders in a served lesson with only its fallback", async () => {
    const state: StateResponse = {
      lesson: { links: { intro: { target: 0 } }, nodes, title: "Queues" },
      progress: { completed_questions: 0, questions: {}, total_questions: 0 },
      run: { enabled: false, token: null },
      runs: {},
    };
    const fetchMock = vi.fn().mockResolvedValue(Response.json(state));
    vi.stubGlobal("fetch", fetchMock);
    const { container } = render(<App />);
    expect(await screen.findByLabelText("Media: queue-demo")).toHaveTextContent(
      "Items leave from the front",
    );
    expect(document.getElementById("block-queue-demo")).toBeInTheDocument();
    expect(container.querySelector("video, audio, img")).toBeNull();
    // Nothing but the state was requested: no media file is fetched.
    expect(fetchMock.mock.calls.map(([url]) => String(url))).toEqual(["/api/v1/state"]);
  });
});
