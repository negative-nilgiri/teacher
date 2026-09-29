## Matching blocks

In `markdown_diff.rs`, blocks are matched by longest common subsequence on their source text. The raw
sequence lists all removals before all additions in a run of changes, which
reads badly when two neighbouring blocks both change. `pair_replacements`
interleaves each run, so every replaced block reads as its old version followed
by its new one.
