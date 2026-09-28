## Planning the requests

`plan` in `src/learnverify/checks.rs` makes one job per multiple-choice block
and one per code block with highlights. Every job's `state` is an excerpt of the
lesson in authored order, with `subject` naming the block under review, because
the checks are about a lesson that happens to contain a quiz, not about a quiz
in isolation.

For a quiz, the excerpt is its teaching unit: the Markdown, code, and diff
blocks back to the previous question. `unit_context` below decides which blocks
those are and how much of them fits in its `max_chars` budget, which comes
from the max_context_chars setting.

Choices are taken from the authored source in authored order, since the compiled
artifact shuffles them and keeps no way back to `/blocks/N/choices/K`.
