# `learnc lint` design notes

Status: implemented. This records the lint contract and remaining optional
follow-up work. The command does not change the lesson format.

## Boundary

`learnc check` and `learnc build` decide whether a lesson is valid and can be
compiled. They own schema validation, source resolution, and correctness checks.
They also run a **partial**, Rust-only Mermaid syntax check on the exact
resolved text of Mermaid code blocks. The intended parser is `mermaid-svg`
0.7.0: its parse-only API accepts the project's current flowchart and sequence
examples and catches common syntax mistakes without a JavaScript or Node.js
runtime dependency. This is not a guarantee that the UI's Mermaid parser will
render every accepted diagram; the authoring agent remains responsible for
checking diagrams. Lint separately checks simple Mermaid authoring-policy
patterns.

`learnc lint` is a separate subcommand in the same binary. It may reuse the
compiler's parsing, validation, resolved sources, and source locations, but
`check` and `build` must never depend on lint. If a lesson fails compilation
checks, lint reports that failure rather than trying to apply authoring policy
to unreliable input. Lint findings never change whether `check` or `build`
succeeds.

Lint output should be actionable and Rustc-like in text mode (`-t`), with
structured, agent-readable JSON by default. Diagnostics should locate the
place the agent should edit, which may be a span in `lesson.json` or in a
referenced source file. Every finding also carries the lesson block ID and
JSON pointer for context; related locations can identify other relevant
files or spans. Each lint diagnostic carries a stable `code`, intrinsic
`severity`, `fatal`, `message`, primary editable `location`, `block_id`, JSON
`pointer`, and a concrete `suggestion`; related locations are optional. Lint
uses its own diagnostic type so adding these fields does not change compiler
diagnostics used by `check` and `build`.

`learnc lint lesson.json` emits JSON by default. `learnc lint -t lesson.json`
renders the same findings as human-readable, Rustc-like text with source
locations and suggestions. A clean JSON run emits `{"diagnostics":[]}` and a
clean text run is silent; neither emits a success diagnostic.

## Lint categories

| Category | Meaning |
| --- | --- |
| `error` | A fatal lint finding with no good authoring justification. It does not block `check` or `build`. |
| `critical` | A high-confidence warning: very likely a mistake, but an intentional exception is possible. |
| `warning` | Suspicious or hard to read, but potentially justified. |
| `info` | A possible authoring improvement, not necessarily a problem. |

By default, compiler-validation failures and lint `error` findings make
`learnc lint` exit nonzero; `critical`, `warning`, and `info` findings do not.
The CLI option `--warning-as-error <SEVERITY>` selects the lowest lint category
that becomes fatal. It uses a `Severity` enum ordered `error > critical >
warning > info`:

| Option | Fatal lint categories |
| --- | --- |
| omitted or `--warning-as-error error` | `error` |
| `--warning-as-error critical` | `error`, `critical` |
| `--warning-as-error warning` | `error`, `critical`, `warning` |
| `--warning-as-error info` | All categories |

The option is a CLI failure threshold, not a per-rule severity override or a
TOML setting. A lint finding keeps its intrinsic `severity` in JSON and gains
`fatal: true` when the selected threshold makes it fatal. None of these
settings affect `check` or `build`.

`--ignore-below <SEVERITY>` suppresses lint findings strictly below the chosen
`Severity` (for example, `critical` hides `warning` and `info`; `warning` hides
only `info`). Suppressed findings are omitted from both JSON and text output.
Filtering happens before computing `fatal`, so an ignored finding cannot fail
lint even with `--warning-as-error`. Compiler-validation failures are not lint
findings and cannot be hidden by this option.

