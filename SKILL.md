---
name: learnc
description: Author and compile interactive lesson DSL documents with the installed `learnc` binary, optionally using `learnpick` to choose a presentation block. Apply when an agent should turn code, diffs, explanations, or quiz material into a validated `.learn` artifact for the user to study later with `learn`; do not run the lesson runtime.
---

# Learn Compiler

`learnc` compiles a JSON lesson into a self-contained `.learn` artifact. The
user runs `learn` later; do not invoke `learn`, start a server, or open a
browser. Use the installed `learnc` binary, not `cargo run` or a substitute.

## Workflow

1. Discover the contract from the binary instead of memory: `learnc --help`,
   `learnc <command> --help`, and `learnc schema` for the exact JSON Schema.
   Do not invent fields. Prefer the default JSON output and parse it.
2. Write `lesson.json`. Paths are relative to the filesystem root (`--root`,
   default: the current directory), not to the lesson file.
3. Run `learnc check` until it succeeds.
4. Run `learnc lint`. Fix every `error`. Fix each `critical` and `warning`, or
   tell the user why the exception is intentional. Treat `info` as advice.
5. Run `learnc build`, report the lesson and artifact paths, and stop.

Keep every diagnostic's code, pointer, message, and suggestion when reporting a
failure. Never replace a missing file, revision, or repository with guessed
content.

## Choosing blocks

- **Markdown** carries prose, headings, lists, and KaTeX math (`$...$` inline,
  `$$` on their own lines for display).
- **Code** shows what a file contains, including a new or untracked file. Use a
  `file` source for current content and `git_blob` for a specific revision,
  each with a narrow `lines` range. Show a new file as explanatory Markdown
  followed by a ranged code block, or as a code block whose highlights carry
  annotations. Never show a new file as a diff.
- **Diff** is only for changes to existing files, when the before/after
  relationship is what the learner must understand. Prefer a declarative `git`
  source over a hand-made patch. Any two revisions Git can resolve may be
  compared. Files owned by different repositories need separate diff blocks.
- **Multiple choice** checks understanding at the point where it matters.

If one teaching unit could plausibly use several block types, you may ask
`learnpick "<one concise unit>"`. Treat its answer as advice only. If it fails
or is ambiguous, choose manually. Never call it to confirm an obvious choice.

## Judgment lint cannot check

- **Local teaching units.** Explain a concept, show its code or diff right
  there, then ask any follow-up question before moving on. Do not collect
  diffs at the end. Repeat a small relevant fragment rather than pointing back
  to a distant block.
- **Captions** (code and diff) are for what the learner cannot infer from the
  content, mainly a Mermaid diagram's assumption or omission. Never narrate
  arrows, labels, reading direction, or adjacent Markdown.
- **Highlights** direct attention inside useful context. Add an `annotation`
  when the reason for a range is not obvious. Use separate groups for separate
  explanations, and never write "the blue lines".
- **Comments in shown code** should carry meaning the code cannot, such as why
  two blocks show different states of one file. Do not edit real source files
  to add lesson commentary; use Markdown or an annotation instead.
- **Questions** should be hard because the alternatives are plausible (real
  misconceptions, nearby APIs, believable consequences), never because they are
  ambiguous. Exactly one choice must be defensibly correct, and the full
  reasoning goes in `explanation`. Do not rotate the correct answer yourself;
  `learnc` shuffles choices.
- **Mermaid** diagrams are inline code blocks with `language: "mermaid"`, never
  fences inside Markdown. Use `sequenceDiagram` for time-ordered calls and
  responses. When color encodes meaning, define a few named `classDef` styles
  and assign them with `class`; the UI builds its legend from them.
- **Markdown as a code language** is only for lessons that teach Markdown
  syntax itself. Lint flags it as `critical`, so tell the user when you
  intend it.

Temporary files are fine for long content: the artifact freezes what it shows.
Keep them only if the lesson source must be rebuildable.
