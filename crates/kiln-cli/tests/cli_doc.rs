//! End-to-end tests for `kiln doc`.
//!
//! Gated behind `e2e` because the doc generator runs slang.

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
fn doc_generates_index_and_navigable_pages() {
    let tmp = copy_example("hello-counter");
    kiln()
        .arg("doc")
        .current_dir(tmp.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("Generated docs"));
    let index = tmp.path().join("target/doc/index.html");
    assert!(index.is_file(), "index.html should exist");
    let counter = tmp.path().join("target/doc/counter.html");
    assert!(counter.is_file(), "counter.html should exist");

    // Cross-reference: index links to counter, counter links back.
    let index_text = std::fs::read_to_string(&index).unwrap();
    assert!(index_text.contains("href=\"counter.html\""));
    let counter_text = std::fs::read_to_string(&counter).unwrap();
    assert!(counter_text.contains("href=\"index.html\""));
}

#[test]
fn doc_output_is_well_formed_html5() {
    let tmp = copy_example("hello-counter");
    kiln().arg("doc").current_dir(tmp.path()).assert().success();
    let index = std::fs::read_to_string(tmp.path().join("target/doc/index.html")).unwrap();

    // Check the markers of a well-formed HTML5 document. We don't pull
    // in a full validator (that would require html5ever or a network
    // call); we assert the structural anchors plus a balanced count of
    // open/close tags for the few tags we control.
    assert!(index.starts_with("<!DOCTYPE html>"));
    assert!(index.contains("<html lang=\"en\">"));
    assert!(index.contains("<meta charset=\"utf-8\">"));
    assert!(index.contains("</html>"));

    for tag in ["body", "head", "ul", "li"] {
        let opens = index.matches(&format!("<{tag}")).count();
        let closes = index.matches(&format!("</{tag}>")).count();
        assert_eq!(opens, closes, "unbalanced <{tag}> in:\n{index}");
    }
}

#[test]
fn doc_picks_up_doc_comments() {
    let tmp = copy_example("hello-counter");
    // Add a `///` comment to the counter module.
    let counter_src = tmp.path().join("src/counter.sv");
    let original = std::fs::read_to_string(&counter_src).unwrap();
    let with_doc = format!(
        "/// A 4-bit synchronous-reset up counter.\n/// Increments every posedge clk while rst_n is high.\n{original}"
    );
    std::fs::write(&counter_src, with_doc).unwrap();
    kiln().arg("doc").current_dir(tmp.path()).assert().success();
    let counter_html = std::fs::read_to_string(tmp.path().join("target/doc/counter.html")).unwrap();
    assert!(counter_html.contains("synchronous-reset up counter"));
}
