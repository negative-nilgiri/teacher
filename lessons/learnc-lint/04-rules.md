# Rules use displayed content and authored intent

`src/lint/rules.rs` pairs each authored block with its compiled node. That lets it count decoded inline characters, inspect resolved languages, find new files through structured diff `is_new`, and measure displayed code rather than the whole source file. Highlight coverage counts the **union** of displayed lines, so overlapping ranges do not inflate the ratio.

The later excerpt from `src/lint/mod.rs` shows the exit policy. Every rule has an intrinsic severity. `--ignore-below` removes findings first; `--warning-as-error` then marks the remaining findings fatal according to its threshold. A warning filtered out of the report cannot fail lint. A clean JSON run emits `{"diagnostics":[]}` and a clean text run emits nothing.
