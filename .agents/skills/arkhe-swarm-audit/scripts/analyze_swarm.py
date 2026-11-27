#!/usr/bin/env python3
"""Milestone-agnostic forensic telemetry extractor for Antigravity swarm runs.

Reads local Antigravity swarm state (`members.json`, `board.jsonl`,
`messages/*.json`, and every member's `transcript.jsonl`) and produces a
reproducible quantitative efficiency report covering:
  1. Wall-clock breakdown (active execution vs mandatory 5h cooldown vs
     avoidable post-reset manual-restart dead wait)
  2. Per-agent and per-model exact token accounting (uncached input, cached
     input, cache hit %, output incl. thinking, peak context, compactions)
  3. Quota window burn & final-20% burn rates (%/min) + [quota]/429 timeline
  4. Zero-work waste quantification (no-op wakeups, dead-gap cron pileups,
     casual mid-body @mentions, oversized board posts)
  5. Mechanical friction (2-strike Rust test/compile loops, CodeGraph vs
     read_file ratio, top repeatedly-read files, edit errors, tool batching,
     duplicate cargo test runs, shutdown hygiene)
"""

from __future__ import annotations

import argparse
import glob
import json
import os
import re
import sys
from collections import Counter, defaultdict
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any

DEFAULT_BRAIN_DIR = "/home/vscode/.gemini/antigravity-cli/brain"
DEFAULT_REPO_DIR = "/workspaces/arkhe"

QUOTA_RE = re.compile(
    r"\[quota\]\s+model\s+(?P<model>\S+)\s+is low on quota\s+"
    r"\((?P<pct>\d+)%\s+remaining,\s+resets at\s+(?P<resets>[^)\s]+)\)",
    re.IGNORECASE,
)
ERR_429_RE = re.compile(
    r"RESOURCE_EXHAUSTED\s*\(code\s*429\).*?Resets in\s+(?P<resets_in>[0-9hms]+)",
    re.IGNORECASE | re.DOTALL,
)
TEST_FAIL_RE = re.compile(r"^test\s+(?P<name>\S+)\s+\.\.\.\s+FAILED\b", re.MULTILINE)
EXIT_CODE_RE = re.compile(r"Command exited with code\s+(?P<code>\d+)")
MENTION_RE = re.compile(r"(?<![A-Za-z0-9_.-])@([a-z0-9][a-z0-9_-]{0,23})\b")
RG_GREP_PREFIX_RE = re.compile(r'^\s*"?(?:rg|grep)\b')


def parse_iso(raw: str | None) -> datetime | None:
    if not raw:
        return None
    try:
        return datetime.fromisoformat(raw.strip().replace("Z", "+00:00"))
    except ValueError:
        return None


def parse_hms(raw: str) -> int:
    total = 0
    for val, unit in re.findall(r"(\d+)([hms])", raw.lower()):
        n = int(val)
        if unit == "h":
            total += n * 3600
        elif unit == "m":
            total += n * 60
        elif unit == "s":
            total += n
    return total


def fmt_hm(seconds: float) -> str:
    mins = max(0, int(round(seconds / 60.0)))
    return f"{mins // 60}h {mins % 60:02d}m"


def load_json(path: Path, default: Any) -> Any:
    if not path.exists():
        return default
    try:
        return json.loads(path.read_text(encoding="utf-8", errors="replace"))
    except (OSError, json.JSONDecodeError):
        return default


def load_jsonl(path: Path) -> list[dict[str, Any]]:
    if not path.exists():
        return []
    rows: list[dict[str, Any]] = []
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            rows.append(json.loads(line))
        except json.JSONDecodeError:
            continue
    return rows


def load_full_row_content(tpath: Path, step_idx: Any) -> str | None:
    full_path = tpath.with_name("transcript_full.jsonl")
    if not full_path.exists():
        return None
    try:
        with full_path.open("r", encoding="utf-8", errors="replace") as fh:
            for line in fh:
                if f'"step_index": {step_idx},' not in line and f'"step_index":{step_idx},' not in line:
                    continue
                try:
                    obj = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if obj.get("step_index") == step_idx:
                    return str(obj.get("content") or "")
    except OSError:
        return None
    return None


