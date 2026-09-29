import type { LessonNode, PublicLesson, ResourceReference } from "./types";

/** Lines the learner selected inside a code or diff block, in source-file numbers. */
export type LineSelection =
  | { kind: "code"; start: number; end: number; group?: number }
  | { kind: "diff"; path?: string; old?: [number, number]; new?: [number, number] };

export type LessonContext = Pick<PublicLesson, "title" | "lesson_path" | "artifact_path">;

const KIND_LABELS: Record<LessonNode["type"], string> = {
  markdown: "Markdown",
  code: "code",
  diff: "diff",
  multiple_choice: "question",
};

function span(start: number, end: number): string {
  return start === end ? `line ${start}` : `lines ${start}–${end}`;
}

function shown(node: LessonNode): string | null {
  const reference = node.reference;
  if (node.type === "code" && reference && reference.kind !== "inline" && reference.kind !== "git_diff") {
    if (node.first_line === undefined) return reference.path;
    const count = node.content.split("\n").length - (node.content.endsWith("\n") ? 1 : 0);
    return `${reference.path}, ${span(node.first_line, node.first_line + Math.max(count, 1) - 1)}`;
  }
  if (!reference) return null;
  switch (reference.kind) {
    case "file":
      return reference.path;
    case "git_blob":
      return reference.path;
    case "git_diff":
      return reference.files.join(", ");
    case "inline":
      return null;
  }
}

function selected(selection: LineSelection | undefined): string | null {
  if (!selection) return null;
  if (selection.kind === "code") {
    const group = selection.group === undefined ? "" : ` (highlight group ${selection.group})`;
    return `selected ${span(selection.start, selection.end)}${group}`;
  }
  const sides = [
    selection.old ? `old ${span(...selection.old)}` : null,
    selection.new ? `new ${span(...selection.new)}` : null,
  ].filter(Boolean);
  if (sides.length === 0) return null;
  return `selected ${selection.path ? `${selection.path} ` : ""}${sides.join(", ")}`;
}

function version(node: LessonNode, reference: ResourceReference | undefined): string {
  if (!reference) return "part of the lesson source";
  switch (reference.kind) {
    case "inline":
      return "inline in the lesson source";
    case "file": {
      const what = node.type === "diff" ? "patch file" : "worktree file";
      const blob = reference.blob_id ? `, blob ${reference.blob_id}` : `, sha256 ${reference.sha256}`;
      const head = reference.head ? `, on top of commit ${reference.head}` : "";
      return `${what} at build time${blob}${head}`;
    }
    case "git_blob":
      return `commit ${reference.revision_object_id} (revision "${reference.revision}"), blob ${reference.content_object_id}`;
    case "git_diff": {
      const base = `base ${reference.base_object_id} (revision "${reference.base_revision}")`;
      if (reference.target.kind === "revision") {
        return `diff of ${base} → target ${reference.target.object_id} (revision "${reference.target.revision}")`;
      }
      const blobs = Object.entries(reference.worktree_blob_ids ?? {})
        .map(([path, blob]) => `${path} blob ${blob}`)
        .join("; ");
      const head = reference.head ? `, on top of commit ${reference.head}` : "";
      return `diff of ${base} → worktree at build time${blobs ? ` (${blobs})` : ""}${head}`;
    }
  }
}

/**
 * Agent-ready reference to one block: what was shown, which exact version,
 * and where the frozen copy lives. It names content but never includes it,
 * and never reveals quiz answers.
 */
export function formatReference(
  lesson: LessonContext,
  node: LessonNode,
  selection?: LineSelection,
  question?: string,
): string {
  const source = lesson.lesson_path ? ` (${lesson.lesson_path})` : "";
  const lines = [
    `Question about lesson "${lesson.title}"${source}, block \`${node.source_id}\` (${KIND_LABELS[node.type]}):`,
  ];
  const shownText = shown(node);
  const selectedText = selected(selection);
  if (shownText || selectedText) {
    lines.push(`- shown: ${[shownText, selectedText].filter(Boolean).join("; ")}`);
  }
  lines.push(`- version: ${version(node, node.reference)}`);
  if (lesson.artifact_path) {
    lines.push(`- exact shown text: block \`${node.source_id}\` in ${lesson.artifact_path}`);
  }
  const trimmed = question?.trim();
  if (trimmed) lines.push(`My question: ${trimmed}`);
  return lines.join("\n");
}
