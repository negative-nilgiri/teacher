## Shared loading

`learnverify` must reject exactly the lessons `learnc check` rejects, and point
at the same editable spans as lint. Rather than copying lint's preparation, the
first commit moves it out of `lint_file` in `src/lint/mod.rs` into
`load_lesson`, which returns the validated source, the artifact, the span index,
and the absolute root. Lint behaves exactly as before; its tests are unchanged.

The same commit adds `prompt_pointer`, which knows that schemas before 2.0.0
store a quiz prompt as a plain string, and splits text rendering so each
finding can carry an optional `= note:` line for the probability.
