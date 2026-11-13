#!/usr/bin/env python3
"""Run and summarize B5 compute and cliff/bound counter profiles for arkhe."""

from __future__ import annotations

import argparse
import json
import pathlib
import subprocess
import sys
from typing import Any


CLIFF_COUNTER_FIELDS = [
    ("reserve_clipped_count", "reserve_clip_cnt"),
    ("reserve_clipped_bits", "reserve_clip_bits"),
    ("reserve_mismatch_floored", "mismatch_floored"),
    ("sprout_blocked_max_nodes", "blocked_max_nodes"),
    ("candidate_undecidable", "cand_undecidable"),
    ("candidate_list_truncated", "cand_truncated"),
    ("lag_horizon_bound", "lag_bound"),
    ("ring_period_bound", "ring_bound"),
]


def run_cmd(cmd: list[str], cwd: pathlib.Path) -> str:
    proc = subprocess.run(cmd, cwd=cwd, check=True, text=True, capture_output=True)
    return proc.stdout


def fmt_cliff_row(c: dict[str, Any]) -> str:
    return " | ".join(
        f"{c.get(k, 0.0):.2f}" if k == "reserve_clipped_bits" else str(c.get(k, 0))
        for k, _ in CLIFF_COUNTER_FIELDS
    )


def render_matrix_table(rows: list[dict[str, Any]], regime: str) -> list[str]:
    subset = [r for r in rows if r["regime"] == regime]
    channels = sorted({int(r["channels"]) for r in subset})
    nodes = sorted({int(r["target_nodes"]) for r in subset})
    by_nk = {(int(r["target_nodes"]), int(r["channels"])): r for r in subset}

    lines = [
        f"#### Controlled `{regime}` (`us/tick` mean [eval_ticks/tick])",
        "",
        "| $N$ | " + " | ".join(f"$K={k}$" for k in channels) + " |",
        "| ---: | " + " | ".join("---:" for _ in channels) + " |",
    ]
    for n in nodes:
        cells = []
        for k in channels:
            r = by_nk.get((n, k))
            if r is None:
                cells.append("n/a")
            else:
                cells.append(
                    f"{r['us_per_tick_mean']:.2f} [{r['eval_ticks_per_tick_mean']:.2f}]"
                )
        lines.append(f"| {n} | " + " | ".join(cells) + " |")
    lines.append("")
    return lines