def list_swarms(brain_dir: Path) -> list[dict[str, Any]]:
    """Return metadata for all discovered swarm sessions sorted newest-first."""
    entries: list[dict[str, Any]] = []
    pattern = str(brain_dir / "*" / ".system_generated" / "swarm" / "members.json")
    for mp_str in glob.glob(pattern):
        mp = Path(mp_str)
        swarm_id = mp.parents[2].name
        doc = load_json(mp, {})
        members = doc.get("members") or {}
        board_path = mp.with_name("board.jsonl")
        board_rows = load_jsonl(board_path)
        coord_t = brain_dir / swarm_id / ".system_generated" / "logs" / "transcript.jsonl"
        coord_rows = load_jsonl(coord_t)
        first_ts = coord_rows[0].get("created_at") if coord_rows else "?"
        last_ts = coord_rows[-1].get("created_at") if coord_rows else "?"
        entries.append(
            {
                "swarm_id": swarm_id,
                "title": doc.get("title") or "(untitled)",
                "mtime": os.path.getmtime(mp),
                "members": len(members),
                "board_posts": len(board_rows),
                "start": first_ts,
                "end": last_ts,
            }
        )
    entries.sort(key=lambda e: e["mtime"], reverse=True)
    return entries


def find_midbody_mentions(text: str, member_names: set[str], author: str) -> tuple[list[str], list[str]]:
    """Return (line1_direct_mentions, casual_midbody_mentions)."""
    lines = text.splitlines()
    if not lines:
        return [], []
    line1 = lines[0].lstrip()
    idx = 0
    direct: list[str] = []
    while idx < len(line1):
        m = re.match(r"^@([a-z0-9][a-z0-9_-]{0,23})\b\s*", line1[idx:])
        if not m:
            break
        target = m.group(1)
        if target in member_names and target != author and target not in direct:
            direct.append(target)
        idx += m.end()
    remainder = line1[idx:] + ("\n" + "\n".join(lines[1:]) if len(lines) > 1 else "")
    casual: list[str] = []
    for match in MENTION_RE.finditer(remainder):
        target = match.group(1)
        if target in member_names and target != author and target not in casual:
            casual.append(target)
    return direct, casual


