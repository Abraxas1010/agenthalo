use std::path::PathBuf;
use std::process::Command;

#[test]
fn repository_theory_boundary_scan_passes() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let output = Command::new("python3")
        .arg("scripts/check_theory_boundary.py")
        .current_dir(&repo_root)
        .output()
        .expect("run theory boundary scan");

    assert!(
        output.status.success(),
        "theory boundary scan failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
