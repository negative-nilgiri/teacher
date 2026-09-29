## Where to look when reviewing

- **Block boundaries** in `markdown_diff.rs`: the one place where the Rust
  parser and the browser's parser must agree on where blocks start and end.
- **Known limits:** no word-level marking inside a changed block; a footnote
  definition that changes on its own renders nothing in the rendered view (the
  source view shows it); no rendered view for patch sources.
- **Tests:** block splitting, matching, trimming and definitions in
  `markdown_diff.rs`; a real repository in `src/repository/tests.rs`; artifact
  validation; the lint rule; a React test for the switch, labels, and gaps.
