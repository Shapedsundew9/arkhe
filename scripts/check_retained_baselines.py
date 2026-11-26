#!/usr/bin/env python3
"""Mechanical gate for Arkhe retained baselines (golden_traces, verify-m2a, verify-m2b, verify-m4a)."""

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

FNV1A_128_OFFSET_BASIS = 0x6C62272E07BB014262B821756295C58D
FNV1A_128_PRIME = 0x0000000001000000000000000000013B
FNV1A_128_MASK = (1 << 128) - 1


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


def fnv1a_128_hex(data: bytes) -> str:
    """Compute the 128-bit FNV-1a lowercase hex digest matching `scripts/build.rs`."""
    h = FNV1A_128_OFFSET_BASIS
    for b in data:
        h ^= b
        h = (h * FNV1A_128_PRIME) & FNV1A_128_MASK
    return f"{h:032x}"


def _collect_rust_sources(root: Path, directory: Path, files: list[Path]) -> None:
    if not directory.is_dir():
        return
    for entry in directory.iterdir():
        if entry.is_dir():
            _collect_rust_sources(root, entry, files)
        elif entry.suffix == ".rs":
            files.append(entry.relative_to(root))


def source_tree_fnv1a128(repo_root: Path = REPO_ROOT) -> str:
    """Compute the canonical source-tree FNV-1a-128 digest matching `scripts/build.rs`."""
    paths = [
        Path("Cargo.toml"),
        Path("Cargo.lock"),
        Path("scripts/build.rs"),
    ]
    _collect_rust_sources(repo_root, repo_root / "src", paths)
    # Match Rust `PathBuf::cmp` (component-wise lexicographical order).
    paths.sort(key=lambda p: p.parts)

    buf = bytearray()
    for rel in paths:
        full_path = repo_root / rel
        if not full_path.is_file():
            raise ValidationError(f"Missing source-tree hash input: {full_path}")
        buf.extend(rel.as_posix().encode("utf-8"))
        buf.append(0)
        buf.extend(full_path.read_bytes())
        buf.append(0)
    return fnv1a_128_hex(bytes(buf))


def cargo_lock_fnv1a128(repo_root: Path = REPO_ROOT) -> str:
    """Compute the `Cargo.lock` FNV-1a-128 digest matching `scripts/build.rs`."""
    lock_path = repo_root / "Cargo.lock"
    if not lock_path.is_file():
        return "unavailable"
    return fnv1a_128_hex(lock_path.read_bytes())


def collect_live_provenance(repo_root: Path = REPO_ROOT) -> dict[str, Any]:
    """Capture live git revision, worktree cleanliness, and source/lock digests."""
    sha_proc = subprocess.run(
        ["git", "rev-parse", "HEAD"],
        cwd=repo_root,
        capture_output=True,
        text=True,
    )
    if sha_proc.returncode != 0 or not sha_proc.stdout.strip():
        raise ValidationError(
            f"Failed to read git HEAD in {repo_root}: {sha_proc.stderr.strip()}"
        )

    status_proc = subprocess.run(
        ["git", "status", "--porcelain"],
        cwd=repo_root,
        capture_output=True,
        text=True,
    )
    if status_proc.returncode != 0:
        raise ValidationError(
            f"Failed to read git status in {repo_root}: {status_proc.stderr.strip()}"
        )

    return {
        "git_commit_sha": sha_proc.stdout.strip(),
        "git_dirty": bool(status_proc.stdout.strip()),
        "source_tree_fnv1a128": source_tree_fnv1a128(repo_root),
        "cargo_lock_fnv1a128": cargo_lock_fnv1a128(repo_root),
    }


def binomial_lower_tail_p_value(k: int, n: int, p: float) -> float:
    """Compute exact Pr[Bin(n, p) <= k] in log-space without float overflow."""
    if n <= 0 or k >= n or p <= 0.0:
        return 1.0
    if k < 0:
        return 0.0
    if p >= 1.0:
        return 0.0
    log_p = math.log(p)
    log_q = math.log1p(-p)
    lgamma_n1 = math.lgamma(n + 1)
    total = 0.0
    for j in range(k + 1):
        log_pmf = (
            lgamma_n1
            - math.lgamma(j + 1)
            - math.lgamma(n - j + 1)
            + j * log_p
            + (n - j) * log_q
        )
        total += math.exp(log_pmf)
    return min(1.0, max(0.0, total))


def clopper_pearson_lower_bound(k: int, n: int, confidence: float = 0.99) -> float:
    """Compute one-sided Clopper-Pearson lower confidence bound matching `src/stats.rs`."""
    if n <= 0 or k <= 0:
        return 0.0
    alpha = min(1.0, max(0.0, 1.0 - confidence))
    if k >= n:
        return alpha ** (1.0 / n)
    low = 0.0
    high = 1.0
    for _ in range(64):
        mid = 0.5 * (low + high)
        p_ge_k = 1.0 - binomial_lower_tail_p_value(k - 1, n, mid)
        if p_ge_k < alpha:
            low = mid
        else:
            high = mid
    return 0.5 * (low + high)


def _resolve_required_tail_keys(
    required_tail_metrics: dict[str, Any] | None,
    suite_name: str,
    gate_name: str,
    task_label: str,
    context: str,
    errors: list[str],
) -> list[str]:
    if required_tail_metrics is None:
        return []

    suite_map = required_tail_metrics.get(suite_name)
    if isinstance(suite_map, dict):
        gate_map = suite_map
    else:
        gate_map = required_tail_metrics

    if gate_name not in gate_map:
        return []

    spec = gate_map[gate_name]
    if isinstance(spec, list):
        return [k for k in spec if isinstance(k, str) and k]

    if isinstance(spec, dict):
        prefixes = spec.get("prefixes")
        if isinstance(prefixes, dict):
            matching = [p for p in prefixes if isinstance(p, str) and task_label.startswith(p)]
            if matching:
                best_prefix = max(matching, key=len)
                keys = prefixes[best_prefix]
                if isinstance(keys, list):
                    return [k for k in keys if isinstance(k, str) and k]
                errors.append(
                    f"{context}: invalid required_tail_metrics prefix list for {best_prefix!r}"
                )
                return []

        default_keys = spec.get("default")
        if isinstance(default_keys, list):
            return [k for k in default_keys if isinstance(k, str) and k]

        errors.append(
            f"{context}: no required_tail_metrics rule matched task_label {task_label!r} "
            f"for gate {gate_name!r}"
        )
        return []

    errors.append(f"{context}: invalid required_tail_metrics spec for gate {gate_name!r}")
    return []


def _validate_tail_metrics(
    tail_metrics: Any,
    context: str,
    require_finite: bool,
    min_tail_metrics_per_task: int,
    required_keys: list[str],
    errors: list[str],
) -> None:
    if not isinstance(tail_metrics, dict):
        errors.append(f"{context}: tail_metrics must be a dict, got {type(tail_metrics).__name__}")
        return
    if len(tail_metrics) == 0 or len(tail_metrics) < min_tail_metrics_per_task:
        errors.append(
            f"{context}: tail_metrics must contain at least {max(1, min_tail_metrics_per_task)} "
            f"metric(s), got {len(tail_metrics)}"
        )

    for metric_name, metric_val in tail_metrics.items():
        if not isinstance(metric_name, str) or not metric_name:
            errors.append(f"{context}: tail_metrics key must be non-empty str")
            continue
        if require_finite and not _is_finite_number(metric_val):
            errors.append(
                f"{context}: tail_metrics[{metric_name!r}] must be a finite int|float "
                f"(not bool/None/NaN/Inf), got {metric_val!r}"
            )

    for req_key in required_keys:
        if req_key not in tail_metrics:
            errors.append(
                f"{context}: tail_metrics missing required key {req_key!r}"
            )


