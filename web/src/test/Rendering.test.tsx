import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodeBlock } from "../components/CodeBlock";
import { DiffBlock } from "../components/DiffBlock";
import { LessonNodeView } from "../components/LessonNodeView";
import { Markdown } from "../components/Markdown";

const mermaidMocks = vi.hoisted(() => ({
  initialize: vi.fn(),
  render: vi.fn(),
}));

vi.mock("mermaid", () => ({ default: mermaidMocks }));

beforeEach(() => {
  mermaidMocks.initialize.mockClear();
  mermaidMocks.render.mockReset();
});

describe("semantic source rendering", () => {
  it("renders inline and display mathematics with accessible MathML", () => {
    const { container } = render(
      <Markdown>
        {"Euler's identity is $e^{i\\pi} + 1 = 0$.\n\n$$\n\\sum_{k=1}^{n} k = \\frac{n(n+1)}{2}\n$$"}
      </Markdown>,
    );

    expect(container.querySelectorAll(".katex")).toHaveLength(2);
    expect(container.querySelector(".katex-display")).toBeInTheDocument();
    expect(container.querySelectorAll("math")).toHaveLength(2);
  });

  it("keeps malformed mathematics visible without aborting Markdown rendering", () => {
    const { container } = render(
      <Markdown>{"Before $\\definitelyUnknown{x}$ after."}</Markdown>,
    );

    expect(screen.getByText("Before", { exact: false })).toHaveTextContent("after.");
    expect(container).toHaveTextContent("\\definitelyUnknown{x}");
  });

  it("does not enable trusted KaTeX commands or raw Markdown HTML", () => {
    const { container } = render(
      <Markdown>{'$\\href{https://example.com}{link}$ <img src="x" alt="raw">'}</Markdown>,
    );

    expect(container.querySelector("a")).not.toBeInTheDocument();
    expect(container.querySelector("img")).not.toBeInTheDocument();
  });

  it("falls back to a generic fold label for an unknown code language", () => {
    render(
      <LessonNodeView
        busy={false}
        node={{
          content: "some source",
          language: "future-language",
          node_id: 0,
          source_id: "future-code",
          type: "code",
        }}
        onReveal={vi.fn()}
        onSubmit={vi.fn()}
      />,
    );

    expect(
      screen.getByRole("button", { name: "Collapse code block future-code" }),
    ).toBeInTheDocument();
  });

  it("highlights a code block with its compiled language", () => {
    const { container } = render(
      <CodeBlock
        node={{
          content: "fn main() { let answer = 42; }",
          filename: "main.rs",
          language: "rust",
          node_id: 1,
          source_id: "main",
          type: "code",
        }}
      />,
    );

    expect(screen.getByLabelText("Language: Rust")).toHaveTextContent("Rust");
    expect(screen.getByText("main.rs")).toHaveClass("source-file-name");
    expect(container.querySelector("code.language-rust")).toHaveTextContent("fn main() { let answer = 42; }");
    expect(container.querySelector(".hljs-keyword")).toHaveTextContent("fn");
  });

  it("renders normalized multi-color line highlights without breaking multiline syntax", () => {
    const { container } = render(
      <CodeBlock
        node={{
          content: "/* first\nsecond */\nfn main() {}\nlet answer = 42;\nanswer\n",
          highlights: [
            {
              annotation: "The comment documents the **shared invariant**.",
              color: "yellow",
              lines: [{ end: 2, start: 1 }],
            },
            { color: "blue", lines: [{ end: 4, start: 4 }] },
            { color: "green", lines: [{ end: 3, start: 3 }] },
            { color: "red", lines: [{ end: 5, start: 5 }] },
          ],
          language: "rust",
          node_id: 2,
          source_id: "highlighted",
          type: "code",
        }}
      />,
    );

    const bands = container.querySelectorAll(".code-highlight");
    expect(bands).toHaveLength(4);
    expect(bands[0]).toHaveClass("code-highlight-yellow");
    expect(bands[0]).toHaveAttribute("data-start", "1");
    expect(bands[0]).toHaveAttribute("data-end", "2");
    expect(bands[1]).toHaveClass("code-highlight-blue");
    expect(bands[2]).toHaveClass("code-highlight-green");
    expect(bands[3]).toHaveClass("code-highlight-red");
    expect(container.querySelector(".code-line-numbers")).toHaveTextContent("1 2 3 4 5");
    expect(container.querySelector(".hljs-comment")).toHaveTextContent("/* first second */");
    expect(screen.getByText("Yellow · Lines 1–2")).toBeInTheDocument();
    expect(screen.getByLabelText("Highlighted code explanations")).toHaveTextContent(
      "The comment documents the shared invariant.",
    );
    expect(screen.getByText("shared invariant").tagName).toBe("STRONG");
    expect(screen.getByText(/Highlighted lines 1–2 in yellow/)).toBeInTheDocument();
  });

  it("folds files independently in a multi-file diff", async () => {
    const user = userEvent.setup();
    render(
      <DiffBlock
        node={{
          files: [
            {
              hunks: [{
                header: "@@ -1 +1 @@",
                lines: [{ content: "first change", kind: "addition", new_line: 1, old_line: null }],
              }],
              language: "rust",
              new_path: "src/first.rs",
              old_path: "src/first.rs",
            },
            {
              hunks: [{
                header: "@@ -1 +1 @@",
                lines: [{ content: "second change", kind: "addition", new_line: 1, old_line: null }],
              }],
              language: "rust",
              new_path: "src/second.rs",
              old_path: "src/second.rs",
            },
          ],
          node_id: 3,
          source_id: "multi-file-diff",
          type: "diff",
        }}
      />,
    );

    const firstToggle = screen.getByRole("button", {
      name: "Collapse diff file src/first.rs",
    });
    expect(screen.getByText("first change")).toBeVisible();
    expect(screen.getByText("second change")).toBeVisible();

    await user.click(firstToggle);
    expect(screen.getByText("first change")).not.toBeVisible();
    expect(screen.getByText("second change")).toBeVisible();
    expect(firstToggle).toHaveAttribute("aria-expanded", "false");

    await user.click(screen.getByRole("button", { name: "Expand diff file src/first.rs" }));
    expect(screen.getByText("first change")).toBeVisible();
  });

  it("uses the language resolved for each diff file", () => {
    const { container } = render(
      <DiffBlock
        node={{
          files: [
            {
              hunks: [
                {
                  header: "@@ -0,0 +1 @@",
                  lines: [{ content: "const answer = 42;", kind: "addition", new_line: 1, old_line: null }],
                },
              ],
              language: "javascript",
              new_path: "answer.js",
              old_path: null,
            },
          ],
          caption: "The **addition** establishes the new default for $x_1$.",
          node_id: 2,
          source_id: "answer-diff",
          type: "diff",
        }}
      />,
    );

    expect(screen.getByLabelText("Language: JavaScript")).toHaveTextContent("JavaScript");
    expect(screen.queryByRole("button", { name: /diff file/ })).not.toBeInTheDocument();
    expect(screen.getByText("addition")).toHaveTextContent("addition");
    expect(screen.getByText("addition").tagName).toBe("STRONG");
    expect(container.querySelector(".source-caption .katex")).toBeInTheDocument();
    expect(container.querySelector("code.language-javascript .hljs-keyword")).toHaveTextContent("const");
  });

  it("renders unknown languages as escaped plain text", () => {
    const source = '<img src=x onerror="alert(1)">';
    const { container } = render(
      <CodeBlock
        node={{ content: source, language: "not-a-language", node_id: 3, source_id: "plain", type: "code" }}
      />,
    );

    expect(container.querySelector("code")).toHaveTextContent(source);
    expect(container.querySelector("img")).not.toBeInTheDocument();
  });

  it("renders explicit Mermaid code as a strict diagram", async () => {
    mermaidMocks.render.mockResolvedValue({ svg: '<svg data-testid="rendered-mermaid"><text>flow</text></svg>' });
    const source =
      "flowchart LR\n  A --> B\n  classDef external fill:#dbeafe,stroke:#3b82f6,color:#172554\n  class A external";

    render(
      <CodeBlock
        node={{
          content: source,
          caption: "The shared queue is the serialization point; producers never hand jobs directly to the consumer.",
          language: "mermaid",
          node_id: 4,
          source_id: "flow",
          type: "code",
        }}
      />,
    );

    expect(await screen.findByRole("img", { name: "Diagram: flow" })).toContainElement(
      screen.getByTestId("rendered-mermaid"),
    );
    expect(screen.getByLabelText("Language: Mermaid")).toHaveTextContent("Mermaid");
    expect(screen.getByText(/shared queue is the serialization point/)).toBeInTheDocument();
    expect(mermaidMocks.render).toHaveBeenCalledWith(expect.stringMatching(/^mermaid-/), source);
    expect(mermaidMocks.initialize).toHaveBeenCalledWith(
      expect.objectContaining({ securityLevel: "strict", startOnLoad: false }),
    );
    expect(screen.getByLabelText("Diagram legend")).toBeInTheDocument();
    expect(screen.getByTitle("Mermaid class: external")).toHaveTextContent("external");
    expect(screen.getByTitle("Mermaid class: external")).toHaveStyle({
      backgroundColor: "#dbeafe",
      borderColor: "#3b82f6",
      color: "#172554",
    });
  });

  it("shows escaped source when Mermaid rejects invalid input", async () => {
    const source = "not a diagram <script>alert(1)</script>";
    mermaidMocks.render.mockRejectedValue(new Error("parse error"));

    const { container } = render(
      <CodeBlock
        node={{ content: source, language: "mermaid", node_id: 5, source_id: "broken", type: "code" }}
      />,
    );

    await waitFor(() => expect(screen.getByRole("status")).toHaveTextContent("could not be rendered"));
    expect(screen.getByRole("status")).toHaveTextContent(source);
    expect(container.querySelector("script")).not.toBeInTheDocument();
  });
});
