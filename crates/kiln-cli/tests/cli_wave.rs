//! End-to-end tests for `kiln test --trace` and `kiln wave`.
//!
//! Gated behind the `e2e` feature. Verilator is required (the test
//! invocation builds + runs each testbench). Surfer is **not** required
//! by these tests; we use `kiln wave --print-path` to assert the FST
//! exists, which doesn't need to spawn the GUI.

#![cfg(feature = "e2e")]

use std::path::{Path, PathBuf};

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
fn test_trace_writes_nonempty_fst() {
    let tmp = copy_example("hello-counter");
    kiln()
        .args(["test", "--trace"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS"));

    let waves = tmp.path().join("target/kiln/waves");
    assert!(waves.is_dir(), "expected waves dir at {}", waves.display());
    let smoke = waves.join("smoke.fst");
    assert!(smoke.is_file(), "expected smoke.fst at {}", smoke.display());
    let size = std::fs::metadata(&smoke).unwrap().len();
    assert!(size > 0, "smoke.fst is empty");
}

#[test]
fn wave_print_path_returns_most_recent() {
    let tmp = copy_example("hello-counter");
    kiln()
        .args(["test", "--trace"])
        .current_dir(tmp.path())
        .assert()
        .success();
    kiln()
        .args(["wave", "--print-path"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains(".fst"));
}

#[test]
fn wave_for_named_test_returns_that_path() {
    let tmp = copy_example("hello-counter");
    kiln()
        .args(["test", "--trace"])
        .current_dir(tmp.path())
        .assert()
        .success();
    let out = kiln()
        .args(["wave", "smoke", "--print-path"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(stdout.contains("smoke.fst"));
}

#[test]
fn wave_missing_test_errors_clearly() {
    let tmp = copy_example("hello-counter");
    kiln()
        .args(["test", "--trace"])
        .current_dir(tmp.path())
        .assert()
        .success();
    kiln()
        .args(["wave", "does_not_exist", "--print-path"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("does_not_exist"));
}
