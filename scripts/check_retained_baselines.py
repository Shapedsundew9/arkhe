#!/usr/bin/env python3
"""Mechanical gate for Arkhe retained baselines (golden_traces, verify-m2a, verify-m2b)."""

from __future__ import annotations

import argparse
import contextlib
import copy
import io
import json
import math
from pathlib import Path
import subprocess
import sys
import tempfile
from typing import Any


REPO_ROOT = Path(__file__).resolve().parent.parent
DEFAULT_MANIFEST = REPO_ROOT / "docs" / "retained_baselines.json"
DEFAULT_BINARY = REPO_ROOT / "target" / "release" / "arkhe"


class ValidationError(ValueError):
    """Raised when a baseline report or manifest fails mechanical validation."""


def _is_real_int(value: Any) -> bool:
    return isinstance(value, int) and not isinstance(value, bool)


def _is_finite_number(value: Any) -> bool:
    return (
        isinstance(value, (int, float))
        and not isinstance(value, bool)
        and math.isfinite(value)
    )


def _reject_nonfinite_json(token: str) -> Any:
    raise ValueError(f"Non-finite JSON constant rejected: {token}")


def parse_json_strict(raw: str) -> Any:
    """Parse JSON while rejecting NaN, Infinity, and -Infinity."""
    return json.loads(raw, parse_constant=_reject_nonfinite_json)


def _validate_tail_metrics(
    tail_metrics: Any,
    context: str,
    require_finite: bool,
    errors: list[str],
) -> None:
    if not isinstance(tail_metrics, dict):
        errors.append(f"{context}: tail_metrics must be a dict, got {type(tail_metrics).__name__}")
        return
    if not require_finite:
        return
    for metric_name, metric_val in tail_metrics.items():
        if not isinstance(metric_name, str) or not metric_name:
            errors.append(f"{context}: tail_metrics key must be non-empty str")
            continue
        if not _is_finite_number(metric_val):
            errors.append(
                f"{context}: tail_metrics[{metric_name!r}] must be a finite int|float "
                f"(not bool/None/NaN/Inf), got {metric_val!r}"
            )


def _validate_task(
    task: Any,
    stratum_name: str,
    gate_name: str,
    idx: int,
    require_per_task_passed: bool,
    require_finite_tail_metrics: bool,
    errors: list[str],
) -> None:
    ctx = f"{gate_name}.{stratum_name}.tasks[{idx}]"
    if not isinstance(task, dict):
        errors.append(f"{ctx}: task must be a dict")
        return

    label = task.get("task_label")
    if isinstance(label, str) and label:
        ctx = f"{gate_name}.{stratum_name}.task[{label}]"
    else:
        errors.append(f"{ctx}: missing or invalid task_label")

    if require_per_task_passed:
        if task.get("passed") is not True:
            errors.append(f"{ctx}: passed must be True, got {task.get('passed')!r}")
        if task.get("passes_fwer_binomial") is not True:
            errors.append(
                f"{ctx}: passes_fwer_binomial must be True, got {task.get('passes_fwer_binomial')!r}"
            )

        dist = task.get("distribution")
        if not isinstance(dist, dict):
            errors.append(f"{ctx}: distribution must be a dict")
        elif dist.get("passes_homogeneity") is not True:
            errors.append(
                f"{ctx}: distribution.passes_homogeneity must be True, "
                f"got {dist.get('passes_homogeneity')!r}"
            )

    total_trials = task.get("total_trials")
    total_successes = task.get("total_successes")
    if not _is_real_int(total_trials) or total_trials <= 0:
        errors.append(f"{ctx}: total_trials must be int > 0, got {total_trials!r}")
    if not _is_real_int(total_successes) or total_successes < 0:
        errors.append(f"{ctx}: total_successes must be int >= 0, got {total_successes!r}")
    elif _is_real_int(total_trials) and total_successes > total_trials:
        errors.append(
            f"{ctx}: total_successes ({total_successes}) exceeds total_trials ({total_trials})"
        )

    if "tail_metrics" in task:
        _validate_tail_metrics(
            task["tail_metrics"],
            ctx,
            require_finite_tail_metrics,
            errors,
        )


