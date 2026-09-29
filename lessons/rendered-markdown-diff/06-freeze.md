## Freezing in Git diffs

The Git diff resolver in `src/repository/git.rs` reads the two documents under
the same snapshot guard as the diff itself: the base blob, and the target blob
or worktree file. A new file has no before, a deleted one no after. The result
is stored on the diff file as `rendered`, and artifact validation rejects it on
a non-Markdown file or with empty segments.

Inline and file patches only contain hunks, so their Markdown files keep the
line view. Lint reports that case as `lint.diff.markdown_patch`, an info finding,
because a hand-made patch is sometimes the only option.