`info` findings are guesses, so individual `info` rules can also be switched
off by code: list them in the TOML `ignore_codes` array, or pass
`--ignore-code <CODE>` (repeatable; CLI codes are added to the file's list).
Only known `info` codes are accepted. Naming an unknown code or a `warning`,
`critical`, or `error` code is a config error (`lint.config.ignore_code.invalid`),
so a typo cannot silently do nothing and stronger findings cannot be silenced
one by one. Ignored findings are removed before `fatal` is computed.

## Rules

Rules are specified as `rule | enforcement | category | replacement`.
Thresholds written as `$name` can be supplied by an optional TOML lint
configuration or by command-line flags; the values below are defaults, not
compiler constants. The configuration is loaded only when explicitly requested
with `learnc lint --config path/to/lint.toml lesson.json`. Without `--config`,
lint uses its defaults; it does not auto-discover a configuration file. The
root [`config.example.toml`](../config.example.toml) lists every setting with
its default and description. The TOML format is flat: the key is each threshold
name without its `$` prefix. Unknown keys are rejected so misspellings cannot
silently disable or misconfigure a rule. Omitted keys use the defaults below.
For example:

```toml
max_code_lines = 60
max_inline_code_diff_chars = 256
min_question_ratio = 0.20
```

Every key also has a `learnc lint` flag with underscores replaced by hyphens,
such as `--max-code-lines 40` or `--min-question-ratio 0.25`. Several flags may
be supplied together. Effective values are chosen in this order: built-in
defaults, explicit TOML file, then CLI flags. The resulting values are
validated before lint runs. For example:

```console
learnc lint --config config.example.toml --max-code-lines 40 lesson.json
```

| Threshold | Default | Comparison |
| --- | ---: | --- |
| `$max_code_lines` | `60` | Warn when displayed lines exceed this value. |
| `$highlight_coverage_ratio` | `0.70` | Warn when highlighted coverage is at least this fraction. |
| `$highlight_coverage_min_lines` | `20` | Apply that warning only when at least this many distinct displayed lines are highlighted. |
| `$many_highlight_ranges` | `3` | Report an info finding when a code block has at least this many highlighted line ranges. |
| `$suggest_highlights_min_lines` | `30` | Report an info finding when a highlight-capable code block has at least this many displayed lines but no highlights. |
| `$filename_reference_gap` | `3` | Report an info finding when at least this many blocks lie between a file-backed code block and the nearest literal filename mention in Markdown. |
| `$max_choice_length_spread` | `0.30` | Report a warning when the longest choice exceeds the shortest by more than this fraction of the shortest. |
| `$min_choice_length_gap_chars` | `10` | Require at least this many characters between the longest and shortest choice before reporting length imbalance. |
| `$min_question_ratio` | `0.20` | Report one lesson-wide info finding when MCQ blocks make up less than this fraction of all top-level blocks. |

Inline-source size uses two separate configurable character limits:
`$max_inline_code_diff_chars` defaults to `256` for code and diff sources, and
`$max_inline_prose_chars` defaults to `512` for Markdown blocks and
multiple-choice prompts. Count Unicode scalar values in the decoded content
(Rust `chars`), not UTF-8 bytes or JSON escape characters. Exceeding either
limit is a lint `error`: use a file source, including a temporary file, instead.
Quiz choices, hints, and explanations are excluded because they currently have
no file-source alternative.

| Rule | Enforcement | Category | Replacement |
| --- | --- | --- | --- |
| Oversized inline code or diff | Count decoded characters of an inline source; trigger when the count exceeds `$max_inline_code_diff_chars` (default `256`). | `error` | Put the content in a file source; a temporary file is fine. |
| Oversized inline Markdown or quiz prompt | Count decoded characters of an inline source; trigger when the count exceeds `$max_inline_prose_chars` (default `512`). | `error` | Put the content in a file source; a temporary file is fine. |
| Markdown presented as code | The code block's resolved presentation language is Markdown, whether authored explicitly or inferred from a source path. No content heuristic is needed. | `critical` | Use a Markdown block for rendered prose. The only legitimate exception is a lesson that teaches the Markdown language itself, which is very rare; that rarity is why this is `critical` rather than `warning`. |
| Plain-text code block | The code block's resolved presentation language is Text, including an omitted or unknown language that falls back to Text. No Markdown-content heuristic is needed in v1. | `warning` | If this is prose, use a Markdown block; if it is code, specify a supported language. Keep literal plain text intentionally when appropriate. |
| New file presented as a diff | Inspect each resolved diff file's `is_new` property. Additions to an existing file do **not** trigger this rule. | `error` | Explain the file in a Markdown block and show the relevant sourced code range, or use a highlighted code block with meaningful explanation. |
| Very large code block | Count displayed logical lines after resolving the selected source; trigger when the count is greater than `$max_code_lines`. | `warning` | Split the excerpt into focused code blocks, with explanations where useful. |
| Long code fragment without highlights | For file- or Git-backed, non-Mermaid code, report when no highlights are present and displayed lines reach `$suggest_highlights_min_lines` (default `30`). Inline code and Mermaid cannot use highlights and are excluded. | `info` | Consider highlighting the important lines if the excerpt has a focal point. |
| Large highlighted portion | Count the union of highlighted displayed lines divided by all displayed lines; trigger when coverage is at least `$highlight_coverage_ratio` and at least `$highlight_coverage_min_lines` distinct displayed lines are highlighted. | `warning` | Split into focused code blocks when the broad highlight obscures the point. |
| Filename mention far from code fragment | For each file-backed code block, look for a literal mention of its source filename in Markdown blocks. If there is at least one mention, count the blocks between it and the nearest mention; trigger at `$filename_reference_gap` or more (default `3`). Do not attempt to determine whether the mention explains the code. | `info` | Tell the agent the filename was mentioned far from this excerpt, so it can decide whether to add nearby context. |
| Many highlights in one fragment | Count highlighted line ranges across all groups; trigger at `$many_highlight_ranges` or more. | `info` | Consider separate code blocks. |
| Uneven multiple-choice answer lengths | Count Unicode scalar values in each full decoded Markdown choice string, including formatting syntax, link destinations, and math source. Compare the longest and shortest choice in one question. Trigger when `(longest - shortest) / shortest > $max_choice_length_spread` (default `0.30`) **and** `longest - shortest >= $min_choice_length_gap_chars` (default `10`). | `warning` | Make choices comparable in length without sacrificing plausible, unambiguous answers; keep fuller teaching detail in the explanation. |
| Few questions relative to lesson size | For a nonempty lesson, divide the number of `multiple_choice` blocks by the number of all top-level blocks. Report one lesson-wide finding when the result is below `$min_question_ratio` (default `0.20`); equality does not trigger. | `info` | If questions would serve the lesson's teaching goal, consider adding more. |
| Code mentioned in prose but never shown | Collect every identifier-like word in displayed code and diff lines (context, additions, and deletions). Scan Markdown blocks, quiz prompts, hints, code/diff captions, and highlight annotations for inline code spans outside fenced code, and report a span when it looks like an identifier (`name`, `name()`, `Type::member`, `value.field`, `node->next`) and one of its segments is not among the displayed words. Commands with spaces, paths, filenames with a known extension, numbers, and `true`/`false`/`null`-style literals are skipped. Choices and explanations are not scanned on their own, because distractors deliberately name things that do not exist; when a reported name also appears in a choice or explanation, those spans are attached as related locations. One finding per name per block. Code `lint.markdown.unshown_code_reference`. | `info` | If the learner needs to see it, show the relevant code; otherwise check the name, or drop the code formatting if it names a concept. |
| Diagram uses `subgraph` or direct `style` | In valid Mermaid flowcharts and class diagrams, report every statement whose first word is `subgraph` or `style`. A statement starts at a line start or after an unquoted `;`; quoted text, bracketed labels and class bodies, and `%%` comments are skipped. Code `lint.mermaid.style_or_subgraph`. | `warning` | If they express semantic distinctions, consider reusable `classDef` and `class` assignments; keep `subgraph` when actual grouping is intended. |

`mermaid-svg` 0.7.0 accepted the project's documented Mermaid diagrams and
both illustrative flowcharts, and rejected an invalid direction and unclosed
node label. It nevertheless accepted an unclosed flowchart `subgraph` and an
unclosed sequence `alt` that the UI's locked Mermaid 11.17.2 parser rejected.
Therefore `check`/`build` reject diagrams **this Rust parser** rejects, but do
not promise complete Mermaid validity. The styling/grouping rule is lint, not a
syntax check, and applies to flowcharts and class diagrams. Sequence diagrams
are excluded.

Source spans are one-based line and Unicode-scalar columns in a primary
`location` object with `path`, `start`, and `end`. Inline JSON strings map
decoded characters back to their encoded source spans. A referenced worktree
file is primary when its content is the edit target; authored settings and
revision-backed content point primarily to the editable `lesson.json` field.
Lesson-wide findings use `block_id: null`.

## Later decisions

- Consider safe mechanical auto-fixes only after the rules and diagnostics are
  useful; auto-fixes are not required for the initial command.

The compiled `.learn` artifact freezes the content actually displayed by the
lesson, so a temporary file can supply content during `check`/`build`. It is
not a full backup of an original file if the lesson selected only a range;
unselected content is not part of that compiled block.