def _validate_multi_bank(
    mb: Any,
    context: str,
    target_rate: float,
    enforce_pooled_cp: bool,
    require_passed: bool,
    errors: list[str],
) -> tuple[int | None, int | None]:
    if not isinstance(mb, dict):
        errors.append(f"{context}: must be a dict, got {type(mb).__name__}")
        return None, None

    pooled = mb.get("pooled")
    total_trials: int | None = None
    total_successes: int | None = None
    pooled_rate: float | None = None
    passes_pooled: bool | None = None

    if not isinstance(pooled, dict):
        errors.append(f"{context}.pooled: must be a dict")
    else:
        total_trials = pooled.get("total_trials")
        total_successes = pooled.get("total_successes")
        if not _is_real_int(total_trials) or total_trials <= 0:
            errors.append(f"{context}.pooled: total_trials must be int > 0, got {total_trials!r}")
            total_trials = None
        if not _is_real_int(total_successes) or total_successes < 0:
            errors.append(
                f"{context}.pooled: total_successes must be int >= 0, got {total_successes!r}"
            )
            total_successes = None
        elif total_trials is not None and total_successes > total_trials:
            errors.append(
                f"{context}.pooled: total_successes ({total_successes}) > total_trials ({total_trials})"
            )

        pooled_rate_val = pooled.get("pooled_rate")
        if not _is_finite_number(pooled_rate_val):
            errors.append(f"{context}.pooled: pooled_rate must be finite, got {pooled_rate_val!r}")
        else:
            pooled_rate = float(pooled_rate_val)
            if total_trials is not None and total_successes is not None and total_successes <= total_trials:
                expected_rate = total_successes / total_trials
                if abs(pooled_rate - expected_rate) > 1e-9:
                    errors.append(
                        f"{context}.pooled: pooled_rate ({pooled_rate}) != "
                        f"total_successes/total_trials ({expected_rate})"
                    )

        mb_target = pooled.get("target_rate")
        if not _is_finite_number(mb_target):
            errors.append(f"{context}.pooled: target_rate must be finite, got {mb_target!r}")
        elif abs(float(mb_target) - target_rate) > 1e-12:
            errors.append(
                f"{context}.pooled: target_rate ({mb_target}) != stratum target_rate ({target_rate})"
            )

        lb99_val = pooled.get("lower_bound_99")
        passes_pooled_val = pooled.get("passes_pooled")
        if not isinstance(passes_pooled_val, bool):
            errors.append(
                f"{context}.pooled: passes_pooled must be bool, got {passes_pooled_val!r}"
            )
        else:
            passes_pooled = passes_pooled_val

        if not _is_finite_number(lb99_val) or not (0.0 <= float(lb99_val) <= 1.0):
            errors.append(
                f"{context}.pooled: lower_bound_99 must be finite in [0, 1], got {lb99_val!r}"
            )
        else:
            lb99 = float(lb99_val)
            if total_trials is not None and total_successes is not None and total_successes <= total_trials:
                expected_lb99 = clopper_pearson_lower_bound(total_successes, total_trials, 0.99)
                if abs(lb99 - expected_lb99) > 1e-6:
                    errors.append(
                        f"{context}.pooled: lower_bound_99 ({lb99}) disagrees with exact "
                        f"99% Clopper-Pearson bound ({expected_lb99})"
                    )
            if passes_pooled is not None:
                if passes_pooled and lb99 < target_rate - 1e-12:
                    errors.append(
                        f"{context}.pooled: passes_pooled=True inconsistent with "
                        f"lower_bound_99 ({lb99}) < target_rate ({target_rate})"
                    )
                if not passes_pooled and lb99 >= target_rate + 1e-12:
                    errors.append(
                        f"{context}.pooled: passes_pooled=False inconsistent with "
                        f"lower_bound_99 ({lb99}) >= target_rate ({target_rate})"
                    )

    dist = mb.get("distribution")
    passes_homog: bool | None = None
    if not isinstance(dist, dict):
        errors.append(f"{context}.distribution: must be a dict")
    else:
        bank_count = dist.get("bank_count")
        p_val = dist.get("p_value")
        homog_val = dist.get("passes_homogeneity")

        if not _is_real_int(bank_count) or bank_count < 1:
            errors.append(
                f"{context}.distribution: bank_count must be int >= 1, got {bank_count!r}"
            )
            bank_count = None

        if not _is_finite_number(p_val) or not (0.0 <= float(p_val) <= 1.0):
            errors.append(
                f"{context}.distribution: p_value must be finite in [0, 1], got {p_val!r}"
            )
            p_num = None
        else:
            p_num = float(p_val)

        if not isinstance(homog_val, bool):
            errors.append(
                f"{context}.distribution: passes_homogeneity must be bool, got {homog_val!r}"
            )
        else:
            passes_homog = homog_val

        if bank_count is not None and p_num is not None and passes_homog is not None:
            if bank_count == 1:
                if abs(p_num - 1.0) > 1e-12 or passes_homog is not True:
                    errors.append(
                        f"{context}.distribution: single-bank must have p_value=1.0 and "
                        f"passes_homogeneity=True, got p_value={p_num}, passes_homogeneity={passes_homog}"
                    )
            else:
                if passes_homog and p_num < 0.01 - 1e-12:
                    errors.append(
                        f"{context}.distribution: passes_homogeneity=True inconsistent with "
                        f"p_value ({p_num}) < 0.01"
                    )
                if not passes_homog and p_num >= 0.01 + 1e-12:
                    errors.append(
                        f"{context}.distribution: passes_homogeneity=False inconsistent with "
                        f"p_value ({p_num}) >= 0.01"
                    )

    mb_passed = mb.get("passed")
    if not isinstance(mb_passed, bool):
        errors.append(f"{context}: passed must be bool, got {mb_passed!r}")
    elif passes_pooled is not None and passes_homog is not None:
        if mb_passed != (passes_pooled and passes_homog):
            errors.append(
                f"{context}: passed ({mb_passed}) != (passes_pooled ({passes_pooled}) "
                f"and passes_homogeneity ({passes_homog}))"
            )

    if require_passed:
        if target_rate >= 1.0:
            if total_trials is None or total_successes is None or total_successes != total_trials:
                errors.append(
                    f"{context}: exact mode (target_rate={target_rate}) requires "
                    f"total_successes == total_trials, got {total_successes}/{total_trials}"
                )
        elif enforce_pooled_cp:
            if mb_passed is not True:
                errors.append(f"{context}: passed must be True under enforce_pooled_cp=True, got {mb_passed!r}")
        else:
            if pooled_rate is None or pooled_rate < target_rate - 1e-12:
                errors.append(
                    f"{context}: fast-tier pooled_rate ({pooled_rate}) < target_rate ({target_rate})"
                )
            if passes_homog is not True:
                errors.append(
                    f"{context}: distribution.passes_homogeneity must be True, got {passes_homog!r}"
                )

    return total_trials, total_successes


def _validate_task(
    task: Any,
    suite_name: str,
    gate_name: str,
    stratum_name: str,
    idx: int,
    target_rate: float,
    expected_fwer_alpha: float,
    require_per_task_passed: bool,
    require_finite_tail_metrics: bool,
    min_tail_metrics_per_task: int,
    required_tail_metrics: dict[str, Any] | None,
    errors: list[str],
) -> tuple[str | None, int, int, float | None]:
    ctx = f"{suite_name}.{gate_name}.{stratum_name}.tasks[{idx}]"
    if not isinstance(task, dict):
        errors.append(f"{ctx}: task must be a dict")
        return None, 0, 0, None

    label = task.get("task_label")
    if isinstance(label, str) and label:
        ctx = f"{suite_name}.{gate_name}.{stratum_name}.task[{label}]"
    else:
        errors.append(f"{ctx}: missing or invalid task_label")
        label = None

    total_trials = task.get("total_trials")
    total_successes = task.get("total_successes")
    trials_ok = _is_real_int(total_trials) and total_trials > 0
    succ_ok = _is_real_int(total_successes) and total_successes >= 0

    if not trials_ok:
        errors.append(f"{ctx}: total_trials must be int > 0, got {total_trials!r}")
    if not succ_ok:
        errors.append(f"{ctx}: total_successes must be int >= 0, got {total_successes!r}")
    elif trials_ok and total_successes > total_trials:
        errors.append(
            f"{ctx}: total_successes ({total_successes}) exceeds total_trials ({total_trials})"
        )

    valid_counts = trials_ok and succ_ok and total_successes <= total_trials
    true_rate = (total_successes / total_trials) if valid_counts else None

    emp_rate = task.get("empirical_rate")
    if not _is_finite_number(emp_rate):
        errors.append(f"{ctx}: empirical_rate must be finite, got {emp_rate!r}")
    elif true_rate is not None and abs(float(emp_rate) - true_rate) > 1e-9:
        errors.append(
            f"{ctx}: empirical_rate ({emp_rate}) != total_successes/total_trials ({true_rate})"
        )

    task_alpha = task.get("fwer_alpha")
    if not _is_finite_number(task_alpha):
        errors.append(f"{ctx}: fwer_alpha must be finite, got {task_alpha!r}")
    elif abs(float(task_alpha) - expected_fwer_alpha) > 1e-12:
        errors.append(
            f"{ctx}: fwer_alpha ({task_alpha}) != 0.01/task_count ({expected_fwer_alpha})"
        )

    lb99 = task.get("lower_bound_99")
    if not _is_finite_number(lb99) or not (0.0 <= float(lb99) <= 1.0):
        errors.append(f"{ctx}: lower_bound_99 must be finite in [0, 1], got {lb99!r}")
    elif valid_counts:
        expected_lb99 = clopper_pearson_lower_bound(total_successes, total_trials, 0.99)
        if abs(float(lb99) - expected_lb99) > 1e-6:
            errors.append(
                f"{ctx}: lower_bound_99 ({lb99}) disagrees with exact 99% Clopper-Pearson "
                f"bound ({expected_lb99})"
            )

    binom_p_raw = task.get("binomial_lower_tail_p")
    expected_p: float | None = None
    if valid_counts:
        expected_p = binomial_lower_tail_p_value(total_successes, total_trials, target_rate)
    if not _is_finite_number(binom_p_raw) or not (0.0 <= float(binom_p_raw) <= 1.0):
        errors.append(
            f"{ctx}: binomial_lower_tail_p must be finite in [0, 1], got {binom_p_raw!r}"
        )
    elif expected_p is not None and abs(float(binom_p_raw) - expected_p) > 1e-6:
        errors.append(
            f"{ctx}: binomial_lower_tail_p ({binom_p_raw}) != exact binomial lower-tail p "
            f"({expected_p})"
        )

    passes_fwer = task.get("passes_fwer_binomial")
    if not isinstance(passes_fwer, bool):
        errors.append(
            f"{ctx}: passes_fwer_binomial must be bool, got {passes_fwer!r}"
        )
        passes_fwer = None
    elif valid_counts and expected_p is not None:
        if target_rate >= 1.0:
            expected_fwer_ok = total_successes == total_trials
            if passes_fwer != expected_fwer_ok:
                errors.append(
                    f"{ctx}: passes_fwer_binomial ({passes_fwer}) inconsistent with exact mode "
                    f"{total_successes}/{total_trials}"
                )
        else:
            if passes_fwer and expected_p < expected_fwer_alpha - 1e-12:
                errors.append(
                    f"{ctx}: passes_fwer_binomial=True inconsistent with exact p ({expected_p}) "
                    f"< fwer_alpha ({expected_fwer_alpha})"
                )
            if not passes_fwer and expected_p >= expected_fwer_alpha + 1e-12:
                errors.append(
                    f"{ctx}: passes_fwer_binomial=False inconsistent with exact p ({expected_p}) "
                    f">= fwer_alpha ({expected_fwer_alpha})"
                )

    dist = task.get("distribution")
    passes_homog: bool | None = None
    bank_count: int | None = None
    dist_p_num: float | None = None
    if not isinstance(dist, dict):
        errors.append(f"{ctx}: distribution must be a dict")
    else:
        bc_raw = dist.get("bank_count")
        if not _is_real_int(bc_raw) or bc_raw < 1:
            errors.append(f"{ctx}: distribution.bank_count must be int >= 1, got {bc_raw!r}")
        else:
            bank_count = bc_raw

        dp_raw = dist.get("p_value")
        if not _is_finite_number(dp_raw) or not (0.0 <= float(dp_raw) <= 1.0):
            errors.append(
                f"{ctx}: distribution.p_value must be finite in [0, 1], got {dp_raw!r}"
            )
        else:
            dist_p_num = float(dp_raw)

        homog_raw = dist.get("passes_homogeneity")
        if not isinstance(homog_raw, bool):
            errors.append(
                f"{ctx}: distribution.passes_homogeneity must be bool, got {homog_raw!r}"
            )
        else:
            passes_homog = homog_raw

        if bank_count is not None and dist_p_num is not None and passes_homog is not None:
            if bank_count == 1:
                if abs(dist_p_num - 1.0) > 1e-12 or passes_homog is not True:
                    errors.append(
                        f"{ctx}: single-bank distribution must have p_value=1.0 and "
                        f"passes_homogeneity=True, got p_value={dist_p_num}, passes_homogeneity={passes_homog}"
                    )
            else:
                if passes_homog and dist_p_num < expected_fwer_alpha - 1e-12:
                    errors.append(
                        f"{ctx}: distribution.passes_homogeneity=True inconsistent with "
                        f"p_value ({dist_p_num}) < fwer_alpha ({expected_fwer_alpha})"
                    )
                if not passes_homog and dist_p_num >= expected_fwer_alpha + 1e-12:
                    errors.append(
                        f"{ctx}: distribution.passes_homogeneity=False inconsistent with "
                        f"p_value ({dist_p_num}) >= fwer_alpha ({expected_fwer_alpha})"
                    )

    task_passed = task.get("passed")
    if not isinstance(task_passed, bool):
        errors.append(f"{ctx}: passed must be bool, got {task_passed!r}")
        task_passed = None
    elif passes_fwer is not None and passes_homog is not None:
        if task_passed != (passes_fwer and passes_homog):
            errors.append(
                f"{ctx}: passed ({task_passed}) != (passes_fwer_binomial ({passes_fwer}) "
                f"and passes_homogeneity ({passes_homog}))"
            )

    if require_per_task_passed:
        if task_passed is not True:
            errors.append(f"{ctx}: passed must be True, got {task.get('passed')!r}")
        if passes_fwer is not True:
            errors.append(
                f"{ctx}: passes_fwer_binomial must be True, got {task.get('passes_fwer_binomial')!r}"
            )
        if passes_homog is not True:
            errors.append(
                f"{ctx}: distribution.passes_homogeneity must be True, "
                f"got {dist.get('passes_homogeneity') if isinstance(dist, dict) else None!r}"
            )
        if valid_counts and expected_p is not None:
            if target_rate >= 1.0:
                if total_successes != total_trials:
                    errors.append(
                        f"{ctx}: exact mode requires total_successes == total_trials, "
                        f"got {total_successes}/{total_trials}"
                    )
            elif expected_p < expected_fwer_alpha - 1e-12:
                errors.append(
                    f"{ctx}: exact binomial lower-tail p ({expected_p}) < fwer_alpha ({expected_fwer_alpha})"
                )
        if bank_count is not None and bank_count > 1 and dist_p_num is not None:
            if dist_p_num < expected_fwer_alpha - 1e-12:
                errors.append(
                    f"{ctx}: multi-bank distribution.p_value ({dist_p_num}) < fwer_alpha ({expected_fwer_alpha})"
                )

    if "tail_metrics" not in task:
        errors.append(f"{ctx}: missing required tail_metrics")
    else:
        req_keys = (
            _resolve_required_tail_keys(
                required_tail_metrics,
                suite_name,
                gate_name,
                label or "",
                ctx,
                errors,
            )
            if label is not None
            else []
        )
        _validate_tail_metrics(
            task["tail_metrics"],
            ctx,
            require_finite_tail_metrics,
            min_tail_metrics_per_task,
            req_keys,
            errors,
        )

    return (
        label,
        total_successes if valid_counts else 0,
        total_trials if valid_counts else 0,
        true_rate,
    )


