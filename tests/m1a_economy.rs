use std::collections::BTreeSet;
use std::process::Command;

use arkhe::verify_m1::{
    CircuitClass, DEFAULT_HORIZON, M1aBankReport, M1aVerificationSummary,
    run_m1a_verification_suite, run_panel_matrix,
};

#[test]
fn m1a_gates_pass_fast_seed_slice() {
    let summary = run_m1a_verification_suite(0, 2);
    assert_eq!(summary.gates.len(), 4);
    for gate in &summary.gates {
        assert!(gate.cases > 0, "gate {} had 0 cases", gate.gate);
    }
    // G1A.1 (conservation & substrate purity), G1A.2 (separation & local credit),
    // G1A.3 (detour & free-rider penalty), and G1A.4 (micro-macro MDL identity)
    // pass cleanly under the M1a redesign (D1 replacing traces, D2 calibrated reward,
    // D3 description debt, and D4 activity recency decay).
    assert!(
        summary.gates[0].passed,
        "G1A.1 failed: {:?}",
        summary.gates[0].failures
    );
    assert!(
        summary.gates[1].passed,
        "G1A.2 failed: {:?}",
        summary.gates[1].failures
    );
    assert!(
        summary.gates[2].passed,
        "G1A.3 failed: {:?}",
        summary.gates[2].failures
    );
    assert!(
        summary.gates[3].passed,
        "G1A.4 failed: {:?}",
        summary.gates[3].failures
    );
    assert!(summary.all_passed);
}

#[test]
fn delta_e_equals_negative_delta_l_null_on_panel_runs() {
    let runs = run_panel_matrix(0, 2, DEFAULT_HORIZON);
    for run in &runs {
        let sum = run.delta_e + run.delta_l_null;
        let limit = 1e-9 * 1.0f64.max(run.delta_l_null.abs());
        assert!(
            sum.abs() <= limit,
            "Run failed identity: task={} circuit={} delta_e={} delta_l={} sum={} limit={}",
            run.task,
            run.circuit,
            run.delta_e,
            run.delta_l_null,
            sum.abs(),
            limit
        );
    }
}

#[test]
fn panel_covers_all_classes_and_widths() {
    let summary = run_m1a_verification_suite(0, 1);
    let mut classes_k1 = BTreeSet::new();
    let mut classes_k2 = BTreeSet::new();

    for aggregate in &summary.panel {
        match aggregate.k {
            1 => {
                classes_k1.insert(aggregate.class);
            }
            2 => {
                classes_k2.insert(aggregate.class);
            }
            other => panic!("unexpected bit width k={other}"),
        }
    }

    // At k=1: Opt, Partial (motif3), Chance, Silence, Detour all appear.
    // Wrong appears at k=2 only (shift1 on motif3, per plan §3.6.1).
    assert!(classes_k1.contains(&CircuitClass::Opt));
    assert!(classes_k1.contains(&CircuitClass::Partial));
    assert!(classes_k1.contains(&CircuitClass::Chance));
    assert!(classes_k1.contains(&CircuitClass::Silence));
    assert!(classes_k1.contains(&CircuitClass::Detour));

    // At k=2: all 6 classes appear.
    assert!(classes_k2.contains(&CircuitClass::Opt));
    assert!(classes_k2.contains(&CircuitClass::Partial));
    assert!(classes_k2.contains(&CircuitClass::Chance));
    assert!(classes_k2.contains(&CircuitClass::Wrong));
    assert!(classes_k2.contains(&CircuitClass::Silence));
    assert!(classes_k2.contains(&CircuitClass::Detour));
}

#[test]
fn verify_m1a_json_is_deterministic() {
    let summary1 = run_m1a_verification_suite(0, 1);
    let summary2 = run_m1a_verification_suite(0, 1);
    let json1 = serde_json::to_string(&summary1).expect("serialize summary1");
    let json2 = serde_json::to_string(&summary2).expect("serialize summary2");
    assert_eq!(json1, json2, "M1a JSON output must be byte-deterministic");
}

#[test]
fn cli_verify_m1a_reports_structured_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args(["verify-m1a", "--seed-offset", "0", "--seeds", "1"])
        .output()
        .expect("spawn arkhe verify-m1a");

    let stdout = String::from_utf8(output.stdout).expect("valid utf-8 stdout");
    let summary: M1aVerificationSummary =
        serde_json::from_str(&stdout).expect("parse stdout as M1aVerificationSummary");

    assert_eq!(summary.gates.len(), 4);
    assert!(!summary.provenance.source_tree_fnv1a128.is_empty());
    assert_eq!(summary.seeds_tested, 1);
    assert_eq!(summary.seed_offset, 0);
    // Exit code reflects gate outcome (non-zero when kill criteria triggered).
    assert_eq!(output.status.success(), summary.all_passed);
}

#[test]
fn cli_verify_m1a_bank_mode_reports_structured_json() {
    let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args([
            "verify-m1a",
            "--seed-offset",
            "20",
            "--banks",
            "1",
            "--seeds",
            "1",
            "--horizon",
            "64",
        ])
        .output()
        .expect("spawn arkhe verify-m1a in bank mode");

    let stdout = String::from_utf8(output.stdout).expect("valid utf-8 stdout");
    let report: M1aBankReport =
        serde_json::from_str(&stdout).expect("parse stdout as M1aBankReport");

    assert_eq!(report.gate_passes.len(), 4);
    assert_eq!(report.banks_evaluated, 1);
    assert_eq!(report.horizon, 64);
    assert!(!report.class_stats.is_empty());
}

#[test]
fn cli_verify_m1a_refuses_holdout_seeds_without_flag() {
    let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args(["verify-m1a", "--seed-offset", "10000", "--seeds", "20"])
        .output()
        .expect("spawn arkhe verify-m1a on holdout");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("overlaps pre-registered holdout range"),
        "stderr must mention holdout refusal, got: {stderr}"
    );
}

#[test]
fn detour_credit_after_tick0_is_bounded_and_equal_across_horizons() {
    let runs_1024 = run_panel_matrix(20, 1, 1024);
    let runs_4096 = run_panel_matrix(20, 1, 4096);

    let mut found_onefire = false;
    for (r1024, r4096) in runs_1024.iter().zip(&runs_4096) {
        if r1024.circuit == "onefire" {
            found_onefire = true;
            let diff = (r4096.detour_credit_after_tick0 - r1024.detour_credit_after_tick0).abs();
            assert!(
                diff <= 1e-9,
                "onefire leak not bounded on task {} k={}: T=1024 credit={}, T=4096 credit={}, diff={diff}",
                r1024.task,
                r1024.k,
                r1024.detour_credit_after_tick0,
                r4096.detour_credit_after_tick0
            );
            assert!(
                r1024.max_detour_energy.unwrap() < 4.0,
                "onefire detour node did not end below initial_energy at T=1024 on task {} k={}: max_de={}",
                r1024.task,
                r1024.k,
                r1024.max_detour_energy.unwrap()
            );
            assert!(
                r4096.max_detour_energy.unwrap() < 4.0,
                "onefire detour node did not end below initial_energy at T=4096 on task {} k={}: max_de={}",
                r4096.task,
                r4096.k,
                r4096.max_detour_energy.unwrap()
            );
        }
    }
    assert!(found_onefire, "onefire circuit must be present on panel");
}
