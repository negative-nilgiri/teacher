# Findings point to an edit

Lint diagnostics have their own shape: stable code, intrinsic severity, computed `fatal`, message, block ID, JSON pointer, suggestion, and a primary file span. A lesson-wide finding uses `block_id: null`. The compiler's existing diagnostics stay unchanged.

`src/lint/span.rs` indexes the original JSON by pointer. Its `string_range` method maps decoded characters back to the bytes of the authored JSON string, so escapes such as `\n` or Unicode surrogate pairs still produce accurate one-based line and column spans. When the problem is in a referenced worktree file, lint can point there; when content comes from a Git revision, the editable declaration in `lesson.json` is primary.
