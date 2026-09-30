## A keyword table, not a parser

A definition almost always starts with a keyword its language reserves for
it: `fn`, `def`, `class`, `struct`, or Go's func. `src/compiler/definitions.rs`
therefore only needs small string helpers to step over modifiers such as `pub`
or `async` and read the name that follows the keyword.