def _validate_stratum(
    stratum: Any,
    stratum_name: str,
    gate_name: str,
    require_stratified_passed: bool,
    require_per_task_passed: bool,
    require_finite_tail_metrics: bool,
    errors: list[str],
) -> dict[str, Any]:
    ctx = f"{gate_name}.stratified.{stratum_name}"
    if not isinstance(stratum, dict):
        errors.append(f"{ctx}: stratum must be a dict")
        return {}

    if require_stratified_passed and stratum.get("passed") is not True:
        errors.append(f"{ctx}: passed must be True, got {stratum.get('passed')!r}")

    pooled_mb = stratum.get("pooled_multi_bank")
    if pooled_mb is not None:
        if not isinstance(pooled_mb, dict):
            errors.append(f"{ctx}: pooled_multi_bank must be a dict when present")
        elif require_stratified_passed and pooled_mb.get("passed") is not True:
            errors.append(
                f"{ctx}: pooled_multi_bank.passed must be True, got {pooled_mb.get('passed')!r}"
            )

    tasks = stratum.get("tasks")
    if not isinstance(tasks, list) or len(tasks) == 0:
        errors.append(f"{ctx}: tasks must be a non-empty list")
        return {}

    task_count = stratum.get("task_count")
    if not _is_real_int(task_count) or task_count != len(tasks):
        errors.append(
            f"{ctx}: task_count ({task_count!r}) must equal len(tasks) ({len(tasks)})"
        )

    for idx, task in enumerate(tasks):
        _validate_task(
            task,
            stratum_name,
            gate_name,
            idx,
            require_per_task_passed,
            require_finite_tail_metrics,
            errors,
        )

    return {
        "passed": stratum.get("passed"),
        "task_count": task_count,
        "target_rate": stratum.get("target_rate"),
        "fwer_alpha": stratum.get("fwer_alpha"),
        "worst_task_label": stratum.get("worst_task_label"),
        "worst_task_rate": stratum.get("worst_task_rate"),
    }


