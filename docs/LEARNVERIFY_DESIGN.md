# `learnverify` design notes

Status: **proposed, not implemented.** This records the agreed design so it can
be reviewed and extended before implementation starts. The existing `learnpick`
binary is deprecated and no longer documented; its code remains until this
replacement is built.

## Purpose

`learnc lint` checks the *shape* of a lesson: sizes, languages, highlights,
names that no code block shows. It cannot judge *meaning*. `learnverify` is a
separate, opt-in binary that asks the TypeSafe Jev model targeted yes/no
questions about a compiled lesson and reports likely semantic mistakes in the
same diagnostic format as lint.

It replaces `learnpick`. Block-type advice is dropped: once lint enforced block
choices, the adviser lost most of its value, and the only remaining job for a
model is checking lessons.

## Boundary

- `learnverify` is its own binary. `learnc` stays offline, deterministic, and
  free of credentials, which preserves the isolation invariant: no core module
  depends on the verifier, so an API failure can never affect `check`, `build`,
  `lint`, or `learn`.
- It depends one way on the core library: it compiles the lesson with the same
  pipeline as `learnc check` (an invalid lesson fails exactly as `check` does,
  and no model call is made) and reuses lint's source-span index for locations.
- It never edits lesson source, writes an artifact, or starts `learn`.
- Findings are advisory. They never change whether a lesson compiles, and they
  do not replace `learnc lint`; agents run both.

## Command line

```console
learnverify [OPTIONS] lesson.json
```

