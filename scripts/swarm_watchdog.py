#!/usr/bin/env python3
"""Zero-token event-driven operational watchdog for Arkhe swarm runs (W1-W9).

Designed to run as a background `run_command` task owned by `@monitor`. While
the swarm is healthy, it polls local disk state (`members.json`, `board.jsonl`,
member `transcript.jsonl`, coordinator `messages/*.json`, `ps`, `git diff`)
every `--interval` seconds consuming ZERO LLM tokens and ZERO context.

The instant any W1-W9 trigger fires (most critically W8 pre-emptive quota pause
at <=15% remaining, W2 per-test 2-strike grind, or W9 context bloat), it prints
a compact <=15-line alert and exits with code 0 so Antigravity reactively wakes
`@monitor`.
"""

from __future__ import annotations

import argparse
import glob
import hashlib
import json
import os
import re
import subprocess
import sys
import time
from dataclasses import dataclass, field
from datetime import datetime, timezone
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

NON_HYDROGEN_LEAK_SUBSTRS = ("carbon", "neon", "orchestrator", "claude", "gpt")
RG_GREP_PREFIX_RE = re.compile(r'^\s*"?(?:rg|grep)\b')
PROTECTED_HOLDOUT_PATTERNS = (
    "allow_with_flag",
    "HOLDOUT_BANKS",
    "HoldoutRangeSpec",
)
GUARDRAIL_MASK_PATTERNS = (
    "SproutBlockedMaxNodes",
    "BlockedMaxNodes",
    "SproutBudgetExhausted",
    "CaseOutcome::Excluded",
)


def parse_iso_utc(raw: str | None) -> datetime | None:
    """Parse an ISO-8601 UTC timestamp string into an aware datetime."""
    if not raw:
        return None
    try:
        return datetime.fromisoformat(raw.strip().replace("Z", "+00:00"))
    except ValueError:
        return None


def parse_duration_hms(raw: str) -> int:
    """Parse Antigravity '2h44m28s' style duration strings into seconds."""
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


def discover_latest_swarm_id(brain_dir: Path) -> str | None:
    """Find the most recently modified swarm coordinator conversation ID."""
    candidates = glob.glob(str(brain_dir / "*" / ".system_generated" / "swarm" / "members.json"))
    if not candidates:
        return None
    latest = max(candidates, key=os.path.getmtime)
    return Path(latest).parents[2].name


def default_state_path(swarm_id: str) -> Path:
    return Path(f"/tmp/arkhe_watchdog_state_{swarm_id[:8]}.json")


def load_json_file(path: Path, default: Any) -> Any:
    if not path.exists():
        return default
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError):
        return default


def save_json_atomic(path: Path, data: Any) -> None:
    tmp = path.with_suffix(path.suffix + ".tmp")
    tmp.write_text(json.dumps(data, sort_keys=True), encoding="utf-8")
    os.replace(tmp, path)


@dataclass
class WatchdogConfig:
    brain_dir: Path
    repo_dir: Path
    swarm_id: str
    state_file: Path
    interval: int = 90
    quota_pause_pct: int = 15
    heartbeat_active_mins: int = 30
    heartbeat_uncached_tokens: int = 2_500_000
    worker_ctx_warn: int = 300_000
    verifier_ctx_warn: int = 150_000
    max_compactions: int = 1
    w6_short_secs: int = 900
    w6_long_secs: int = 1800
    max_output_lines: int = 15
    now_override: datetime | None = None

    def now(self) -> datetime:
        return self.now_override or datetime.now(timezone.utc)


@dataclass
class CheckResult:
    alerts: list[str] = field(default_factory=list)
    board_head: int = 0
    active_members: list[str] = field(default_factory=list)
    ps_summary: str = "none"
    swarm_done: bool = False
    quota_pause_triggered: bool = False
    deltas: int = 0


def init_state_dict(state: dict[str, Any]) -> dict[str, Any]:
    state.setdefault("lines", {})
    state.setdefault("last_board_seq", 0)
    state.setdefault("seen_msg_files", [])
    state.setdefault("worker_strikes", {})
    state.setdefault("alerted_pids", [])
    state.setdefault("alerted_keys", {})
    state.setdefault("last_diff_hash", "")
    state.setdefault("idle_ticks", 0)
    state.setdefault("last_quota", None)
    state.setdefault("last_heartbeat_ts", 0.0)
    state.setdefault("window_uncached_since_hb", 0)
    state.setdefault("active_since_hb", False)
    state.setdefault("quota_paused_reset", None)
    state.setdefault("authorized_tokens", [])
    state.setdefault("hygiene_last_seq", 0)
    return state


