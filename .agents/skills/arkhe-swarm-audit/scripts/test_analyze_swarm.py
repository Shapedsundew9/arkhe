#!/usr/bin/env python3
"""Unit tests for .agents/skills/arkhe-swarm-audit/scripts/analyze_swarm.py."""

from __future__ import annotations

import json
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import analyze_swarm as az  # noqa: E402


class AnalyzeSwarmTests(unittest.TestCase):
    def setUp(self) -> None:
        self.tmp = tempfile.TemporaryDirectory()
        self.brain = Path(self.tmp.name) / "brain"
        self.repo = Path(self.tmp.name) / "repo"
        self.repo.mkdir(parents=True)
        (self.repo / "src").mkdir()
        (self.repo / "src" / "big.rs").write_text("fn x() {}\n" * 500, encoding="utf-8")

        self.swarm_id = "coord-test-1"
        sw_dir = self.brain / self.swarm_id / ".system_generated" / "swarm"
        sw_dir.mkdir(parents=True)
        msg_dir = self.brain / self.swarm_id / ".system_generated" / "messages"
        msg_dir.mkdir(parents=True)

        (sw_dir / "members.json").write_text(
            json.dumps(
                {
                    "title": "Synthetic Audit Swarm",
                    "members": {
                        "coordinator": {
                            "conversation_id": self.swarm_id,
                            "role": "coordinator",
                            "applied_model": "gemini-4-neon-xhigh",
                            "phase": "RUNNING",
                            "killed": False,
                        },
                        "chief-scientist": {
                            "conversation_id": "cs-1",
                            "role": "worker",
                            "applied_model": "gemini-4-carbon-high",
                            "phase": "KILLED",
                            "killed": True,
                        },
                        "worker1": {
                            "conversation_id": "w-1",
                            "role": "worker",
                            "applied_model": "gemini-4-hydrogen-high",
                            "phase": "KILLED",
                            "killed": True,
                        },
                        "monitor": {
                            "conversation_id": "mon-1",
                            "role": "worker",
                            "applied_model": "gemini-4-hydrogen-high",
                            "phase": "RUNNING",
                            "killed": False,
                        },
                    },
                }
            ),
            encoding="utf-8",
        )

        # Board with direct mention, casual mid-body @chief-scientist, oversized post, and [done]
        board_rows = [
            {"seq": 1, "agent": "coordinator", "text": "@worker1 Start task per `@chief-scientist` spec.\n" + ("x" * 3500)},
            {"seq": 2, "agent": "coordinator", "text": "[done] Milestone closed."},
        ]
        (sw_dir / "board.jsonl").write_text("\n".join(json.dumps(r) for r in board_rows) + "\n", encoding="utf-8")

        # 429 error message at 02:00Z resetting in 2h0m0s (-> 04:00Z)
        (msg_dir / "err429.json").write_text(
            json.dumps(
                {
                    "timestamp": "2026-11-25T02:00:00Z",
                    "renderDetails": {"messageTitle": "Error from worker1"},
                    "content": "RESOURCE_EXHAUSTED (code 429): Individual quota reached. Resets in 2h0m0s.",
                }
            ),
            encoding="utf-8",
        )

        # Coordinator transcript: active 01:00 -> 02:00 (with normal 35m benchmark gap 01:10 -> 01:45),
        # then crashed at 02:00 and resumed manually at 06:00 (2h cooldown + 2h post-reset dead wait!)
        self._write_t(
            self.swarm_id,
            [
                {"step_index": 1, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:00:00Z", "model": "gemini-4-neon-xhigh", "input_tokens": 10000, "cache_read_tokens": 0, "output_tokens": 500, "tool_calls": []},
                {"step_index": 2, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:45:00Z", "model": "gemini-4-neon-xhigh", "input_tokens": 2000, "cache_read_tokens": 10000, "output_tokens": 200, "tool_calls": []},
                {"step_index": 3, "type": "SYSTEM_MESSAGE", "created_at": "2026-11-25T01:50:00Z", "content": "[quota] model gemini-4-neon-xhigh is low on quota (15% remaining, resets at 2026-11-25T04:00:00Z)"},
                {"step_index": 4, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T02:00:00Z", "model": "gemini-4-neon-xhigh", "input_tokens": 5000, "cache_read_tokens": 12000, "output_tokens": 300, "tool_calls": []},
                {"step_index": 5, "type": "USER_INPUT", "created_at": "2026-11-25T06:00:00Z", "content": "resume"},
                {"step_index": 6, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T06:00:05Z", "model": "gemini-4-neon-xhigh", "input_tokens": 15000, "cache_read_tokens": 0, "output_tokens": 400, "tool_calls": []},
            ],
        )

        # Chief scientist with 1 no-op wake (100K uncached!)
        self._write_t(
            "cs-1",
            [
                {"step_index": 1, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:10:00Z", "model": "gemini-4-carbon-high", "input_tokens": 100000, "cache_read_tokens": 0, "output_tokens": 100, "tool_calls": [{"name": "read_board", "arguments": {}}]},
                {"step_index": 2, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:10:05Z", "model": "gemini-4-carbon-high", "input_tokens": 2000, "cache_read_tokens": 100000, "output_tokens": 50, "tool_calls": [{"name": "swarm_wait", "arguments": {"mentions_only": True}}]},
            ],
        )

        # Worker with 2-strike Rust test grind, 1 compaction, and repeated file read
        self._write_t(
            "w-1",
            [
                {"step_index": 1, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:05:00Z", "model": "gemini-4-hydrogen-high", "input_tokens": 20000, "cache_read_tokens": 0, "output_tokens": 1000, "tool_calls": [{"name": "read_file", "arguments": {"Path": str(self.repo / "src/big.rs")}}, {"name": "edit_file", "arguments": {}}]},
                {"step_index": 2, "type": "GENERIC", "created_at": "2026-11-25T01:06:00Z", "content": "test my_rust_test ... FAILED\nCommand exited with code 101."},
                {"step_index": 3, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:07:00Z", "model": "gemini-4-hydrogen-high", "input_tokens": 5000, "cache_read_tokens": 20000, "output_tokens": 1000, "tool_calls": [{"name": "edit_file", "arguments": {}}]},
                {"step_index": 4, "type": "SYSTEM_MESSAGE", "created_at": "2026-11-25T01:08:00Z", "content": "finished with result:\ntest my_rust_test ... FAILED\nCommand exited with code 101."},
                {"step_index": 5, "type": "CHECKPOINT", "created_at": "2026-11-25T01:09:00Z", "input_tokens": 1000, "cache_read_tokens": 25000, "output_tokens": 500},
            ],
        )

        # Monitor still running after [done]
        self._write_t(
            "mon-1",
            [
                {"step_index": 1, "type": "PLANNER_RESPONSE", "created_at": "2026-11-25T01:01:00Z", "model": "gemini-4-hydrogen-high", "input_tokens": 12000, "cache_read_tokens": 0, "output_tokens": 200, "tool_calls": [{"name": "swarm_wait", "arguments": {}}]},
            ],
        )

    def tearDown(self) -> None:
        self.tmp.cleanup()

    def _write_t(self, cid: str, rows: list[dict]) -> None:
        p = self.brain / cid / ".system_generated" / "logs" / "transcript.jsonl"
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text("\n".join(json.dumps(r) for r in rows) + "\n", encoding="utf-8")

    def test_analyze_swarm_quantifies_stalls_noop_mentions_grind_and_markdown(self) -> None:
        rep = az.analyze_swarm(self.brain, self.swarm_id, repo_dir=self.repo)
        wc = rep["wall_clock"]
        self.assertEqual(len(wc["stalls"]), 1)
        self.assertEqual(wc["stalls"][0]["cooldown_s"], 7200)
        self.assertEqual(wc["stalls"][0]["post_reset_wait_s"], 7205)
        self.assertTrue(wc["stalls"][0]["had_429"])

        # Burn rate: 15% -> 0% in 10 min = 1.5%/min
        self.assertEqual(rep["quota"]["burn_rates"][0]["pct_per_min"], 1.5)

        # Casual mid-body mention + oversized post
        self.assertEqual(rep["board"]["casual_midbody_mentions"], {"chief-scientist": 1})
        self.assertEqual(len(rep["board"]["oversized_over_3kb"]), 1)

        # Chief scientist no-op wake quantified
        cs = next(a for a in rep["agents"] if a["name"] == "chief-scientist")
        self.assertEqual(cs["noop_wakes"], 1)
        self.assertEqual(cs["noop_unc"], 102000)

        # Grind + compaction + unkilled monitor + file size
        self.assertEqual(len(rep["friction"]["grind_events"]), 1)
        self.assertEqual(rep["friction"]["grind_events"][0]["test"], "my_rust_test")
        self.assertEqual(rep["friction"]["still_running_after_done"], ["monitor"])
        self.assertEqual(rep["friction"]["top_read_files"][0]["lines"], 500)

        md = az.render_markdown(rep)
        self.assertIn("Synthetic Audit Swarm", md)
        self.assertIn("2h 00m", md)
        self.assertIn("my_rust_test", md)


if __name__ == "__main__":
    unittest.main()
