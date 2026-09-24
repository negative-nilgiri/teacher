# Agent Teacher

Agent Teacher compiles agent-authored JSON lessons into self-contained `.learn`
artifacts and serves them as a local interactive experience.

The project ships three Rust binaries from one Cargo package:

- `learnc` validates, lints, and compiles lesson sources.
- `learn` serves a compiled artifact and owns learner-session state.
- `learnpick` optionally recommends one block type for one teaching unit.

The accepted v1 behavior is specified in [`DESIGN_REVIEW.md`](DESIGN_REVIEW.md).
Agents generating lessons should start with the concise
[`docs/AUTHORING.md`](docs/AUTHORING.md) guide and checked examples.
Developers changing the implementation should start with the architecture and
code map in [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md).

Run `just` (or `just --list`) to see the documented development commands.

## Install and use

The installed binaries require only `git` at runtime. Building or installing
from a source checkout also requires Node.js and npm because the React bundle is
generated locally and intentionally not tracked. `just install` installs the
pinned frontend dependencies, builds the bundle, and installs all three Rust
binaries:

```sh
just install
just lesson-check path/to/lesson.json
just lesson-lint path/to/lesson.json
just lesson-build path/to/lesson.json --output path/to/lesson.learn
just serve path/to/lesson.learn --open
```

For local development, `just run --open --text` rebuilds the UI, compiles
`examples/inline-lesson.json` to `target/dev-lesson.learn`, and serves it while
forwarding every supplied option to `learn serve`.
All `lesson-check`, `lesson-lint`, `lesson-build`, and `serve` arguments are forwarded without
interpretation. `just learnc ...` and `just learn ...` expose completely raw
passthroughs to either core binary. `just learnpick ...` does the same for the
optional adviser; see [`docs/LEARNPICK.md`](docs/LEARNPICK.md).
The repository-backed example has real committed and dirty-worktree inputs;
`just repository-example` creates that disposable Git fixture, compiles it, and
serves it. See the [`examples` guide](examples/README.md) for the manual flow.

Commands emit JSON by default for agent use, including help, version, invalid
usage, and warnings. Pass `--text` or `-t` for human-readable output; for
example, `learnc -t --help`. `learn serve` binds to a random loopback port and
opens a browser only when `--open` is supplied.

`learnc lint` first runs the same validity checks as `learnc check`, then reports
separate authoring-policy findings. Its findings never change `check` or `build`
results. Pass `--config path/to/lint.toml` for a partial config file, or set
individual thresholds with flags such as `--max-code-lines 40`; CLI flags take
precedence over the file. See [`config.example.toml`](config.example.toml) for
all settings and [`docs/LINT_DESIGN.md`](docs/LINT_DESIGN.md) for the rules.

## Package contract

`web/dist` is an ignored build output. The Cargo package deliberately includes
that generated directory while excluding frontend source, `node_modules`, and
generated TypeScript metadata. `learn` embeds the bundle at Rust compile time,
so running the installed binaries—and installing an already assembled Cargo
package—does not invoke npm or require Node.js. Source-checkout builds use the
`just` recipes to install and build the frontend first.

Source schema, artifact schema, and package versions are independent SemVer
values. An incompatible `.learn` artifact is disposable: rebuild it from its
lesson JSON with a compatible `learnc` instead of migrating it.

## Release verification

Run `just release-check` before tagging a package release. It rebuilds the web
bundle, runs the normal Rust checks, verifies the Cargo package contents, installs
all three binaries into an isolated temporary Cargo root with failing Node/npm shims,
compiles and serves a smoke lesson, fetches its embedded UI and API, and checks
that incompatible artifacts return actionable rebuild guidance.

`just package-smoke` first installs and builds the frontend, then assembles the
Cargo package and disables Node for the consumer-install phase. This proves that
the generated package is self-contained even though producing it from source
requires Node.js.