def run_shell(cmd: list[str], cwd: Path | None = None) -> str:
    try:
        proc = subprocess.run(
            cmd,
            cwd=str(cwd) if cwd else None,
            capture_output=True,
            text=True,
            timeout=15,
            check=False,
        )
        return proc.stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ""


def get_running_arkhe_procs() -> list[tuple[int, int, float, str]]:
    """Return list of (pid, etimes_s, pcpu, args) for active arkhe/cargo procs."""
    out = run_shell(["ps", "-eo", "pid=,etimes=,pcpu=,args="])
    procs: list[tuple[int, int, float, str]] = []
    for line in out.splitlines():
        line = line.strip()
        if not line:
            continue
        if "swarm_watchdog.py" in line:
            continue
        if not any(k in line for k in ("target/release/arkhe", "target/debug/arkhe", "cargo test", "check_retained_baselines.py")):
            continue
        parts = line.split(None, 3)
        if len(parts) < 4:
            continue
        try:
            pid = int(parts[0])
            etimes = int(parts[1])
            pcpu = float(parts[2])
            args = parts[3]
            procs.append((pid, etimes, pcpu, args))
        except ValueError:
            continue
    return procs


def find_midbody_mentions(text: str, live_names: set[str], author: str) -> list[str]:
    """Return any live member names mentioned outside line-1 leading address prefix."""
    lines = text.splitlines()
    if not lines:
        return []
    # Allow leading @mentions on line 1 (e.g., "@coordinator @phase04b [done] ...")
    line1 = lines[0].lstrip()
    idx = 0
    while idx < len(line1):
        m = re.match(r"^@[a-z0-9][a-z0-9_-]{0,23}\b\s*", line1[idx:])
        if not m:
            break
        idx += m.end()
    remainder = line1[idx:] + ("\n" + "\n".join(lines[1:]) if len(lines) > 1 else "")
    found: list[str] = []
    for match in MENTION_RE.finditer(remainder):
        target = match.group(1)
        if target in live_names and target != author and target not in found:
            found.append(target)
    return found


def load_full_row_content(tpath: Path, step_idx: Any) -> str | None:
    """Lazily read untruncated `content` for a single step from transcript_full.jsonl."""
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


