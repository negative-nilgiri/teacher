import { createContext, useContext } from "react";
import type { BlockLink, CodeHighlight, CodeNode, DefinitionSite, DiffNode, LessonNode } from "./types";

export interface LinkContextValue {
  links: Record<string, BlockLink>;
  nodes: LessonNode[];
  /** Shown definitions by name, for go-to-definition in code. */
  definitions?: Record<string, DefinitionSite[]>;
  /** True inside a preview, where links render plainly to avoid nesting. */
  inPreview: boolean;
}

export const LinkContext = createContext<LinkContextValue>({ links: {}, nodes: [], inPreview: false });

export function useLinks(): LinkContextValue {
  return useContext(LinkContext);
}

/** The DOM anchor of a block, used by jumps and the URL hash. */
export function blockAnchor(sourceId: string): string {
  return `block-${sourceId}`;
}

/**
 * The part of a node a link previews. Code ranges keep only those lines and
 * the highlight ranges overlapping them; diff ranges keep the hunk lines in
 * that new-side range and the deletions between them; quizzes keep only their
 * prompt, never choices or answers.
 */
export function previewNode(node: LessonNode, lines?: { start: number; end: number }): LessonNode {
  if (node.type === "multiple_choice") {
    return { ...node, choices: [], hints: [] };
  }
  if (!lines) return node;
  if (node.type === "code") return trimCode(node, lines);
  if (node.type === "diff") return trimDiff(node, lines);
  return node;
}

function trimCode(node: CodeNode, lines: { start: number; end: number }): CodeNode {
  const first = node.first_line ?? 1;
  const from = lines.start - first;
  const to = lines.end - first;
  const content = node.content.split("\n").slice(from, to + 1).join("\n");
  const highlights: CodeHighlight[] = (node.highlights ?? [])
    .map((highlight) => ({
      ...highlight,
      lines: highlight.lines
        .map((range) => ({
          start: Math.max(range.start, from + 1) - from,
          end: Math.min(range.end, to + 1) - from,
        }))
        .filter((range) => range.start <= range.end),
    }))
    .filter((highlight) => highlight.lines.length > 0);
  return {
    ...node,
    content,
    highlights,
    ...(node.first_line === undefined ? {} : { first_line: lines.start }),
  };
}

function trimDiff(node: DiffNode, lines: { start: number; end: number }): DiffNode {
  const inRange = (line: number | null) => line !== null && line >= lines.start && line <= lines.end;
  const files = node.files
    .map((file) => ({
      ...file,
      rendered: undefined,
      hunks: file.hunks
        .map((hunk) => {
          const kept = hunk.lines.map((line) => inRange(line.new_line));
          const first = kept.indexOf(true);
          const last = kept.lastIndexOf(true);
          return {
            ...hunk,
            lines: first < 0 ? [] : hunk.lines.slice(first, last + 1),
          };
        })
        .filter((hunk) => hunk.lines.length > 0),
    }))
    .filter((file) => file.hunks.length > 0);
  return { ...node, files };
}
