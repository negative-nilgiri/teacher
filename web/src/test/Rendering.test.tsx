import { render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { CodeBlock } from "../components/CodeBlock";
import { DiffBlock } from "../components/DiffBlock";

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
  it("highlights a code block with its compiled language", () => {
    const { container } = render(
      <CodeBlock
        node={{
          content: "fn main() { let answer = 42; }",
          language: "rust",
          node_id: 1,
          source_id: "main",
          type: "code",
        }}
      />,
    );

    expect(container.querySelector("code.language-rust")).toHaveTextContent("fn main() { let answer = 42; }");
    expect(container.querySelector(".hljs-keyword")).toHaveTextContent("fn");
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
          node_id: 2,
          source_id: "answer-diff",
          type: "diff",
        }}
      />,
    );

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

    render(
      <CodeBlock
        node={{
          content: "flowchart LR\n  A --> B",
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
    expect(mermaidMocks.render).toHaveBeenCalledWith(expect.stringMatching(/^mermaid-/), "flowchart LR\n  A --> B");
    expect(mermaidMocks.initialize).toHaveBeenCalledWith(
      expect.objectContaining({ securityLevel: "strict", startOnLoad: false }),
    );
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
