//! Black-box tests of the `auto-post` binary (no network involved).

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn example_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples/basic")
}

fn auto_post(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_auto-post"))
        .args(args)
        .output()
        .expect("binary runs")
}

#[test]
fn validate_accepts_the_basic_example() {
    let dir = example_dir();
    let output = auto_post(&["validate", "--dir", dir.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&output.stdout).starts_with("OK"));
}

#[test]
fn validate_reports_specific_errors_with_exit_code_1() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("requests")).unwrap();
    std::fs::create_dir(dir.path().join("workflows")).unwrap();
    std::fs::write(
        dir.path().join("workflows/main.json"),
        r#"{"steps":[{"id":"a","request":"ghost"}]}"#,
    )
    .unwrap();

    let output = auto_post(&["validate", "--dir", dir.path().to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown request `ghost`"),
        "stderr: {stderr}"
    );
}

#[test]
fn run_of_unknown_workflow_fails_with_exit_code_1() {
    let dir = example_dir();
    let output = auto_post(&["run", "nope", "--dir", dir.to_str().unwrap()]);
    assert_eq!(output.status.code(), Some(1));
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown workflow `nope`"));
}

#[test]
fn usage_errors_exit_with_code_2() {
    assert_eq!(auto_post(&[]).status.code(), Some(2));
    assert_eq!(auto_post(&["run"]).status.code(), Some(2));
    assert_eq!(
        auto_post(&["run", "profile", "--input", "novalue"])
            .status
            .code(),
        Some(2)
    );
}

#[test]
fn help_exits_with_code_0() {
    assert_eq!(auto_post(&["--help"]).status.code(), Some(0));
}
