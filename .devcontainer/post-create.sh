#!/bin/bash

# This script runs after the container is created.
# The 'set -e' command ensures that the script will exit immediately if a command fails.
set -e

configure_git_identity() {
    local key="$1" value="$2"
    if [[ -n "$value" ]] && ! git config --global --get "$key" >/dev/null 2>&1; then
        git config --global "$key" "$value"
        echo "Set git $key from host environment."
    fi
}

if command -v git >/dev/null 2>&1; then
    configure_git_identity user.name "${GIT_USER_NAME:-}"
    configure_git_identity user.email "${GIT_USER_EMAIL:-}"
fi

if [[ -f Cargo.toml ]]; then
    cargo check
fi

if [[ -x .shared/tools/scripts/configure-subtree.sh ]]; then
    .shared/tools/scripts/configure-subtree.sh
fi

if [[ -x .shared/devcontainer/post-create.sh ]]; then
    .shared/devcontainer/post-create.sh
fi