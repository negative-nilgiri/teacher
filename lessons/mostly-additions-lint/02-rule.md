## The rule

`mostly_additions` in `src/lint/rules.rs` runs for every file of every diff
block. It works per hunk, because one file can combine a real change in one
place with a large new function in another: only the addition-heavy hunk needs
to move into a code block.

The suggestion names the fix concretely: narrow the diff, then add a code block
right after it for the reported lines. Its source matches the diff's after
side, so the code block and the diff never show different versions of the file.
