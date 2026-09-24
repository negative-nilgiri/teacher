# Configuring lint without changing compilation

The `learnc lint` branch in `src/bin/learnc.rs` loads an explicit TOML config, applies any threshold flags, validates the effective values, runs `lint_file`, then applies filtering and fatality. There is no config auto-discovery. Missing keys retain defaults, while unknown keys fail so a typo cannot silently disable a rule. The root `config.example.toml` lists every key and its default.

For example, `max_code_lines = 40` changes only the lint threshold for a long displayed code block. `--max-code-lines 50` on the same invocation takes precedence over that file value. Neither setting changes the source schema, a rule's intrinsic severity, or the validity criteria used by `check` and `build`.
