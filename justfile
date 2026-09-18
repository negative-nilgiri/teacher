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

# Validate an authored lesson without writing an artifact.
lesson-check lesson="examples/inline-lesson.json":
    cargo run --quiet --bin learnc -- check "{{lesson}}"

# Compile an authored lesson to a disposable development artifact.
lesson-build lesson="examples/inline-lesson.json" artifact="target/dev-lesson.learn":
    cargo run --quiet --bin learnc -- build "{{lesson}}" --output "{{artifact}}"

# Serve an existing compiled artifact.
serve artifact="target/dev-lesson.learn":
    cargo run --quiet --bin learn -- serve "{{artifact}}"

# Rebuild the UI, compile an authored lesson, and serve it.
run lesson="examples/inline-lesson.json" artifact="target/dev-lesson.learn": web-build
    cargo run --quiet --bin learnc -- build "{{lesson}}" --output "{{artifact}}"
    cargo run --quiet --bin learn -- serve "{{artifact}}"

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
