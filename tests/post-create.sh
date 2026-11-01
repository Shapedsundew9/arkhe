#!/usr/bin/env bash
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TEST_DIR="$(mktemp -d)"
trap 'rm -rf -- "$TEST_DIR"' EXIT
export COMMAND_LOG="$TEST_DIR/commands.log"

rustup() {
    printf 'rustup %s\n' "$*" >> "$COMMAND_LOG"
    echo "Post-create setup must not modify the image-provided Rust toolchain." >&2
    return 1
}

cargo() {
    printf 'cargo %s\n' "$*" >> "$COMMAND_LOG"
    return "${CARGO_STATUS:-0}"
}

export -f rustup cargo
export HOME="$TEST_DIR/home"
mkdir -p "$HOME"
unset GIT_USER_NAME GIT_USER_EMAIL GIT_CONFIG_GLOBAL
export GIT_CONFIG_NOSYSTEM=1
cd "$TEST_DIR"
touch Cargo.toml

bash "$REPO_ROOT/.devcontainer/post-create.sh"
printf '%s\n' 'cargo check' > expected.log
diff -u expected.log "$COMMAND_LOG"

export CARGO_STATUS=23
: > "$COMMAND_LOG"
status=0
bash "$REPO_ROOT/.devcontainer/post-create.sh" || status=$?
if [[ "$status" -ne "$CARGO_STATUS" ]]; then
    echo "Expected cargo check failure $CARGO_STATUS, got $status" >&2
    exit 1
fi
printf '%s\n' 'cargo check' > expected.log
diff -u expected.log "$COMMAND_LOG"
unset CARGO_STATUS

assert_git_config() {
    local key="$1" expected="$2" actual
    actual="$(git config --global --get "$key" || true)"
    if [[ "$actual" != "$expected" ]]; then
        echo "Expected git $key '$expected', got '$actual'" >&2
        exit 1
    fi
}

# Unset or empty host variables leave the identity unconfigured.
GIT_USER_NAME="" bash "$REPO_ROOT/.devcontainer/post-create.sh" >/dev/null
assert_git_config user.name ""
assert_git_config user.email ""

# Host variables populate a missing identity.
GIT_USER_NAME="Host Name" GIT_USER_EMAIL="host@example.com" \
    bash "$REPO_ROOT/.devcontainer/post-create.sh" >/dev/null
assert_git_config user.name "Host Name"
assert_git_config user.email "host@example.com"

# An existing identity is never overwritten.
GIT_USER_NAME="Other Name" GIT_USER_EMAIL="other@example.com" \
    bash "$REPO_ROOT/.devcontainer/post-create.sh" >/dev/null
assert_git_config user.name "Host Name"
assert_git_config user.email "host@example.com"

echo "Container setup tests passed."