| Option | Meaning |
| --- | --- |
| `--root <ROOT>` | Filesystem root, as for `learnc check`. |
| `--config <FILE>` | Explicit verify TOML file (see [Configuration](#configuration)). |
| `--warning-as-error <SEVERITY>` | Same semantics as lint; default `error`, so verify findings never fail the run by default. |
| `--ignore-below <SEVERITY>` | Same semantics as lint. |
| `--ignore-code <CODE>` | Repeatable; suppresses one verify code. |
| `--no-cache` | Ignore and do not write cached answers. |
| `-t`, `--text` | Human-readable, Rustc-like output instead of JSON. |

Credentials come only from the environment (`TYPESAFE_API_KEY`, with
`TYPESAFE_BASE_URL` and `TYPESAFE_DEFAULT_MODEL` overrides), as today. There
are no credential flags.

## Output

The report has lint's shape, `{"diagnostics":[...]}`, and each finding has
lint's fields (`code`, `severity`, `fatal`, `message`, `location`, `block_id`,
`pointer`, `suggestion`, optional `related`) plus:

- `probability`: the model's probability for the "problem" answer;
- `model`: the resolved model name that produced it.

Codes use the `verify.` prefix so they never collide with `lint.` codes and can
be merged into one list by an agent. Locations point at the editable span in
`lesson.json` (or the referenced Markdown file), exactly like lint.

## Checks

### First set: quiz checks

One request per `multiple_choice` block asks all four questions at once (the
Jev API accepts several questions per request, keyed by ID).

| Code | Question | "Yes" means | Suggestion |
| --- | --- | --- | --- |
| `verify.hint_reveals_answer` | Does any hint name or directly imply the correct choice? | A learner could pick the answer from the hint alone, without the reasoning. | Rewrite the hint to point at the relevant code or reasoning instead of the answer. |
| `verify.explanation_contradicts_answer` | Does the explanation argue for a choice other than the one marked correct? | The explanation and the `correct` marker disagree. | Check which choice is actually correct, then align the explanation or the marker. |
| `verify.multiple_defensible_choices` | Could a careful learner defend a second choice as correct? | The question is ambiguous. | Tighten the prompt or rewrite the second choice so exactly one answer is defensible. |
| `verify.implausible_distractor` | Is any distractor obviously wrong without understanding the material? | A choice is a giveaway (joke, off-topic, grammatically mismatched). | Replace it with a realistic misconception, nearby API, or believable consequence. |

Where a question concerns one specific hint or choice, the answer options
include that item so the finding can point at it (for example, the choice ID
of the implausible distractor), not only at the whole question.

### Second set: highlight checks

Lint already judges how much is highlighted (coverage, number of ranges,
missing highlights on long excerpts). These checks judge whether a highlight
is actually *explained*. One request per code block that has highlights asks
the questions below for every highlight group, keyed by group
(`group_0_annotation_does_not_explain`, ...) so each finding points at its
group.

| Code | Applies to | Question | "Yes" means | Suggestion |
| --- | --- | --- | --- | --- |
| `verify.annotation_does_not_explain` | Annotated group | Does the annotation fail to explain what the highlighted lines do or why they matter? | It restates the code, stays vague ("the important part", "see here"), or only names the color or line numbers. | Say what these lines do and why the learner should look at them. |
| `verify.annotation_contradicts_code` | Annotated group | Does the annotation describe behavior the highlighted lines do not have? | The annotation is factually wrong about the code it points at (for example, it names another function or the opposite condition). | Correct the annotation, or move the highlight to the lines it describes. |
| `verify.highlight_unexplained` | Group without annotation | Is it unclear why these lines are highlighted, given the caption and the adjacent Markdown? | Nothing near the block says why these lines matter, so the highlight directs attention without a reason. | Add an `annotation`, or explain the lines in the Markdown next to the block. |

Findings point at the group's `annotation` in `lesson.json`, or at the
group's `lines` when it has no annotation. The request's `state` holds:

- the displayed code with its source-file line numbers (`first_line`) and
  language;
- each highlight group: color, ranges in source-file lines, the text of the
  highlighted lines, and the annotation (or `null`);
- the block's caption and the Markdown blocks immediately before and after it,
  since an unannotated highlight is often explained there.

Of the three, `verify.annotation_contradicts_code` matters most: a wrong
annotation teaches something false, while the other two only waste the
learner's attention. It therefore reaches `warning` at a lower probability
than the other checks (see [Severity](#severity)).

### Later candidates

These need more thought about which context to send:

- prose that describes code differently from the code shown next to it;
- a code or diff caption that only narrates visible content;
- an explanation that restates the answer without teaching the reasoning;
- a distractor explanation that does not actually say why that choice is
  wrong, or that contradicts the correct answer.

## Request shape

Each request sends one quiz and its local context as `state`, and defines each
check as a choice question with explicit criteria:

```json
{
  "model": "jev-latest",
  "state": {
    "quiz": {
      "prompt": "What does `pop_front` return?",
      "choices": [
        { "id": "a", "content": "The oldest item", "correct": true },
        { "id": "b", "content": "The newest item", "correct": false }
      ],
      "hints": ["Think about FIFO ordering."],
      "explanation": "A queue removes the oldest item first."
    },
    "context": [
      { "kind": "code", "path": "src/queue.rs", "first_line": 40, "content": "..." }
    ]
  },
  "questions": {
    "hint_reveals_answer": {
      "type": "choice",
      "instructions": { "question": "Does any hint name or directly imply the correct choice?" },
      "criteria": {
        "yes": { "what": "...", "not_for": "..." },
        "no": { "what": "...", "not_for": "..." }
      }
    }
  }
}
```

- `context` holds the compiled code and diff blocks of the quiz's local
  teaching unit: the blocks between the previous question (or the lesson start)
  and this one, capped at a configurable size. The compiled artifact already
  holds the resolved text, so no repository access is needed.
- Criteria wording is versioned in the binary (`check_version`), because
  changing it changes answers and must invalidate the cache.
- Highlight checks use the same envelope with a different `state` (see
  [Second set: highlight checks](#second-set-highlight-checks)): one code block
  and its highlight groups instead of one quiz.

## Severity

Each answer's probability for "yes" (a problem) maps to a severity:

| Probability | Result |
| --- | --- |
| below `min_info_probability` (default `0.60`) | nothing reported |
| from `min_info_probability` | `info` |
| from `min_warning_probability` (default `0.85`) | `warning` |

`verify.annotation_contradicts_code` is the one exception: it becomes a
`warning` from `min_contradiction_warning_probability` (default `0.70`),
because an annotation that is wrong about its code teaches something false.

Verify findings are never `critical` or `error`: they are probabilistic
judgments. With the default `--warning-as-error error`, they never make the
command exit nonzero.

## Failure behavior

If credentials are missing, the network fails, or the API returns an error or
an invalid response, `learnverify` reports **one** `info` finding,
`verify.unavailable`, whose message names the reason (for example
"semantic checks skipped: TypeSafe API returned HTTP 503"), and exits
successfully. Silence would be indistinguishable from "no problems found", so
the skipped run is always visible. A lesson that fails compilation is still a
nonzero exit with compiler diagnostics, as in lint.

Partial failures report the checks that succeeded plus one `verify.unavailable`
listing the quizzes that could not be checked.

## Cache

Agents rerun checks after every fix, and model answers are not deterministic.
A short-lived cache avoids repaying for unchanged quizzes and stops findings
from flickering between runs.

- **Location:** `std::env::temp_dir()/learnverify-<user>/`. On macOS
  `$TMPDIR` is already per-user; on Linux `/tmp` is shared, so the directory is
  created with `0700` permissions and ignored if it is owned by another user.
  It holds lesson text and code, so it must not be world-readable. Quizzes are not long
  lived; losing the cache on reboot or temp cleanup only costs a new request.
- **Key:** SHA-256 of `check_version`, the resolved model, and the exact
  `state` sent (one quiz plus its context, or one code block plus its
  highlights and neighbouring prose). Builds are reproducible, so an unchanged
  quiz or code block always produces the same key.
- **Value:** the raw Jev answers (probabilities per question), not findings.
  Changing a threshold or `ignore_codes` re-grades cached answers without new
  requests.
- `--no-cache` bypasses it for a fresh opinion.

## Configuration

`learnverify --config verify.toml` loads a flat TOML file with the same rules
as lint: explicit only (never auto-discovered), unknown keys rejected, CLI
flags override file values.

```toml
min_info_probability = 0.60
min_warning_probability = 0.85
min_contradiction_warning_probability = 0.70
max_context_chars = 6000
ignore_codes = []
```

- All probabilities lie in `[0, 1]`. `min_warning_probability` and
  `min_contradiction_warning_probability` must each be at least
  `min_info_probability`.
- `ignore_codes` accepts only known `verify.` codes. Unlike lint, every verify
  code is ignorable, because none is stronger than `warning`.

The verify config is a **separate file** from the lint config. Lint's TOML is
flat and rejects unknown keys to catch typos, so verify keys in the same file
would break `learnc lint`. The settings also mean different things
(probabilities versus line and character counts).

## Privacy

Quiz text with the code or diff excerpts of its local context, and highlighted
code blocks with their annotations, caption, and neighbouring Markdown, are
sent to the TypeSafe API. `learnverify` is opt-in and never runs as part of `learnc`;
`--help` and the skill state what leaves the machine. The API key is only sent
in the `Authorization` header.

## Skill integration

`SKILL.md` gains one step after lint: if `learnverify` is installed, run it and
treat `warning` findings like lint warnings (fix, or tell the user why the
exception is intentional) and `info` findings as advice. If it reports
`verify.unavailable`, continue without it and mention that semantic checks were
skipped.

## Migration from `learnpick`

- Rename the binary, library module, docs, and `just` recipe to `learnverify`.
- Remove block-type recommendation and its tests.
- Keep the private HTTP client and its environment handling (including the
  trimming and configuration fixes already made).
- Write a user-facing `LEARNVERIFY.md` when implemented; this design note then
  becomes its rationale.

## Open questions

- **Shared config file.** A single TOML file with `[lint]` and `[verify]`
  tables, each binary reading only its own table and staying strict inside it,
  would be safe. It would change the existing flat lint format, so it is left
  out unless a real need appears.
- **Concurrency and budget.** Requests are independent per quiz; a small pool
  (for example four in flight) with a per-request timeout and an overall time
  budget keeps large lessons responsive.
- **Cost ceiling.** Whether to cap the number of requests per run.
- **Model pinning.** `jev-latest` changes over time; whether to record or pin
  the model so results stay comparable.
- **Later checks.** Which of the candidates above to add next, and what context
  each needs.
- **More per-check thresholds.** `verify.annotation_contradicts_code` already
  has its own warning threshold; whether other checks need one too.
