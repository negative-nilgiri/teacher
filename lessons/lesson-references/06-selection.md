## From a selection to source lines

A code listing is one block of highlighted text, not one element per line.
`selectedLines` in `web/src/selection.ts` counts the newlines before the start
and the end of the selection, then adds the block's `first_line` offset to get
file lines. For a diff, every row the selection spans contributes its old and
new line numbers.
