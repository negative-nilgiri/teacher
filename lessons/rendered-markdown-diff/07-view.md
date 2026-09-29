## The rendered view

`RenderedMarkdown` in `web/src/components/DiffBlock.tsx` renders every segment
with the normal Markdown component (GFM, KaTeX, raw HTML off). Added and
removed segments get their colors plus visually hidden "Added:" and
"Removed:" prefixes, so the change does not rely on color alone. A file with
segments starts in this view; the `Rendered | Source` switch shows the line
diff.
