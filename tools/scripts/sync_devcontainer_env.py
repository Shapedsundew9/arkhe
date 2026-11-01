#!/usr/bin/env python3
"""Mirror host-sourced variables from the shared compose file into devcontainer.json remoteEnv.

Compose cannot see the host environment when a repository is cloned into a container
volume, but ${localEnv:...} in devcontainer.json is resolved on the host.
"""

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path

Token = tuple[str, int, int]

ENV_LINE = re.compile(
    r"""^\s+([A-Za-z_]\w*):\s*["']?\$\{([A-Za-z_]\w*)(?::?-([^}]*))?\}["']?\s*$"""
)


def read_compose_env(path: Path) -> tuple[dict[str, str], list[str]]:
    """Return remoteEnv entries built from compose `${VAR}` lines, and skipped names."""
    entries: dict[str, str] = {}
    skipped: list[str] = []
    for line in path.read_text(encoding="utf-8").splitlines():
        match = ENV_LINE.match(line)
        if not match:
            continue
        name, host_name, default = match.group(1), match.group(2), match.group(3) or ""
        # ${localEnv:VAR:default} splits on ':', so such defaults would be truncated.
        if ":" in default:
            skipped.append(name)
            continue
        suffix = f":{default}" if default else ""
        entries[name] = f"${{localEnv:{host_name}{suffix}}}"
    return entries, skipped


def tokens(text: str):
    """Yield (kind, start, end) for JSONC structural tokens, strings and comments."""
    i, n = 0, len(text)
    while i < n:
        char = text[i]
        if char == '"':
            j = i + 1
            while text[j] != '"':
                j += 2 if text[j] == "\\" else 1
            yield "str", i, j + 1
            i = j + 1
        elif text.startswith("//", i):
            end = text.find("\n", i)
            end = n if end == -1 else end
            yield "comment", i, end
            i = end
        elif text.startswith("/*", i):
            end = text.index("*/", i) + 2
            yield "comment", i, end
            i = end
        elif char in "{}[]:,":
            yield char, i, i + 1
            i += 1
        else:
            i += 1


def parse_object(text: str, toks: list[Token]) -> dict:
    """Parse a JSONC object span, ignoring comments and trailing commas."""
    cleaned = "".join(text[s:e] for kind, s, e in toks if kind != "comment")
    return json.loads(re.sub(r",(\s*[}\]])", r"\1", cleaned))


def render(env: dict[str, str], indent: str) -> str:
    """Render a top-level member object value."""
    body = ",\n".join(
        f"{indent * 2}{json.dumps(k)}: {json.dumps(v)}" for k, v in env.items()
    )
    return "{\n" + body + "\n" + indent + "}"


def locate_member(
    text: str, structural: list[Token], key: str
) -> tuple[int | None, int | None]:
    """Return the indexes of the top-level `key` value and the root closing brace."""
    depth = 0
    root_close = None
    value_index = None
    for k, (kind, start, end) in enumerate(structural):
        if kind in "{[":
            depth += 1
        elif kind in "}]":
            depth -= 1
            if depth == 0:
                root_close = k
        elif kind == "str" and depth == 1 and structural[k + 1][0] == ":":
            if json.loads(text[start:end]) == key:
                value_index = k + 2
    return value_index, root_close


def matching_close(structural: list[Token], open_index: int) -> int:
    """Return the index of the bracket closing the one at `open_index`."""
    depth = 0
    for index in range(open_index, len(structural)):
        kind = structural[index][0]
        depth += kind in "{["
        depth -= kind in "}]"
        if depth == 0:
            return index
    raise SystemExit("Error: unbalanced brackets in devcontainer.json.")


def update_devcontainer(
    text: str, managed: dict[str, str], key: str = "remoteEnv"
) -> str:
    """Return `text` with `managed` merged into the top-level `key` object."""
    toks = list(tokens(text))
    structural = [t for t in toks if t[0] != "comment"]
    indent = "\t" if "\n\t" in text else "    "
    value_index, root_close = locate_member(text, structural, key)

    if value_index is not None:
        if structural[value_index][0] != "{":
            raise SystemExit(f"Error: '{key}' in devcontainer.json is not an object.")
        start = structural[value_index][1]
        end = structural[matching_close(structural, value_index)][2]
        span = [t for t in toks if start <= t[1] and t[2] <= end]
        merged = parse_object(text, span)
        merged.update(managed)
        return text[:start] + render(merged, indent) + text[end:]

    if root_close is None:
        raise SystemExit("Error: devcontainer.json has no top-level object.")
    last = structural[root_close - 1]
    comma = "" if last[0] in "{," else ","
    insert = f'{comma}\n{indent}"{key}": {render(managed, indent)}'
    return text[: last[2]] + insert + text[last[2] :]


def repo_root() -> Path:
    """Return the git repository root, or the current directory outside git."""
    try:
        out = subprocess.run(
            ["git", "rev-parse", "--show-toplevel"],
            capture_output=True,
            text=True,
            check=True,
        )
        return Path(out.stdout.strip())
    except (OSError, subprocess.CalledProcessError):
        return Path.cwd()


def main() -> int:
    """Command-line entry point."""
    root = repo_root()
    parser = argparse.ArgumentParser(description=(__doc__ or "").partition("\n")[0])
    parser.add_argument(
        "--compose", type=Path, default=root / ".devcontainer/docker-compose.shared.yml"
    )
    parser.add_argument(
        "--devcontainer", type=Path, default=root / ".devcontainer/devcontainer.json"
    )
    parser.add_argument(
        "--dry-run", action="store_true", help="print the result instead of writing"
    )
    parser.add_argument(
        "--if-opted-in",
        action="store_true",
        help="do nothing unless devcontainer.json already has a remoteEnv object",
    )
    args = parser.parse_args()

    original = args.devcontainer.read_text(encoding="utf-8")
    if args.if_opted_in:
        structural = [t for t in tokens(original) if t[0] != "comment"]
        if locate_member(original, structural, "remoteEnv")[0] is None:
            return 0

    managed, skipped = read_compose_env(args.compose)
    if not managed:
        print(
            f"No ${{VAR}} environment entries found in {args.compose}.", file=sys.stderr
        )
        return 1

    updated = update_devcontainer(original, managed)

    for name in skipped:
        print(
            f"Skipped {name}: its default contains ':', so the compose default is kept."
        )
    if args.dry_run:
        print(updated)
    elif updated == original:
        print(f"{args.devcontainer} is already up to date.")
    else:
        args.devcontainer.write_text(updated, encoding="utf-8")
        print(f"Updated remoteEnv in {args.devcontainer} ({len(managed)} variables).")
        print("Rebuild the dev container to apply the change.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
