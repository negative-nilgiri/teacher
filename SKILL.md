---
name: learnc
description: Author and compile interactive lesson DSL documents with the installed `learnc` binary. Apply when an agent should turn code, diffs, explanations, or quiz material into a validated `.learn` artifact for the user to study later with `learn`; do not run the lesson runtime.
---

# Learn Compiler

`learnc` is the agent-facing compiler for interactive lessons. Use it to learn
the current lesson schema, author a JSON lesson document, validate all file and
Git-backed sources, and produce a self-contained `.learn` artifact.

The user runs `learn` later when they are ready to study. Do not invoke `learn`,
start the lesson server, or open a browser as part of this skill. For lesson
tooling, use the installed `learnc` binary rather than `cargo run` or a
hand-written substitute.

## Discover the contract

Treat the binary as authoritative instead of relying on a remembered schema or
command line. Start with its help and inspect command-specific help as needed:

```sh
learnc --help
learnc schema --help
learnc check --help
learnc build --help
```

Use `learnc schema` to obtain the exact authored-document JSON Schema before
writing unfamiliar lesson structures. The source language is JSON in v1; do not
invent fields or a different syntax.

The default command output is intended for agents. Prefer the default structured
output and parse its diagnostics. Use `-t` only when human-readable terminal
output is specifically useful.

## Author and compile a lesson

Write a clear lesson source such as `lesson.json` from the material in scope.
Lessons may explain code, embed selected files or Git revisions, present
generated diffs, and ask multiple-choice questions. Follow the emitted schema
and use paths relative to the selected filesystem root.

Use each block for its presentation semantics:

- Put rendered prose, headings, lists, emphasis, and explanations in Markdown
  blocks. Never dump prose written in Markdown into a code block: its formatting
  will not render there.
- Use a code block with language `markdown` only when the literal Markdown
  syntax is itself being taught, and include only the smallest fragment needed
  for that example.
- Set a code block's optional language when it is known, especially for inline
  sources. File and Git-blob sources can infer a recognized language from their
  path. Prefer the canonical names `rust`, `python`, `javascript`, `typescript`,
  `c`, `cpp`, `go`, `java`, `shell`, `json`, `yaml`, `toml`, `html`, `xml`,
  `css`, `sql`, `markdown`, `mermaid`, and `text`. Common filename-style aliases
  such as `rs`, `py`, `js`, `tsx`, `c++`, `golang`, `bash`, `yml`, `md`, `mmd`,
  and `txt` are accepted and normalized. An unknown explicit language, an
  omitted inline language, or an unrecognized source path falls back safely to
  plain text.
- Author a Mermaid diagram as a code block with an inline source and language
  `mermaid`. Do not put it in a fenced section inside a Markdown block.
  When colors express semantic categories, use a small set of meaningfully
  named `classDef` declarations and compact `class A,B category` assignments.
  The lesson UI derives its legend from those definitions. Do not add
  decorative subgraphs solely to group similarly styled nodes.

Order blocks as local teaching units. Introduce a concept, show the relevant
code or diff near that explanation, explain the change, and add any follow-up
question before moving to the next concept. Do not collect all diffs at the end
when they explain different parts of the lesson; preserve authored order to
interleave explanations, code, diffs, and questions.

Use `learnc check` while authoring. It runs the complete validation and source
resolution pipeline without writing an artifact. Read every structured
diagnostic, correct the lesson source, and check again until it succeeds.

When source paths are involved, consult `check` or `build` help for the current
filesystem-root option and pass the common anchor for every relative lesson
path. A lesson may draw from multiple repositories, but files owned by different
repositories belong in separate Git diff blocks.

A Git diff may compare any two commit-ish revisions that Git can resolve in the
selected files' owning repository; neither side has to be the current `HEAD`,
and the revisions do not need an ancestor relationship. Commit hashes, branches,
tags, and relative expressions such as `HEAD~2` are valid. Put the first
revision in `base` and use a revision target for the second:

```json
{
  "type": "diff",
  "id": "release-change",
  "source": {
    "kind": "git",
    "base": "v1.1.0",
    "target": { "kind": "revision", "revision": "v1.2.0" },
    "files": [{ "path": "src/compiler.rs" }],
    "context_lines": 3
  }
}
```

Specify the intended revision pair and let `learnc` resolve, compare, and freeze
both commits. Do not generate or copy the patch manually when a declarative Git
source can express the comparison.

Do not assume that an untracked file cannot appear in a lesson just because
ordinary `git diff` omits it. For a Git diff source whose target is `worktree`,
`learnc` treats every path in `files` as an explicit selection. If a selected
path is an untracked, non-ignored regular file, `learnc` synthesizes a diff that
shows the entire file as a new addition:

```json
{
  "type": "diff",
  "id": "new-document",
  "source": {
    "kind": "git",
    "base": "HEAD",
    "target": { "kind": "worktree" },
    "files": [{ "path": "docs/new-document.md" }],
    "context_lines": 3
  }
}
```

This support is limited to explicitly selected worktree files. Ignored files,
non-regular files, and files outside an owning Git repository are rejected; an
untracked file also cannot appear in a revision-to-revision comparison. Let
`learnc check` determine whether the selected path is valid instead of inferring
failure from ordinary Git behavior.

After a successful check, use `learnc build` to create the `.learn` artifact.
Consult command help for the current argument order and output-path option rather
than assuming them. Report the authored JSON path and generated artifact path to
the user, then stop. Do not serve the artifact.

If compilation fails because a source file, Git revision, repository owner, or
artifact input is unavailable, preserve the diagnostic code, pointer, message,
and suggestion. Do not replace missing source context with guessed content.
