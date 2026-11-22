use std::path::PathBuf;
use std::process::Command;

#[test]
fn test_greeting() {
    assert_eq!(arkhe::greeting(), "arkhe m0");
}

#[test]
fn documentation_relative_links_and_anchors_are_valid() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let script = manifest_dir.join("scripts").join("check_doc_links.py");
    let output = Command::new("python3")
        .arg(&script)
        .current_dir(&manifest_dir)
        .output()
        .expect("python3 must be available to run scripts/check_doc_links.py");
    assert!(
        output.status.success(),
        "documentation link/anchor check failed:\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn cli_verify_m2a_refuses_holdout_ranges() {
    for seed_offset in [
        "10000", "30000", "40000", "50000", "50100", "60000", "60050",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args(["verify-m2a", "--seed-offset", seed_offset, "--seeds", "20"])
            .output()
            .expect("spawn arkhe verify-m2a on holdout");
        assert!(
            !output.status.success(),
            "verify-m2a must refuse seed_offset={seed_offset} without --holdout"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("overlaps pre-registered") && stderr.contains("holdout range"),
            "stderr for {seed_offset} must mention holdout refusal, got: {stderr}"
        );
    }

    // M1, closed M2a (30000), closed M2b (40000), closed M3 v1 (50000), M3 v2 (50100),
    // and M4a (60000..=60099) are all unconditionally refused even with --holdout.
    for seed_offset in [
        "10000", "30000", "40000", "50000", "50100", "60000", "60050",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args([
                "verify-m2a",
                "--seed-offset",
                seed_offset,
                "--seeds",
                "20",
                "--holdout",
            ])
            .output()
            .expect("spawn arkhe verify-m2a --holdout on closed/forbidden holdout range");
        assert!(
            !output.status.success(),
            "verify-m2a --holdout must still refuse closed/forbidden holdout seed_offset={seed_offset}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unconditionally forbidden under verify-m2a"),
            "stderr for {seed_offset} --holdout must mention unconditional refusal, got: {stderr}"
        );
    }
}

#[test]
fn cli_verify_m2b_refuses_holdout_ranges() {
    for seed_offset in [
        "10000", "30000", "40000", "50000", "50100", "60000", "60050",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args(["verify-m2b", "--seed-offset", seed_offset, "--seeds", "20"])
            .output()
            .expect("spawn arkhe verify-m2b on holdout");
        assert!(
            !output.status.success(),
            "verify-m2b must refuse seed_offset={seed_offset} without --holdout"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("overlaps pre-registered") && stderr.contains("holdout range"),
            "stderr for {seed_offset} must mention holdout refusal, got: {stderr}"
        );
    }

    // M1 (10000), closed M2a (30000), closed M2b (40000), closed M3 v1 (50000),
    // M3 v2 (50100), and M4a (60000..=60099) are all unconditionally forbidden even with --holdout.
    for seed_offset in [
        "10000", "30000", "40000", "50000", "50100", "60000", "60050",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args([
                "verify-m2b",
                "--seed-offset",
                seed_offset,
                "--seeds",
                "20",
                "--holdout",
            ])
            .output()
            .expect("spawn arkhe verify-m2b --holdout on closed/forbidden holdout range");
        assert!(
            !output.status.success(),
            "verify-m2b --holdout must still refuse closed/forbidden holdout seed_offset={seed_offset}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unconditionally forbidden under verify-m2b"),
            "stderr for {seed_offset} --holdout must mention unconditional refusal, got: {stderr}"
        );
    }

    // Multi-bank overlap check against closed M2b holdout even with --holdout and M3 with --holdout.
    let bank_m2b = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args([
            "verify-m2b",
            "--banks",
            "5",
            "--seed-offset",
            "39920",
            "--seeds",
            "20",
            "--holdout",
        ])
        .output()
        .expect("spawn arkhe verify-m2b --banks overlapping M2b holdout");
    assert!(!bank_m2b.status.success());
    assert!(
        String::from_utf8_lossy(&bank_m2b.stderr)
            .contains("unconditionally forbidden under verify-m2b")
    );

    let bank_m3 = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args([
            "verify-m2b",
            "--banks",
            "5",
            "--seed-offset",
            "49920",
            "--seeds",
            "20",
            "--holdout",
        ])
        .output()
        .expect("spawn arkhe verify-m2b --banks overlapping M3 holdout");
    assert!(!bank_m3.status.success());
    assert!(
        String::from_utf8_lossy(&bank_m3.stderr)
            .contains("unconditionally forbidden under verify-m2b")
    );
}

#[test]
fn cli_verify_m3_refuses_holdout_ranges() {
    for seed_offset in [
        "10000", "30000", "40000", "50000", "50100", "60000", "60050",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args(["verify-m3", "--seed-offset", seed_offset, "--seeds", "20"])
            .output()
            .expect("spawn arkhe verify-m3 on holdout");
        assert!(
            !output.status.success(),
            "verify-m3 must refuse seed_offset={seed_offset} without --holdout"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("overlaps pre-registered") && stderr.contains("holdout range"),
            "stderr for {seed_offset} must mention holdout refusal, got: {stderr}"
        );
    }

    // M1 (10000), closed M2a (30000), closed M2b (40000), closed M3 v1 (50000),
    // and M4a (60000..=60099) remain unconditionally forbidden even with --holdout.
    for seed_offset in ["10000", "30000", "40000", "50000", "60000", "60050"] {
        let output = Command::new(env!("CARGO_BIN_EXE_arkhe"))
            .args([
                "verify-m3",
                "--seed-offset",
                seed_offset,
                "--seeds",
                "20",
                "--holdout",
            ])
            .output()
            .expect("spawn arkhe verify-m3 --holdout on closed/sibling holdout range");
        assert!(
            !output.status.success(),
            "verify-m3 --holdout must still refuse closed/sibling holdout seed_offset={seed_offset}"
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("unconditionally forbidden under verify-m3"),
            "stderr for {seed_offset} --holdout must mention unconditional refusal, got: {stderr}"
        );
    }

    let bank_m3_v2 = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args([
            "verify-m3",
            "--banks",
            "5",
            "--seed-offset",
            "50020",
            "--seeds",
            "20",
        ])
        .output()
        .expect("spawn arkhe verify-m3 --banks overlapping M3 v2 holdout without --holdout");
    assert!(!bank_m3_v2.status.success());
    assert!(String::from_utf8_lossy(&bank_m3_v2.stderr).contains("overlaps pre-registered"));

    let bank_m4a = Command::new(env!("CARGO_BIN_EXE_arkhe"))
        .args([
            "verify-m3",
            "--banks",
            "5",
            "--seed-offset",
            "59920",
            "--seeds",
            "20",
            "--holdout",
        ])
        .output()
        .expect("spawn arkhe verify-m3 --banks overlapping M4a holdout");
    assert!(!bank_m4a.status.success());
    assert!(
        String::from_utf8_lossy(&bank_m4a.stderr)
            .contains("unconditionally forbidden under verify-m3")
    );
}
