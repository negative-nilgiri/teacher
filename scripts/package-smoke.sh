#!/usr/bin/env bash
set -euo pipefail

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
smoke_root=$(mktemp -d "${TMPDIR:-/tmp}/agent-teacher-package-smoke.XXXXXX")
server_pid=

cleanup() {
    if [[ -n "$server_pid" ]] && kill -0 "$server_pid" 2>/dev/null; then
        kill "$server_pid" 2>/dev/null || true
        wait "$server_pid" 2>/dev/null || true
    fi
    rm -rf "$smoke_root"
}
trap cleanup EXIT INT TERM

cd "$project_root"

package_list="$smoke_root/package-list.txt"
cargo package --list --allow-dirty >"$package_list"

for required in \
    Cargo.toml \
    Cargo.lock \
    src/bin/learn.rs \
    src/bin/learnc.rs \
    web/dist/index.html \
    tests/fixtures/smoke-lesson.json
do
    if ! grep -Fxq "$required" "$package_list"; then
        echo "package smoke: required file is missing: $required" >&2
        exit 1
    fi
done

if ! grep -Eq '^web/dist/assets/.+' "$package_list"; then
    echo "package smoke: web/dist has no packaged assets" >&2
    exit 1
fi

if grep -Eq '(^|/)node_modules/|^web/src/|\.tsbuildinfo$' "$package_list"; then
    echo "package smoke: development-only frontend files leaked into the package" >&2
    exit 1
fi

# This verifies the crate assembled from the allowlist, including the embedded
# frontend, rather than merely compiling the current worktree.
cargo package --allow-dirty

mkdir -p "$smoke_root/no-node" "$smoke_root/cargo-root"
for command_name in node npm npx; do
    printf '#!/usr/bin/env sh\necho "%s must not run during cargo install" >&2\nexit 99\n' \
        "$command_name" >"$smoke_root/no-node/$command_name"
    chmod +x "$smoke_root/no-node/$command_name"
done

PATH="$smoke_root/no-node:$PATH" cargo install \
    --path "$project_root" \
    --root "$smoke_root/cargo-root" \
    --locked \
    --force

learn_bin="$smoke_root/cargo-root/bin/learn"
learnc_bin="$smoke_root/cargo-root/bin/learnc"
test -x "$learn_bin"
test -x "$learnc_bin"
"$learn_bin" --version >/dev/null
"$learnc_bin" --version >/dev/null

artifact="$smoke_root/smoke.learn"
PATH="$smoke_root/no-node:$PATH" "$learnc_bin" build \
    "$project_root/tests/fixtures/smoke-lesson.json" \
    --output "$artifact" >"$smoke_root/build.json"
grep -Fq '"ok":true' "$smoke_root/build.json"
test -s "$artifact"

set +e
version_output=$("$learn_bin" serve \
    "$project_root/tests/fixtures/unsupported-artifact.learn.json" 2>/dev/null)
version_status=$?
set -e
if [[ "$version_status" -eq 0 ]]; then
    echo "package smoke: unsupported artifact unexpectedly started" >&2
    exit 1
fi
if [[ "$version_output" != *'"code":"artifact_version_unsupported"'* ]] \
    || [[ "$version_output" != *rebuild* ]]; then
    echo "package smoke: version failure lacks structured rebuild guidance" >&2
    exit 1
fi

"$learn_bin" serve "$artifact" >"$smoke_root/server.json" 2>"$smoke_root/server.log" &
server_pid=$!
for _ in {1..100}; do
    if [[ -s "$smoke_root/server.json" ]]; then
        break
    fi
    if ! kill -0 "$server_pid" 2>/dev/null; then
        echo "package smoke: learn exited before reporting its URL" >&2
        sed -n '1,120p' "$smoke_root/server.log" >&2
        exit 1
    fi
    sleep 0.05
done

server_url=$(sed -n 's/.*"url":"\([^"]*\)".*/\1/p' "$smoke_root/server.json" | head -n 1)
if [[ -z "$server_url" || "$server_url" != http://127.0.0.1:* ]]; then
    echo "package smoke: learn did not report an IPv4 loopback URL" >&2
    exit 1
fi

curl --fail --silent --show-error "$server_url" >"$smoke_root/index.html"
curl --fail --silent --show-error "${server_url}api/v1/state" >"$smoke_root/state.json"
grep -Fq '<div id="root"></div>' "$smoke_root/index.html"
grep -Fq '"title":"Packaged smoke lesson"' "$smoke_root/state.json"
if grep -Fq 'correct_choice_id' "$smoke_root/state.json" \
    || grep -Fq '"explanation"' "$smoke_root/state.json"; then
    echo "package smoke: initial state exposed private quiz material" >&2
    exit 1
fi

kill "$server_pid"
wait "$server_pid" 2>/dev/null || true
server_pid=

echo "package smoke: packaged assets, Node-free install, both binaries, server, and version gate passed"
