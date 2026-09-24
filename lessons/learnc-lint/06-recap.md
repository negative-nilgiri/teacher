# What to remember

The implementation keeps correctness and authoring policy separate. Compilation freezes trustworthy content and performs a partial Mermaid syntax check. Lint examines that content with source-aware rules, reports editable locations, and applies caller-selected output and exit thresholds. The `.learn` artifact format and learner runtime did not need to change.

The relevant implementation is in `src/compiler/mod.rs`, `src/lint/`, and `src/bin/learnc.rs`. The contract tests exercise rule behavior, clean and failing CLI output, configuration, compiler failures, and the Mermaid gate.
