//! End-to-end tests for `kiln add`, `kiln remove`, `kiln update`,
//! `kiln tree`, and `kiln build` against a path-based dependency.
//!
//! Gated behind the `e2e` feature because the build path also needs
//! Verilator. `bender` is a `cargo install`-able pure-Rust binary and
//! is treated as a runtime dep; the test asserts up front that it is
//! on PATH.

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

fn copy_with_deps_example() -> tempfile::TempDir {
    // Copy both `with-deps` and `local-ip` so the relative path dep
    // resolves inside the tempdir.
    let tmp = tempfile::tempdir().unwrap();
    let src_root = workspace_root().join("examples");
    for sub in ["with-deps", "local-ip"] {
        copy_recursive(&src_root.join(sub), &tmp.path().join(sub));
    }
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

fn assert_bender_present() {
    if which::which("bender").is_err() {
        panic!("`bender` must be on PATH for these tests (cargo install bender)");
    }
}

#[test]
fn build_with_path_dep_prints_pass() {
    assert_bender_present();
    let tmp = copy_with_deps_example();
    let project = tmp.path().join("with-deps");
    kiln()
        .arg("build")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("Built `consumer_top`"));
    kiln()
        .arg("run")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("PASS"));
}

#[test]
fn update_writes_kiln_lock() {
    assert_bender_present();
    let tmp = copy_with_deps_example();
    let project = tmp.path().join("with-deps");
    kiln()
        .arg("update")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("Updated `Kiln.lock`"));
    let lock = std::fs::read_to_string(project.join("Kiln.lock")).unwrap();
    assert!(lock.contains("local_ip"));
}

#[test]
fn add_path_dep_modifies_manifest() {
    assert_bender_present();
    let tmp = copy_with_deps_example();
    let project = tmp.path().join("with-deps");
    let local_ip_abs = tmp.path().join("local-ip");

    // First, remove the existing local_ip entry, then re-add it.
    kiln()
        .args(["remove", "local_ip"])
        .current_dir(&project)
        .assert()
        .success();
    let manifest_after_remove = std::fs::read_to_string(project.join("Kiln.toml")).unwrap();
    assert!(!manifest_after_remove.contains("local_ip"));

    kiln()
        .args(["add", "local_ip", "--path"])
        .arg(&local_ip_abs)
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("Added dependency `local_ip`"));
    let manifest_after_add = std::fs::read_to_string(project.join("Kiln.toml")).unwrap();
    assert!(manifest_after_add.contains("local_ip"));
    assert!(manifest_after_add.contains("path"));
}

#[test]
fn tree_lists_local_ip() {
    assert_bender_present();
    let tmp = copy_with_deps_example();
    let project = tmp.path().join("with-deps");
    kiln()
        .arg("tree")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("local_ip"));
}

#[test]
fn add_requires_git_or_path() {
    assert_bender_present();
    let tmp = copy_with_deps_example();
    let project = tmp.path().join("with-deps");
    kiln()
        .args(["add", "newdep"])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("--git").or(predicate::str::contains("--path")));
}
