---
name: learnc
description: Author and compile interactive lesson DSL documents with the installed `learnc` binary. Apply when an agent should turn code, diffs, explanations, or quiz material into a validated `.learn` artifact for the user to study later with `learn`; do not run the lesson runtime.
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
   tell the user why the exception is intentional. Treat `info` as advice:
   read each finding, because some are real mistakes (for example a name in
   prose or a question that no code block shows). If an `info` rule only
   produces noise for this lesson, rerun with `--ignore-code <CODE>` for that
   code and mention it to the user. Only `info` codes can be ignored this way.
5. If `learnverify` is installed, run `learnverify lesson.json` (same `--root`).
   It sends quiz and highlight content to the TypeSafe API. Treat `warning`
   findings like lint warnings (fix, or tell the user why the exception is
   intentional) and `info` findings as advice; `probability` says how sure the
   model was. If it reports `verify.unavailable`, continue without it and tell
   the user the semantic checks were skipped.
6. Run `learnc build`, report the lesson and artifact paths, and stop.

Keep every diagnostic's code, pointer, message, and suggestion when reporting a
failure. Never replace a missing file, revision, or repository with guessed
content.

## When the user pastes a lesson reference

A reference starts with `Question about lesson "..."` and names a block, the
file and lines shown, and a version. To see what the user saw:

1. Open the path and lines. For a `git_blob` version, read that commit and
   blob (`git show <commit>:<path>`); for a worktree version, check whether the
   file's current blob (`git hash-object <path>`) still matches.
2. If it does not, look for the content in history:
   `git log --all --find-object=<blob>` finds the commit that added it, even
   after a move or rename. The commit it was built on gives a base to diff
   against.
3. If the content exists nowhere, read the frozen copy: the named block in the
   `.learn` file (its `presentation.nodes` entry with that `source_id`).

Answer the user's question in the session; a reference never needs a rebuild.

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
  A Markdown file in a `git` diff renders as formatted Markdown with changed
  blocks marked; from a patch it shows as raw lines, so diff docs with `git`.
  A hunk that mostly adds code is new code: keep the diff for the lines that
  change, and show the additions as a ranged code block.
- **Multiple choice** checks understanding at the point where it matters.
- **Run code** (`run_code`, source schema 2.4.0) is for code whose behavior the
  learner should see happen: a short self-contained program, with the output it
  prints. Give it its own `source`, or `of` naming the code block that already
  shows the code, so the code appears once and the run block shows only "Runs
  `<id>`". Python, JavaScript, and shell run without more; any other language
  needs `argv` (for example `["ruby", "{file}"]`). Always give an
  `expected_output`: the learner can only run the block if they start
  `learn serve` with `--allow-run` themselves, and otherwise the block shows
  its code and that frozen output. You never start `learn`, with or without
  that flag, so produce the output by running the code yourself (with the
  interpreter, not the runtime), never by guessing it. Prefer small programs
  that finish in well under 10 seconds (the timeout stops a run at
  `timeout_secs`, default 10); the code that runs is the frozen copy, never the
  learner's repository.

## Judgment lint cannot check

- **Local teaching units.** Explain a concept, show its code or diff right
  there, then ask any follow-up question before moving on. Do not collect
  diffs at the end. Show a definition once and link it where later blocks rely
  on it: `` [`Queue`](#queue-def) `` or, for part of a block,
  `#queue-def:12-18` (the lines its gutter shows; source schema 2.3.0).
  Learners preview the target in place. Names in code also link to the
  definitions the lesson shows (go to definition), so a definition shown once
  serves every later use. Still repeat a small fragment when the learner must
  compare it line by line with nearby code.
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
  reasoning goes in `explanation`. Show the code a question relies on before
  asking it: lint only catches names formatted as code. Give each distractor its own `explanation`
  saying why it is wrong; learners see it only after answering or revealing.
  Hints should nudge toward the reasoning, never name the answer. Do not rotate the correct answer yourself; `learnc`
  shuffles choices.
- **Mermaid** diagrams are inline code blocks with `language: "mermaid"`, never
  fences inside Markdown. Use `sequenceDiagram` for time-ordered calls and
  responses. When color encodes meaning, define a few named `classDef` styles
  and assign them with `class`; the UI builds its legend from them.
- **Markdown as a code language** is only for lessons that teach Markdown
  syntax itself. Lint flags it as `critical`, so tell the user when you
  intend it.

Temporary files are fine for long content: the artifact freezes what it shows.
Keep them only if the lesson source must be rebuildable.