def validate_suite_report(
    suite_name: str,
    report: Any,
    expected_gates: list[str],
    require_clean: bool,
    requirements: dict[str, Any] | None = None,
) -> dict[str, Any]:
    """Validate a verify-m2a or verify-m2b JSON report against strict gate invariants."""
    reqs = {
        "require_all_passed": True,
        "max_excluded_cases": 0,
        "max_sprout_blocked_max_nodes": 0,
        "require_stratified_passed": True,
        "require_per_task_passed": True,
        "require_finite_tail_metrics": True,
    }
    if requirements:
        reqs.update(requirements)

    errors: list[str] = []
    if not isinstance(report, dict):
        raise ValidationError(f"{suite_name}: report must be a JSON object")

    if reqs["require_all_passed"] and report.get("all_passed") is not True:
        errors.append(f"{suite_name}: top-level all_passed must be True, got {report.get('all_passed')!r}")

    excluded_cases = report.get("excluded_cases")
    max_excluded = reqs["max_excluded_cases"]
    if not _is_real_int(excluded_cases) or excluded_cases < 0 or excluded_cases > max_excluded:
        errors.append(
            f"{suite_name}: top-level excluded_cases must be int in [0, {max_excluded}], "
            f"got {excluded_cases!r}"
        )

    cliff_counters = report.get("cliff_counters")
    max_sprout_blocked = reqs["max_sprout_blocked_max_nodes"]
    if not isinstance(cliff_counters, dict):
        errors.append(f"{suite_name}: cliff_counters must be a dict")
    else:
        sprout_blocked = cliff_counters.get("sprout_blocked_max_nodes")
        if (
            not _is_real_int(sprout_blocked)
            or sprout_blocked < 0
            or sprout_blocked > max_sprout_blocked
        ):
            errors.append(
                f"{suite_name}: cliff_counters.sprout_blocked_max_nodes must be int in "
                f"[0, {max_sprout_blocked}], got {sprout_blocked!r}"
            )

    provenance = report.get("provenance")
    if require_clean:
        if not isinstance(provenance, dict):
            errors.append(f"{suite_name}: provenance must be a dict under --require-clean")
        elif provenance.get("git_dirty") is not False:
            errors.append(
                f"{suite_name}: provenance.git_dirty must be False under --require-clean, "
                f"got {provenance.get('git_dirty')!r}"
            )
    elif provenance is not None and not isinstance(provenance, dict):
        errors.append(f"{suite_name}: provenance must be a dict when present")

    gates_raw = report.get("gates")
    gates_by_name: dict[str, dict[str, Any]] = {}
    if not isinstance(gates_raw, list) or len(gates_raw) == 0:
        errors.append(f"{suite_name}: gates must be a non-empty list")
    else:
        for idx, gate_entry in enumerate(gates_raw):
            if not isinstance(gate_entry, dict):
                errors.append(f"{suite_name}: gates[{idx}] must be a dict")
                continue
            gate_name = gate_entry.get("gate")
            if not isinstance(gate_name, str) or not gate_name:
                errors.append(f"{suite_name}: gates[{idx}].gate must be a non-empty str")
                continue
            if gate_name in gates_by_name:
                errors.append(f"{suite_name}: duplicate gate {gate_name!r}")
                continue
            gates_by_name[gate_name] = gate_entry

        missing_gates = [g for g in expected_gates if g not in gates_by_name]
        if missing_gates:
            errors.append(f"{suite_name}: missing expected gates: {missing_gates}")

    gate_summaries: list[dict[str, Any]] = []
    for gate_name, gate_entry in gates_by_name.items():
        if gate_entry.get("passed") is not True:
            errors.append(
                f"{suite_name}.{gate_name}: gate.passed must be True, got {gate_entry.get('passed')!r}"
            )

        gate_excluded = gate_entry.get("excluded_cases")
        if not _is_real_int(gate_excluded) or gate_excluded < 0 or gate_excluded > max_excluded:
            errors.append(
                f"{suite_name}.{gate_name}: excluded_cases must be int in [0, {max_excluded}], "
                f"got {gate_excluded!r}"
            )

        stratified = gate_entry.get("stratified")
        strata_summary: dict[str, Any] = {}
        if not isinstance(stratified, dict):
            errors.append(f"{suite_name}.{gate_name}: stratified must be a dict")
        else:
            if reqs["require_stratified_passed"] and stratified.get("passed") is not True:
                errors.append(
                    f"{suite_name}.{gate_name}: stratified.passed must be True, "
                    f"got {stratified.get('passed')!r}"
                )

            present_strata = [
                key
                for key in ("clean", "noisy")
                if key in stratified and stratified[key] is not None
            ]
            if not present_strata:
                errors.append(
                    f"{suite_name}.{gate_name}: stratified must have at least one non-null "
                    "stratum ('clean' or 'noisy')"
                )
            else:
                for stratum_name in present_strata:
                    strata_summary[stratum_name] = _validate_stratum(
                        stratified[stratum_name],
                        stratum_name,
                        f"{suite_name}.{gate_name}",
                        reqs["require_stratified_passed"],
                        reqs["require_per_task_passed"],
                        reqs["require_finite_tail_metrics"],
                        errors,
                    )

        gate_summaries.append(
            {
                "gate": gate_name,
                "passed": gate_entry.get("passed"),
                "excluded_cases": gate_excluded,
                "strata": strata_summary,
            }
        )

    if errors:
        raise ValidationError("; ".join(errors))

    prov_summary = {}
    if isinstance(provenance, dict):
        for key in (
            "git_commit_sha",
            "git_dirty",
            "source_tree_fnv1a128",
            "cargo_lock_fnv1a128",
            "package_version",
            "rustc_version",
            "target_triple",
        ):
            if key in provenance:
                prov_summary[key] = provenance[key]

    return {
        "suite": suite_name,
        "passed": True,
        "excluded_cases": excluded_cases,
        "cliff_counters": cliff_counters,
        "provenance": prov_summary,
        "gates": gate_summaries,
    }


