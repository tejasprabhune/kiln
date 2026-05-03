//! End-to-end tests for `kiln check`. Gated behind `e2e` because they
//! require slang on PATH.

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
fn check_passes_on_hello_counter() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("check")
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn check_fails_on_lint_demo_with_promoted_width_trunc() {
    let tmp = copy_example("lint-demo");
    let out = kiln()
        .arg("check")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .clone();
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    assert!(
        combined.contains("error:"),
        "expected an error label after width-trunc was promoted; got:\n{combined}"
    );
    assert!(
        combined.contains("width-trunc") || combined.contains("truncates"),
        "expected the width-trunc message; got:\n{combined}"
    );
}

#[test]
fn lint_allow_suppresses_warning() {
    // Edit lint-demo's Kiln.toml to set width-trunc = "off" and verify
    // `kiln check` is clean.
    let tmp = copy_example("lint-demo");
    let manifest = tmp.path().join("Kiln.toml");
    let original = std::fs::read_to_string(&manifest).unwrap();
    let edited = original.replace(r#"width-trunc = "error""#, r#"width-trunc = "off""#);
    std::fs::write(&manifest, edited).unwrap();
    kiln()
        .arg("check")
        .current_dir(tmp.path())
        .assert()
        .success();
}

#[test]
fn check_renders_with_caret() {
    let tmp = copy_example("lint-demo");
    let out = kiln()
        .arg("check")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .get_output()
        .clone();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("^"),
        "expected `^` caret in rendered diagnostic; stdout:\n{stdout}"
    );
}

#[test]
fn check_invalid_manifest_reports_clearly() {
    // `kiln new` is fine; corrupt its manifest.
    let tmp = tempfile::tempdir().unwrap();
    kiln()
        .args(["new", "demo"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();
    let project = tmp.path().join("demo");
    let m = project.join("Kiln.toml");
    std::fs::write(
        &m,
        "[package]\nname=\"1bad\"\nversion=\"0.1.0\"\n[design]\ntop=\"t\"\n",
    )
    .unwrap();
    kiln()
        .arg("check")
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "not a valid SystemVerilog identifier",
        ));
}

#[test]
fn check_completes_quickly_on_hello_counter() {
    let tmp = copy_example("hello-counter");
    let start = std::time::Instant::now();
    kiln()
        .arg("check")
        .current_dir(tmp.path())
        .assert()
        .success();
    let elapsed = start.elapsed();
    // Fail-soft: warn if the check is slower than the milestone target.
    if elapsed > std::time::Duration::from_millis(200) {
        eprintln!(
            "warning: kiln check took {elapsed:?}, exceeds 200ms target (CI runners can be slow)"
        );
    }
}
