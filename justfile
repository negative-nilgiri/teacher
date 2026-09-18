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

# Build both Rust binaries after refreshing embedded frontend assets.
build: web-build
    cargo build

# Run all non-mutating local verification checks.
verify: fmt-check lint test

# Show the exact files that Cargo will place in the distributable crate.
package-list:
    cargo package --list --allow-dirty

# Package, install, and exercise both binaries without allowing Node.js to run.
package-smoke:
    bash scripts/package-smoke.sh

# Rebuild production assets and run every source and distribution check.
release-check: web-build verify package-smoke
