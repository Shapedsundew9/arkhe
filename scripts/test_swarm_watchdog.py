#!/usr/bin/env python3
"""Unit tests for scripts/swarm_watchdog.py covering W1-W9 and quota pause."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import swarm_watchdog as sw  # noqa: E402


class SwarmWatchdogTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.brain = Path(self.tmp.name) / "brain"
        self.repo = Path(self.tmp.name) / "repo"
        self.repo.mkdir(parents=True)
        self.swarm_id = "coord-0001"
        self.state_file = Path(self.tmp.name) / "state.json"
        self.now = datetime(2026, 11, 25, 22, 40, 0, tzinfo=timezone.utc)

        self._write_members(
            {
                "coordinator": {
                    "conversation_id": self.swarm_id,
                    "role": "coordinator",
                    "applied_model": "gemini-4-neon-xhigh",
                    "phase": "RUNNING",
                    "wake_mode": "mentions",
                    "last_read_seq": 1,
                    "killed": False,
                },
                "monitor": {
                    "conversation_id": "mon-0001",
                    "role": "worker",
                    "applied_model": "gemini-4-hydrogen-high",
                    "phase": "RUNNING",
                    "wake_mode": "mentions",
                    "last_read_seq": 1,
                    "killed": False,
                },
                "phase1": {
                    "conversation_id": "w-0001",
                    "role": "worker",
                    "applied_model": "gemini-4-hydrogen-high",
                    "phase": "RUNNING",
                    "wake_mode": "working",
                    "last_read_seq": 1,
                    "killed": False,
                },
                "chief-scientist": {
                    "conversation_id": "cs-0001",
                    "role": "worker",
                    "applied_model": "gemini-4-carbon-high",
                    "phase": "RUNNING",
                    "wake_mode": "mentions",
                    "last_read_seq": 1,
                    "killed": False,
                },
                "verifier": {
                    "conversation_id": "ver-0001",
                    "role": "worker",
                    "applied_model": "gemini-4-hydrogen-high",
                    "phase": "RUNNING",
                    "wake_mode": "mentions",
                    "last_read_seq": 1,
                    "killed": False,
                },
            }
        )
        self._write_board([{"seq": 1, "agent": "coordinator", "text": "@phase1 Start phase 1."}])

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def _write_members(self, members: dict) -> None:
        p = self.brain / self.swarm_id / ".system_generated" / "swarm" / "members.json"
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps({"title": "Test", "members": members}), encoding="utf-8")

    def _write_board(self, rows: list[dict]) -> None:
        p = self.brain / self.swarm_id / ".system_generated" / "swarm" / "board.jsonl"
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")

    def _append_transcript(self, cid: str, rows: list[dict]) -> None:
        p = self.brain / cid / ".system_generated" / "logs" / "transcript.jsonl"
        p.parent.mkdir(parents=True, exist_ok=True)
        with p.open("a", encoding="utf-8") as fh:
            for r in rows:
                fh.write(json.dumps(r) + "\n")

    def _cfg(self) -> sw.WatchdogConfig:
        return sw.WatchdogConfig(
            brain_dir=self.brain,
            repo_dir=self.repo,
            swarm_id=self.swarm_id,
            state_file=self.state_file,
            now_override=self.now,
        )

    def test_w8_preemptive_quota_pause_triggers_from_any_member_transcript(self) -> None:
        state = sw.init_state_dict({})
        sw.inspect_once(self._cfg(), state, init_only=True, procs_override=[], diff_files_override=[])
        # Worker receives [quota] 14% remaining with future reset timestamp
        self._append_transcript(
            "w-0001",
            [
                {
                    "step_index": 5,
                    "type": "SYSTEM_MESSAGE",
                    "created_at": "2026-11-25T22:39:51Z",
                    "content": "[quota] model gemini-4-hydrogen-high is low on quota (14% remaining, resets at 2026-11-26T02:14:58Z) — check swarm.",
                }
            ],
        )
        res = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertTrue(res.quota_pause_triggered)
        self.assertTrue(any("[W8_PAUSE]" in a and "14%" in a for a in res.alerts))

        # Second tick at same % does not spam duplicate pause alert
        res2 = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertFalse(any("[W8_PAUSE]" in a for a in res2.alerts))

    def test_w2_catches_two_strike_rust_test_grind_and_ignores_rg_exit_1(self) -> None:
        state = sw.init_state_dict({})
        sw.inspect_once(self._cfg(), state, init_only=True, procs_override=[], diff_files_override=[])
        # Harmless rg exit code 1 must NOT trigger W2
        self._append_transcript(
            "w-0001",
            [
                {
                    "step_index": 1,
                    "type": "PLANNER_RESPONSE",
                    "tool_calls": [{"name": "edit_file", "arguments": {}}],
                },
                {
                    "step_index": 2,
                    "type": "PLANNER_RESPONSE",
                    "tool_calls": [{"name": "run_command", "arguments": {"CommandLine": "rg nonexistent src/"}}],
                },
                {
                    "step_index": 3,
                    "type": "GENERIC",
                    "content": "Command exited with code 1.",
                },
            ],
        )
        res_rg = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertFalse(any("[W2_" in a for a in res_rg.alerts))

        # Two consecutive edit -> test FAILED cycles on same Rust test MUST trigger W2_GRIND
        self._append_transcript(
            "w-0001",
            [
                {"step_index": 4, "type": "PLANNER_RESPONSE", "tool_calls": [{"name": "edit_file", "arguments": {}}]},
                {"step_index": 5, "type": "GENERIC", "content": "test i4_mdl_conservation ... FAILED\nCommand exited with code 101."},
                {"step_index": 6, "type": "PLANNER_RESPONSE", "tool_calls": [{"name": "edit_file", "arguments": {}}]},
                {"step_index": 7, "type": "GENERIC", "content": "test i4_mdl_conservation ... FAILED\nCommand exited with code 101."},
            ],
        )
        res_fail = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertTrue(any("[W2_GRIND]" in a and "i4_mdl_conservation" in a for a in res_fail.alerts))

    def test_w2_catches_async_system_message_and_truncated_cargo_test_failures(self) -> None:
        state = sw.init_state_dict({})
        sw.inspect_once(self._cfg(), state, init_only=True, procs_override=[], diff_files_override=[])
        # Write untruncated content to transcript_full.jsonl for step 11
        full_p = self.brain / "w-0001" / ".system_generated" / "logs" / "transcript_full.jsonl"
        full_p.parent.mkdir(parents=True, exist_ok=True)
        full_p.write_text(
            json.dumps(
                {
                    "step_index": 11,
                    "type": "SYSTEM_MESSAGE",
                    "content": (
                        "Task id 'w-0001/task-10' finished with result:\nCommand exited with code 101.\n"
                        "test golden_traces ... ok\ntest result: ok. 1 passed; 0 failed;\n"
                        "test async_sleep_check ... FAILED\ntest result: FAILED. 0 passed; 1 failed;"
                    ),
                }
            )
            + "\n",
            encoding="utf-8",
        )
        self._append_transcript(
            "w-0001",
            [
                {"step_index": 10, "type": "PLANNER_RESPONSE", "tool_calls": [{"name": "edit_file", "arguments": {}}]},
                {
                    "step_index": 11,
                    "type": "SYSTEM_MESSAGE",
                    "truncated_fields": ["content"],
                    "content": "Task id 'w-0001/task-10' finished with result:\nCommand exited with code 101.\ntest result: ok. 1 passed; 0 failed;",
                },
                {"step_index": 12, "type": "PLANNER_RESPONSE", "tool_calls": [{"name": "edit_file", "arguments": {}}]},
                {
                    "step_index": 13,
                    "type": "SYSTEM_MESSAGE",
                    "content": "Task id 'w-0001/task-12' finished with result:\nCommand exited with code 101.\ntest async_sleep_check ... FAILED",
                },
            ],
        )
        res = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertTrue(any("[W2_GRIND]" in a and "async_sleep_check" in a for a in res.alerts))

    def test_w1_detects_non_hydrogen_worker_wrong_cs_model_and_midbody_mentions(self) -> None:
        members_path = self.brain / self.swarm_id / ".system_generated" / "swarm" / "members.json"
        doc = json.loads(members_path.read_text(encoding="utf-8"))
        doc["members"]["phase1"]["applied_model"] = "gemini-4-carbon-high"
        doc["members"]["chief-scientist"]["applied_model"] = "gemini-4-neon-xhigh"
        doc["members"]["chief-scientist"]["wake_mode"] = "all"
        members_path.write_text(json.dumps(doc), encoding="utf-8")

        self._write_board(
            [
                {"seq": 1, "agent": "coordinator", "text": "@phase1 Start"},
                {
                    "seq": 6,
                    "agent": "phase1",
                    "text": "@coordinator Ready after `@verifier`-approved check per `@chief-scientist`.",
                },
            ]
        )
        state = sw.init_state_dict({"last_board_seq": 1})
        res = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertTrue(any("[W1_MODEL]" in a and "phase1" in a for a in res.alerts))
        self.assertTrue(any("[W1_MODEL]" in a and "chief-scientist" in a and "carbon" in a for a in res.alerts))
        self.assertTrue(any("[W1_WAKE]" in a and "chief-scientist" in a for a in res.alerts))
        self.assertTrue(any("[W1_MENTION]" in a and "verifier" in a for a in res.alerts))

    def test_w9_detects_compaction_and_verifier_context_ceiling(self) -> None:
        state = sw.init_state_dict({})
        sw.inspect_once(self._cfg(), state, init_only=True, procs_override=[], diff_files_override=[])
        self._append_transcript(
            "w-0001",
            [{"step_index": 200, "type": "CHECKPOINT", "content": "# Resuming from a compaction"}],
        )
        self._append_transcript(
            "ver-0001",
            [{"step_index": 80, "type": "PLANNER_RESPONSE", "input_tokens": 12_000, "cache_read_tokens": 148_000}],
        )
        res = sw.inspect_once(self._cfg(), state, procs_override=[], diff_files_override=[])
        self.assertTrue(any("[W9_COMPACT]" in a and "phase1" in a for a in res.alerts))
        self.assertTrue(any("[W9_CTX]" in a and "verifier" in a for a in res.alerts))

    def test_w7_and_w3_and_w6_and_w8_429(self) -> None:
        state = sw.init_state_dict({"last_board_seq": 1})
        self._write_board(
            [
                {"seq": 1, "agent": "coordinator", "text": "@phase1 Start"},
                {"seq": 2, "agent": "coordinator", "text": "@chief-scientist S3 sign-off request. Everything looks good."},
            ]
        )
        msg_dir = self.brain / self.swarm_id / ".system_generated" / "messages"
        msg_dir.mkdir(parents=True, exist_ok=True)
        (msg_dir / "err1.json").write_text(
            json.dumps(
                {
                    "content": "RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 2h30m0s."
                }
            ),
            encoding="utf-8",
        )
        res = sw.inspect_once(
            self._cfg(),
            state,
            procs_override=[(9999, 1200, 99.0, "./target/release/arkhe explore-m4a")],
            diff_files_override=["tests/golden_traces.rs"],
            diff_text_override="+allow_with_flag: true\n-SproutBlockedMaxNodes\n",
        )
        joined = "\n".join(res.alerts)
        self.assertIn("[W7_BASELINE]", joined)
        self.assertIn("[W8_429]", joined)
        self.assertIn("9180s", joined)  # 2h30m = 9000s + 180s buffer
        self.assertIn("[W3_GOLDEN]", joined)
        self.assertIn("[W3_HOLDOUT]", joined)
        self.assertIn("[W4_GUARDRAIL]", joined)
        self.assertIn("[W6_RUNAWAY]", joined)


if __name__ == "__main__":
    unittest.main()
