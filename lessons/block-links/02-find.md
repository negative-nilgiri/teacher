## Finding links

The compiler looks for links in every Markdown-bearing field: Markdown blocks
(after reading file sources), captions, highlight annotations, quiz prompts,
choices, their explanations, hints, and quiz explanations. `find_links` in
`src/compiler/links.rs` lets `pulldown-cmark` parse each text, so a `#` inside
a code span or a fence is never mistaken for a link.
