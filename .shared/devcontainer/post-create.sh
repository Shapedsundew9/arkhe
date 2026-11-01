#!/usr/bin/env bash
# Shared dev container post-create steps, reused across repositories.
# Call from the repository's .devcontainer/post-create.sh:
#   bash .shared/devcontainer/post-create.sh
set -euo pipefail

BIN_DIR="$HOME/.local/bin"
case ":$PATH:" in
    *":$BIN_DIR:"*) ;;
    *) export PATH="$BIN_DIR:$PATH" ;;
esac

echo "Installing CodeGraph..."
if ! command -v codegraph >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL https://raw.githubusercontent.com/colbymchenry/codegraph/main/install.sh | bash
fi

echo "Installing Antigravity CLI..."
if ! command -v agy >/dev/null 2>&1 && ! command -v antigravity >/dev/null 2>&1; then
    curl --proto '=https' --tlsv1.2 -fsSL https://antigravity.google/cli/install.sh | bash
fi

echo "Configuring Antigravity CLI..."
ANTIGRAVITY_SETTINGS="$HOME/.gemini/antigravity-cli/settings.json"
if [[ ! -f "$ANTIGRAVITY_SETTINGS" ]]; then
    mkdir -p "$(dirname "$ANTIGRAVITY_SETTINGS")"
    cat > "$ANTIGRAVITY_SETTINGS" <<'EOF'
{
    "colorScheme": "solarized dark",
    "trustedWorkspaces": [
        "/workspaces"
    ],
    "executionMode": "accept-edits",
    "model": "Gemini 3.8 Flash (High)",
    "permissions": {
        "allow": [
            "write_file(*)",
            "command(*)",
            "read_url(*)",
            "read_browser_page(*)",
            "search_web(*)"            
        ],
        "deny": [
            "command(rm -rf)",
            "command(sudo)"
        ],
        "ask": [
            "plan"
        ]
    }
}
EOF
fi

AGY="$(command -v agy || command -v antigravity || true)"
if [[ -n "$AGY" ]]; then
    # Only add when missing so a server the user has disabled stays disabled.
    if ! "$AGY" mcp list 2>/dev/null | grep -q '^codegraph[[:space:]]'; then
        "$AGY" mcp add codegraph codegraph serve --mcp
    fi
else
    echo "Antigravity CLI not found; skipping MCP server configuration." >&2
fi

# Wires codegraph into auto-detected agents. Auto-detection does not reliably
# cover the agy CLI, which is why it is registered explicitly above.
echo "Configuring CodeGraph for detected agents and indexing the repository..."
codegraph install --yes </dev/null
REPO_ROOT="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
codegraph init --yes "$REPO_ROOT" </dev/null