def inspect_once(
    cfg: WatchdogConfig,
    state: dict[str, Any],
    *,
    init_only: bool = False,
    procs_override: list[tuple[int, int, float, str]] | None = None,
    diff_files_override: list[str] | None = None,
    diff_text_override: str | None = None,
) -> CheckResult:
    """Execute one deterministic W1-W9 inspection pass and mutate `state`."""
    init_state_dict(state)
    res = CheckResult()
    now_dt = cfg.now()
    now_ts = now_dt.timestamp()

    swarm_dir = cfg.brain_dir / cfg.swarm_id / ".system_generated" / "swarm"
    members_path = swarm_dir / "members.json"
    board_path = swarm_dir / "board.jsonl"
    msg_dir = cfg.brain_dir / cfg.swarm_id / ".system_generated" / "messages"

    members_doc = load_json_file(members_path, {"members": {}})
    members: dict[str, dict[str, Any]] = members_doc.get("members") or {}

    live_members: dict[str, dict[str, Any]] = {
        name: info
        for name, info in members.items()
        if not info.get("killed") and info.get("phase") != "KILLED"
    }
    res.active_members = sorted(live_members.keys())
    live_names = set(live_members.keys())

    # ------------------------------------------------------------------
    # 1. Read new board messages directly from board.jsonl (0 LLM tokens)
    # ------------------------------------------------------------------
    new_board_msgs: list[dict[str, Any]] = []
    last_seq = int(state.get("last_board_seq", 0))
    highest_seq = last_seq
    if board_path.exists():
        for raw_line in board_path.read_text(encoding="utf-8", errors="replace").splitlines():
            raw_line = raw_line.strip()
            if not raw_line:
                continue
            try:
                row = json.loads(raw_line)
            except json.JSONDecodeError:
                continue
            seq = int(row.get("seq", 0))
            if seq > highest_seq:
                highest_seq = seq
            if seq > last_seq:
                new_board_msgs.append(row)
    res.board_head = highest_seq
    state["last_board_seq"] = highest_seq

    auth_tokens: set[str] = set(state.get("authorized_tokens") or [])
    for msg in new_board_msgs:
        author = str(msg.get("agent") or "")
        text = str(msg.get("text") or "")
        seq = int(msg.get("seq", 0))
        low = text.lower()

        if author == "coordinator" and "[done]" in low and ("officially closed" in low or "milestone" in low and "closed" in low):
            res.swarm_done = True
        if author in ("coordinator", "chief-scientist"):
            for kw in ("golden_traces", "allow_with_flag", "holdout_banks", "seal ", "closeout", "tests/m4a"):
                if kw in low:
                    auth_tokens.add(kw.strip())
            if "[quota-pause]" in low:
                # Track that coordinator/monitor entered quota pause
                pass
            if "[quota-resume]" in low or "quota has reset" in low:
                state["quota_paused_reset"] = None

        if not init_only:
            # Reset worker strike counter if worker escalated or finished
            if "[escalation]" in low or "[done]" in low:
                if author in state["worker_strikes"]:
                    state["worker_strikes"][author] = {
                        "saw_edit": False,
                        "test_streaks": {},
                        "compile_streak": 0,
                    }

            # W1 hygiene: detect casual mid-body @mentions waking chief-scientist / verifier
            wake_sensitive = {n for n in live_names if n == "chief-scientist" or n.startswith("verifier")}
            casual = find_midbody_mentions(text, wake_sensitive, author)
            if casual and seq - int(state.get("hygiene_last_seq", 0)) >= 5:
                state["hygiene_last_seq"] = seq
                res.alerts.append(
                    f"[W1_MENTION] #{seq} ({author}) mid-body @{',@'.join(casual)} wakes sleeping agent; use plain role names outside line 1."
                )

            # W7: unverified prose-only S3/S4 or regression/bit-identity claims
            if author != "monitor":
                requests_gate = bool(re.search(r"\b(s3|s4)\b.*(sign-off|authorization|request)", low))
                claims_noreg = any(p in low for p in ("bit-identical", "no regressions", "no regression"))
                has_script = "check_retained_baselines.py" in low and "--require-clean" in low
                if requests_gate and not has_script:
                    res.alerts.append(
                        f"[W7_BASELINE] #{seq} ({author}) requests S3/S4 without check_retained_baselines.py --require-clean evidence."
                    )
                elif claims_noreg and not has_script and "golden_traces" not in low and author != "coordinator":
                    res.alerts.append(
                        f"[W7_PROSE] #{seq} ({author}) asserts regression/bit-identity without golden_traces or check_retained_baselines.py evidence."
                    )
                if re.search(r"excluded_cases\s*(?:=|:|>)\s*[1-9]", low):
                    res.alerts.append(
                        f"[W7_EXCLUDED] #{seq} ({author}) reports nonzero excluded_cases; must investigate before S3/S4."
                    )

    state["authorized_tokens"] = sorted(auth_tokens)

    # Check if all non-monitor workers are killed and coordinator is done
    non_mon_workers = [n for n, info in live_members.items() if n not in ("monitor", "coordinator")]
    if res.swarm_done or (not init_only and highest_seq > 0 and not non_mon_workers and res.swarm_done):
        res.alerts.append(f"[SWARM_DONE] Milestone closed at board #{highest_seq}; safe to stop watcher and sleep.")

    # ------------------------------------------------------------------
    # 2. W1 Model Leak & Wake-Mode Audit (from members.json)
    # ------------------------------------------------------------------
    for name, info in sorted(live_members.items()):
        model = str(info.get("applied_model") or info.get("model") or "")
        wake_mode = str(info.get("wake_mode") or "")
        if name not in ("coordinator", "chief-scientist", "monitor"):
            if model and not model.startswith("gemini-4-hydrogen-"):
                key = f"w1_model:{name}:{model}"
                if not state["alerted_keys"].get(key):
                    state["alerted_keys"][key] = True
                    res.alerts.append(
                        f"[W1_MODEL] Worker {name} runs on {model} instead of gemini-4-hydrogen-*. Kill and respawn without fork."
                    )
        elif name == "chief-scientist":
            if model and not model.startswith("gemini-4-carbon-"):
                key = f"w1_cs_model:{name}:{model}"
                if not state["alerted_keys"].get(key):
                    state["alerted_keys"][key] = True
                    res.alerts.append(
                        f"[W1_MODEL] chief-scientist runs on {model}; must use gemini-4-carbon-high."
                    )
        if name == "chief-scientist" or name.startswith("verifier"):
            if wake_mode == "all":
                key = f"w1_wake:{name}:{highest_seq}"
                if not state["alerted_keys"].get(key):
                    state["alerted_keys"][key] = True
                    res.alerts.append(
                        f"[W1_WAKE] {name} has wake_mode='all' (must use mentions_only: true)."
                    )

    # ------------------------------------------------------------------
    # 3. Coordinator messages/*.json check for 429 RESOURCE_EXHAUSTED
    # ------------------------------------------------------------------
    seen_msgs: set[str] = set(state.get("seen_msg_files") or [])
    if msg_dir.exists():
        for mp in sorted(msg_dir.glob("*.json")):
            if mp.name == "read.json" or mp.name in seen_msgs:
                continue
            seen_msgs.add(mp.name)
            if init_only:
                continue
            raw_msg = load_json_file(mp, {})
            content = str(raw_msg.get("content") or "")
            m429 = ERR_429_RE.search(content)
            if m429:
                wait_s = parse_duration_hms(m429.group("resets_in"))
                res.quota_pause_triggered = True
                res.alerts.append(
                    f"[W8_429] Worker hit 429 RESOURCE_EXHAUSTED (resets in {m429.group('resets_in')} = {wait_s}s). Pause swarm & arm resume timer +{wait_s + 180}s."
                )
    state["seen_msg_files"] = sorted(seen_msgs)[-200:]

    # ------------------------------------------------------------------
    # 4. Scan incremental transcript.jsonl for every live member
    #    (W8 [quota], W2 2-strike per-test streaks, W9 context bloat)
    # ------------------------------------------------------------------
    latest_quota: dict[str, Any] | None = state.get("last_quota")
    for name, info in sorted(live_members.items()):
        cid = str(info.get("conversation_id") or "")
        if not cid:
            continue
        tpath = cfg.brain_dir / cid / ".system_generated" / "logs" / "transcript.jsonl"
        if not tpath.exists():
            continue
        raw_lines = tpath.read_text(encoding="utf-8", errors="replace").splitlines()
        old_n = int(state["lines"].get(cid, 0))
        if init_only:
            state["lines"][cid] = len(raw_lines)
            # Still inspect recent tail (last 10 rows) on init for immediate [quota] warnings
            slice_rows = raw_lines[-10:]
        else:
            if len(raw_lines) <= old_n:
                continue
            res.deltas += 1
            if name != "monitor":
                state["active_since_hb"] = True
            slice_rows = raw_lines[old_n:]
            state["lines"][cid] = len(raw_lines)

        ws = state["worker_strikes"].setdefault(
            name, {"saw_edit": False, "test_streaks": {}, "compile_streak": 0, "compactions": 0}
        )
        ws.setdefault("compactions", 0)
        last_cmd_str = ""
        peak_ctx = 0

        for idx_offset, raw_row in enumerate(slice_rows):
            try:
                obj = json.loads(raw_row)
            except json.JSONDecodeError:
                continue
            rtype = str(obj.get("type") or "")
            content = str(obj.get("content") or "")
            created_at = str(obj.get("created_at") or "")
            step_idx = obj.get("step_index", old_n + idx_offset)

            # W8: scan ANY member's SYSTEM_MESSAGE for Antigravity [quota] warnings
            if rtype == "SYSTEM_MESSAGE" and "[quota]" in content:
                qm = QUOTA_RE.search(content)
                if qm:
                    pct = int(qm.group("pct"))
                    resets_iso = qm.group("resets")
                    reset_dt = parse_iso_utc(resets_iso)
                    if reset_dt and reset_dt > now_dt:
                        cand = {
                            "pct": pct,
                            "resets": resets_iso,
                            "ts": created_at or now_dt.isoformat(),
                            "source": name,
                            "model": qm.group("model"),
                        }
                        if (
                            latest_quota is None
                            or cand["resets"] > str(latest_quota.get("resets") or "")
                            or (
                                cand["resets"] == latest_quota.get("resets")
                                and cand["ts"] >= str(latest_quota.get("ts") or "")
                            )
                        ):
                            latest_quota = cand

            if init_only:
                continue

            # Track token burn & peak context for W8 heartbeat & W9 bloat
            if rtype == "PLANNER_RESPONSE":
                unc = int(obj.get("input_tokens") or 0)
                cr = int(obj.get("cache_read_tokens") or 0)
                tot_ctx = unc + cr
                peak_ctx = max(peak_ctx, tot_ctx)
                if name != "monitor":
                    state["window_uncached_since_hb"] = int(state.get("window_uncached_since_hb", 0)) + unc

                for tc in obj.get("tool_calls") or []:
                    tname = str(tc.get("name") or "")
                    if tname in ("edit_file", "write_file"):
                        ws["saw_edit"] = True
                    elif tname == "run_command":
                        args = tc.get("arguments") or tc.get("args") or {}
                        if isinstance(args, str):
                            try:
                                args = json.loads(args)
                            except json.JSONDecodeError:
                                args = {"raw": args}
                        last_cmd_str = str(args.get("CommandLine") or args.get("command") or "")
                    elif tname == "post_message":
                        args = str(tc.get("arguments") or tc.get("args") or "").lower()
                        if "[escalation]" in args or "[done]" in args:
                            ws["saw_edit"] = False
                            ws["test_streaks"] = {}
                            ws["compile_streak"] = 0

            elif rtype == "CHECKPOINT":
                ws["compactions"] = int(ws.get("compactions", 0)) + 1
                if name not in ("coordinator", "monitor", "chief-scientist"):
                    ckey = f"w9_cp:{cid}:{ws['compactions']}"
                    if ws["compactions"] >= cfg.max_compactions and not state["alerted_keys"].get(ckey):
                        state["alerted_keys"][ckey] = True
                        res.alerts.append(
                            f"[W9_COMPACT] {name} hit compaction #{ws['compactions']} (step {step_idx}); land checkpoint commit & respawn fresh."
                        )

            elif rtype in ("GENERIC", "SYSTEM_MESSAGE") and name not in ("coordinator", "monitor", "chief-scientist"):
                # W2: inspect both synchronous tool results (GENERIC) and async task completions (SYSTEM_MESSAGE)
                if rtype == "SYSTEM_MESSAGE" and "finished with result:" not in content and "Command exited with code" not in content:
                    continue
                exit_m = EXIT_CODE_RE.search(content)
                exit_code = int(exit_m.group("code")) if exit_m else 0
                is_rg = bool(RG_GREP_PREFIX_RE.match(last_cmd_str))

                # Recover untruncated tail for long failing cargo test outputs
                scan_content = content
                if exit_code == 101 and "content" in (obj.get("truncated_fields") or []):
                    full_c = load_full_row_content(tpath, step_idx)
                    if full_c:
                        scan_content = full_c

                failed_tests = TEST_FAIL_RE.findall(scan_content)
                if failed_tests:
                    if ws.get("saw_edit"):
                        for tname in failed_tests[:5]:
                            streak = int(ws["test_streaks"].get(tname, 0)) + 1
                            ws["test_streaks"][tname] = streak
                            if streak >= 2:
                                akey = f"w2_test:{cid}:{tname}:{streak}"
                                if not state["alerted_keys"].get(akey):
                                    state["alerted_keys"][akey] = True
                                    res.alerts.append(
                                        f"[W2_GRIND] {name} reached {streak} edit+test failures on `{tname}` (step {step_idx}); halt & require escalation."
                                    )
                        ws["saw_edit"] = False
                    else:
                        # Initial baseline failure before first fix attempt: initialize at 0 so 2 fix attempts trigger
                        for tname in failed_tests[:5]:
                            ws["test_streaks"].setdefault(tname, 0)
                elif exit_code == 0 and "test result: ok." in scan_content and "0 failed" in scan_content and "FAILED" not in scan_content:
                    ws["test_streaks"] = {}
                    ws["compile_streak"] = 0
                elif exit_code == 101 and ("error[E" in scan_content or "could not compile" in scan_content) and not is_rg:
                    if ws.get("saw_edit"):
                        ws["compile_streak"] = int(ws.get("compile_streak", 0)) + 1
                        ws["saw_edit"] = False
                        if ws["compile_streak"] >= 3:
                            akey = f"w2_compile:{cid}:{ws['compile_streak']}"
                            if not state["alerted_keys"].get(akey):
                                state["alerted_keys"][akey] = True
                                res.alerts.append(
                                    f"[W2_COMPILE] {name} hit {ws['compile_streak']} consecutive compile-fail edit cycles (step {step_idx}); halt & escalate."
                                )
                elif exit_code == 0 and "Command exited with code 0" in scan_content:
                    ws["compile_streak"] = 0

        # W9: context-ceiling check for implementation workers and verifiers
        if not init_only and peak_ctx > 0 and name not in ("coordinator", "monitor", "chief-scientist"):
            threshold = cfg.verifier_ctx_warn if name.startswith("verifier") else cfg.worker_ctx_warn
            if peak_ctx >= threshold:
                ckey = f"w9_ctx:{cid}:{threshold}"
                if not state["alerted_keys"].get(ckey):
                    state["alerted_keys"][ckey] = True
                    res.alerts.append(
                        f"[W9_CTX] {name} reached {peak_ctx:,} tokens (limit {threshold:,}); finish atomic step & respawn fresh."
                    )

    state["last_quota"] = latest_quota

    # ------------------------------------------------------------------
    # 5. Evaluate W8 Pre-Emptive Quota Pause & Active-Work Heartbeat
    # ------------------------------------------------------------------
    if latest_quota:
        q_pct = int(latest_quota.get("pct", 100))
        q_resets = str(latest_quota.get("resets") or "")
        q_reset_dt = parse_iso_utc(q_resets)
        if q_reset_dt and q_reset_dt > now_dt:
            if q_pct <= cfg.quota_pause_pct:
                paused_info = state.get("quota_paused_reset")
                should_alert = (
                    paused_info is None
                    or paused_info.get("resets") != q_resets
                    or q_pct <= int(paused_info.get("pct", 100)) - 4
                )
                if should_alert:
                    wait_secs = max(60, int((q_reset_dt - now_dt).total_seconds()) + 180)
                    state["quota_paused_reset"] = {"resets": q_resets, "pct": q_pct}
                    res.quota_pause_triggered = True
                    res.alerts.insert(
                        0,
                        f"[W8_PAUSE] Gemini quota at {q_pct}% (src={latest_quota.get('source')}, resets {q_resets}, wait={wait_secs}s). Verify swarm models, post [quota-pause], STOP watcher, arm coordinator resume timer.",
                    )
        elif q_reset_dt and q_reset_dt <= now_dt:
            # Stale previous-window quota warning expired
            state["last_quota"] = None
            state["quota_paused_reset"] = None

    if not init_only and not res.quota_pause_triggered and not res.swarm_done:
        last_hb = float(state.get("last_heartbeat_ts", 0.0))
        if last_hb == 0.0:
            state["last_heartbeat_ts"] = now_ts
        elif state.get("active_since_hb"):
            elapsed_mins = (now_ts - last_hb) / 60.0
            unc_since_hb = int(state.get("window_uncached_since_hb", 0))
            if elapsed_mins >= cfg.heartbeat_active_mins or unc_since_hb >= cfg.heartbeat_uncached_tokens:
                state["last_heartbeat_ts"] = now_ts
                state["window_uncached_since_hb"] = 0
                state["active_since_hb"] = False
                res.alerts.append(
                    f"[W8_HEARTBEAT] Active work ({elapsed_mins:.0f}m, +{unc_since_hb:,} unc tok). Run swarm{{action:'models'}} and --record-quota."
                )

    # ------------------------------------------------------------------
    # 6. W3 / W4 Git Diff Audit (only when diff hash changes)
    # ------------------------------------------------------------------
    if diff_files_override is not None:
        diff_files = diff_files_override
    else:
        raw_df = run_shell(["git", "-C", str(cfg.repo_dir), "diff", "--name-only"])
        diff_files = [ln.strip() for ln in raw_df.splitlines() if ln.strip()]

    if diff_text_override is not None:
        diff_text = diff_text_override
    elif diff_files and not init_only:
        diff_text = run_shell(["git", "-C", str(cfg.repo_dir), "diff", "-U0"])
    else:
        diff_text = ""

    diff_hash = hashlib.sha256(("\n".join(diff_files) + "\n" + diff_text).encode("utf-8")).hexdigest()[:16]
    if not init_only and diff_files and diff_hash != state.get("last_diff_hash"):
        state["last_diff_hash"] = diff_hash
        if "tests/golden_traces.rs" in diff_files and "golden_traces" not in auth_tokens:
            res.alerts.append(
                "[W3_GOLDEN] Unauthorized edit to tests/golden_traces.rs without coordinator/Chief Scientist approval."
            )
        added_lines = [ln[1:] for ln in diff_text.splitlines() if ln.startswith("+") and not ln.startswith("+++")]
        removed_lines = [ln[1:] for ln in diff_text.splitlines() if ln.startswith("-") and not ln.startswith("---")]
        added_blob = "\n".join(added_lines)
        removed_blob = "\n".join(removed_lines)
        if (
            any(p in added_blob or p in removed_blob for p in PROTECTED_HOLDOUT_PATTERNS)
            and "allow_with_flag" not in auth_tokens
            and "seal" not in auth_tokens
            and "closeout" not in auth_tokens
        ):
            res.alerts.append(
                "[W3_HOLDOUT] Diff touches holdout guards (allow_with_flag/HOLDOUT_BANKS) without closeout/seal authorization."
            )
        if any(p in removed_blob for p in GUARDRAIL_MASK_PATTERNS) and not any(
            p in added_blob for p in GUARDRAIL_MASK_PATTERNS
        ):
            res.alerts.append(
                "[W4_GUARDRAIL] Diff removes guardrail counter reference (BlockedMaxNodes/Excluded) without replacement; inspect immediately."
            )

    # ------------------------------------------------------------------
    # 7. W6 Runaway Process Check & W5 Swarm Stall Check
    # ------------------------------------------------------------------
    procs = procs_override if procs_override is not None else get_running_arkhe_procs()
    if procs:
        res.ps_summary = "; ".join(f"pid={p}({et}s,{cpu:.0f}%):{a[:45]}" for p, et, cpu, a in procs[:3])
    else:
        res.ps_summary = "none"

    alerted_pids: set[int] = set(state.get("alerted_pids") or [])
    if not init_only:
        for pid, etimes, _pcpu, args in procs:
            is_large_sweep = "--banks 50" in args or "1000" in args or "check_retained_baselines" in args
            limit = cfg.w6_long_secs if is_large_sweep else cfg.w6_short_secs
            if etimes > limit and pid not in alerted_pids:
                alerted_pids.add(pid)
                res.alerts.append(
                    f"[W6_RUNAWAY] PID {pid} running {etimes}s (>{limit}s): {args[:70]}. Recommend extrapolation check / kill."
                )
    state["alerted_pids"] = sorted(alerted_pids)[-50:]

    # W5: all non-monitor live members waiting/stopped, PS=none, not quota-paused, not done
    if not init_only and not res.swarm_done and not state.get("quota_paused_reset"):
        non_mon = {n: info for n, info in live_members.items() if n != "monitor"}
        all_sleeping = bool(non_mon) and all(info.get("wake_mode") != "working" for info in non_mon.values())
        if all_sleeping and not procs and res.deltas == 0:
            state["idle_ticks"] = int(state.get("idle_ticks", 0)) + 1
            if state["idle_ticks"] == 2:
                coord_read = int(non_mon.get("coordinator", {}).get("last_read_seq", 0))
                if highest_seq > coord_read:
                    res.alerts.append(
                        f"[W5_STALL] All agents sleeping, PS=none, coordinator unread #{coord_read}<#{highest_seq}; ping coordinator."
                    )
                else:
                    res.alerts.append(
                        f"[W5_IDLE] All agents idle for 2 ticks with PS=none at #{highest_seq}; check if stalled or go dormant."
                    )
        else:
            state["idle_ticks"] = 0

    return res


