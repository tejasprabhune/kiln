//! End-to-end tests for `kiln fmt`. Gated behind the `e2e` feature
//! because they require `verible-verilog-format` on PATH.

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
fn fmt_check_reports_diff_on_unformatted_input() {
    let tmp = copy_example("hello-counter");
    kiln()
        .args(["fmt", "--check"])
        .current_dir(tmp.path())
        .assert()
        .failure()
        // After the UX refresh, the diff goes to stdout (--- header etc.)
        // and the summary goes to stderr ("need formatting"). Either is
        // sufficient evidence that --check detected a problem.
        .stderr(predicate::str::contains("need formatting").or(predicate::str::contains("---")));
}

#[test]
fn fmt_in_place_then_check_succeeds() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("fmt")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("formatted").or(predicate::str::contains("Result")));
    kiln()
        .args(["fmt", "--check"])
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn fmt_check_json_has_documented_shape() {
    let tmp = copy_example("hello-counter");
    let out = kiln()
        .args(["fmt", "--check", "--format", "json"])
        .current_dir(tmp.path())
        .assert()
        // Exit-1 on any unformatted; we intentionally take the failure path.
        .failure()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v.get("results").is_some(), "missing `results`: {v}");
    assert!(v.get("summary").is_some(), "missing `summary`: {v}");
    assert!(v["summary"]["total"].is_number());
    assert!(v["summary"]["needs_formatting"].is_number());
    let results = v["results"].as_array().unwrap();
    assert!(!results.is_empty());
    let first = &results[0];
    assert!(first["file"].is_string());
    assert!(first["ok"].is_boolean());
    assert!(first["diff"].is_string());
}

#[test]
fn fmt_json_after_format_lists_only_formatted() {
    let tmp = copy_example("hello-counter");
    let out = kiln()
        .args(["fmt", "--format", "json"])
        .current_dir(tmp.path())
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();
    let stdout = String::from_utf8_lossy(&out);
    let v: serde_json::Value = serde_json::from_str(&stdout).expect("valid JSON");
    assert!(v["formatted"].is_array());
    assert!(v["unchanged"].is_array());
}
