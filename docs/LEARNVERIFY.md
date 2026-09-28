# `learnverify` semantic checks

`learnc lint` checks the *shape* of a lesson. `learnverify` is an optional,
separate binary that checks some of its *meaning*: it asks the TypeSafe Jev
model targeted yes/no questions about each quiz and each highlighted code
block, and reports likely mistakes in lint's diagnostic format. The rationale
for every decision below is in [`LEARNVERIFY_DESIGN.md`](LEARNVERIFY_DESIGN.md).

Findings are advice. They never change whether a lesson compiles, and they do
not replace `learnc lint`: run both.

## Use it

```sh
export TYPESAFE_API_KEY=...
learnverify lesson.json
learnverify -t --root path/to/workspace lesson.json
```

The lesson is first compiled exactly like `learnc check`. A lesson that does
not compile fails with the same compiler diagnostics (`{"ok":false,...}`,
nonzero exit) and nothing is sent. A lesson with no quiz and no highlighted
code block produces `{"diagnostics":[]}` without contacting the API.

| Option | Meaning |
| --- | --- |
| `--root <ROOT>` | Filesystem root, as for `learnc check`. |
| `--config <FILE>` | Explicit verify TOML file (see [Configuration](#configuration)). |
| `--warning-as-error <SEVERITY>` | Lowest category that makes the run exit nonzero; default `error`, so findings never fail the run by default. |
| `--ignore-below <SEVERITY>` | Omit findings below this category. |
| `--ignore-code <CODE>` | Repeatable; suppresses one check code. |
| `--min-info-probability`, `--min-warning-probability`, `--min-contradiction-warning-probability`, `--max-context-chars` | Override the matching config value. |
| `--no-cache` | Neither read nor write cached answers. |
| `-t`, `--text` | Human-readable, Rustc-like output instead of JSON. |

Credentials come only from the environment: `TYPESAFE_API_KEY`, with optional
`TYPESAFE_BASE_URL` (default `https://api.typesafe.ai`) and
`TYPESAFE_DEFAULT_MODEL` (default `jev-latest`). There are no credential flags.

## Checks

One request per multiple-choice block and one per code block with highlights.
Each question is about one exact target, and "yes" always means that target
has the problem.

| Code | Asked for | Points at |
| --- | --- | --- |
| `verify.hint_reveals_answer` | each hint | the hint |
| `verify.explanation_contradicts_answer` | the block explanation | the explanation (related: the choice marked correct) |
| `verify.multiple_defensible_choices` | each distractor | the choice (related: the prompt) |
| `verify.implausible_distractor` | each distractor | the choice |
| `verify.annotation_does_not_explain` | each annotated highlight group | the annotation |
| `verify.annotation_contradicts_code` | each annotated highlight group | the annotation (related: the group's `lines`) |
| `verify.highlight_unexplained` | each highlight group without annotation | the group's `lines` |

The model's probability that the answer is "yes" becomes the severity:

| Probability | Result |
| --- | --- |
| below `min_info_probability` (0.60) | nothing |
| from `min_info_probability` | `info` |
| from `min_warning_probability` (0.85) | `warning` |

`verify.annotation_contradicts_code` becomes a `warning` from
`min_contradiction_warning_probability` (0.70), because a wrong annotation
teaches something false. Findings are never `critical` or `error`.

## Output

`{"diagnostics":[...]}`, with each finding in lint's shape (`code`, `severity`,
`fatal`, `message`, `location`, `block_id`, `pointer`, `suggestion`, optional
`related`) plus `probability` and `model` (the resolved model name). Lint and
verify findings can be merged into one list. Text mode adds a
`= note: probability 0.91 from jev-…` line to each finding.

### `verify.unavailable`

If checks could not run (no API key, network or HTTP failure, invalid
response), `learnverify` reports **one** `info` finding, `verify.unavailable`,
naming the reason, with a related location for each block that was not
checked. Findings from the blocks that were checked are still reported.

`verify.unavailable` is a status, not a check: `--ignore-below`,
`--ignore-code`, and `ignore_codes` never hide it, and it never makes the run
exit nonzero, whatever `--warning-as-error` says. A run that reports it should
continue without the semantic checks and say they were skipped.

## Configuration

`--config verify.toml` loads a flat TOML file. It is never discovered
automatically, unknown keys are rejected, and CLI flags override it. It is a
separate file from the lint config. The packaged
[`verify.example.toml`](../verify.example.toml) lists every key with its
default:

```toml
min_info_probability = 0.60
min_warning_probability = 0.85
min_contradiction_warning_probability = 0.70
max_context_chars = 6000
ignore_codes = []
```

Probabilities lie in `[0, 1]`, and both warning thresholds must be at least
`min_info_probability`. `ignore_codes` accepts only the seven check codes.

## What is sent

Each request's `state` is an excerpt of the lesson with `subject` naming the
block under review:

- **Quiz:** the lesson title; the quiz's teaching unit, meaning the Markdown,
  code, and diff blocks back to the previous question (adjacent questions
  share one unit), nearest first up to `max_context_chars`; then the quiz with
  its prompt, choices in authored order with the `correct` marker and
  distractor explanations, hints, and explanation.
- **Highlighted code block:** the lesson title; the Markdown blocks directly
  before and after it (up to `max_context_chars`); the code with a source line
  number on every line, its caption, and each highlight group's ranges, text,
  and annotation.

The API key is only sent in the `Authorization` header.

## Cache

Agents rerun checks after every fix and model answers are not deterministic,
so answers are cached for 24 hours in `learnverify-<uid>` inside the system
temporary directory. The directory is created with `0700` permissions and is
used only if it is a real directory owned by the current user that no one
else can open; otherwise the cache is disabled with a warning on stderr.

Entries are keyed by the check version, the endpoint, and the exact request,
and hold the raw answers rather than findings, so changing a threshold or
`ignore_codes` re-grades cached answers without a request. The API key is
only needed for requests that miss the cache. `--no-cache` asks for a fresh
opinion.

## Limits

- Four requests run at once, each with a 10-second timeout, and all of a
  run's requests share a 20-second budget: none starts after it ends, and a
  running one gets only the time left. Blocks cut off this way are reported
  in `verify.unavailable` ("time budget of 20 s exceeded"). Cached answers do
  not count against the budget. Both limits are constants in
  `src/learnverify/client.rs`.
- An HTTP 401 or 403 stops the remaining requests.
- There is no cap on the number of requests: calls are cheap, so only time is
  bounded.
- `jev-latest` changes over time; each finding records the resolved `model`,
  and `TYPESAFE_DEFAULT_MODEL` can select another one.
