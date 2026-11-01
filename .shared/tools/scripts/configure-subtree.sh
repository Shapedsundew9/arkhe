#!/usr/bin/env bash
# Prepare a clone of a repository that already contains the .shared subtree.
set -euo pipefail

# Keep in sync with setup.sh.
REMOTE_NAME="shared-dev"
FETCH_URL="https://github.com/shapedsundew9/shared-dev.git"
PUSH_URL="git@github.com:shapedsundew9/shared-dev.git"
PREFIX=".shared"
BRANCH="main"

cd "$(git rev-parse --show-toplevel)"

# 1. Make `git subtree` available when git is built from source without contrib/subtree.
BIN_DIR="$HOME/.local/bin"
if ! git subtree --help >/dev/null 2>&1; then
    for dir in /usr/lib/git-core /usr/libexec/git-core /usr/local/libexec/git-core /usr/share/doc/git/contrib/subtree; do
        if [ -x "$dir/git-subtree" ]; then
            mkdir -p "$BIN_DIR"
            ln -sf "$dir/git-subtree" "$BIN_DIR/git-subtree"
            echo "Linked git-subtree into $BIN_DIR."
            break
        fi
    done
    case ":$PATH:" in
        *":$BIN_DIR:"*) ;;
        *) export PATH="$BIN_DIR:$PATH" ;;
    esac
fi

if ! git subtree --help >/dev/null 2>&1; then
    echo "Error: 'git subtree' is not available. Install git-subtree (e.g. the 'git' apt package)." >&2
    exit 1
fi

# 2. Remotes are not cloned, so re-add the subtree remote.
# Using HTTPS for fetch avoids SSH authentication requirements during container creation.
if ! git remote | grep -qx "$REMOTE_NAME"; then
    git remote add "$REMOTE_NAME" "$FETCH_URL"
    git remote set-url --push "$REMOTE_NAME" "$PUSH_URL"
    echo "Added remote '$REMOTE_NAME' (fetch: $FETCH_URL, push: $PUSH_URL)."
else
    CURRENT_PUSH_URL="$(git remote get-url --push "$REMOTE_NAME" 2>/dev/null || true)"
    if [ "$CURRENT_PUSH_URL" != "$PUSH_URL" ]; then
        git remote set-url --push "$REMOTE_NAME" "$PUSH_URL"
    fi
fi

# Repo-local shortcuts: `git shared-pull` and `git shared-push`
SYNC_ENV="python3 $PREFIX/tools/scripts/sync_devcontainer_env.py --if-opted-in"
git config alias.shared-pull "!git subtree pull --prefix=$PREFIX $REMOTE_NAME $BRANCH --squash && $SYNC_ENV"
git config alias.shared-push "subtree push --prefix=$PREFIX $REMOTE_NAME $BRANCH"

# 3. Ensure pull.rebase preserves merge commits
CURRENT_REBASE="$(git config pull.rebase 2>/dev/null || true)"
if [ "$CURRENT_REBASE" = "true" ]; then
    echo "Notice: Setting local repo 'pull.rebase' to 'merges' to preserve subtree merge commits."
    git config pull.rebase merges
fi

# 4. Fetch subtree remote history.
if ! git fetch --quiet "$REMOTE_NAME" "$BRANCH"; then
    echo "Warning: could not fetch '$REMOTE_NAME' ($BRANCH). Run 'git fetch $REMOTE_NAME' before subtree pull/push." >&2
fi

# 5. Verify subtree metadata exists. If $PREFIX exists but has no subtree squash commits,
# the repository was likely created from a GitHub template. Link it to the remote.
has_subtree_metadata() {
    [ -n "$(git log -1 --grep="^git-subtree-dir: $PREFIX/*\$" HEAD 2>/dev/null)" ]
}

if [ -d "$PREFIX" ] && ! has_subtree_metadata; then
    if git rev-parse -q --verify "$REMOTE_NAME/$BRANCH" >/dev/null 2>&1; then
        echo "Notice: '$PREFIX' exists but lacks git-subtree metadata (e.g. created from a template)."
        echo "Linking '$PREFIX' as a squash subtree of '$REMOTE_NAME/$BRANCH'..."

        REMOTE_REV="$(git rev-parse "$REMOTE_NAME/$BRANCH")"
        REMOTE_REV_SHORT="$(git rev-parse --short "$REMOTE_REV")"
        REMOTE_TREE="$(git rev-parse "$REMOTE_REV^{tree}")"

        SQUASH_MSG="$(printf "Squashed '%s/' content from commit %s\n\ngit-subtree-dir: %s\ngit-subtree-split: %s\n" "$PREFIX" "$REMOTE_REV_SHORT" "$PREFIX" "$REMOTE_REV")"
        SQUASH_COMMIT="$(printf "%s" "$SQUASH_MSG" | git commit-tree "$REMOTE_TREE")"

        HEAD_TREE="$(git write-tree)"
        HEAD_REV="$(git rev-parse HEAD)"
        MERGE_COMMIT="$(printf "Merge commit '%s' as '%s'\n" "$SQUASH_COMMIT" "$PREFIX" | git commit-tree "$HEAD_TREE" -p "$HEAD_REV" -p "$SQUASH_COMMIT")"

        git reset "$MERGE_COMMIT"
        echo "Successfully linked '$PREFIX' to '$REMOTE_NAME/$BRANCH'."
    else
        echo "Warning: '$PREFIX' lacks git-subtree metadata and '$REMOTE_NAME/$BRANCH' is not available." >&2
        echo "Run 'git fetch $REMOTE_NAME' and re-run this script to link '$PREFIX'." >&2
    fi
fi

# 6. Catch compose changes that arrived without `git shared-pull` (takes effect on the next rebuild).
if [ -f "$PREFIX/tools/scripts/sync_devcontainer_env.py" ]; then
    $SYNC_ENV
fi
