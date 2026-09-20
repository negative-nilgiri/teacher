#!/usr/bin/env bash
set -euo pipefail

examples=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
repository=${1:-$(mktemp -d "${TMPDIR:-/tmp}/agent-teacher-repository-example.XXXXXX")}

mkdir -p "$repository"
cp "$examples/repository-lesson.json" "$repository/lesson.json"
cp -R "$examples/repository-fixture/." "$repository"

git -C "$repository" init -q
git -C "$repository" config user.name "Agent Teacher Example"
git -C "$repository" config user.email "example@example.invalid"
git -C "$repository" config commit.gpgsign false
git -C "$repository" add .
git -C "$repository" commit -qm "base lesson inputs"

cp "$repository/changes/queue.after.rs" "$repository/src/queue.rs"

printf '%s\n' "$repository"
