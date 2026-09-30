## How far a definition reaches

A preview shows the whole definition, so `extent` in `definitions.rs` also
finds where it ends: by matching braces for brace languages (ignoring braces in strings,
comments, and char literals), by indentation for Python, and up to `;` for SQL,
capped at 40 lines. Tracking parentheses lets a signature spread over several
lines before its body starts.
