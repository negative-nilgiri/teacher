# Preserve recipe arguments as separate shell arguments for safe passthrough.
set positional-arguments := true

# Show the available project commands. This is intentionally the default.
default:
    @just --list

# Check every Rust target without producing release artifacts.
check: web-build
    cargo check --all-targets

# Run the Rust test suite.
test: web-build
    cargo test --all-targets

# Format Rust sources in place.
fmt:
    cargo fmt --all

# Verify formatting without changing files.
fmt-check:
    cargo fmt --all -- --check

# Run Clippy across every Rust target and fail on warnings.
lint: web-build
    cargo clippy --all-targets -- -D warnings

# Install the frontend's pinned JavaScript dependencies for development.
web-install:
    npm --prefix web ci

# Start the frontend development server.
web-dev: web-install
    npm --prefix web run dev

# Build the frontend bundle embedded by production Rust builds.
web-build: web-install
    npm --prefix web run build

# Run the React behavior tests once.
web-test: web-install
    npm --prefix web test

# Build all three Rust binaries after refreshing embedded frontend assets.
build: web-build
    cargo build

# Install learnc, learn, and the optional learnpick adviser from this checkout.
install: web-build
    cargo install --path . --locked --force

# Pass arbitrary arguments directly to the compiler binary.
learnc *args: web-build
    cargo run --quiet --bin learnc -- "$@"

# Pass arbitrary arguments directly to the runtime binary.
learn *args: web-build
    cargo run --quiet --bin learn -- "$@"

# Pass arbitrary arguments directly to the optional block-picker binary.
learnpick *args: web-build
    cargo run --quiet --bin learnpick -- "$@"

# Validate an authored lesson, forwarding every argument after `check`.
lesson-check *args: web-build
    cargo run --quiet --bin learnc -- check "$@"

# Compile an authored lesson, forwarding every argument after `build`.
lesson-build *args: web-build
    cargo run --quiet --bin learnc -- build "$@"

# Serve an existing artifact, forwarding every argument after `serve`.
serve *args: web-build
    cargo run --quiet --bin learn -- serve "$@"

# Rebuild the UI, compile the inline example, and forward all arguments to `learn serve`.
run *serve_args: web-build
    cargo run --quiet --bin learnc -- build examples/inline-lesson.json --output target/dev-lesson.learn
    cargo run --quiet --bin learn -- serve target/dev-lesson.learn "$@"

# Create, compile, and serve the repository-backed example in a temporary Git repository.
repository-example: web-build
    bash examples/run-repository-lesson.sh

# Run all non-mutating Rust and frontend verification checks.
verify: fmt-check lint test web-test

# Show the exact files that Cargo will place in the distributable crate.
package-list: web-build
    cargo package --list --allow-dirty

# Build assets, then package and exercise installation with Node.js disabled.
package-smoke: web-build
    bash scripts/package-smoke.sh

# Rebuild production assets and run every source and distribution check.
release-check: web-build verify package-smoke
