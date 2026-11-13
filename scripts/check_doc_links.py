#!/usr/bin/env python3
"""Check that all relative links in README.md and doc/**/*.md point to existing files."""

from __future__ import annotations

import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
EXTERNAL_PREFIXES = ("http://", "https://", "mailto:", "file://", "conversation://", "#")


def iter_markdown_files() -> list[pathlib.Path]:
    files = [REPO_ROOT / "README.md"]
    doc_dir = REPO_ROOT / "doc"
    if doc_dir.is_dir():
        files.extend(sorted(doc_dir.rglob("*.md")))
    return [f for f in files if f.is_file()]


def strip_code(line: str) -> str:
    return re.sub(r"`[^`]*`", "", line)


def extract_target(raw_target: str) -> str:
    target = raw_target.strip()
    if target.startswith("<") and ">" in target:
        return target[1 : target.index(">")].strip()
    # Handle optional markdown link title: path "title"
    return target.split()[0] if target else ""


def check_file(md_path: pathlib.Path) -> list[str]:
    errors: list[str] = []
    in_fence = False
    fence_marker = ""
    text = md_path.read_text(encoding="utf-8")
    for lineno, raw_line in enumerate(text.splitlines(), start=1):
        stripped = raw_line.lstrip()
        if stripped.startswith(("```", "~~~")):
            marker = stripped[:3]
            if not in_fence:
                in_fence = True
                fence_marker = marker
            elif stripped.startswith(fence_marker):
                in_fence = False
                fence_marker = ""
            continue
        if in_fence:
            continue

        line = strip_code(raw_line)
        for match in LINK_RE.finditer(line):
            target = extract_target(match.group(1))
            if not target or target.startswith(EXTERNAL_PREFIXES):
                continue
            rel_part = target.split("#", 1)[0]
            if not rel_part:
                continue
            resolved = (md_path.parent / rel_part).resolve()
            if not resolved.exists():
                rel_md = md_path.relative_to(REPO_ROOT)
                errors.append(f"{rel_md}:{lineno}: broken link '{target}' -> {resolved}")
    return errors


def main() -> int:
    all_errors: list[str] = []
    files = iter_markdown_files()
    for md_path in files:
        all_errors.extend(check_file(md_path))
    if all_errors:
        for err in all_errors:
            print(err, file=sys.stderr)
        print(f"Found {len(all_errors)} broken relative link(s).", file=sys.stderr)
        return 1
    print(f"OK: verified relative links across {len(files)} markdown files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