def _validate_stratum(
    stratum: Any,
    stratum_name: str,
    suite_name: str,
    gate_name: str,
    enforce_pooled_cp: bool,
    require_stratified_passed: bool,
    require_per_task_passed: bool,
    require_finite_tail_metrics: bool,
    min_tail_metrics_per_task: int,
    required_tail_metrics: dict[str, Any] | None,
    errors: list[str],
) -> dict[str, Any]:
    ctx = f"{suite_name}.{gate_name}.stratified.{stratum_name}"
    if not isinstance(stratum, dict):
        errors.append(f"{ctx}: stratum must be a dict")
        return {}

    if require_stratified_passed and stratum.get("passed") is not True:
        errors.append(f"{ctx}: passed must be True, got {stratum.get('passed')!r}")

    tasks = stratum.get("tasks")
    if not isinstance(tasks, list) or len(tasks) == 0:
        errors.append(f"{ctx}: tasks must be a non-empty list")
        return {}

    task_count = stratum.get("task_count")
    if not _is_real_int(task_count) or task_count != len(tasks):
        errors.append(
            f"{ctx}: task_count ({task_count!r}) must equal len(tasks) ({len(tasks)})"
        )
        effective_task_count = len(tasks)
    else:
        effective_task_count = task_count

    target_rate_raw = stratum.get("target_rate")
    if not _is_finite_number(target_rate_raw) or not (0.0 < float(target_rate_raw) <= 1.0):
        errors.append(
            f"{ctx}: target_rate must be finite in (0, 1], got {target_rate_raw!r}"
        )
        return {}
    target_rate = float(target_rate_raw)

    expected_fwer_alpha = 0.01 / effective_task_count
    stratum_alpha = stratum.get("fwer_alpha")
    if not _is_finite_number(stratum_alpha):
        errors.append(f"{ctx}: fwer_alpha must be finite, got {stratum_alpha!r}")
    elif abs(float(stratum_alpha) - expected_fwer_alpha) > 1e-12:
        errors.append(
            f"{ctx}: fwer_alpha ({stratum_alpha}) != 0.01/task_count ({expected_fwer_alpha})"
        )

    # Validate each task and accumulate totals for pooled_multi_bank cross-check.
    sum_successes = 0
    sum_trials = 0
    task_rates: list[tuple[str, float]] = []
    for idx, task in enumerate(tasks):
        t_label, t_succ, t_trials, t_rate = _validate_task(
            task=task,
            suite_name=suite_name,
            gate_name=gate_name,
            stratum_name=stratum_name,
            idx=idx,
            target_rate=target_rate,
            expected_fwer_alpha=expected_fwer_alpha,
            require_per_task_passed=require_per_task_passed,
            require_finite_tail_metrics=require_finite_tail_metrics,
            min_tail_metrics_per_task=min_tail_metrics_per_task,
            required_tail_metrics=required_tail_metrics,
            errors=errors,
        )
        sum_successes += t_succ
        sum_trials += t_trials
        if t_label is not None and t_rate is not None:
            task_rates.append((t_label, t_rate))

    # Require pooled_multi_bank and verify against task sums.
    pooled_mb = stratum.get("pooled_multi_bank")
    if pooled_mb is None:
        errors.append(f"{ctx}: missing required pooled_multi_bank")
    else:
        mb_trials, mb_succ = _validate_multi_bank(
            pooled_mb,
            f"{ctx}.pooled_multi_bank",
            target_rate=target_rate,
            enforce_pooled_cp=enforce_pooled_cp,
            require_passed=require_stratified_passed,
            errors=errors,
        )
        if mb_trials is not None and sum_trials > 0 and mb_trials != sum_trials:
            errors.append(
                f"{ctx}.pooled_multi_bank: total_trials ({mb_trials}) != sum of task trials ({sum_trials})"
            )
        if mb_succ is not None and sum_trials > 0 and mb_succ != sum_successes:
            errors.append(
                f"{ctx}.pooled_multi_bank: total_successes ({mb_succ}) != sum of task successes ({sum_successes})"
            )

    # When stratum is clean, require seed_conjunction_multi_bank.
    conj_mb = stratum.get("seed_conjunction_multi_bank")
    if stratum_name == "clean":
        if conj_mb is None:
            errors.append(f"{ctx}: clean stratum requires non-null seed_conjunction_multi_bank")
        else:
            _validate_multi_bank(
                conj_mb,
                f"{ctx}.seed_conjunction_multi_bank",
                target_rate=target_rate,
                enforce_pooled_cp=enforce_pooled_cp,
                require_passed=require_stratified_passed,
                errors=errors,
            )
    elif conj_mb is not None:
        _validate_multi_bank(
            conj_mb,
            f"{ctx}.seed_conjunction_multi_bank",
            target_rate=target_rate,
            enforce_pooled_cp=enforce_pooled_cp,
            require_passed=require_stratified_passed,
            errors=errors,
        )

    # Validate worst_task_label and worst_task_rate against true minimum across tasks.
    worst_label = stratum.get("worst_task_label")
    worst_rate = stratum.get("worst_task_rate")
    if not isinstance(worst_label, str) or not worst_label:
        errors.append(f"{ctx}: worst_task_label must be a non-empty str, got {worst_label!r}")
    if not _is_finite_number(worst_rate):
        errors.append(f"{ctx}: worst_task_rate must be finite, got {worst_rate!r}")
    elif task_rates:
        true_min_rate = min(r for _, r in task_rates)
        if abs(float(worst_rate) - true_min_rate) > 1e-9:
            errors.append(
                f"{ctx}: worst_task_rate ({worst_rate}) != true min task rate ({true_min_rate})"
            )
        if isinstance(worst_label, str) and worst_label:
            matching_rates = [r for lbl, r in task_rates if lbl == worst_label]
            if not matching_rates:
                errors.append(
                    f"{ctx}: worst_task_label ({worst_label!r}) not found in tasks"
                )
            elif abs(matching_rates[0] - true_min_rate) > 1e-9:
                errors.append(
                    f"{ctx}: worst_task_label ({worst_label!r}) has rate {matching_rates[0]} "
                    f"!= true min rate {true_min_rate}"
                )
            else:
                tied_labels = [lbl for lbl, r in task_rates if abs(r - true_min_rate) <= 2.3e-16]
                if tied_labels and worst_label != min(tied_labels):
                    errors.append(
                        f"{ctx}: worst_task_label ({worst_label!r}) != lexicographic min "
                        f"tied task ({min(tied_labels)!r})"
                    )

    return {
        "passed": stratum.get("passed"),
        "task_count": task_count,
        "target_rate": target_rate,
        "fwer_alpha": stratum.get("fwer_alpha"),
        "worst_task_label": worst_label,
        "worst_task_rate": worst_rate,
    }


