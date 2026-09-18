# Agent Teacher

Agent Teacher compiles agent-authored JSON lessons into self-contained `.learn`
artifacts and serves them as a local interactive experience.

The project ships two Rust binaries from one Cargo package:

- `learnc` validates and compiles lesson sources.
- `learn` serves a compiled artifact and owns learner-session state.

The accepted v1 behavior is specified in [`DESIGN_REVIEW.md`](DESIGN_REVIEW.md).
Agents generating lessons should start with the concise
[`docs/AUTHORING.md`](docs/AUTHORING.md) guide and checked examples.

Run `just` (or `just --list`) to see the documented development commands.

## Install and use

The only runtime prerequisite is `git`; Node.js is needed only when changing the
React frontend. Install both Rust binaries from the package root:

```sh
cargo install --path . --locked
learnc check lesson.json
learnc build lesson.json
learn serve lesson.learn
```

Commands emit JSON by default for agent use. Pass `--text` or `-t` for concise
human-readable output. `learn serve` binds to a random loopback port and opens a
browser only when `--open` is supplied.

## Package contract

The Cargo package deliberately includes `web/dist` and excludes frontend source,
`node_modules`, and generated TypeScript metadata. `learn` embeds that prebuilt
bundle at Rust compile time, so installing and running the packaged binaries does
not invoke npm or require Node.js.

Source schema, artifact schema, and package versions are independent SemVer
values. An incompatible `.learn` artifact is disposable: rebuild it from its
lesson JSON with a compatible `learnc` instead of migrating it.

## Release verification

Run `just release-check` before tagging a package release. It rebuilds the web
bundle, runs the normal Rust checks, verifies the Cargo package contents, installs
both binaries into an isolated temporary Cargo root with failing Node/npm shims,
compiles and serves a smoke lesson, fetches its embedded UI and API, and checks
that incompatible artifacts return actionable rebuild guidance.

`just package-smoke` runs only the packaging/install smoke test against the
already-built `web/dist`; it intentionally does not rebuild the frontend. This is
the command that proves a consumer installation does not need Node.js.
