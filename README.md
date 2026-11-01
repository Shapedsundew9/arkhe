# Shared Development

This repository contains shared development environment configuration and tools that should be kept synchronized across projects. For example, style guides, generic AI prompts, etc.

## Setup

Run the following command in the root of the repository you wish to add the shared development environment repository. The standard, safe command line to download and run the script in one step is:

```bash
curl -fsSL https://raw.githubusercontent.com/Shapedsundew9/shared-dev/main/tools/scripts/setup.sh | bash
```  

### Breakdown of Flags

- **-f (--fail)**: Fails silently on HTTP server errors (such as 404 Not Found or 500) so curl will never pipe an error webpage into bash.
- **-s (--silent)**: Mutes the progress meter and progress output.
- **-S (--show-error)**: When used with -s, ensures error messages (like network failure or host unreachable) are still shown.
- **-L (--location)**: Follows HTTP 3xx redirects (standard for GitHub URLs).
- **| bash**: Streams the fetched script directly into a bash shell to execute.

### Notes for Users

1. Run this from anywhere inside the target project's Git repository (the script automatically identifies the top-level repository root).
2. The working tree must be clean (no uncommitted changes) before running.
3. The script contains the pull.rebase = merges safeguard, running this one-liner will also automatically configure the repo against rebase/sync issues.

### Why is `pull.rebase = merges` needed? (The "Pull-Before-Push" Trap)

When `git pull origin main` runs with `rebase = true` on unpushed commits standard git rebase drops all merge commits by default.

This issue isn't unique to the initial setup—it can happen whenever two conditions meet:

1. You have a local, unpushed subtree commit on your branch (such as right after running git subtree add or in the future when running `git subtree pull` to get updates).
2. Git runs a rebase pull against the remote before those commits have been pushed.

#### The VS Code "Synchronize" Behavior

In VS Code, clicking the Synchronize button executes:

1. `git pull` first
2. `git push` second

If there are unpushed subtree merges when git pull runs:

- With `pull.rebase = true`: Git runs plain git rebase, which silently deletes all merge commits. It strips the subtree merge, takes the squashed files, and dumps them into your repository root.
- With `pull.rebase = merges` (the modern setting): Git runs `git rebase --rebase-merges`, which keeps merge commits and folder mappings intact.
  
Once the commits are pushed to GitHub (origin/main), they are part of upstream history. Subsequent normal pulls or syncs won't rebase them anyway.
──────

### Why `pull.rebase = merges` is Best Practice

In Git versions prior to 2.18 (pre-2018), `pull.rebase` only supported true or false. Setting it to true was popular to avoid messy "Merge branch 'main' of ..." commits, but it had this known flaw: it destroyed intentional merges (like subtrees or feature branches).

Git 2.18 introduced:

```bash
git config pull.rebase merges
```

This gives you the best of both worlds:

- It keeps your regular linear commit history clean when pulling.
- It protects and preserves intentional merge commits (like your .shared subtree).

## Usage

To make best use of the configuration use symbolic links, e.g. `ln -s .shared/ai/core/prompts/* .github/prompts/`

### Syncing with the shared repository

`setup.sh` and `configure-subtree.sh` add repository-local git aliases:

```bash
git shared-pull   # git subtree pull --prefix=.shared shared-dev main --squash
git shared-push   # git subtree push --prefix=.shared shared-dev main
```

Commit changes before pushing; only committed history under `.shared/` is sent. In a fresh clone (for example a container volume), run `.shared/tools/scripts/configure-subtree.sh` (or call it from the dev container `postCreateCommand`) to restore the `shared-dev` remote, the aliases and `git subtree`.

### Shared dev container post-create

`devcontainer/post-create.sh` holds post-create steps common to all repositories: it installs [CodeGraph](https://github.com/colbymchenry/codegraph) and the Antigravity CLI (`agy`) if missing, seeds `~/.gemini/antigravity-cli/settings.json` if absent, registers the `codegraph` MCP server (`codegraph serve --mcp`) with `agy` if it is not already configured, wires CodeGraph into other auto-detected agents (`codegraph install --yes`, which may write files such as `~/.copilot/mcp-config.json` or `~/.claude.json`), and indexes the repository (`codegraph init --yes`, a no-op if `.codegraph/` already exists). Call it from the repository's own `.devcontainer/post-create.sh`:

```bash
bash .shared/devcontainer/post-create.sh
```

### Host environment variables in container volumes

When a repository is cloned into a container volume, Docker Compose cannot read the host environment, so `${VAR:-}` entries in `docker-compose.shared.yml` resolve to their defaults. To opt in to host values, run the following from the repository and then rebuild the dev container:

```bash
.shared/tools/scripts/sync_devcontainer_env.py            # add --dry-run to preview
```

This mirrors each compose entry into `remoteEnv` in `.devcontainer/devcontainer.json` as `${localEnv:VAR}`, which VS Code resolves on the host. Entries whose default contains `:` (such as URLs) are skipped because `localEnv` defaults cannot contain colons. Once a `remoteEnv` block exists, `git shared-pull` and `configure-subtree.sh` rerun the sync automatically; changes made during container creation apply on the next rebuild.
