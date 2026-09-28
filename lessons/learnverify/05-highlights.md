## Highlight state

A highlight request needs the model to connect line ranges with code, and
models count lines badly. So `numbered` in `checks.rs` prefixes every displayed line with its
source-file number, and `group_state` sends each range's text verbatim next to
its numbers. The artifact stores ranges relative to the displayed fragment;
`first_line + start - 1` turns them back into the file lines the author wrote.

Only the Markdown blocks directly before and after the code block are sent as
its surroundings, via `adjacent_markdown`. A neighbour that is code or a quiz
counts as no prose at all, which is what `verify.highlight_unexplained` needs.
