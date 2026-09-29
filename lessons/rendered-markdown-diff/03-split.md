## Splitting into blocks

`split` in `src/repository/markdown_diff.rs` walks `pulldown-cmark` events with
their source offsets and keeps one unit per top-level block. Lists and block
quotes are containers: their direct children become the units, so adding one
bullet marks one bullet. Every unit is taken as whole source lines, which keeps
list markers, ordered-list numbers, and `>` prefixes, so each unit renders on
its own exactly as it did in the document.
