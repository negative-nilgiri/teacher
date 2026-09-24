# One command, two decisions

`learnc check` and `learnc build` answer whether a lesson can be compiled. They validate the schema, resolve repository inputs, and freeze the resulting content. `learnc lint` runs that same pipeline first, then asks whether the **authoring choices** make a useful lesson. A lint finding can fail the lint command, but it never changes the result of `check` or `build`.

The sequence diagram shows the control flow. In `src/lint/mod.rs`, `lint_file` reads `lesson.json` once, compiles it in memory, and uses the frozen artifact for policy checks. The original JSON remains available for editable source locations. If compilation fails, policy rules do not run.
