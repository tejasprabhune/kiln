//! End-to-end tests for `kiln build`, `kiln run`, and `kiln clean`.
//!
//! Gated behind the `e2e` feature so they don't run on machines without
//! Verilator installed. Local dev: `cargo test -p kiln-cli --features e2e`.
//! CI installs verilator and runs with the feature on.

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
fn build_then_run_prints_pass_for_hello_counter() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Built `tb`"));
    kiln()
        .arg("run")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS"));
}

#[test]
fn second_build_is_a_cache_hit() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success();
    let out = kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    // No "Built ..." print on a cache hit.
    assert!(
        !stdout.contains("Built `tb`"),
        "expected cache hit; got stdout: {stdout}"
    );
}

#[test]
fn editing_source_invalidates_cache() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success();
    // Edit the testbench (cosmetically) and rebuild.
    let tb = tmp.path().join("src/tb.sv");
    let original = std::fs::read_to_string(&tb).unwrap();
    let edited = format!("{original}\n// touched\n");
    std::fs::write(&tb, edited).unwrap();
    let out = kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    assert!(
        stdout.contains("Built `tb`"),
        "expected cache miss after edit; stdout: {stdout}"
    );
}

#[test]
fn syntax_error_reports_correct_file_line_col() {
    let tmp = copy_example("hello-counter");
    // Truncate counter.sv to introduce a syntax error.
    let bad = tmp.path().join("src/counter.sv");
    std::fs::write(&bad, "module counter\n    input logic clk\n);\nendmodule\n").unwrap();
    let out = kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let combined = format!("{stdout}{stderr}");
    assert!(
        combined.contains("counter.sv"),
        "expected the source path in the diagnostic; got:\n{combined}"
    );
    assert!(
        combined.contains(":2:") || combined.contains(":3:"),
        "expected line 2 or 3 in the diagnostic; got:\n{combined}"
    );
    assert!(
        combined.contains("error:"),
        "expected an `error:` label; got:\n{combined}"
    );
    // Plain-text caret rendering should be present.
    assert!(
        combined.contains("^"),
        "expected a `^` caret; got:\n{combined}"
    );
}

#[test]
fn clean_removes_target_kiln() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success();
    assert!(tmp.path().join("target/kiln").is_dir());
    kiln()
        .arg("clean")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Removed build cache"));
    assert!(!tmp.path().join("target/kiln").exists());
}

#[test]
fn release_profile_distinct_from_debug() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("build")
        .current_dir(tmp.path())
        .assert()
        .success();
    let out = kiln()
        .args(["build", "--release"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("profile=release"),
        "expected release profile rebuild; got: {stdout}"
    );
}
