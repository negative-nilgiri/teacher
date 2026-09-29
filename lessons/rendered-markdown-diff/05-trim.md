## Showing the same region as the line diff

The author's line ranges and `context_lines` still decide what the diff
shows. `render_markdown_diff`, also in `markdown_diff.rs`, keeps a block only if it overlaps a
displayed old or new line; consecutive blocks outside that region collapse into
a gap. Link and footnote definitions are appended to every segment so a
reference-style link still resolves when its paragraph renders alone.