def _validate_provenance(
    suite_name: str,
    provenance: Any,
    require_clean: bool,
    expected_provenance: dict[str, Any] | None,
    errors: list[str],
) -> None:
    if require_clean:
        if expected_provenance is None:
            errors.append(
                f"{suite_name}: expected_provenance is required under --require-clean"
            )
            return
        if expected_provenance.get("git_dirty") is not False:
            errors.append(
                f"{suite_name}: live worktree must be clean under --require-clean "
                f"(git_dirty={expected_provenance.get('git_dirty')!r})"
            )
        if not isinstance(provenance, dict):
            errors.append(f"{suite_name}: provenance must be a dict under --require-clean")
            return
        if provenance.get("git_dirty") is not False:
            errors.append(
                f"{suite_name}: provenance.git_dirty must be False under --require-clean, "
                f"got {provenance.get('git_dirty')!r}"
            )
    elif provenance is not None and not isinstance(provenance, dict):
        errors.append(f"{suite_name}: provenance must be a dict when present")
        return

    if expected_provenance is not None:
        if not isinstance(provenance, dict):
            errors.append(
                f"{suite_name}: provenance missing or invalid when binding against live tree "
                "(stale or malformed binary — rerun with --rebuild)"
            )
            return
        for key in ("git_commit_sha", "source_tree_fnv1a128", "cargo_lock_fnv1a128"):
            expected_val = expected_provenance.get(key)
            actual_val = provenance.get(key)
            if not isinstance(expected_val, str) or not expected_val:
                errors.append(
                    f"{suite_name}: expected_provenance[{key!r}] must be a non-empty str"
                )
            elif actual_val != expected_val:
                errors.append(
                    f"{suite_name}: stale binary or revision mismatch for provenance.{key} "
                    f"(report={actual_val!r}, live={expected_val!r}; "
                    f"rerun with --rebuild or run 'cargo build --release --locked')"
                )


def _validate_h6_decision(
    h6: Any,
    errors: list[str],
) -> tuple[str | None, dict[str, Any] | None]:
    ctx = "m4a.h6_decision"
    if not isinstance(h6, dict):
        errors.append(f"{ctx}: must be a dict, got {type(h6).__name__}")
        return None, None

    if h6.get("gate") != "G4A.7":
        errors.append(f"{ctx}: gate must be 'G4A.7', got {h6.get('gate')!r}")

    active_cnt = h6.get("active_task_count")
    if not _is_real_int(active_cnt) or active_cnt <= 0:
        errors.append(f"{ctx}: active_task_count must be int > 0, got {active_cnt!r}")
        active_cnt = None

    obs_cnt = h6.get("total_paired_observations")
    if not _is_real_int(obs_cnt) or obs_cnt <= 0:
        errors.append(f"{ctx}: total_paired_observations must be int > 0, got {obs_cnt!r}")

    margin_raw = h6.get("equivalence_margin_bits")
    if not _is_finite_number(margin_raw) or float(margin_raw) <= 0.0:
        errors.append(f"{ctx}: equivalence_margin_bits must be finite > 0, got {margin_raw!r}")
        margin = None
    else:
        margin = float(margin_raw)

    delta_raw = h6.get("delta_6_sb_minus_sa_l_epi_bits")
    if not _is_finite_number(delta_raw):
        errors.append(f"{ctx}: delta_6_sb_minus_sa_l_epi_bits must be finite, got {delta_raw!r}")
        delta_6 = None
    else:
        delta_6 = float(delta_raw)

    decision = h6.get("decision")
    canon_arm = h6.get("canonical_arm")
    if decision not in ("SaSuperiorDropReplay", "SaEquivalentDropReplay", "SbSuperiorRetainReplay"):
        errors.append(f"{ctx}: invalid H6 decision {decision!r}")
    if canon_arm not in ("Sa", "Sb"):
        errors.append(f"{ctx}: canonical_arm must be 'Sa' or 'Sb', got {canon_arm!r}")
        canon_arm = None

    if margin is not None and delta_6 is not None and canon_arm is not None:
        if delta_6 > margin:
            exp_dec, exp_arm = "SaSuperiorDropReplay", "Sa"
        elif delta_6 < -margin:
            exp_dec, exp_arm = "SbSuperiorRetainReplay", "Sb"
        else:
            exp_dec, exp_arm = "SaEquivalentDropReplay", "Sa"
        if decision != exp_dec or canon_arm != exp_arm:
            errors.append(
                f"{ctx}: (decision={decision!r}, canonical_arm={canon_arm!r}) inconsistent with "
                f"delta_6={delta_6} and margin={margin} (expected {exp_dec!r}, {exp_arm!r})"
            )

    if h6.get("canonical_arm_passed_all_gates") is not True:
        errors.append(
            f"{ctx}: canonical_arm_passed_all_gates must be True, got "
            f"{h6.get('canonical_arm_passed_all_gates')!r}"
        )

    per_task = h6.get("per_task")
    if not isinstance(per_task, list) or len(per_task) == 0:
        errors.append(f"{ctx}: per_task must be a non-empty list")
    else:
        if active_cnt is not None and len(per_task) != active_cnt:
            errors.append(
                f"{ctx}: len(per_task) ({len(per_task)}) != active_task_count ({active_cnt})"
            )
        for idx, row in enumerate(per_task):
            rctx = f"{ctx}.per_task[{idx}]"
            if not isinstance(row, dict):
                errors.append(f"{rctx}: must be a dict")
                continue
            if not isinstance(row.get("task_id"), str) or not row["task_id"]:
                errors.append(f"{rctx}: task_id must be a non-empty str")
            cnt = row.get("count")
            if not _is_real_int(cnt) or cnt <= 0:
                errors.append(f"{rctx}: count must be int > 0, got {cnt!r}")
            for num_key in (
                "mean_delta_6_sb_minus_sa_l_epi_bits",
                "median_delta_6_bits",
                "min_delta_6_bits",
                "max_delta_6_bits",
                "sa_mean_l_epi_advantage_bits",
                "sb_mean_l_epi_advantage_bits",
            ):
                if not _is_finite_number(row.get(num_key)):
                    errors.append(f"{rctx}: {num_key} must be finite, got {row.get(num_key)!r}")

    return canon_arm, {
        "gate": h6.get("gate"),
        "decision": decision,
        "canonical_arm": canon_arm,
        "delta_6_sb_minus_sa_l_epi_bits": delta_6,
        "active_task_count": active_cnt,
        "total_paired_observations": obs_cnt,
        "canonical_arm_passed_all_gates": h6.get("canonical_arm_passed_all_gates"),
    }


def _validate_arm_audit(
    audit: Any,
    audit_key: str,
    suite_name: str,
    gate_name: str,
    is_canonical: bool,
    max_excluded: int,
    reqs: dict[str, Any],
    enforce_pooled_cp: bool,
    required_tail_metrics: dict[str, Any] | None,
    errors: list[str],
) -> None:
    ctx = f"{suite_name}.{gate_name}.{audit_key}"
    if not isinstance(audit, dict):
        errors.append(f"{ctx}: must be a dict")
        return

    expected_arm = "Sa" if audit_key == "sa_audit" else "Sb"
    if audit.get("arm") != expected_arm:
        errors.append(f"{ctx}: arm must be {expected_arm!r}, got {audit.get('arm')!r}")

    arm_excluded = audit.get("excluded_cases")
    if not _is_real_int(arm_excluded) or arm_excluded < 0 or arm_excluded > max_excluded:
        errors.append(
            f"{ctx}: excluded_cases must be int in [0, {max_excluded}], got {arm_excluded!r}"
        )

    if is_canonical:
        for flag in ("passed", "stratified_passed", "bonferroni_half_passed", "tail_checks_passed"):
            if audit.get(flag) is not True:
                errors.append(f"{ctx}: canonical arm {flag} must be True, got {audit.get(flag)!r}")

    stratified = audit.get("stratified")
    if not isinstance(stratified, dict):
        errors.append(f"{ctx}: stratified must be a dict")
        return

    present_strata = [
        key for key in ("clean", "noisy") if key in stratified and stratified[key] is not None
    ]
    if not present_strata:
        errors.append(f"{ctx}: stratified must have at least one non-null stratum")
        return

    min_tails = int(reqs.get("min_tail_metrics_per_task", 1))
    req_strat = bool(reqs["require_stratified_passed"]) and is_canonical
    req_task = bool(reqs["require_per_task_passed"]) and is_canonical
    for stratum_name in present_strata:
        _validate_stratum(
            stratum=stratified[stratum_name],
            stratum_name=stratum_name,
            suite_name=suite_name,
            gate_name=gate_name,
            enforce_pooled_cp=enforce_pooled_cp,
            require_stratified_passed=req_strat,
            require_per_task_passed=req_task,
            require_finite_tail_metrics=reqs["require_finite_tail_metrics"],
            min_tail_metrics_per_task=min_tails,
            required_tail_metrics=required_tail_metrics if is_canonical else None,
            errors=errors,
        )


