## Validating and freezing

The resolver in `links.rs` checks each link: it must name an existing block, and a line range must lie inside the
lines that block displays: file lines for file and Git-blob code, positions
for inline code, new-side lines for diffs. Anything else fails `learnc check`.
A valid link goes into one lesson-wide table in the artifact (1.6.0); the
Markdown text itself is not rewritten.
