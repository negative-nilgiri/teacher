#!/usr/bin/env bash
set -euo pipefail

usage() {
    echo "usage: $0 [empty-destination]" >&2
}

if [[ $# -gt 1 ]]; then
    usage
    exit 2
fi

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
created_temporary=false

if [[ $# -eq 1 ]]; then
    example_root=$1
    mkdir -p "$example_root"
    if [[ -n $(find "$example_root" -mindepth 1 -maxdepth 1 -print -quit) ]]; then
        echo "repository example: destination must be empty: $example_root" >&2
        exit 1
    fi
else
    example_root=$(mktemp -d "${TMPDIR:-/tmp}/agent-teacher-repository-example.XXXXXX")
    created_temporary=true
fi

cleanup_failed_temporary_directory() {
    status=$?
    if [[ $status -ne 0 && $created_temporary == true ]]; then
        rm -rf "$example_root"
    fi
    exit "$status"
}
trap cleanup_failed_temporary_directory EXIT

example_root=$(cd "$example_root" && pwd)
mkdir -p "$example_root/docs" "$example_root/src" "$example_root/changes"

cp "$project_root/examples/repository-lesson.json" "$example_root/lesson.json"

cat >"$example_root/docs/overview.md" <<'EOF'
# Queue change

The queue now removes from the front.
EOF

cat >"$example_root/src/queue.rs" <<'EOF'
pub fn take_next(queue: &mut Vec<i32>) -> Option<i32> {
    queue.pop()
}
EOF

cat >"$example_root/changes/queue.patch" <<'EOF'
diff --git a/src/queue.rs b/src/queue.rs
index 1111111..2222222 100644
--- a/src/queue.rs
+++ b/src/queue.rs
@@ -2 +2 @@
-    queue.pop()
+    Some(queue.remove(0))
EOF

git -C "$example_root" init -q
git -C "$example_root" config user.name "Agent Teacher Example"
git -C "$example_root" config user.email "example@example.invalid"
git -C "$example_root" config commit.gpgsign false
git -C "$example_root" add .
git -C "$example_root" commit -qm "base lesson inputs"

cat >"$example_root/src/queue.rs" <<'EOF'
pub fn take_next(queue: &mut Vec<i32>) -> Option<i32> {
    Some(queue.remove(0))
}
EOF

trap - EXIT
printf '%s\n' "$example_root"
