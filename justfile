# Preserve recipe arguments as separate shell arguments for safe passthrough.
set positional-arguments := true

# Show the available project commands. This is intentionally the default.
default:
    @just --list

# Check every Rust target without producing release artifacts.
check:
    cargo check --all-targets

# Run the Rust test suite.
test:
    cargo test --all-targets

# Format Rust sources in place.
fmt:
    cargo fmt --all

# Verify formatting without changing files.
fmt-check:
    cargo fmt --all -- --check

# Run Clippy across every Rust target and fail on warnings.
lint:
    cargo clippy --all-targets -- -D warnings

# Install the frontend's pinned JavaScript dependencies for development.
web-install:
    npm --prefix web ci

# Start the frontend development server.
web-dev:
    npm --prefix web run dev

# Build the frontend bundle embedded by production Rust builds.
web-build:
    npm --prefix web run build

# Run the React behavior tests once.
web-test:
    npm --prefix web test

# Build both Rust binaries after refreshing embedded frontend assets.
build: web-build
    cargo build

# Install the learnc and learn binaries from this checkout.
install: web-build
    cargo install --path . --locked --force

# Pass arbitrary arguments directly to the compiler binary.
learnc *args:
    cargo run --quiet --bin learnc -- "$@"

# Pass arbitrary arguments directly to the runtime binary.
learn *args:
    cargo run --quiet --bin learn -- "$@"

# Validate an authored lesson, forwarding every argument after `check`.
lesson-check *args:
    cargo run --quiet --bin learnc -- check "$@"

# Compile an authored lesson, forwarding every argument after `build`.
lesson-build *args:
    cargo run --quiet --bin learnc -- build "$@"

# Serve an existing artifact, forwarding every argument after `serve`.
serve *args:
    cargo run --quiet --bin learn -- serve "$@"

# Rebuild the UI, compile the inline example, and forward all arguments to `learn serve`.
run *serve_args: web-build
    cargo run --quiet --bin learnc -- build examples/inline-lesson.json --output target/dev-lesson.learn
    cargo run --quiet --bin learn -- serve target/dev-lesson.learn "$@"

# Create, compile, and serve the repository-backed example in a temporary Git repository.
repository-example:
    bash examples/run-repository-lesson.sh

# Run all non-mutating Rust and frontend verification checks.
verify: fmt-check lint test web-test

# Show the exact files that Cargo will place in the distributable crate.
package-list:
    cargo package --list --allow-dirty

# Package, install, and exercise both binaries without allowing Node.js to run.
package-smoke:
    bash scripts/package-smoke.sh

# Rebuild production assets and run every source and distribution check.
release-check: web-build verify package-smoke