def load_manifest(manifest_path: Path) -> dict[str, Any]:
    """Load and sanity-check the retained baselines manifest JSON."""
    if not manifest_path.is_file():
        raise ValidationError(f"Manifest file not found: {manifest_path}")
    manifest = parse_json_strict(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise ValidationError("Manifest must be a JSON object with schema_version == 1")
    if "golden_traces" not in manifest or "suites" not in manifest:
        raise ValidationError("Manifest must define 'golden_traces' and 'suites'")
    return manifest


def validate_cli_args(args: argparse.Namespace) -> None:
    """Enforce CLI invariants such as --require-clean forbidding --fast and --skip-golden."""
    if args.require_clean and args.fast:
        raise ValidationError("--require-clean forbids --fast (must run full 100-seed standard tier)")
    if args.require_clean and args.skip_golden:
        raise ValidationError("--require-clean forbids --skip-golden (must run golden_traces)")


def build_parser() -> argparse.ArgumentParser:
    parser = argparse.ArgumentParser(
        description="Run and strictly validate Arkhe retained baseline gates."
    )
    parser.add_argument(
        "--manifest",
        type=Path,
        default=DEFAULT_MANIFEST,
        help=f"Path to retained baselines manifest (default: {DEFAULT_MANIFEST})",
    )
    parser.add_argument(
        "--binary",
        type=Path,
        default=DEFAULT_BINARY,
        help=f"Path to arkhe release binary (default: {DEFAULT_BINARY})",
    )
    parser.add_argument(
        "--suite",
        choices=("all", "m2a", "m2b"),
        default="all",
        help="Suite(s) to run and validate (default: all)",
    )
    parser.add_argument(
        "--fast",
        action="store_true",
        help="Use fast tier (seeds 0..20) instead of standard tier (5 banks x 20 seeds)",
    )
    parser.add_argument(
        "--require-clean",
        action="store_true",
        help="Strict mode: forbid --fast and --skip-golden, and require git_dirty == false",
    )
    parser.add_argument(
        "--skip-golden",
        action="store_true",
        help="Skip golden_traces test execution (forbidden under --require-clean)",
    )
    parser.add_argument(
        "--artifacts-dir",
        type=Path,
        default=None,
        help="Optional directory to save raw JSON stdout artifacts per suite",
    )
    parser.add_argument(
        "--json-out",
        type=Path,
        default=None,
        help="Optional path to write structured machine-readable verdict summary JSON",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="Run built-in adversarial validation and CLI guard tests without invoking arkhe",
    )
    return parser


def _make_task(
    label: str,
    regime: str = "clean",
    successes: int = 20,
    trials: int = 20,
    passed: bool = True,
    passes_fwer: bool = True,
    passes_homogeneity: bool = True,
    tail_metrics: dict[str, Any] | None = None,
) -> dict[str, Any]:
    task: dict[str, Any] = {
        "task_label": label,
        "regime": regime,
        "total_successes": successes,
        "total_trials": trials,
        "empirical_rate": successes / trials if trials else 0.0,
        "lower_bound_99": 0.75 if passed else 0.0,
        "binomial_lower_tail_p": 0.9 if passed else 0.0001,
        "fwer_alpha": 0.001,
        "passes_fwer_binomial": passes_fwer,
        "distribution": {
            "passes_homogeneity": passes_homogeneity,
        },
        "passed": passed,
    }
    if tail_metrics is not None:
        task["tail_metrics"] = tail_metrics
    return task


def _make_stratum(
    tasks: list[dict[str, Any]],
    passed: bool = True,
    pooled_passed: bool = True,
    target_rate: float = 0.9,
) -> dict[str, Any]:
    worst = min(tasks, key=lambda t: t["empirical_rate"])
    return {
        "passed": passed,
        "task_count": len(tasks),
        "target_rate": target_rate,
        "fwer_alpha": 0.001,
        "worst_task_label": worst["task_label"],
        "worst_task_rate": worst["empirical_rate"],
        "pooled_multi_bank": {
            "passed": pooled_passed,
            "pooled": {"empirical_rate": sum(t["total_successes"] for t in tasks) / sum(t["total_trials"] for t in tasks)},
            "distribution": {"passes_homogeneity": True},
        },
        "seed_conjunction_multi_bank": None,
        "tasks": tasks,
    }


def _make_valid_m2a_report(git_dirty: bool = False) -> dict[str, Any]:
    gates = []
    for gate_name in ("G2A.1", "G2A.2", "G2A.3", "G2A.5"):
        tasks = [
            _make_task(f"{gate_name}_T1", regime="clean"),
            _make_task(
                f"{gate_name}_T2",
                regime="clean",
                tail_metrics={"p95_steps": 142.5, "max_steps": 310},
            ),
        ]
        gates.append(
            {
                "gate": gate_name,
                "passed": True,
                "excluded_cases": 0,
                "stratified": {
                    "passed": True,
                    "clean": _make_stratum(tasks, passed=True),
                },
            }
        )
    # G2A.4 is noisy-only
    noisy_tasks = [
        _make_task(
            "G2A.4_Noisy_T1",
            regime="noisy",
            tail_metrics={"p99_steps": 500},
        ),
    ]
    gates.append(
        {
            "gate": "G2A.4",
            "passed": True,
            "excluded_cases": 0,
            "stratified": {
                "passed": True,
                "noisy": _make_stratum(noisy_tasks, passed=True),
            },
        }
    )
    return {
        "provenance": {
            "git_commit_sha": "deadbeef" * 5,
            "git_dirty": git_dirty,
            "source_tree_fnv1a128": "0x01",
            "cargo_lock_fnv1a128": "0x02",
            "package_version": "0.1.0",
            "rustc_version": "rustc 1.85.0",
            "target_triple": "x86_64-unknown-linux-gnu",
        },
        "all_passed": True,
        "excluded_cases": 0,
        "cliff_counters": {"sprout_blocked_max_nodes": 0},
        "gates": gates,
    }


def _make_valid_m2b_report(git_dirty: bool = False) -> dict[str, Any]:
    gates = []
    for gate_name in ("G2B.1", "G2B.2", "G2B.3", "G2B.4", "G2B.5"):
        clean_tasks = [
            _make_task(f"{gate_name}_T1", regime="clean"),
            _make_task(
                f"{gate_name}_T2",
                regime="clean",
                tail_metrics={"p95_latency": 12.25},
            ),
        ]
        stratified: dict[str, Any] = {
            "passed": True,
            "clean": _make_stratum(clean_tasks, passed=True),
        }
        if gate_name == "G2B.4":
            stratified["noisy"] = _make_stratum(
                [_make_task("G2B.4_Noisy", regime="noisy")],
                passed=True,
            )
        gates.append(
            {
                "gate": gate_name,
                "passed": True,
                "excluded_cases": 0,
                "stratified": stratified,
            }
        )
    return {
        "provenance": {
            "git_commit_sha": "cafebabe" * 5,
            "git_dirty": git_dirty,
            "source_tree_fnv1a128": "0x03",
            "cargo_lock_fnv1a128": "0x04",
            "package_version": "0.1.0",
            "rustc_version": "rustc 1.85.0",
            "target_triple": "x86_64-unknown-linux-gnu",
        },
        "all_passed": True,
        "excluded_cases": 0,
        "cliff_counters": {"sprout_blocked_max_nodes": 0},
        "gates": gates,
    }


def run_self_test() -> int:
    """Execute built-in adversarial validation and CLI guard tests."""
    m2a_gates = ["G2A.1", "G2A.2", "G2A.3", "G2A.4", "G2A.5"]
    m2b_gates = ["G2B.1", "G2B.2", "G2B.3", "G2B.4", "G2B.5"]
    checks: list[tuple[str, bool, str]] = []

    def expect_pass(name: str, fn: Any) -> None:
        try:
            fn()
            checks.append((name, True, "passed as expected"))
        except Exception as exc:
            checks.append((name, False, f"unexpected failure: {exc}"))

    def expect_reject(name: str, fn: Any, exc_type: type[Exception] = ValidationError) -> None:
        try:
            fn()
            checks.append((name, False, "unexpectedly succeeded (expected rejection)"))
        except exc_type as exc:
            checks.append((name, True, f"rejected as expected: {exc}"))
        except Exception as exc:
            checks.append((name, False, f"wrong exception type {type(exc).__name__}: {exc}"))

    # 1. Valid passing m2a and m2b reports (clean-only, noisy-only, omitted & finite tail_metrics)
    expect_pass(
        "1a. valid m2a (require_clean=False)",
        lambda: validate_suite_report("m2a", _make_valid_m2a_report(git_dirty=False), m2a_gates, False),
    )
    expect_pass(
        "1b. valid m2a (require_clean=True)",
        lambda: validate_suite_report("m2a", _make_valid_m2a_report(git_dirty=False), m2a_gates, True),
    )
    expect_pass(
        "1c. valid m2b (require_clean=False)",
        lambda: validate_suite_report("m2b", _make_valid_m2b_report(git_dirty=False), m2b_gates, False),
    )
    expect_pass(
        "1d. valid m2b (require_clean=True)",
        lambda: validate_suite_report("m2b", _make_valid_m2b_report(git_dirty=False), m2b_gates, True),
    )

    # 2. Concentrated G2B.2 MV_AddEqz_k2_Comb 0/20 failure (other 9 tasks 20/20 -> pooled 180/200 = 0.90)
    def make_concentrated_g2b2_failure(forge_top_level: bool) -> dict[str, Any]:
        rep = _make_valid_m2b_report(git_dirty=False)
        tasks = [_make_task(f"Task_{i}", regime="clean", successes=20, trials=20) for i in range(9)]
        tasks.append(
            _make_task(
                "MV_AddEqz_k2_Comb",
                regime="clean",
                successes=0,
                trials=20,
                passed=False,
                passes_fwer=False,
                passes_homogeneity=False,
            )
        )
        stratum = _make_stratum(tasks, passed=forge_top_level, pooled_passed=True)
        for g in rep["gates"]:
            if g["gate"] == "G2B.2":
                g["passed"] = forge_top_level
                g["stratified"] = {"passed": forge_top_level, "clean": stratum}
        rep["all_passed"] = forge_top_level
        return rep

    expect_reject(
        "2a. concentrated G2B.2 MV_AddEqz_k2_Comb 0/20 failure (unforged)",
        lambda: validate_suite_report("m2b", make_concentrated_g2b2_failure(False), m2b_gates, False),
    )
    expect_reject(
        "2b. concentrated G2B.2 MV_AddEqz_k2_Comb 0/20 failure (forged passed=True)",
        lambda: validate_suite_report("m2b", make_concentrated_g2b2_failure(True), m2b_gates, False),
    )

    # 3. Forged top-level all_passed=True and gate.passed=True with hidden failures
    def forged_stratified_false() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        rep["gates"][0]["stratified"]["passed"] = False
        return rep

    def forged_task_passed_false() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        rep["gates"][0]["stratified"]["clean"]["tasks"][0]["passed"] = False
        return rep

    def forged_task_fwer_false() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        rep["gates"][0]["stratified"]["clean"]["tasks"][0]["passes_fwer_binomial"] = False
        return rep

    def forged_task_homog_false() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        rep["gates"][0]["stratified"]["clean"]["tasks"][0]["distribution"]["passes_homogeneity"] = False
        return rep

    expect_reject(
        "3a. forged all_passed=True & gate.passed=True with stratified.passed=False",
        lambda: validate_suite_report("m2b", forged_stratified_false(), m2b_gates, False),
    )
    expect_reject(
        "3b. forged all_passed=True & gate.passed=True with task.passed=False",
        lambda: validate_suite_report("m2b", forged_task_passed_false(), m2b_gates, False),
    )
    expect_reject(
        "3c. forged all_passed=True & gate.passed=True with passes_fwer_binomial=False",
        lambda: validate_suite_report("m2b", forged_task_fwer_false(), m2b_gates, False),
    )
    expect_reject(
        "3d. forged all_passed=True & gate.passed=True with passes_homogeneity=False",
        lambda: validate_suite_report("m2b", forged_task_homog_false(), m2b_gates, False),
    )

    # 4. provenance.git_dirty=True and CLI --require-clean guards
    expect_pass(
        "4a. git_dirty=True allowed when require_clean=False",
        lambda: validate_suite_report("m2a", _make_valid_m2a_report(git_dirty=True), m2a_gates, False),
    )
    expect_reject(
        "4b. git_dirty=True rejected when require_clean=True",
        lambda: validate_suite_report("m2a", _make_valid_m2a_report(git_dirty=True), m2a_gates, True),
    )
    parser = build_parser()
    expect_reject(
        "4c. CLI guard rejects --require-clean --fast",
        lambda: validate_cli_args(parser.parse_args(["--require-clean", "--fast"])),
    )
    expect_reject(
        "4d. CLI guard rejects --require-clean --skip-golden",
        lambda: validate_cli_args(parser.parse_args(["--require-clean", "--skip-golden"])),
    )

    # 5. cliff_counters.sprout_blocked_max_nodes > 0 or excluded_cases > 0
    def sprout_blocked_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        rep["cliff_counters"]["sprout_blocked_max_nodes"] = 1
        return rep

    def top_excluded_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        rep["excluded_cases"] = 2
        return rep

    def gate_excluded_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        rep["gates"][0]["excluded_cases"] = 1
        return rep

    expect_reject(
        "5a. sprout_blocked_max_nodes > 0 rejected",
        lambda: validate_suite_report("m2a", sprout_blocked_report(), m2a_gates, False),
    )
    expect_reject(
        "5b. top-level excluded_cases > 0 rejected",
        lambda: validate_suite_report("m2a", top_excluded_report(), m2a_gates, False),
    )
    expect_reject(
        "5c. gate-level excluded_cases > 0 rejected",
        lambda: validate_suite_report("m2a", gate_excluded_report(), m2a_gates, False),
    )

    # 6. Missing expected gate (G2B.2 omitted) & duplicate gate & empty stratified
    def missing_g2b2_report() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        rep["gates"] = [g for g in rep["gates"] if g["gate"] != "G2B.2"]
        return rep

    def empty_strata_report() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        rep["gates"][0]["stratified"] = {"passed": True}
        return rep

    expect_reject(
        "6a. missing expected gate G2B.2 rejected",
        lambda: validate_suite_report("m2b", missing_g2b2_report(), m2b_gates, False),
    )
    expect_reject(
        "6b. stratified with neither clean nor noisy rejected",
        lambda: validate_suite_report("m2b", empty_strata_report(), m2b_gates, False),
    )

    # 7. Non-finite / null / bool / NaN in tail_metrics + strict JSON parser rejection
    for label, bad_val in (
        ("NaN", float("nan")),
        ("Inf", float("inf")),
        ("null", None),
        ("bool", True),
    ):
        def make_bad_tail(val: Any = bad_val) -> dict[str, Any]:
            rep = copy.deepcopy(_make_valid_m2a_report())
            rep["gates"][0]["stratified"]["clean"]["tasks"][0]["tail_metrics"] = {"p99": val}
            return rep

        expect_reject(
            f"7. tail_metrics with {label} rejected",
            lambda fn=make_bad_tail: validate_suite_report("m2a", fn(), m2a_gates, False),
        )

    expect_reject(
        "7e. strict JSON parser rejects NaN literal",
        lambda: parse_json_strict('{"x": NaN}'),
        exc_type=ValueError,
    )

    # Verify manifest exists & loads cleanly alongside self-test if present
    if DEFAULT_MANIFEST.is_file():
        expect_pass("8. default manifest loads cleanly", lambda: load_manifest(DEFAULT_MANIFEST))

    # Also test main() end-to-end rejection via temp manifest? No arkhe invocation needed;
    # check that main(["--require-clean", "--fast"]) returns non-zero without running arkhe.
    def test_main_cli_guard() -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            json_out = Path(tmpdir) / "verdict.json"
            with contextlib.redirect_stderr(io.StringIO()):
                rc = main(["--require-clean", "--fast", "--json-out", str(json_out)])
            if rc == 0:
                raise AssertionError("Expected non-zero exit for --require-clean --fast")
            if not json_out.is_file():
                raise AssertionError("Expected --json-out verdict file on CLI guard failure")

    expect_pass("9. main() rejects --require-clean --fast with exit != 0", test_main_cli_guard)

    passed_count = sum(1 for _, ok, _ in checks if ok)
    total_count = len(checks)
    for name, ok, detail in checks:
        status = "PASS" if ok else "FAIL"
        print(f"[{status}] {name}: {detail}")
    print(f"Self-test summary: {passed_count}/{total_count} passed")
    return 0 if passed_count == total_count else 1


def _write_json_out(path: Path | None, payload: dict[str, Any]) -> None:
    if path is None:
        return
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(payload, indent=2) + "\n", encoding="utf-8")


