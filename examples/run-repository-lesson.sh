#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
example_root=$("$project_root/examples/create-repository-lesson.sh")
artifact="$example_root/lesson.learn"

cleanup() {
    rm -rf "$example_root"
}
trap cleanup EXIT INT TERM

cd "$project_root"
cargo run --quiet --bin learnc -- check --repo "$example_root" "$example_root/lesson.json"
cargo run --quiet --bin learnc -- build --repo "$example_root" \
    "$example_root/lesson.json" --output "$artifact"
cargo run --quiet --bin learn -- serve "$artifact"