def analyze_swarm(brain_dir: Path, swarm_id: str, repo_dir: Path | None = None) -> dict[str, Any]:
    """Run full forensic telemetry analysis on a swarm session."""
    swarm_dir = brain_dir / swarm_id / ".system_generated" / "swarm"
    members_doc = load_json(swarm_dir / "members.json", {"title": "(unknown)", "members": {}})
    members: dict[str, dict[str, Any]] = members_doc.get("members") or {}
    board_rows = load_jsonl(swarm_dir / "board.jsonl")
    msg_dir = brain_dir / swarm_id / ".system_generated" / "messages"

    all_member_names = set(members.keys())

    # ------------------------------------------------------------------
    # 1. Board analysis (sizes, oversized posts, casual mid-body mentions)
    # ------------------------------------------------------------------
    board_by_author: dict[str, dict[str, int]] = defaultdict(lambda: {"posts": 0, "bytes": 0})
    oversized_posts: list[dict[str, Any]] = []
    direct_mentions: Counter[str] = Counter()
    casual_mentions: Counter[str] = Counter()
    board_total_bytes = 0

    for row in board_rows:
        author = str(row.get("agent") or "?")
        text = str(row.get("text") or "")
        seq = int(row.get("seq") or 0)
        blen = len(text.encode("utf-8"))
        board_total_bytes += blen
        board_by_author[author]["posts"] += 1
        board_by_author[author]["bytes"] += blen
        if blen > 3072:
            oversized_posts.append(
                {"seq": seq, "agent": author, "bytes": blen, "preview": text.splitlines()[0][:80] if text else ""}
            )
        d_m, c_m = find_midbody_mentions(text, all_member_names, author)
        for t in d_m:
            direct_mentions[t] += 1
        for t in c_m:
            casual_mentions[t] += 1
    oversized_posts.sort(key=lambda x: -x["bytes"])

    # ------------------------------------------------------------------
    # 2. Coordinator 429 error messages
    # ------------------------------------------------------------------
    err_429_events: list[dict[str, Any]] = []
    if msg_dir.exists():
        for mp in sorted(msg_dir.glob("*.json")):
            if mp.name == "read.json":
                continue
            m = load_json(mp, {})
            content = str(m.get("content") or "")
            hit = ERR_429_RE.search(content)
            if hit:
                ts_dt = parse_iso(m.get("timestamp"))
                resets_s = parse_hms(hit.group("resets_in"))
                reset_dt = (ts_dt + timedelta(seconds=resets_s)) if ts_dt else None
                err_429_events.append(
                    {
                        "timestamp": m.get("timestamp"),
                        "title": (m.get("renderDetails") or {}).get("messageTitle", ""),
                        "resets_in": hit.group("resets_in"),
                        "resets_in_secs": resets_s,
                        "reset_utc": reset_dt.isoformat().replace("+00:00", "Z") if reset_dt else None,
                    }
                )
    err_429_events.sort(key=lambda e: str(e.get("timestamp") or ""))

    # ------------------------------------------------------------------
    # 3. Per-member transcript deep scan
    # ------------------------------------------------------------------
    agent_stats: list[dict[str, Any]] = []
    model_totals: dict[str, dict[str, int]] = defaultdict(
        lambda: {"calls": 0, "unc_in": 0, "cache": 0, "out": 0}
    )
    compaction_totals = {"count": 0, "unc_in": 0, "cache": 0, "out": 0}
    all_planner_events: list[tuple[datetime, str, str, int, int, int]] = []
    quota_warnings: list[dict[str, Any]] = []
    grind_events: list[dict[str, Any]] = []
    global_file_reads: Counter[str] = Counter()
    user_restarts: list[dict[str, Any]] = []
    still_running_after_done: list[str] = []

    board_has_done = any(
        str(r.get("agent")) == "coordinator" and "[done]" in str(r.get("text") or "").lower()
        for r in board_rows[-5:]
    )

    for name, info in sorted(members.items()):
        cid = str(info.get("conversation_id") or "")
        if not cid:
            continue
        if board_has_done and name != "coordinator" and not info.get("killed") and info.get("phase") != "KILLED":
            still_running_after_done.append(name)

        tpath = brain_dir / cid / ".system_generated" / "logs" / "transcript.jsonl"
        rows = load_jsonl(tpath)
        if not rows:
            continue

        calls = 0
        unc_in = 0
        cache = 0
        out = 0
        peak_ctx = 0
        compactions = 0
        models_used: Counter[str] = Counter()
        reads = 0
        edits = 0
        edit_errs = 0
        cmds = 0
        cargo_tests = 0
        cg_mcp = 0
        cg_cli = 0
        rg_cmds = 0
        single_tool = 0
        multi_tool = 0
        inline_script_bytes = 0
        max_queued_sys_run = 0
        cur_sys_run = 0
        file_reads: Counter[str] = Counter()

        # Wakeup segmentation for no-op waste calculation
        wakes: list[dict[str, Any]] = []
        cur_wake: dict[str, Any] = {"calls": 0, "unc": 0, "cache": 0, "out": 0, "tools": []}

        # 2-strike Rust failure tracking
        saw_edit = False
        test_streaks: dict[str, int] = {}
        last_cmd_str = ""

        timestamps: list[datetime] = []

        for r in rows:
            rtype = str(r.get("type") or "")
            content = str(r.get("content") or "")
            ts_str = r.get("created_at")
            ts_dt = parse_iso(ts_str)
            step_idx = r.get("step_index")
            if ts_dt:
                timestamps.append(ts_dt)

            if rtype == "USER_INPUT" and name == "coordinator":
                if step_idx and int(step_idx) > 0:
                    user_restarts.append(
                        {"step": step_idx, "ts": ts_str, "preview": content.replace("\n", " ")[:120]}
                    )

            if rtype == "SYSTEM_MESSAGE":
                cur_sys_run += 1
                max_queued_sys_run = max(max_queued_sys_run, cur_sys_run)
                if "[quota]" in content:
                    qm = QUOTA_RE.search(content)
                    if qm and ts_dt:
                        quota_warnings.append(
                            {
                                "ts": ts_str,
                                "dt": ts_dt,
                                "agent": name,
                                "step": step_idx,
                                "model": qm.group("model"),
                                "pct": int(qm.group("pct")),
                                "resets": qm.group("resets"),
                            }
                        )
            else:
                cur_sys_run = 0

            if rtype == "PLANNER_RESPONSE":
                calls += 1
                mname = str(r.get("model") or info.get("applied_model") or "unknown")
                models_used[mname] += 1
                u = int(r.get("input_tokens") or 0)
                c = int(r.get("cache_read_tokens") or 0)
                o = int(r.get("output_tokens") or 0)
                unc_in += u
                cache += c
                out += o
                peak_ctx = max(peak_ctx, u + c)
                model_totals[mname]["calls"] += 1
                model_totals[mname]["unc_in"] += u
                model_totals[mname]["cache"] += c
                model_totals[mname]["out"] += o
                if ts_dt:
                    all_planner_events.append((ts_dt, name, mname, u, c, o))

                cur_wake["calls"] += 1
                cur_wake["unc"] += u
                cur_wake["cache"] += c
                cur_wake["out"] += o

                tcs = r.get("tool_calls") or []
                if len(tcs) == 1:
                    single_tool += 1
                elif len(tcs) > 1:
                    multi_tool += 1

                for tc in tcs:
                    tname = str(tc.get("name") or "")
                    cur_wake["tools"].append(tname)
                    args = tc.get("arguments") or tc.get("args") or {}
                    if isinstance(args, str):
                        try:
                            args = json.loads(args)
                        except json.JSONDecodeError:
                            args = {"raw": args}
                    if tname == "read_file":
                        reads += 1
                        p = str(args.get("Path") or args.get("path") or "").strip('"')
                        if repo_dir:
                            prefix = str(repo_dir).rstrip("/") + "/"
                            if p.startswith(prefix):
                                p = p[len(prefix) :]
                        if p:
                            file_reads[p] += 1
                            global_file_reads[p] += 1
                    elif tname in ("edit_file", "write_file"):
                        edits += 1
                        saw_edit = True
                    elif tname == "call_mcp_tool":
                        if "codegraph" in str(args).lower():
                            cg_mcp += 1
                    elif tname == "run_command":
                        cmds += 1
                        cmd_s = str(args.get("CommandLine") or args.get("command") or "")
                        last_cmd_str = cmd_s
                        if "codegraph" in cmd_s:
                            cg_cli += 1
                        if "cargo test" in cmd_s and "ps -eo" not in cmd_s and "swarm_watchdog" not in cmd_s:
                            cargo_tests += 1
                        if RG_GREP_PREFIX_RE.match(cmd_s):
                            rg_cmds += 1
                        if "<<" in cmd_s and "python" in cmd_s:
                            inline_script_bytes += len(cmd_s)
                    elif tname == "post_message":
                        if "[escalation]" in str(args).lower() or "[done]" in str(args).lower():
                            saw_edit = False
                            test_streaks = {}

                if "swarm_wait" in [str(tc.get("name") or "") for tc in tcs]:
                    wakes.append(cur_wake)
                    cur_wake = {"calls": 0, "unc": 0, "cache": 0, "out": 0, "tools": []}

            elif rtype == "CHECKPOINT":
                compactions += 1
                compaction_totals["count"] += 1
                compaction_totals["unc_in"] += int(r.get("input_tokens") or 0)
                compaction_totals["cache"] += int(r.get("cache_read_tokens") or 0)
                compaction_totals["out"] += int(r.get("output_tokens") or 0)

            elif rtype in ("GENERIC", "SYSTEM_MESSAGE"):
                if r.get("status") == "ERROR" and "target content not found" in content:
                    edit_errs += 1
                if name not in ("coordinator", "monitor", "chief-scientist"):
                    if rtype == "SYSTEM_MESSAGE" and "finished with result:" not in content and "Command exited with code" not in content:
                        continue
                    exit_m = EXIT_CODE_RE.search(content)
                    exit_code = int(exit_m.group("code")) if exit_m else 0
                    scan_c = content
                    if exit_code == 101 and "content" in (r.get("truncated_fields") or []):
                        full_c = load_full_row_content(tpath, step_idx)
                        if full_c:
                            scan_c = full_c
                    failed_tests = TEST_FAIL_RE.findall(scan_c)
                    if failed_tests:
                        if saw_edit:
                            for tn in failed_tests[:5]:
                                test_streaks[tn] = test_streaks.get(tn, 0) + 1
                                if test_streaks[tn] == 2:
                                    grind_events.append(
                                        {"agent": name, "test": tn, "step": step_idx, "ts": ts_str, "streak": 2}
                                    )
                                elif test_streaks[tn] > 2:
                                    # update highest streak for this test
                                    for ge in reversed(grind_events):
                                        if ge["agent"] == name and ge["test"] == tn:
                                            ge["streak"] = max(ge["streak"], test_streaks[tn])
                                            break
                            saw_edit = False
                        else:
                            for tn in failed_tests[:5]:
                                test_streaks.setdefault(tn, 0)
                    elif exit_code == 0 and "test result: ok." in scan_c and "0 failed" in scan_c and "FAILED" not in scan_c:
                        test_streaks = {}

        if cur_wake["calls"]:
            wakes.append(cur_wake)

        # Classify no-op wakes (no post_message, no edits, and for verifier no run_command)
        noop_wakes = [
            w
            for w in wakes
            if "post_message" not in w["tools"]
            and "edit_file" not in w["tools"]
            and "write_file" not in w["tools"]
            and (not name.startswith("verifier") or "run_command" not in w["tools"])
        ]
        # Exclude single-turn implementation workers that didn't use swarm_wait repeatedly
        is_sleep_role = name in ("monitor", "chief-scientist") or name.startswith("verifier")
        noop_unc = sum(w["unc"] for w in noop_wakes) if is_sleep_role else 0
        noop_cnt = len(noop_wakes) if is_sleep_role else 0

        wall_s = (timestamps[-1] - timestamps[0]).total_seconds() if len(timestamps) > 1 else 0.0
        hit_pct = (100.0 * cache / (unc_in + cache)) if (unc_in + cache) else 0.0
        primary_model = models_used.most_common(1)[0][0] if models_used else str(info.get("applied_model") or "?")

        agent_stats.append(
            {
                "name": name,
                "cid": cid,
                "model": primary_model,
                "calls": calls,
                "compactions": compactions,
                "peak_ctx": peak_ctx,
                "unc_in": unc_in,
                "cache": cache,
                "hit_pct": round(hit_pct, 1),
                "out": out,
                "avg_out": round(out / max(1, calls)),
                "wall_s": round(wall_s),
                "start": timestamps[0].isoformat().replace("+00:00", "Z") if timestamps else None,
                "end": timestamps[-1].isoformat().replace("+00:00", "Z") if timestamps else None,
                "wakes": len(wakes) if is_sleep_role else 0,
                "noop_wakes": noop_cnt,
                "noop_unc": noop_unc,
                "reads": reads,
                "edits": edits,
                "edit_errs": edit_errs,
                "cmds": cmds,
                "cargo_tests": cargo_tests,
                "cg_mcp": cg_mcp,
                "cg_cli": cg_cli,
                "rg_cmds": rg_cmds,
                "single_tool": single_tool,
                "multi_tool": multi_tool,
                "inline_script_bytes": inline_script_bytes,
                "max_queued_sys_run": max_queued_sys_run,
                "top_files": file_reads.most_common(4),
            }
        )

    # ------------------------------------------------------------------
    # 4. Wall-clock segmentation (Active Windows vs Stalls vs Post-Reset Wait)
    # ------------------------------------------------------------------
    all_planner_events.sort(key=lambda e: e[0])
    non_mon_events = [e for e in all_planner_events if e[1] != "monitor"]
    stalls: list[dict[str, Any]] = []
    windows: list[dict[str, Any]] = []

    if non_mon_events:
        win_start = non_mon_events[0][0]
        prev_t = win_start
        for ev in non_mon_events[1:]:
            t = ev[0]
            gap_s = (t - prev_t).total_seconds()
            if gap_s >= 1800:  # >= 30 min gap in non-monitor LLM activity -> window boundary / stall
                # Find matching 429 or [quota] reset timestamp near prev_t
                matched_reset: datetime | None = None
                had_429 = False
                for e429 in err_429_events:
                    e_dt = parse_iso(e429["timestamp"])
                    if e_dt and abs((e_dt - prev_t).total_seconds()) <= 900:
                        had_429 = True
                        matched_reset = parse_iso(e429["reset_utc"])
                        break
                if matched_reset is None:
                    for qw in quota_warnings:
                        if abs((qw["dt"] - prev_t).total_seconds()) <= 900:
                            matched_reset = parse_iso(qw["resets"])
                            break

                if not (had_429 or matched_reset is not None or gap_s >= 3600):
                    prev_t = t
                    continue

                cooldown_s = max(0.0, (matched_reset - prev_t).total_seconds()) if matched_reset else 0.0
                post_reset_s = max(0.0, (t - matched_reset).total_seconds()) if matched_reset else 0.0
                windows.append(_summarize_window(all_planner_events, win_start, prev_t + timedelta(seconds=600)))
                stalls.append(
                    {
                        "start": prev_t.isoformat().replace("+00:00", "Z"),
                        "end": t.isoformat().replace("+00:00", "Z"),
                        "total_s": round(gap_s),
                        "had_429": had_429,
                        "reset_utc": matched_reset.isoformat().replace("+00:00", "Z") if matched_reset else None,
                        "cooldown_s": round(cooldown_s),
                        "post_reset_wait_s": round(post_reset_s),
                    }
                )
                win_start = t
            prev_t = t
        windows.append(_summarize_window(all_planner_events, win_start, non_mon_events[-1][0] + timedelta(seconds=600)))

    total_wall_s = (
        (non_mon_events[-1][0] - non_mon_events[0][0]).total_seconds() if len(non_mon_events) > 1 else 0.0
    )
    total_stall_s = sum(s["total_s"] for s in stalls)
    total_cooldown_s = sum(s["cooldown_s"] for s in stalls)
    total_post_reset_s = sum(s["post_reset_wait_s"] for s in stalls)
    total_active_s = max(0.0, total_wall_s - total_stall_s)

    # Final-20% burn rate per unique quota reset window
    burn_rates: list[dict[str, Any]] = []
    by_reset: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for qw in quota_warnings:
        by_reset[qw["resets"]].append(qw)
    for reset_iso, qlist in sorted(by_reset.items()):
        qlist.sort(key=lambda x: x["dt"])
        first_q = qlist[0]
        # Find earliest 429 or last warning for this reset window
        end_dt = qlist[-1]["dt"]
        end_pct = qlist[-1]["pct"]
        for e429 in err_429_events:
            e_dt = parse_iso(e429["timestamp"])
            r_dt = parse_iso(e429["reset_utc"])
            ref_dt = parse_iso(reset_iso)
            if e_dt and r_dt and ref_dt and abs((r_dt - ref_dt).total_seconds()) <= 120:
                if e_dt < end_dt or end_pct > 0:
                    end_dt = e_dt
                    end_pct = 0
                break
        mins = (end_dt - first_q["dt"]).total_seconds() / 60.0
        drop = first_q["pct"] - end_pct
        if mins > 0.2 and drop > 0:
            burn_rates.append(
                {
                    "reset_utc": reset_iso,
                    "start_ts": first_q["ts"],
                    "start_pct": first_q["pct"],
                    "end_ts": end_dt.isoformat().replace("+00:00", "Z"),
                    "end_pct": end_pct,
                    "mins": round(mins, 1),
                    "pct_per_min": round(drop / mins, 2),
                }
            )

    # File sizes for top repeatedly read files
    top_read_files: list[dict[str, Any]] = []
    for rel, cnt in global_file_reads.most_common(8):
        entry: dict[str, Any] = {"path": rel, "reads": cnt, "lines": None, "est_tokens": None}
        if repo_dir:
            fp = repo_dir / rel
            if fp.exists() and fp.is_file():
                try:
                    raw = fp.read_bytes()
                    entry["lines"] = raw.count(b"\n") + (1 if raw and not raw.endswith(b"\n") else 0)
                    entry["est_tokens"] = len(raw) // 4
                except OSError:
                    pass
        top_read_files.append(entry)

    # Sort agents by uncached input descending
    agent_stats.sort(key=lambda a: -a["unc_in"])
    tot_unc = sum(a["unc_in"] for a in agent_stats) + compaction_totals["unc_in"]
    tot_cache = sum(a["cache"] for a in agent_stats) + compaction_totals["cache"]
    tot_out = sum(a["out"] for a in agent_stats) + compaction_totals["out"]
    tot_calls = sum(a["calls"] for a in agent_stats) + compaction_totals["count"]
    total_noop_unc = sum(a["noop_unc"] for a in agent_stats)

    return {
        "swarm_id": swarm_id,
        "title": members_doc.get("title") or "(untitled)",
        "wall_clock": {
            "start": non_mon_events[0][0].isoformat().replace("+00:00", "Z") if non_mon_events else None,
            "end": non_mon_events[-1][0].isoformat().replace("+00:00", "Z") if non_mon_events else None,
            "total_s": round(total_wall_s),
            "active_s": round(total_active_s),
            "stall_s": round(total_stall_s),
            "cooldown_s": round(total_cooldown_s),
            "post_reset_wait_s": round(total_post_reset_s),
            "windows": windows,
            "stalls": stalls,
        },
        "totals": {
            "calls": tot_calls,
            "unc_in": tot_unc,
            "cache": tot_cache,
            "out": tot_out,
            "compactions": compaction_totals,
            "noop_unc_waste": total_noop_unc,
            "noop_unc_pct": round(100.0 * total_noop_unc / max(1, tot_unc), 1),
        },
        "models": dict(sorted(model_totals.items())),
        "agents": agent_stats,
        "quota": {
            "warnings_count": len(quota_warnings),
            "err_429_events": err_429_events,
            "burn_rates": burn_rates,
            "user_restarts": user_restarts,
        },
        "board": {
            "posts": len(board_rows),
            "total_bytes": board_total_bytes,
            "by_author": dict(sorted(board_by_author.items(), key=lambda kv: -kv[1]["bytes"])),
            "oversized_count": len(oversized_posts),
            "oversized_over_3kb": oversized_posts[:8],
            "direct_mentions": dict(direct_mentions),
            "casual_midbody_mentions": dict(casual_mentions),
        },
        "friction": {
            "grind_events": grind_events,
            "top_read_files": top_read_files,
            "still_running_after_done": still_running_after_done,
        },
    }


