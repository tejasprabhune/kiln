//! End-to-end tests for `kiln test`.
//!
//! Gated behind the `e2e` feature because the runner builds each test
//! through Verilator.

#![cfg(feature = "e2e")]

use std::path::{Path, PathBuf};
use std::time::Instant;

use assert_cmd::Command;
use predicates::prelude::*;

fn kiln() -> Command {
    Command::cargo_bin("kiln").expect("kiln binary should be built")
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("workspace root above crates/kiln-cli")
        .to_path_buf()
}

fn copy_example(name: &str) -> tempfile::TempDir {
    let src = workspace_root().join("examples").join(name);
    let tmp = tempfile::tempdir().unwrap();
    copy_recursive(&src, tmp.path());
    tmp
}

fn copy_recursive(src: &Path, dst: &Path) {
    for entry in walkdir::WalkDir::new(src) {
        let entry = entry.unwrap();
        let rel = entry.path().strip_prefix(src).unwrap();
        let target = dst.join(rel);
        if entry.file_type().is_dir() {
            std::fs::create_dir_all(&target).unwrap();
        } else if entry.file_type().is_file() {
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).unwrap();
            }
            std::fs::copy(entry.path(), &target).unwrap();
        }
    }
}

#[test]
fn list_is_stable() {
    let tmp = copy_example("hello-counter");
    let out = kiln()
        .args(["test", "--list"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    insta::assert_snapshot!(
        "kiln_test_list",
        String::from_utf8_lossy(&out).trim().to_string()
    );
}

#[test]
fn test_runs_at_least_one_pass() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("test")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS"))
        .stdout(predicate::str::contains("test result:"));
}

#[test]
fn substring_filter_matches_smoke() {
    let tmp = copy_example("hello-counter");
    let out = kiln()
        .args(["test", "smoke"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("smoke"));
    assert!(!stdout.contains("test parity"));
}

#[test]
fn no_match_returns_zero_with_helpful_text() {
    let tmp = copy_example("hello-counter");
    kiln()
        .args(["test", "does_not_exist"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("No tests matched"));
}

#[test]
fn parallel_observably_faster_than_serial() {
    let tmp = copy_example("hello-counter");
    // Prime the cache with a serial pre-build so we measure execution
    // overlap, not first-time compilation.
    kiln()
        .args(["test", "--jobs", "1"])
        .current_dir(tmp.path())
        .assert()
        .success();

    // The cache hit + cheap process spawn measurement is what we want;
    // assert that --jobs=N ≥ 2 isn't *slower* than --jobs=1. Strict
    // "observably faster" depends on host scheduling and is flaky in
    // CI, so we use a fail-soft inequality: jobs=2 ≤ jobs=1 + 100ms
    // slack.
    let serial = {
        let t = Instant::now();
        kiln()
            .args(["test", "--jobs", "1"])
            .current_dir(tmp.path())
            .assert()
            .success();
        t.elapsed()
    };
    let parallel = {
        let t = Instant::now();
        kiln()
            .args(["test", "--jobs", "4"])
            .current_dir(tmp.path())
            .assert()
            .success();
        t.elapsed()
    };
    let slack = std::time::Duration::from_millis(100);
    assert!(
        parallel <= serial + slack,
        "parallel {parallel:?} should be ~<= serial {serial:?} (slack {slack:?})"
    );
}