def validate_suite_report(
    suite_name: str,
    report: Any,
    expected_gates: list[str],
    require_clean: bool,
    requirements: dict[str, Any] | None = None,
    enforce_pooled_cp: bool = True,
    expected_provenance: dict[str, Any] | None = None,
    required_tail_metrics: dict[str, Any] | None = None,
    require_overall_passed: bool = False,
    require_h6_decision: bool = False,
) -> dict[str, Any]:
    """Validate a verify-m2a, verify-m2b, or verify-m4a JSON report against strict gate invariants."""
    reqs: dict[str, Any] = {
        "require_all_passed": True,
        "max_excluded_cases": 0,
        "max_sprout_blocked_max_nodes": 0,
        "require_stratified_passed": True,
        "require_per_task_passed": True,
        "require_finite_tail_metrics": True,
        "min_tail_metrics_per_task": 1,
    }
    if requirements:
        reqs.update(requirements)

    errors: list[str] = []
    if not isinstance(report, dict):
        raise ValidationError(f"{suite_name}: report must be a JSON object")

    if reqs["require_all_passed"] and report.get("all_passed") is not True:
        errors.append(
            f"{suite_name}: top-level all_passed must be True, got {report.get('all_passed')!r}"
        )

    if require_overall_passed and report.get("overall_passed") is not True:
        errors.append(
            f"{suite_name}: top-level overall_passed must be True, got {report.get('overall_passed')!r}"
        )

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
    _validate_provenance(
        suite_name=suite_name,
        provenance=provenance,
        require_clean=require_clean,
        expected_provenance=expected_provenance,
        errors=errors,
    )

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

    canonical_arm: str | None = None
    h6_summary: dict[str, Any] | None = None
    if require_h6_decision:
        canonical_arm, h6_summary = _validate_h6_decision(report.get("h6_decision"), errors)

    min_tails = int(reqs.get("min_tail_metrics_per_task", 1))
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
                        stratum=stratified[stratum_name],
                        stratum_name=stratum_name,
                        suite_name=suite_name,
                        gate_name=gate_name,
                        enforce_pooled_cp=enforce_pooled_cp,
                        require_stratified_passed=reqs["require_stratified_passed"],
                        require_per_task_passed=reqs["require_per_task_passed"],
                        require_finite_tail_metrics=reqs["require_finite_tail_metrics"],
                        min_tail_metrics_per_task=min_tails,
                        required_tail_metrics=required_tail_metrics,
                        errors=errors,
                    )

        if require_h6_decision and gate_name not in ("G4A.1", "G4A.2"):
            canon_audit_key = "sa_audit" if canonical_arm == "Sa" else "sb_audit"
            if canonical_arm is not None and canon_audit_key not in gate_entry:
                errors.append(
                    f"{suite_name}.{gate_name}: missing canonical arm audit {canon_audit_key!r}"
                )
            for audit_key, arm_label in (("sa_audit", "Sa"), ("sb_audit", "Sb")):
                if audit_key in gate_entry and gate_entry[audit_key] is not None:
                    _validate_arm_audit(
                        audit=gate_entry[audit_key],
                        audit_key=audit_key,
                        suite_name=suite_name,
                        gate_name=gate_name,
                        is_canonical=(canonical_arm == arm_label),
                        max_excluded=max_excluded,
                        reqs=reqs,
                        enforce_pooled_cp=enforce_pooled_cp,
                        required_tail_metrics=required_tail_metrics,
                        errors=errors,
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

    result = {
        "suite": suite_name,
        "passed": True,
        "excluded_cases": excluded_cases,
        "cliff_counters": cliff_counters,
        "provenance": prov_summary,
        "gates": gate_summaries,
    }
    if h6_summary is not None:
        result["h6_decision"] = h6_summary
    return result


def load_manifest(manifest_path: Path) -> dict[str, Any]:
    """Load and sanity-check the retained baselines manifest JSON."""
    if not manifest_path.is_file():
        raise ValidationError(f"Manifest file not found: {manifest_path}")
    manifest = parse_json_strict(manifest_path.read_text(encoding="utf-8"))
    if not isinstance(manifest, dict) or manifest.get("schema_version") != 1:
        raise ValidationError("Manifest must be a JSON object with schema_version == 1")
    if "golden_traces" not in manifest or "suites" not in manifest:
        raise ValidationError("Manifest must define 'golden_traces' and 'suites'")
    if "required_tail_metrics" not in manifest or not isinstance(
        manifest["required_tail_metrics"], dict
    ):
        raise ValidationError("Manifest must define 'required_tail_metrics' mapping")
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
        choices=("all", "m2a", "m2b", "m4a"),
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
        help="Strict mode: forbid --fast and --skip-golden, and require live & binary clean provenance",
    )
    parser.add_argument(
        "--skip-golden",
        action="store_true",
        help="Skip golden_traces test execution (forbidden under --require-clean)",
    )
    parser.add_argument(
        "--rebuild",
        action="store_true",
        help="Run `cargo build --release --locked` before running golden_traces and baseline suites",
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


def _make_multi_bank(
    successes: int,
    trials: int,
    target_rate: float = 0.9,
    bank_count: int = 5,
    p_value: float = 0.85,
) -> dict[str, Any]:
    lb99 = clopper_pearson_lower_bound(successes, trials, 0.99)
    passes_pooled = lb99 >= target_rate
    passes_homogeneity = True if bank_count <= 1 else (p_value >= 0.01)
    return {
        "pooled": {
            "total_trials": trials,
            "total_successes": successes,
            "pooled_rate": successes / trials if trials else 0.0,
            "lower_bound_99": lb99,
            "target_rate": target_rate,
            "passes_pooled": passes_pooled,
        },
        "distribution": {
            "bank_count": bank_count,
            "mean": successes / trials if trials else 0.0,
            "std_dev": 0.0,
            "percentile_5th": successes / trials if trials else 0.0,
            "chi_squared": 0.0,
            "df": max(0, bank_count - 1),
            "p_value": 1.0 if bank_count <= 1 else p_value,
            "passes_homogeneity": passes_homogeneity,
        },
        "passed": passes_pooled and passes_homogeneity,
    }


def _make_task(
    label: str,
    regime: str = "clean",
    successes: int = 100,
    trials: int = 100,
    target_rate: float = 0.9,
    task_count: int = 2,
    bank_count: int = 5,
    homog_p_value: float = 0.85,
    tail_metrics: dict[str, Any] | None = None,
) -> dict[str, Any]:
    fwer_alpha = 0.01 / task_count
    binom_p = binomial_lower_tail_p_value(successes, trials, target_rate)
    passes_fwer = (successes == trials) if target_rate >= 1.0 else (binom_p >= fwer_alpha)
    passes_homog = True if bank_count <= 1 else (homog_p_value >= fwer_alpha)
    return {
        "task_label": label,
        "regime": regime,
        "total_successes": successes,
        "total_trials": trials,
        "empirical_rate": successes / trials if trials else 0.0,
        "lower_bound_99": clopper_pearson_lower_bound(successes, trials, 0.99),
        "binomial_lower_tail_p": binom_p,
        "fwer_alpha": fwer_alpha,
        "passes_fwer_binomial": passes_fwer,
        "distribution": {
            "bank_count": bank_count,
            "mean": successes / trials if trials else 0.0,
            "std_dev": 0.0,
            "percentile_5th": successes / trials if trials else 0.0,
            "chi_squared": 0.0,
            "df": max(0, bank_count - 1),
            "p_value": 1.0 if bank_count <= 1 else homog_p_value,
            "passes_homogeneity": passes_homog,
        },
        "passed": passes_fwer and passes_homog,
        "tail_metrics": (
            tail_metrics
            if tail_metrics is not None
            else {"max_target_kt_bits": 0.5}
        ),
    }


def _make_stratum(
    tasks: list[dict[str, Any]],
    regime: str = "clean",
    passed: bool = True,
    target_rate: float = 0.9,
    bank_count: int = 5,
    pooled_mb: dict[str, Any] | None = None,
    conj_mb: dict[str, Any] | None = None,
) -> dict[str, Any]:
    sum_succ = sum(t["total_successes"] for t in tasks)
    sum_trials = sum(t["total_trials"] for t in tasks)
    worst = min(tasks, key=lambda t: (t["empirical_rate"], t["task_label"]))
    if pooled_mb is None:
        pooled_mb = _make_multi_bank(
            sum_succ, sum_trials, target_rate=target_rate, bank_count=bank_count
        )
    if regime == "clean" and conj_mb is None:
        conj_trials = tasks[0]["total_trials"]
        conj_succ = min(t["total_successes"] for t in tasks)
        conj_mb = _make_multi_bank(
            conj_succ, conj_trials, target_rate=target_rate, bank_count=bank_count
        )
    return {
        "regime": regime,
        "passed": passed,
        "task_count": len(tasks),
        "target_rate": target_rate,
        "fwer_alpha": 0.01 / len(tasks),
        "worst_task_label": worst["task_label"],
        "worst_task_rate": worst["empirical_rate"],
        "pooled_multi_bank": pooled_mb,
        "seed_conjunction_multi_bank": conj_mb,
        "tasks": tasks,
    }


_SELF_TEST_PROV_A = {
    "git_commit_sha": "deadbeef" * 5,
    "git_dirty": False,
    "source_tree_fnv1a128": "0123456789abcdef0123456789abcdef",
    "cargo_lock_fnv1a128": "fedcba9876543210fedcba9876543210",
}

_SELF_TEST_PROV_B = {
    "git_commit_sha": "cafebabe" * 5,
    "git_dirty": False,
    "source_tree_fnv1a128": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "cargo_lock_fnv1a128": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
}


def _make_valid_m2a_report(git_dirty: bool = False) -> dict[str, Any]:
    gate_tails = {
        "G2A.1": {"max_target_kt_bits": 0.12},
        "G2A.2": {"max_target_kt_bits": 0.15},
        "G2A.3": {"max_multi_lock_tick": 142.0},
        "G2A.5": {"min_kt_advantage": 0.35},
    }
    gates = []
    for gate_name in ("G2A.1", "G2A.2", "G2A.3", "G2A.5"):
        target = 0.85 if gate_name == "G2A.5" else 0.90
        tasks = [
            _make_task(
                f"{gate_name}:T1",
                regime="clean",
                target_rate=target,
                task_count=2,
                tail_metrics=dict(gate_tails[gate_name]),
            ),
            _make_task(
                f"{gate_name}:T2",
                regime="clean",
                target_rate=target,
                task_count=2,
                tail_metrics=dict(gate_tails[gate_name]),
            ),
        ]
        gates.append(
            {
                "gate": gate_name,
                "passed": True,
                "excluded_cases": 0,
                "stratified": {
                    "passed": True,
                    "clean": _make_stratum(tasks, regime="clean", passed=True, target_rate=target),
                },
            }
        )

    noisy_tasks = [
        _make_task(
            "G2A.4a:Noisy_T1",
            regime="noisy",
            target_rate=0.85,
            task_count=2,
            tail_metrics={"max_target_kt_bits": 0.55, "max_paired_regret": 0.01},
        ),
        _make_task(
            "G2A.4b:Noisy_T2",
            regime="noisy",
            target_rate=0.85,
            task_count=2,
            tail_metrics={
                "max_target_kt_bits": 0.52,
                "max_abs_kt_diff": 0.02,
                "max_h_reactive_dev": 0.01,
            },
        ),
    ]
    gates.append(
        {
            "gate": "G2A.4",
            "passed": True,
            "excluded_cases": 0,
            "stratified": {
                "passed": True,
                "noisy": _make_stratum(noisy_tasks, regime="noisy", passed=True, target_rate=0.85),
            },
        }
    )
    return {
        "provenance": {
            "git_commit_sha": _SELF_TEST_PROV_A["git_commit_sha"],
            "git_dirty": git_dirty,
            "source_tree_fnv1a128": _SELF_TEST_PROV_A["source_tree_fnv1a128"],
            "cargo_lock_fnv1a128": _SELF_TEST_PROV_A["cargo_lock_fnv1a128"],
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
        target = {"G2B.1": 0.90, "G2B.2": 0.90, "G2B.3": 0.95, "G2B.4": 0.85, "G2B.5": 0.90}[
            gate_name
        ]
        if gate_name == "G2B.1":
            clean_tasks = [
                _make_task(
                    "G2B.1a:epistemic_mdl",
                    regime="clean",
                    target_rate=target,
                    task_count=3,
                    tail_metrics={"max_epistemic_mdl_err": 0.001},
                ),
                _make_task(
                    "G2B.1a:starvation",
                    regime="clean",
                    target_rate=target,
                    task_count=3,
                    tail_metrics={"max_starved_energy": 0.0},
                ),
                _make_task(
                    "G2B.1b:invariance",
                    regime="clean",
                    target_rate=target,
                    task_count=3,
                    tail_metrics={"max_target_kt_bits": 0.25},
                ),
            ]
        elif gate_name == "G2B.2":
            clean_tasks = [
                _make_task(
                    "G2B.2:T1",
                    regime="clean",
                    target_rate=target,
                    task_count=2,
                    tail_metrics={"min_target_comps": 1.0, "min_settled_comp_ticks": 64.0},
                ),
                _make_task(
                    "G2B.2:T2",
                    regime="clean",
                    target_rate=target,
                    task_count=2,
                    tail_metrics={"min_target_comps": 2.0, "min_settled_comp_ticks": 96.0},
                ),
            ]
        elif gate_name == "G2B.3":
            clean_tasks = [
                _make_task(
                    "G2B.3a:T1",
                    regime="clean",
                    target_rate=target,
                    task_count=2,
                    tail_metrics={
                        "max_loopback_disagreement_ticks": 3.0,
                        "max_circular_discrepancy": 0.0,
                    },
                ),
                _make_task(
                    "G2B.3b:T2",
                    regime="clean",
                    target_rate=target,
                    task_count=2,
                    tail_metrics={
                        "max_loopback_disagreement_ticks": 2.0,
                        "max_circular_discrepancy": 0.0,
                    },
                ),
            ]
        elif gate_name == "G2B.4":
            clean_tasks = [
                _make_task(
                    "G2B.4:Clean_T1",
                    regime="clean",
                    target_rate=target,
                    task_count=1,
                    tail_metrics={"max_abs_kt_diff": 0.015},
                ),
            ]
        else:
            clean_tasks = [
                _make_task(
                    "G2B.5:T1",
                    regime="clean",
                    target_rate=target,
                    task_count=1,
                    tail_metrics={"min_post_fault_kt_advantage": 0.42},
                ),
            ]

        stratified: dict[str, Any] = {
            "passed": True,
            "clean": _make_stratum(clean_tasks, regime="clean", passed=True, target_rate=target),
        }
        if gate_name == "G2B.4":
            stratified["noisy"] = _make_stratum(
                [
                    _make_task(
                        "G2B.4:Noisy",
                        regime="noisy",
                        target_rate=target,
                        task_count=1,
                        tail_metrics={"max_abs_kt_diff": 0.02},
                    )
                ],
                regime="noisy",
                passed=True,
                target_rate=target,
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
            "git_commit_sha": _SELF_TEST_PROV_B["git_commit_sha"],
            "git_dirty": git_dirty,
            "source_tree_fnv1a128": _SELF_TEST_PROV_B["source_tree_fnv1a128"],
            "cargo_lock_fnv1a128": _SELF_TEST_PROV_B["cargo_lock_fnv1a128"],
            "package_version": "0.1.0",
            "rustc_version": "rustc 1.85.0",
            "target_triple": "x86_64-unknown-linux-gnu",
        },
        "all_passed": True,
        "excluded_cases": 0,
        "cliff_counters": {"sprout_blocked_max_nodes": 0},
        "gates": gates,
    }


def _make_valid_m4a_report(
    git_dirty: bool = False,
    sb_fails_g4a5: bool = True,
) -> dict[str, Any]:
    g1_tails = {
        "sprout_blocked_max_nodes": 0.0,
        "max_epistemic_mdl_err": 1e-13,
        "max_compute_ledger_diff": 0.0,
        "max_node_eval_ratio_sleep_over_wake": 1.0,
    }
    g2_tails = {
        "sprout_blocked_max_nodes": 0.0,
        "max_i2_violations": 0.0,
        "max_i6_violations": 0.0,
        "max_i7_violations": 0.0,
    }
    g3_tails = {
        "sprout_blocked_max_nodes": 0.0,
        "mean_l_epi_advantage": 95.0,
        "median_l_epi_advantage": 88.0,
        "min_l_epi_advantage": -20.0,
        "p05_l_epi_advantage": 5.0,
        "max_wrongful_excisions": 0.0,
        "max_port_locks_lost": 0.0,
    }
    g4_tails = {
        "sprout_blocked_max_nodes": 0.0,
        "max_eval_kt_regret": 0.0,
        "mean_eval_kt_regret": 0.0,
        "p95_eval_kt_regret": 0.0,
    }
    g5_tails = {
        "sprout_blocked_max_nodes": 0.0,
        "max_spurious_survival_segments": 0.0,
        "median_spurious_survival_segments": 0.0,
        "max_wrongful_excisions": 0.0,
        "pl_excision_rate": 1.0,
        "pl_spurious_extinction_rate": 1.0,
    }
    g6_tails = {
        "sprout_blocked_max_nodes": 0.0,
        "max_wrongful_excisions": 0.0,
        "max_port_locks_lost": 0.0,
    }

    def arm_audit(arm: str, gate: str, clean_strat: dict[str, Any] | None, noisy_strat: dict[str, Any] | None, ok: bool = True) -> dict[str, Any]:
        strat: dict[str, Any] = {"passed": ok}
        if clean_strat is not None:
            strat["clean"] = clean_strat
        if noisy_strat is not None:
            strat["noisy"] = noisy_strat
        return {
            "arm": arm,
            "passed": ok,
            "stratified_passed": ok,
            "bonferroni_half_alpha": 0.00125 if gate in ("G4A.3", "G4A.5") else None,
            "bonferroni_half_passed": ok,
            "tail_checks_passed": ok,
            "excluded_cases": 0,
            "stratified": strat,
        }

    gates = []
    for gn, tails in (("G4A.1", g1_tails), ("G4A.2", g2_tails)):
        ct = [_make_task(f"{gn}:CT:clean", regime="clean", target_rate=1.0, task_count=1, tail_metrics=dict(tails))]
        nt = [_make_task(f"{gn}:CT:noisy", regime="noisy", target_rate=1.0, task_count=1, tail_metrics=dict(tails))]
        gates.append({
            "gate": gn,
            "passed": True,
            "excluded_cases": 0,
            "stratified": {
                "passed": True,
                "clean": _make_stratum(ct, regime="clean", passed=True, target_rate=1.0),
                "noisy": _make_stratum(nt, regime="noisy", passed=True, target_rate=1.0),
            },
        })

    g3_nt = [_make_task("SP:MV_Periodic:k1:noisy", regime="noisy", successes=90, trials=100, target_rate=0.55, task_count=1, tail_metrics=dict(g3_tails))]
    g3_noisy = _make_stratum(g3_nt, regime="noisy", passed=True, target_rate=0.55)
    gates.append({
        "gate": "G4A.3",
        "passed": True,
        "excluded_cases": 0,
        "stratified": {"passed": True, "noisy": copy.deepcopy(g3_noisy)},
        "sa_audit": arm_audit("Sa", "G4A.3", None, copy.deepcopy(g3_noisy), ok=True),
        "sb_audit": arm_audit("Sb", "G4A.3", None, copy.deepcopy(g3_noisy), ok=True),
    })

    for gn, n_target, tails in (("G4A.4", 0.85, g4_tails), ("G4A.5", 0.75, g5_tails), ("G4A.6", 0.90, g6_tails)):
        ct = [_make_task(f"{gn}:Clean", regime="clean", successes=100, trials=100, target_rate=1.0, task_count=1, tail_metrics=dict(tails))]
        nt = [_make_task(f"{gn}:Noisy", regime="noisy", successes=98, trials=100, target_rate=n_target, task_count=1, tail_metrics=dict(tails))]
        cs = _make_stratum(ct, regime="clean", passed=True, target_rate=1.0)
        ns = _make_stratum(nt, regime="noisy", passed=True, target_rate=n_target)
        if gn == "G4A.5" and sb_fails_g4a5:
            sb_nt = [_make_task("PL:PL2:k2:noisy", regime="noisy", successes=50, trials=100, target_rate=0.75, task_count=1, tail_metrics=dict(tails))]
            sb_ns = _make_stratum(sb_nt, regime="noisy", passed=False, target_rate=0.75, pooled_mb=_make_multi_bank(50, 100, target_rate=0.75))
            sb_aud = arm_audit("Sb", gn, copy.deepcopy(cs), sb_ns, ok=False)
        else:
            sb_aud = arm_audit("Sb", gn, copy.deepcopy(cs), copy.deepcopy(ns), ok=True)
        gates.append({
            "gate": gn,
            "passed": True,
            "excluded_cases": 0,
            "stratified": {"passed": True, "clean": copy.deepcopy(cs), "noisy": copy.deepcopy(ns)},
            "sa_audit": arm_audit("Sa", gn, copy.deepcopy(cs), copy.deepcopy(ns), ok=True),
            "sb_audit": sb_aud,
        })

    return {
        "provenance": {
            "git_commit_sha": _SELF_TEST_PROV_A["git_commit_sha"],
            "git_dirty": git_dirty,
            "source_tree_fnv1a128": _SELF_TEST_PROV_A["source_tree_fnv1a128"],
            "cargo_lock_fnv1a128": _SELF_TEST_PROV_A["cargo_lock_fnv1a128"],
            "package_version": "0.1.0",
            "rustc_version": "rustc 1.99.1",
            "target_triple": "x86_64-unknown-linux-gnu",
        },
        "all_passed": True,
        "overall_passed": True,
        "excluded_cases": 0,
        "cliff_counters": {"sprout_blocked_max_nodes": 0},
        "gates": gates,
        "h6_decision": {
            "gate": "G4A.7",
            "active_task_count": 1,
            "total_paired_observations": 100,
            "equivalence_margin_bits": 10.0,
            "delta_6_sb_minus_sa_l_epi_bits": 14.61,
            "decision": "SaSuperiorDropReplay",
            "canonical_arm": "Sa",
            "canonical_arm_passed_all_gates": True,
            "per_task": [
                {
                    "task_id": "PL:PL2_RareContext:k2:noisy",
                    "count": 100,
                    "mean_delta_6_sb_minus_sa_l_epi_bits": 14.61,
                    "median_delta_6_bits": 12.0,
                    "min_delta_6_bits": -5.0,
                    "max_delta_6_bits": 200.0,
                    "sa_mean_l_epi_advantage_bits": 640.0,
                    "sb_mean_l_epi_advantage_bits": 495.0,
                }
            ],
        },
    }


def run_self_test() -> int:
    """Execute built-in adversarial validation and CLI guard tests."""
    m2a_gates = ["G2A.1", "G2A.2", "G2A.3", "G2A.4", "G2A.5"]
    m2b_gates = ["G2B.1", "G2B.2", "G2B.3", "G2B.4", "G2B.5"]
    m4a_gates = ["G4A.1", "G4A.2", "G4A.3", "G4A.4", "G4A.5", "G4A.6"]
    checks: list[tuple[str, bool, str]] = []

    manifest_req_tails = (
        load_manifest(DEFAULT_MANIFEST).get("required_tail_metrics")
        if DEFAULT_MANIFEST.is_file()
        else None
    )

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

    # 1. Valid passing m2a and m2b reports (with expected_provenance & manifest required_tail_metrics)
    expect_pass(
        "1a. valid m2a (require_clean=False)",
        lambda: validate_suite_report(
            "m2a",
            _make_valid_m2a_report(git_dirty=False),
            m2a_gates,
            False,
            required_tail_metrics=manifest_req_tails,
        ),
    )
    expect_pass(
        "1b. valid m2a (require_clean=True, matching expected_provenance)",
        lambda: validate_suite_report(
            "m2a",
            _make_valid_m2a_report(git_dirty=False),
            m2a_gates,
            True,
            expected_provenance=_SELF_TEST_PROV_A,
            required_tail_metrics=manifest_req_tails,
        ),
    )
    expect_pass(
        "1c. valid m2b (require_clean=False)",
        lambda: validate_suite_report(
            "m2b",
            _make_valid_m2b_report(git_dirty=False),
            m2b_gates,
            False,
            required_tail_metrics=manifest_req_tails,
        ),
    )
    expect_pass(
        "1d. valid m2b (require_clean=True, matching expected_provenance)",
        lambda: validate_suite_report(
            "m2b",
            _make_valid_m2b_report(git_dirty=False),
            m2b_gates,
            True,
            expected_provenance=_SELF_TEST_PROV_B,
            required_tail_metrics=manifest_req_tails,
        ),
    )

    # 2. Concentrated G2B.2 MV_AddEqz_k2_Comb 0/20 failure (other 9 tasks 20/20 -> pooled 180/200 = 0.90)
    def make_concentrated_g2b2_failure(forge_top_level: bool, forge_task_flags: bool = False) -> dict[str, Any]:
        rep = _make_valid_m2b_report(git_dirty=False)
        tasks = [
            _make_task(
                f"G2B.2:Task_{i}",
                regime="clean",
                successes=20,
                trials=20,
                target_rate=0.90,
                task_count=10,
                bank_count=1,
                tail_metrics={"min_target_comps": 1.0, "min_settled_comp_ticks": 64.0},
            )
            for i in range(9)
        ]
        bad_task = _make_task(
            "G2B.2:MV_AddEqz_k2_Comb",
            regime="clean",
            successes=0,
            trials=20,
            target_rate=0.90,
            task_count=10,
            bank_count=1,
            tail_metrics={"min_target_comps": 0.0, "min_settled_comp_ticks": 0.0},
        )
        if forge_task_flags:
            bad_task["passed"] = True
            bad_task["passes_fwer_binomial"] = True
        tasks.append(bad_task)
        stratum = _make_stratum(
            tasks,
            regime="clean",
            passed=forge_top_level,
            target_rate=0.90,
            bank_count=1,
            conj_mb=_make_multi_bank(0, 20, target_rate=0.90, bank_count=1),
        )
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
        lambda: validate_suite_report("m2b", make_concentrated_g2b2_failure(True, True), m2b_gates, False),
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

    # 4. R2 provenance binding, git_dirty=True, stale 7cb1ee0, mismatched digests, CLI guards
    expect_pass(
        "4a. git_dirty=True allowed when require_clean=False and no expected_provenance",
        lambda: validate_suite_report("m2a", _make_valid_m2a_report(git_dirty=True), m2a_gates, False),
    )
    expect_reject(
        "4b. git_dirty=True rejected when require_clean=True",
        lambda: validate_suite_report(
            "m2a",
            _make_valid_m2a_report(git_dirty=True),
            m2a_gates,
            True,
            expected_provenance=_SELF_TEST_PROV_A,
        ),
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

    def stale_7cb1ee0_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report(git_dirty=False)
        rep["provenance"]["git_commit_sha"] = "7cb1ee0000000000000000000000000000000000"
        return rep

    def mismatched_source_digest_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report(git_dirty=False)
        rep["provenance"]["source_tree_fnv1a128"] = "f" * 32
        return rep

    expect_reject(
        "4e. R2 stale 7cb1ee0 clean report rejected under require_clean=True",
        lambda: validate_suite_report(
            "m2a",
            stale_7cb1ee0_report(),
            m2a_gates,
            True,
            expected_provenance=_SELF_TEST_PROV_A,
        ),
    )
    expect_reject(
        "4f. R2 stale 7cb1ee0 report rejected even when require_clean=False if expected_provenance provided",
        lambda: validate_suite_report(
            "m2a",
            stale_7cb1ee0_report(),
            m2a_gates,
            False,
            expected_provenance=_SELF_TEST_PROV_A,
        ),
    )
    expect_reject(
        "4g. R2 mismatched source_tree_fnv1a128 rejected",
        lambda: validate_suite_report(
            "m2a",
            mismatched_source_digest_report(),
            m2a_gates,
            True,
            expected_provenance=_SELF_TEST_PROV_A,
        ),
    )
    expect_reject(
        "4h. R2 require_clean=True fails if expected_provenance omitted",
        lambda: validate_suite_report(
            "m2a",
            _make_valid_m2a_report(git_dirty=False),
            m2a_gates,
            True,
            expected_provenance=None,
        ),
    )
    expect_reject(
        "4i. R2 require_clean=True fails if live worktree is dirty",
        lambda: validate_suite_report(
            "m2a",
            _make_valid_m2a_report(git_dirty=False),
            m2a_gates,
            True,
            expected_provenance={**_SELF_TEST_PROV_A, "git_dirty": True},
        ),
    )

    def test_rebuild_flag_and_fnv1a_vectors() -> None:
        parsed_default = parser.parse_args([])
        parsed_rebuild = parser.parse_args(["--rebuild"])
        if parsed_default.rebuild is not False or parsed_rebuild.rebuild is not True:
            raise AssertionError("--rebuild flag parse mismatch")
        if fnv1a_128_hex(b"") != "6c62272e07bb014262b821756295c58d":
            raise AssertionError("FNV-1a-128 offset basis mismatch")
        if abs(binomial_lower_tail_p_value(0, 2, 0.5) - 0.25) > 1e-12:
            raise AssertionError("binomial_lower_tail_p_value(0, 2, 0.5) != 0.25")
        if abs(binomial_lower_tail_p_value(1, 2, 0.5) - 0.75) > 1e-12:
            raise AssertionError("binomial_lower_tail_p_value(1, 2, 0.5) != 0.75")
        if binomial_lower_tail_p_value(0, 10000, 0.9) != 0.0 and binomial_lower_tail_p_value(0, 10000, 0.9) > 1e-300:
            raise AssertionError("binomial_lower_tail_p_value large n precision error")
        with tempfile.TemporaryDirectory() as tmpdir:
            root = Path(tmpdir)
            (root / "Cargo.toml").write_text("[package]\nname = \"x\"\n", encoding="utf-8")
            (root / "Cargo.lock").write_text("# lock\n", encoding="utf-8")
            (root / "scripts").mkdir()
            (root / "scripts" / "build.rs").write_text("fn main() {}\n", encoding="utf-8")
            (root / "src").mkdir()
            (root / "src" / "lib.rs").write_text("pub fn f() {}\n", encoding="utf-8")
            src_hex = source_tree_fnv1a128(root)
            lock_hex = cargo_lock_fnv1a128(root)
            if len(src_hex) != 32 or len(lock_hex) != 32:
                raise AssertionError("Expected 32-hex-char digests")

    expect_pass("4j. --rebuild CLI flag, FNV-1a-128 vectors, & binomial p-value", test_rebuild_flag_and_fnv1a_vectors)

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

    # 6. Missing expected gate (G2B.2 omitted) & empty stratified
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

    # 7. Non-finite / null / bool / NaN / missing / empty / missing-required-key tail_metrics
    for label, bad_val in (
        ("NaN", float("nan")),
        ("Inf", float("inf")),
        ("null", None),
        ("bool", True),
    ):
        def make_bad_tail(val: Any = bad_val) -> dict[str, Any]:
            rep = copy.deepcopy(_make_valid_m2a_report())
            rep["gates"][0]["stratified"]["clean"]["tasks"][0]["tail_metrics"] = {
                "max_target_kt_bits": val
            }
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

    def missing_tail_metrics_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        del rep["gates"][0]["stratified"]["clean"]["tasks"][0]["tail_metrics"]
        return rep

    def empty_tail_metrics_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        rep["gates"][0]["stratified"]["clean"]["tasks"][0]["tail_metrics"] = {}
        return rep

    def missing_manifest_required_key_report() -> dict[str, Any]:
        rep = _make_valid_m2b_report()
        # G2B.2 requires both min_target_comps and min_settled_comp_ticks
        del rep["gates"][1]["stratified"]["clean"]["tasks"][0]["tail_metrics"][
            "min_settled_comp_ticks"
        ]
        return rep

    expect_reject(
        "7f. R4 missing tail_metrics rejected",
        lambda: validate_suite_report("m2a", missing_tail_metrics_report(), m2a_gates, False),
    )
    expect_reject(
        "7g. R4 empty tail_metrics rejected",
        lambda: validate_suite_report("m2a", empty_tail_metrics_report(), m2a_gates, False),
    )
    if manifest_req_tails is not None:
        expect_reject(
            "7h. R4 missing manifest-required metric key rejected",
            lambda: validate_suite_report(
                "m2b",
                missing_manifest_required_key_report(),
                m2b_gates,
                False,
                required_tail_metrics=manifest_req_tails,
            ),
        )

    # 8. R4 statistical & conjunction counterexamples
    def clean_missing_conj_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        rep["gates"][0]["stratified"]["clean"]["seed_conjunction_multi_bank"] = None
        return rep

    def clean_conj_0_of_100_passed_false_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        rep["gates"][0]["stratified"]["clean"]["seed_conjunction_multi_bank"] = _make_multi_bank(
            0, 100, target_rate=0.90, bank_count=5
        )
        return rep

    def zero_of_100_forged_passed_and_p0_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        stratum = rep["gates"][0]["stratified"]["clean"]
        task = stratum["tasks"][0]
        task["total_successes"] = 0
        task["total_trials"] = 100
        task["empirical_rate"] = 0.0
        task["lower_bound_99"] = 0.0
        task["binomial_lower_tail_p"] = 0.0
        task["passed"] = True
        task["passes_fwer_binomial"] = True
        stratum["worst_task_label"] = task["task_label"]
        stratum["worst_task_rate"] = 0.0
        return rep

    def zero_of_100_forged_p_099_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        stratum = rep["gates"][0]["stratified"]["clean"]
        task = stratum["tasks"][0]
        task["total_successes"] = 0
        task["total_trials"] = 100
        task["empirical_rate"] = 0.0
        task["lower_bound_99"] = 0.0
        task["binomial_lower_tail_p"] = 0.99
        task["passed"] = True
        task["passes_fwer_binomial"] = True
        stratum["worst_task_label"] = task["task_label"]
        stratum["worst_task_rate"] = 0.0
        return rep

    def failed_pooled_mb_with_passing_conj_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        stratum = rep["gates"][0]["stratified"]["clean"]
        # Homogeneity fails on pooled_multi_bank (p_value=0.001 < 0.01) while conj passes
        stratum["pooled_multi_bank"] = _make_multi_bank(
            200, 200, target_rate=0.90, bank_count=5, p_value=0.001
        )
        return rep

    def make_fast_single_bank_m2a_report() -> dict[str, Any]:
        rep = _make_valid_m2a_report()
        for g in rep["gates"]:
            target = 0.85 if g["gate"] in ("G2A.4", "G2A.5") else 0.90
            for sname in ("clean", "noisy"):
                if sname not in g["stratified"]:
                    continue
                s = g["stratified"][sname]
                new_tasks = [
                    _make_task(
                        t["task_label"],
                        regime=sname,
                        successes=19,
                        trials=20,
                        target_rate=target,
                        task_count=len(s["tasks"]),
                        bank_count=1,
                        tail_metrics=t["tail_metrics"],
                    )
                    for t in s["tasks"]
                ]
                g["stratified"][sname] = _make_stratum(
                    new_tasks,
                    regime=sname,
                    passed=True,
                    target_rate=target,
                    bank_count=1,
                )
        return rep

    expect_reject(
        "8a. R4 clean stratum missing seed_conjunction_multi_bank rejected",
        lambda: validate_suite_report("m2a", clean_missing_conj_report(), m2a_gates, False),
    )
    expect_reject(
        "8b. R4 clean conjunction 0/100 passed=False rejected",
        lambda: validate_suite_report(
            "m2a", clean_conj_0_of_100_passed_false_report(), m2a_gates, False
        ),
    )
    expect_reject(
        "8c. R4 0/100 task with forged passed=True & binomial_lower_tail_p=0 rejected",
        lambda: validate_suite_report(
            "m2a", zero_of_100_forged_passed_and_p0_report(), m2a_gates, False
        ),
    )
    expect_reject(
        "8d. R4 forged binomial_lower_tail_p=0.99 on 0/100 rejected",
        lambda: validate_suite_report("m2a", zero_of_100_forged_p_099_report(), m2a_gates, False),
    )
    expect_reject(
        "8e. R4 failed pooled_multi_bank with passing conj rejected",
        lambda: validate_suite_report(
            "m2a", failed_pooled_mb_with_passing_conj_report(), m2a_gates, False
        ),
    )
    expect_pass(
        "8f. fast single-bank (enforce_pooled_cp=False) passes when pooled_rate >= target_rate",
        lambda: validate_suite_report(
            "m2a",
            make_fast_single_bank_m2a_report(),
            m2a_gates,
            False,
            enforce_pooled_cp=False,
        ),
    )
    expect_reject(
        "8g. single-bank 19/20 rejected under standard enforce_pooled_cp=True",
        lambda: validate_suite_report(
            "m2a",
            make_fast_single_bank_m2a_report(),
            m2a_gates,
            False,
            enforce_pooled_cp=True,
        ),
    )

    # 9. Verify manifest exists & loads cleanly alongside self-test if present
    if DEFAULT_MANIFEST.is_file():
        expect_pass("9a. default manifest loads cleanly", lambda: load_manifest(DEFAULT_MANIFEST))

    def test_main_cli_guard() -> None:
        with tempfile.TemporaryDirectory() as tmpdir:
            json_out = Path(tmpdir) / "verdict.json"
            with contextlib.redirect_stderr(io.StringIO()):
                rc = main(["--require-clean", "--fast", "--json-out", str(json_out)])
            if rc == 0:
                raise AssertionError("Expected non-zero exit for --require-clean --fast")
            if not json_out.is_file():
                raise AssertionError("Expected --json-out verdict file on CLI guard failure")

    expect_pass("9b. main() rejects --require-clean --fast with exit != 0", test_main_cli_guard)

    # 10. M4a schema, G4A.7 (H6), canonical arm audit, and tail checks
    expect_pass(
        "10a. valid m4a (S-B fails G4A.5 while canonical S-A passes, require_clean=True)",
        lambda: validate_suite_report(
            "m4a",
            _make_valid_m4a_report(git_dirty=False, sb_fails_g4a5=True),
            m4a_gates,
            True,
            expected_provenance=_SELF_TEST_PROV_A,
            required_tail_metrics=manifest_req_tails,
            require_overall_passed=True,
            require_h6_decision=True,
        ),
    )

    def m4a_overall_false() -> dict[str, Any]:
        rep = _make_valid_m4a_report()
        rep["overall_passed"] = False
        return rep

    def m4a_bad_h6() -> dict[str, Any]:
        rep = _make_valid_m4a_report()
        rep["h6_decision"]["canonical_arm_passed_all_gates"] = False
        return rep

    def m4a_canonical_sa_failed() -> dict[str, Any]:
        rep = _make_valid_m4a_report()
        rep["gates"][4]["sa_audit"]["passed"] = False
        return rep

    def m4a_missing_tail() -> dict[str, Any]:
        rep = _make_valid_m4a_report()
        del rep["gates"][2]["stratified"]["noisy"]["tasks"][0]["tail_metrics"]["mean_l_epi_advantage"]
        return rep

    expect_reject(
        "10b. m4a overall_passed=False rejected",
        lambda: validate_suite_report(
            "m4a", m4a_overall_false(), m4a_gates, False, require_overall_passed=True, require_h6_decision=True
        ),
    )
    expect_reject(
        "10c. m4a canonical_arm_passed_all_gates=False rejected",
        lambda: validate_suite_report(
            "m4a", m4a_bad_h6(), m4a_gates, False, require_overall_passed=True, require_h6_decision=True
        ),
    )
    expect_reject(
        "10d. m4a canonical S-A audit passed=False rejected",
        lambda: validate_suite_report(
            "m4a", m4a_canonical_sa_failed(), m4a_gates, False, require_overall_passed=True, require_h6_decision=True
        ),
    )
    if manifest_req_tails is not None:
        expect_reject(
            "10e. m4a missing G4A.3 required tail metric rejected",
            lambda: validate_suite_report(
                "m4a",
                m4a_missing_tail(),
                m4a_gates,
                False,
                required_tail_metrics=manifest_req_tails,
                require_overall_passed=True,
                require_h6_decision=True,
            ),
        )

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
    enforce_pooled_cp = tier == "standard"
    verdict: dict[str, Any] = {
        "passed": False,
        "tier": tier,
        "require_clean": bool(args.require_clean),
        "rebuild": bool(args.rebuild),
        "live_provenance": None,
        "golden_traces": None,
        "suites": {},
        "errors": [],
    }

    try:
        validate_cli_args(args)
        manifest = load_manifest(args.manifest)
        live_prov = collect_live_provenance(REPO_ROOT)
        verdict["live_provenance"] = live_prov
        if args.require_clean and live_prov.get("git_dirty") is not False:
            raise ValidationError(
                "Live worktree is dirty under --require-clean "
                f"(git_commit_sha={live_prov.get('git_commit_sha')}); commit or clean all changes first"
            )
    except Exception as exc:
        verdict["errors"].append(str(exc))
        _write_json_out(args.json_out, verdict)
        print(f"ERROR: {exc}", file=sys.stderr)
        return 1

    if args.rebuild:
        rebuild_cmd = ["cargo", "build", "--release", "--locked"]
        print(f"[RUN] rebuild: {' '.join(rebuild_cmd)}")
        rebuild_proc = subprocess.run(rebuild_cmd, cwd=REPO_ROOT, capture_output=True, text=True)
        if rebuild_proc.returncode != 0:
            msg = (
                f"cargo build --release --locked failed with exit code {rebuild_proc.returncode}\n"
                f"stdout:\n{rebuild_proc.stdout}\nstderr:\n{rebuild_proc.stderr}"
            )
            verdict["errors"].append(msg)
            _write_json_out(args.json_out, verdict)
            print(f"ERROR: {msg}", file=sys.stderr)
            return 1
        print("[PASS] rebuild")

    golden_cfg = manifest["golden_traces"]
    golden_cmd = list(golden_cfg["command"])
    if args.skip_golden:
        verdict["golden_traces"] = {
            "skipped": True,
            "command": golden_cmd,
            "passed": None,
            "provenance": live_prov,
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
            "provenance": live_prov,
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
        msg = f"Arkhe binary does not exist: {args.binary} (pass --rebuild or run 'cargo build --release --locked')"
        verdict["errors"].append(msg)
        _write_json_out(args.json_out, verdict)
        print(f"ERROR: {msg}", file=sys.stderr)
        return 1

    selected_suites = list(manifest["suites"].keys()) if args.suite == "all" else [args.suite]
    requirements = manifest.get("requirements", {})
    required_tail_metrics = manifest.get("required_tail_metrics")
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
                enforce_pooled_cp=enforce_pooled_cp,
                expected_provenance=live_prov,
                required_tail_metrics=required_tail_metrics,
                require_overall_passed=bool(suite_cfg.get("require_overall_passed", False)),
                require_h6_decision=bool(suite_cfg.get("require_h6_decision", False)),
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