def _summarize_window(
    events: list[tuple[datetime, str, str, int, int, int]],
    start: datetime,
    end: datetime,
) -> dict[str, Any]:
    sub = [e for e in events if start <= e[0] <= end]
    return {
        "start": start.isoformat().replace("+00:00", "Z"),
        "end": end.isoformat().replace("+00:00", "Z"),
        "duration_s": round(max(0.0, (end - start).total_seconds() - 600)),
        "calls": len(sub),
        "unc_in": sum(e[3] for e in sub),
        "cache": sum(e[4] for e in sub),
        "out": sum(e[5] for e in sub),
    }


def render_markdown(rep: dict[str, Any]) -> str:
    wc = rep["wall_clock"]
    tot = rep["totals"]
    lines: list[str] = []
    lines.append(f"# Swarm Telemetry Report: {rep['title']} (`{rep['swarm_id'][:8]}`)")
    lines.append("")
    lines.append("## 1. Wall-Clock & Stall Breakdown")
    lines.append("")
    if wc["total_s"] > 0:
        act_p = 100.0 * wc["active_s"] / wc["total_s"]
        st_p = 100.0 * wc["stall_s"] / wc["total_s"]
        cd_p = 100.0 * wc["cooldown_s"] / wc["total_s"]
        pr_p = 100.0 * wc["post_reset_wait_s"] / wc["total_s"]
    else:
        act_p = st_p = cd_p = pr_p = 0.0
    lines.append(
        f"- **Span:** `{wc['start']}` -> `{wc['end']}` (**{fmt_hm(wc['total_s'])}** total)"
    )
    lines.append(
        f"- **Active:** `{fmt_hm(wc['active_s'])}` ({act_p:.1f}%) | **Stall:** `{fmt_hm(wc['stall_s'])}` ({st_p:.1f}%) "
        f"[Mandatory 5h cooldown: `{fmt_hm(wc['cooldown_s'])}` ({cd_p:.1f}%), **Avoidable post-reset wait: `{fmt_hm(wc['post_reset_wait_s'])}` ({pr_p:.1f}%)**]"
    )
    lines.append(
        f"- **Totals:** `{tot['calls']:,}` calls (`{tot['compactions']['count']}` compactions) | "
        f"Uncached In: `{tot['unc_in']:,}` | Cached In: `{tot['cache']:,}` | Output: `{tot['out']:,}`"
    )
    lines.append(
        f"- **Measured Pure No-Op Waste (sleep roles):** `{tot['noop_unc_waste']:,}` uncached tokens (**{tot['noop_unc_pct']:.1f}%** of swarm uncached input)"
    )
    if wc["stalls"]:
        lines.append("")
        lines.append("| Stall # | Start (UTC) | End (UTC) | Total | 429? | Reset (UTC) | Cooldown | Avoidable Post-Reset |")
        lines.append("| :--- | :--- | :--- | ---: | :--- | :--- | ---: | ---: |")
        for idx, st in enumerate(wc["stalls"], 1):
            lines.append(
                f"| {idx} | `{st['start']}` | `{st['end']}` | {fmt_hm(st['total_s'])} | "
                f"{'YES' if st['had_429'] else 'no'} | `{st['reset_utc'] or '?'}` | "
                f"{fmt_hm(st['cooldown_s'])} | **{fmt_hm(st['post_reset_wait_s'])}** |"
            )

    if rep["quota"]["burn_rates"]:
        lines.append("")
        lines.append("### Final-20% Quota Burn Rates")
        lines.append("")
        for br in rep["quota"]["burn_rates"]:
            lines.append(
                f"- Reset `{br['reset_utc']}`: `{br['start_pct']}%` -> `{br['end_pct']}%` in `{br['mins']}m` (**{br['pct_per_min']}%/min**)"
            )

    lines.append("")
    lines.append("## 2. Per-Agent Token, Context & Waste Table")
    lines.append("")
    lines.append(
        "| Agent | Model | Calls | CP | Peak Ctx | Uncached In | Cache Hit | Out (Avg) | No-Op Wakes (Unc) | CG / Reads / Edits(err) | Tests |"
    )
    lines.append(
        "| :--- | :--- | ---: | ---: | ---: | ---: | ---: | ---: | :--- | :--- | ---: |"
    )
    for a in rep["agents"]:
        noop_str = f"{a['noop_wakes']}/{a['wakes']} ({a['noop_unc']:,})" if a["wakes"] else "-"
        cg_str = f"{a['cg_mcp']}+{a['cg_cli']} / {a['reads']} / {a['edits']}({a['edit_errs']})"
        lines.append(
            f"| `{a['name']}` | `{a['model']}` | {a['calls']} | {a['compactions']} | {a['peak_ctx']:,} | "
            f"{a['unc_in']:,} | {a['hit_pct']:.1f}% | {a['out']:,} ({a['avg_out']}) | {noop_str} | {cg_str} | {a['cargo_tests']} |"
        )

    lines.append("")
    lines.append("## 3. Per-Model Totals")
    lines.append("")
    lines.append("| Model | Calls | Uncached In | Cached In | Cache Hit | Output |")
    lines.append("| :--- | ---: | ---: | ---: | ---: | ---: |")
    for m, d in rep["models"].items():
        hp = (100.0 * d["cache"] / (d["unc_in"] + d["cache"])) if (d["unc_in"] + d["cache"]) else 0.0
        lines.append(
            f"| `{m}` | {d['calls']} | {d['unc_in']:,} | {d['cache']:,} | {hp:.1f}% | {d['out']:,} |"
        )

    lines.append("")
    lines.append("## 4. Board Hygiene, Mid-Body `@mentions` & Operational Friction")
    lines.append("")
    bd = rep["board"]
    lines.append(
        f"- **Board volume:** `{bd['posts']}` posts, `{bd['total_bytes']:,}` bytes "
        f"(oversized `>3KB`: `{bd.get('oversized_count', len(bd['oversized_over_3kb']))}`)"
    )
    lines.append(f"- **Casual mid-body `@mentions`:** `{bd['casual_midbody_mentions']}`")
    lines.append(f"- **Direct line-1 `@mentions`:** `{bd['direct_mentions']}`")
    if rep["friction"]["still_running_after_done"]:
        lines.append(f"- **Unkilled after `[done]`:** `{rep['friction']['still_running_after_done']}`")
    if rep["friction"]["grind_events"]:
        lines.append("- **>=2-Strike Test Grinds Detected:**")
        for ge in rep["friction"]["grind_events"][:10]:
            lines.append(
                f"  - `{ge['agent']}` hit **{ge['streak']} strikes** on `{ge['test']}` (starting step `{ge['step']}` @ `{ge['ts']}`)"
            )
    if rep["friction"]["top_read_files"]:
        lines.append("- **Top Repeatedly-Read Files:**")
        for tf in rep["friction"]["top_read_files"]:
            sz = f", {tf['lines']} lines (~{tf['est_tokens']:,} tok)" if tf["lines"] is not None else ""
            lines.append(f"  - `{tf['path']}`: `{tf['reads']}` reads{sz}")

    mon = next((a for a in rep["agents"] if a["name"] == "monitor"), None)
    if mon and (mon["max_queued_sys_run"] >= 5 or mon["inline_script_bytes"] > 5000):
        lines.append(
            f"- **Watchdog diagnostics:** `max_queued_sys_run={mon['max_queued_sys_run']}`, "
            f"`inline_script_bytes={mon['inline_script_bytes']:,}`"
        )

    return "\n".join(lines)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Forensic telemetry analyzer for Antigravity swarm runs.")
    parser.add_argument("--brain-dir", default=DEFAULT_BRAIN_DIR, help="Antigravity brain root directory.")
    parser.add_argument("--repo", default=DEFAULT_REPO_DIR, help="Repository root for file-size lookup.")
    parser.add_argument("--list", action="store_true", help="List recent swarm sessions and exit.")
    parser.add_argument("--latest", action="store_true", help="Analyze the most recently modified swarm.")
    parser.add_argument("--swarm-id", help="Explicit coordinator conversation ID to analyze.")
    parser.add_argument("--json", action="store_true", help="Output raw JSON instead of Markdown.")
    args = parser.parse_args(argv)

    brain_dir = Path(args.brain_dir)
    if args.list:
        swarms = list_swarms(brain_dir)
        if not swarms:
            print("No swarm sessions found.")
            return 0
        print(f"{'SWARM_ID':36s} | {'POSTS':>5s} | {'MEM':>3s} | {'START':20s} | TITLE")
        for s in swarms[:15]:
            print(f"{s['swarm_id']:36s} | {s['board_posts']:5d} | {s['members']:3d} | {str(s['start'])[:20]:20s} | {s['title']}")
        return 0

    swarm_id = args.swarm_id
    if not swarm_id:
        swarms = list_swarms(brain_dir)
        if not swarms:
            print("ERROR: no swarm sessions found.", file=sys.stderr)
            return 2
        swarm_id = swarms[0]["swarm_id"]

    repo_dir = Path(args.repo) if args.repo else None
    report = analyze_swarm(brain_dir, swarm_id, repo_dir=repo_dir)
    if args.json:
        print(json.dumps(report, indent=2, sort_keys=True))
    else:
        print(render_markdown(report))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