def main(argv: list[str] | None = None) -> int:
    parser = build_parser()
    args = parser.parse_args(argv)

    if args.self_test:
        return run_self_test()

    tier = "fast" if args.fast else "standard"
    verdict: dict[str, Any] = {
        "passed": False,
        "tier": tier,
        "require_clean": bool(args.require_clean),
        "golden_traces": None,
        "suites": {},
        "errors": [],
    }

    try:
        validate_cli_args(args)
        manifest = load_manifest(args.manifest)
    except Exception as exc:
        verdict["errors"].append(str(exc))
        _write_json_out(args.json_out, verdict)
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    golden_cfg = manifest["golden_traces"]
    golden_cmd = list(golden_cfg["command"])
    if args.skip_golden:
        verdict["golden_traces"] = {
            "skipped": True,
            "command": golden_cmd,
            "passed": None,
        }
        print("[SKIP] golden_traces (--skip-golden)")
    else:
        print(f"[RUN] golden_traces: {' '.join(golden_cmd)}")
        proc = subprocess.run(golden_cmd, cwd=REPO_ROOT, capture_output=True, text=True)
        golden_ok = proc.returncode == 0
        verdict["golden_traces"] = {
            "skipped": False,
            "command": golden_cmd,
            "passed": golden_ok,
            "returncode": proc.returncode,
        }
        if not golden_ok:
            msg = (
                f"golden_traces failed with exit code {proc.returncode}\n"
                f"stdout:\n{proc.stdout}\nstderr:\n{proc.stderr}"
            )
            verdict["errors"].append(msg)
            _write_json_out(args.json_out, verdict)
            print(f"ERROR: {msg}", file=sys.stderr)
            return 1
        print("[PASS] golden_traces")

    if not args.binary.is_file():
        msg = f"Arkhe binary does not exist: {args.binary}"
        verdict["errors"].append(msg)
        _write_json_out(args.json_out, verdict)
        print(f"ERROR: {msg}", file=sys.stderr)
        return 1

    selected_suites = ["m2a", "m2b"] if args.suite == "all" else [args.suite]
    requirements = manifest.get("requirements", {})
    all_suites_ok = True

    if args.artifacts_dir is not None:
        args.artifacts_dir.mkdir(parents=True, exist_ok=True)

    for suite_name in selected_suites:
        suite_cfg = manifest["suites"][suite_name]
        tier_cfg = suite_cfg["tiers"][tier]
        cmd = [str(args.binary), suite_cfg["subcommand"], *tier_cfg["args"]]
        print(f"[RUN] {suite_name} ({tier}): {' '.join(cmd)}")
        proc = subprocess.run(cmd, cwd=REPO_ROOT, capture_output=True, text=True)
        if proc.returncode != 0:
            all_suites_ok = False
            msg = (
                f"{suite_name} failed with exit code {proc.returncode}: "
                f"{proc.stderr.strip() or proc.stdout.strip()}"
            )
            verdict["errors"].append(msg)
            verdict["suites"][suite_name] = {
                "suite": suite_name,
                "passed": False,
                "error": msg,
            }
            print(f"[FAIL] {suite_name}: {msg}", file=sys.stderr)
            continue

        if args.artifacts_dir is not None:
            artifact_path = args.artifacts_dir / tier_cfg["artifact_name"]
            artifact_path.write_text(proc.stdout, encoding="utf-8")

        try:
            report = parse_json_strict(proc.stdout)
            suite_summary = validate_suite_report(
                suite_name=suite_name,
                report=report,
                expected_gates=list(suite_cfg["expected_gates"]),
                require_clean=bool(args.require_clean),
                requirements=requirements,
            )
            verdict["suites"][suite_name] = suite_summary
            print(f"[PASS] {suite_name} ({len(suite_summary['gates'])} gates validated)")
        except Exception as exc:
            all_suites_ok = False
            msg = f"{suite_name} validation failed: {exc}"
            verdict["errors"].append(msg)
            verdict["suites"][suite_name] = {
                "suite": suite_name,
                "passed": False,
                "error": str(exc),
            }
            print(f"[FAIL] {suite_name}: {exc}", file=sys.stderr)

    verdict["passed"] = all_suites_ok
    _write_json_out(args.json_out, verdict)
    return 0 if verdict["passed"] else 1


if __name__ == "__main__":
    sys.exit(main())
