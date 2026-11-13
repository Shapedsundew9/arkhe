#!/usr/bin/env python3
"""Check that all relative links and heading anchors in README.md and doc/**/*.md resolve."""

from __future__ import annotations

import pathlib
import re
import sys

REPO_ROOT = pathlib.Path(__file__).resolve().parent.parent
LINK_RE = re.compile(r"!?\[[^\]]*\]\(([^)]+)\)")
HEADING_RE = re.compile(r"^(#{1,6})\s+(.*?)\s*#*\s*$")
EXTERNAL_PREFIXES = ("http://", "https://", "mailto:", "file://", "conversation://")


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


def github_heading_slug(raw_heading: str) -> str:
    """Convert a Markdown heading text to its GitHub-compatible anchor slug."""
    # Unwrap links: [label](url) -> label
    text = re.sub(r"!?\[([^\]]*)\]\([^)]*\)", r"\1", raw_heading)
    # Unwrap inline code spans while preserving underscores inside identifiers
    text = re.sub(r"`([^`]*)`", r"\1", text)
    # Remove emphasis markers (*, ~, and standalone/boundary _)
    text = re.sub(r"[*~]+", "", text)
    text = re.sub(r"(?<!\w)_+|_+(?!\w)", "", text)
    # Remove HTML tags
    text = re.sub(r"<[^>]+>", "", text)
    text = text.strip().lower()
    out: list[str] = []
    for ch in text:
        if ch.isalnum() or ch in (" ", "-", "_"):
            out.append(ch)
    return "".join(out).replace(" ", "-")


def collect_heading_anchors(md_path: pathlib.Path, cache: dict[pathlib.Path, set[str]]) -> set[str]:
    resolved = md_path.resolve()
    if resolved in cache:
        return cache[resolved]

    anchors: set[str] = set()
    counts: dict[str, int] = {}
    in_fence = False
    fence_marker = ""
    for raw_line in resolved.read_text(encoding="utf-8").splitlines():
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

        match = HEADING_RE.match(raw_line)
        if not match:
            continue
        base_slug = github_heading_slug(match.group(2))
        if not base_slug:
            continue
        seen = counts.get(base_slug, 0)
        counts[base_slug] = seen + 1
        slug = base_slug if seen == 0 else f"{base_slug}-{seen}"
        anchors.add(slug)

    cache[resolved] = anchors
    return anchors


def check_file(md_path: pathlib.Path, anchor_cache: dict[pathlib.Path, set[str]]) -> list[str]:
    errors: list[str] = []
    in_fence = False
    fence_marker = ""
    text = md_path.read_text(encoding="utf-8")
    rel_md = md_path.relative_to(REPO_ROOT)

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

            if "#" in target:
                rel_part, fragment = target.split("#", 1)
            else:
                rel_part, fragment = target, ""

            resolved = md_path.resolve() if not rel_part else (md_path.parent / rel_part).resolve()
            if not resolved.exists():
                errors.append(f"{rel_md}:{lineno}: broken link '{target}' -> {resolved}")
                continue

            if fragment and resolved.is_file() and resolved.suffix.lower() == ".md":
                valid_anchors = collect_heading_anchors(resolved, anchor_cache)
                if fragment not in valid_anchors:
                    errors.append(
                        f"{rel_md}:{lineno}: broken anchor '#{fragment}' in '{target}' "
                        f"(target: {resolved.relative_to(REPO_ROOT)})"
                    )
    return errors


def main() -> int:
    all_errors: list[str] = []
    anchor_cache: dict[pathlib.Path, set[str]] = {}
    files = iter_markdown_files()
    for md_path in files:
        all_errors.extend(check_file(md_path, anchor_cache))
    if all_errors:
        for err in all_errors:
            print(err, file=sys.stderr)
        print(f"Found {len(all_errors)} broken relative link(s)/anchor(s).", file=sys.stderr)
        return 1
    print(f"OK: verified relative links and anchors across {len(files)} markdown files.")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