def format_output(res: CheckResult, max_lines: int = 15) -> str:
    lines: list[str] = []
    if res.alerts:
        lines.extend(res.alerts[: max_lines - 1])
    else:
        lines.append("OK NO_ALERT")
    status_line = (
        f"STATUS head=#{res.board_head} deltas={res.deltas} "
        f"live={','.join(res.active_members)} ps={res.ps_summary}"
    )
    lines.append(status_line[:240])
    return "\n".join(lines[:max_lines])


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Zero-token Arkhe swarm watchdog (W1-W9).")
    parser.add_argument("--swarm-id", help="Coordinator conversation ID (auto-detected if omitted).")
    parser.add_argument("--brain-dir", default=DEFAULT_BRAIN_DIR, help="Antigravity brain root directory.")
    parser.add_argument("--repo", default=DEFAULT_REPO_DIR, help="Repository root path.")
    parser.add_argument("--state-file", help="Explicit state JSON path.")
    parser.add_argument("--init", action="store_true", help="Initialize state offsets at current tip.")
    parser.add_argument("--once", action="store_true", help="Run a single inspection pass and exit.")
    parser.add_argument("--watch", action="store_true", help="Loop silently until any alert fires, then exit.")
    parser.add_argument("--interval", type=int, default=90, help="Polling interval in seconds for --watch.")
    parser.add_argument("--quota-pause-pct", type=int, default=15, help="W8 pre-emptive pause percentage.")
    parser.add_argument("--heartbeat-active-mins", type=int, default=30, help="Active minutes between quota heartbeats.")
    parser.add_argument("--record-quota", type=int, help="Persist authoritative quota % from swarm{action:'models'}.")
    parser.add_argument("--record-reset", help="Persist authoritative ISO-8601 reset timestamp with --record-quota.")
    parser.add_argument("--clear-pause", action="store_true", help="Clear quota-pause marker after resume.")
    args = parser.parse_args(argv)

    brain_dir = Path(args.brain_dir)
    swarm_id = args.swarm_id or discover_latest_swarm_id(brain_dir)
    if not swarm_id:
        print("ERROR: no active swarm members.json found under brain dir.", file=sys.stderr)
        return 2

    state_file = Path(args.state_file) if args.state_file else default_state_path(swarm_id)
    cfg = WatchdogConfig(
        brain_dir=brain_dir,
        repo_dir=Path(args.repo),
        swarm_id=swarm_id,
        state_file=state_file,
        interval=max(10, args.interval),
        quota_pause_pct=args.quota_pause_pct,
        heartbeat_active_mins=args.heartbeat_active_mins,
    )
    state = init_state_dict(load_json_file(state_file, {}))

    if args.clear_pause:
        state["quota_paused_reset"] = None
        state["last_heartbeat_ts"] = cfg.now().timestamp()
        state["window_uncached_since_hb"] = 0
        state["active_since_hb"] = False
        save_json_atomic(state_file, state)
        if not (args.init or args.once or args.watch or args.record_quota is not None):
            print(f"CLEARED_PAUSE swarm={swarm_id[:8]}")
            return 0

    if args.record_quota is not None:
        now_dt = cfg.now()
        state["last_quota"] = {
            "pct": int(args.record_quota),
            "resets": args.record_reset or "",
            "ts": now_dt.isoformat(),
            "source": "swarm_models",
            "model": "gemini-pool",
        }
        state["last_heartbeat_ts"] = now_dt.timestamp()
        state["window_uncached_since_hb"] = 0
        state["active_since_hb"] = False
        if int(args.record_quota) >= 90:
            state["quota_paused_reset"] = None
        save_json_atomic(state_file, state)
        if not (args.init or args.once or args.watch):
            print(f"RECORDED_QUOTA pct={args.record_quota}% resets={args.record_reset or 'none'}")
            return 0

    if args.init:
        res = inspect_once(cfg, state, init_only=True)
        state["last_heartbeat_ts"] = cfg.now().timestamp()
        save_json_atomic(state_file, state)
        print(f"INIT swarm={swarm_id} state={state_file}")
        print(format_output(res, cfg.max_output_lines))
        return 0

    if args.once or not args.watch:
        res = inspect_once(cfg, state, init_only=False)
        save_json_atomic(state_file, state)
        print(format_output(res, cfg.max_output_lines))
        return 0

    # Event-driven watch loop: zero LLM tokens while healthy; exit immediately on alert
    while True:
        res = inspect_once(cfg, state, init_only=False)
        save_json_atomic(state_file, state)
        if res.alerts or res.swarm_done or res.quota_pause_triggered:
            print(format_output(res, cfg.max_output_lines))
            return 0
        time.sleep(cfg.interval)


if __name__ == "__main__":
    raise SystemExit(main())
