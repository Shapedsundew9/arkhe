//! Compile-time provenance capture for reproducible `RunReport` and `verify-m0` manifests.
//!
//! Records toolchain version, target triple, `Cargo.lock` FNV-1a-128 digest, and (when built inside
//! a Git checkout) commit SHA and working-tree dirty state. Degrades cleanly to `"unavailable"` /
//! `"none"` when invoked inside packaged source archives (`cargo package`) where `.git` is absent.

use std::env;
use std::fs;
use std::path::Path;
use std::process::Command;

fn fnv1a_128_hex(bytes: &[u8]) -> String {
    const OFFSET_BASIS: u128 = 0x6c62_272e_07bb_0142_62b8_2175_6295_c58d;
    const PRIME: u128 = 0x0000_0000_0100_0000_0000_0000_0000_013b;
    let mut hash = OFFSET_BASIS;
    for &b in bytes {
        hash ^= u128::from(b);
        hash = hash.wrapping_mul(PRIME);
    }
    format!("{hash:032x}")
}

fn main() {
    println!("cargo::rerun-if-changed=scripts/build.rs");

    let manifest_dir = env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".to_owned());
    let root = Path::new(&manifest_dir);

    let lock_path = root.join("Cargo.lock");
    let lock_digest = if lock_path.is_file() {
        println!("cargo::rerun-if-changed=Cargo.lock");
        fs::read(&lock_path).map_or_else(|_| "unavailable".to_owned(), |b| fnv1a_128_hex(&b))
    } else {
        "unavailable".to_owned()
    };
    println!("cargo::rustc-env=ARKHE_CARGO_LOCK_FNV1A128={lock_digest}");

    let target = env::var("TARGET").unwrap_or_else(|_| "unknown".to_owned());
    println!("cargo::rustc-env=ARKHE_TARGET={target}");

    let rustc_bin = env::var("RUSTC").unwrap_or_else(|_| "rustc".to_owned());
    let rustc_version = Command::new(rustc_bin)
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map_or_else(|| "unknown".to_owned(), |s| s.trim().to_owned());
    println!("cargo::rustc-env=ARKHE_RUSTC_VERSION={rustc_version}");

    let git_dir = root.join(".git");
    let (git_sha, git_dirty) = if git_dir.exists() {
        if git_dir.join("HEAD").is_file() {
            println!("cargo::rerun-if-changed=.git/HEAD");
        }
        if git_dir.join("index").is_file() {
            println!("cargo::rerun-if-changed=.git/index");
        }

        let sha = Command::new("git")
            .args(["rev-parse", "HEAD"])
            .current_dir(root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map_or_else(|| "none".to_owned(), |s| s.trim().to_owned());

        let dirty = Command::new("git")
            .args(["status", "--porcelain"])
            .current_dir(root)
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map_or_else(|| "none".to_owned(), |s| (!s.trim().is_empty()).to_string());

        (sha, dirty)
    } else {
        ("none".to_owned(), "none".to_owned())
    };

    println!("cargo::rustc-env=ARKHE_GIT_SHA={git_sha}");
    println!("cargo::rustc-env=ARKHE_GIT_DIRTY={git_dirty}");
}
