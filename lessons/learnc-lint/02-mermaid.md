# Mermaid is checked at the compilation boundary

The change in `src/compiler/mod.rs` checks Mermaid code after its inline, file, or Git blob source has resolved and its display language is known. The `validate_mermaid` helper passes the **exact displayed text** to `mermaid-svg` 0.7.0. A parser rejection is a compiler diagnostic, so it stops both `check` and `build`.

This Rust parser is deliberately a partial gate. Some diagrams it accepts may still fail in the browser's Mermaid parser; authors should inspect diagrams in the lesson UI. Lint separately warns about simple flowchart `subgraph` and direct `style` constructs, which can be legitimate authoring choices.