def render_report(
    profile: dict[str, Any],
    m1a: dict[str, Any] | None = None,
    m1b: dict[str, Any] | None = None,
) -> str:
    prov = profile["provenance"]
    lines: list[str] = [
        "### B5 Compute & Counter Profile",
        "",
        f"- **Provenance**: commit `{prov.get('git_commit_sha')}` "
        f"(dirty={prov.get('git_dirty')}), "
        f"`source_tree_fnv1a128={prov.get('source_tree_fnv1a128')}`, "
        f"`rustc={prov.get('rustc_version')}`",
        f"- **Config**: `seed_offset={profile['seed_offset']}`, "
        f"`seeds={profile['seeds_tested']}`, "
        f"`horizon={profile['horizon']}`, "
        f"`endogenous_horizon={profile['endogenous_horizon']}`, "
        f"`max_eval_ticks={profile['max_eval_ticks_budget']}`",
        "",
    ]

    for regime in ("steady_clean", "diagnostic_noise"):
        lines.extend(render_matrix_table(profile["controlled_sweep"], regime))

    lines.extend(
        [
            "#### Controlled Sweep Counters & Scale Detail",
            "",
            "| regime | $N$ | $K$ | us/tick (mean/max) | eval/tick | final_nodes | sprout/ring/starve/idle | "
            + " | ".join(short for _, short in CLIFF_COUNTER_FIELDS)
            + " |",
            "| :--- | ---: | ---: | ---: | ---: | ---: | :---: | "
            + " | ".join("---:" for _ in CLIFF_COUNTER_FIELDS)
            + " |",
        ]
    )
    for r in profile["controlled_sweep"]:
        nodes_rng = (
            str(r["final_nodes_min"])
            if r["final_nodes_min"] == r["final_nodes_max"]
            else f"{r['final_nodes_min']}..{r['final_nodes_max']}"
        )
        events = f"{r['sprout_events']}/{r['ring_closure_events']}/{r['starvation_prune_events']}/{r['idle_prune_events']}"
        lines.append(
            f"| {r['regime']} | {r['target_nodes']} | {r['channels']} | "
            f"{r['us_per_tick_mean']:.2f} / {r['us_per_tick_max']:.2f} | "
            f"{r['eval_ticks_per_tick_mean']:.2f} | {nodes_rng} | {events} | "
            f"{fmt_cliff_row(r['cliff_counters'])} |"
        )
    lines.append("")

    lines.extend(
        [
            "#### Endogenous Online Workloads",
            "",
            "| task | $k$ | $K$ | $\\varepsilon$ | us/tick (mean/max) | eval/tick | final_nodes | post500_err | sprout/ring/starve/idle | "
            + " | ".join(short for _, short in CLIFF_COUNTER_FIELDS)
            + " |",
            "| :--- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | :---: | "
            + " | ".join("---:" for _ in CLIFF_COUNTER_FIELDS)
            + " |",
        ]
    )
    for r in profile["endogenous_runs"]:
        nodes_rng = (
            str(r["final_nodes_min"])
            if r["final_nodes_min"] == r["final_nodes_max"]
            else f"{r['final_nodes_min']}..{r['final_nodes_max']}"
        )
        events = f"{r['sprout_events']}/{r['ring_closure_events']}/{r['starvation_prune_events']}/{r['idle_prune_events']}"
        lines.append(
            f"| {r['task']} | {r['width_bits']} | {r['channels']} | {r['epsilon']:.2f} | "
            f"{r['us_per_tick_mean']:.2f} / {r['us_per_tick_max']:.2f} | "
            f"{r['eval_ticks_per_tick_mean']:.2f} | {nodes_rng} | "
            f"{r['clean_mismatches_post500_total']} | {events} | "
            f"{fmt_cliff_row(r['cliff_counters'])} |"
        )
    lines.append("")

    lines.extend(
        [
            "#### Aggregated Cliff & Bound Counter Totals",
            "",
            "| Suite | " + " | ".join(short for _, short in CLIFF_COUNTER_FIELDS) + " |",
            "| :--- | " + " | ".join("---:" for _ in CLIFF_COUNTER_FIELDS) + " |",
            f"| profile:controlled_sweep | {fmt_cliff_row(profile['controlled_cliff_totals'])} |",
            f"| profile:endogenous_runs | {fmt_cliff_row(profile['endogenous_cliff_totals'])} |",
        ]
    )
    if m1a is not None:
        lines.append(
            f"| verify-m1a (seeds {m1a['seed_offset']}..{m1a['seed_offset'] + m1a['seeds_tested']}, all_passed={m1a['all_passed']}) | "
            f"{fmt_cliff_row(m1a['cliff_counters'])} |"
        )
    if m1b is not None:
        lines.append(
            f"| verify-m1b (seeds {m1b['seed_offset']}..{m1b['seed_offset'] + m1b['seeds_tested']}, all_passed={m1b['all_passed']}, excluded={m1b['excluded_cases']}) | "
            f"{fmt_cliff_row(m1b['cliff_counters'])} |"
        )
    lines.append("")
    return "\n".join(lines)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--seed-offset", type=int, default=0)
    parser.add_argument("--seeds", type=int, default=5)
    parser.add_argument("--horizon", type=int, default=512)
    parser.add_argument("--endogenous-horizon", type=int, default=1024)
    parser.add_argument("--gate-seeds", type=int, default=20)
    parser.add_argument("--skip-build", action="store_true")
    parser.add_argument("--with-m1-gates", action="store_true")
    parser.add_argument("--markdown-out", type=pathlib.Path)
    parser.add_argument("--json-out-dir", type=pathlib.Path)
    args = parser.parse_args()

    repo_root = pathlib.Path(__file__).resolve().parent.parent
    if not args.skip_build:
        run_cmd(["cargo", "build", "--locked", "--release"], repo_root)

    bin_path = repo_root / "target" / "release" / "arkhe"
    profile_raw = run_cmd(
        [
            str(bin_path),
            "profile-compute",
            "--seed-offset",
            str(args.seed_offset),
            "--seeds",
            str(args.seeds),
            "--horizon",
            str(args.horizon),
            "--endogenous-horizon",
            str(args.endogenous_horizon),
        ],
        repo_root,
    )
    profile = json.loads(profile_raw)

    m1a = None
    m1b = None
    if args.with_m1_gates:
        m1a_raw = run_cmd(
            [
                str(bin_path),
                "verify-m1a",
                "--seed-offset",
                str(args.seed_offset),
                "--seeds",
                str(args.gate_seeds),
            ],
            repo_root,
        )
        m1a = json.loads(m1a_raw)

        m1b_raw = run_cmd(
            [
                str(bin_path),
                "verify-m1b",
                "--seed-offset",
                str(args.seed_offset),
                "--seeds",
                str(args.gate_seeds),
            ],
            repo_root,
        )
        m1b = json.loads(m1b_raw)

    if args.json_out_dir is not None:
        args.json_out_dir.mkdir(parents=True, exist_ok=True)
        (args.json_out_dir / "profile_compute.json").write_text(profile_raw, encoding="utf-8")
        if m1a is not None:
            (args.json_out_dir / "verify_m1a.json").write_text(
                json.dumps(m1a, indent=2), encoding="utf-8"
            )
        if m1b is not None:
            (args.json_out_dir / "verify_m1b.json").write_text(
                json.dumps(m1b, indent=2), encoding="utf-8"
            )

    markdown = render_report(profile, m1a=m1a, m1b=m1b)
    if args.markdown_out is not None:
        args.markdown_out.write_text(markdown, encoding="utf-8")
    else:
        sys.stdout.write(markdown)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
