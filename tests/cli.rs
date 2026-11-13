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
