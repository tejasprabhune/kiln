use std::path::Path;

use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn kiln() -> Command {
    Command::cargo_bin("kiln").expect("kiln binary should be built")
}

fn snapshot_layout(root: &Path) -> String {
    let mut entries: Vec<String> = walkdir(root)
        .into_iter()
        .map(|p| {
            let rel = p.strip_prefix(root).unwrap_or(&p);
            rel.to_string_lossy().replace('\\', "/")
        })
        .filter(|s| !s.is_empty())
        .collect();
    entries.sort();
    entries.join("\n")
}

fn walkdir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn rec(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for entry in rd.flatten() {
            let p = entry.path();
            out.push(p.clone());
            if p.is_dir() {
                rec(&p, out);
            }
        }
    }
    rec(root, &mut out);
    out
}

#[test]
fn new_creates_project_layout() {
    let tmp = tempdir().unwrap();
    kiln()
        .args(["new", "demo"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success()
        .stderr(predicate::str::contains("Created kiln project"));

    let project = tmp.path().join("demo");
    insta::assert_snapshot!("new_demo_layout", snapshot_layout(&project));

    let manifest_text = std::fs::read_to_string(project.join("Kiln.toml")).unwrap();
    assert!(manifest_text.contains("name = \"demo\""));
    assert!(manifest_text.contains("top = \"demo\""));

    let module_text = std::fs::read_to_string(project.join("src/demo.sv")).unwrap();
    assert!(module_text.contains("module demo"));
    assert!(module_text.contains("endmodule"));
}

#[test]
fn new_then_check_manifest_succeeds() {
    let tmp = tempdir().unwrap();
    kiln()
        .args(["new", "widget"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .success();

    let project = tmp.path().join("widget");
    kiln()
        .arg("check-manifest")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains("name = \"widget\""))
        .stdout(predicate::str::contains("top = \"widget\""));
}

#[test]
fn new_rejects_existing_destination() {
    let tmp = tempdir().unwrap();
    std::fs::create_dir_all(tmp.path().join("collision")).unwrap();
    kiln()
        .args(["new", "collision"])
        .arg("--path")
        .arg(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

#[test]
fn check_manifest_reports_invalid_manifest() {
    let tmp = tempdir().unwrap();
    std::fs::write(
        tmp.path().join("Kiln.toml"),
        "[package]\nname = \"1bad\"\nversion = \"0.1.0\"\n[design]\ntop = \"t\"\n",
    )
    .unwrap();
    kiln()
        .arg("check-manifest")
        .current_dir(tmp.path())
        .assert()
        .failure()
        .stderr(predicate::str::contains(
            "not a valid SystemVerilog identifier",
        ));
}
