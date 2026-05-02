use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

fn kiln() -> Command {
    Command::cargo_bin("kiln").expect("kiln binary should be built")
}

#[test]
fn init_uses_directory_name_as_package_name() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("counter_top");
    std::fs::create_dir_all(&project).unwrap();

    kiln()
        .arg("init")
        .current_dir(&project)
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "Initialized kiln project `counter_top`",
        ));

    let manifest = std::fs::read_to_string(project.join("Kiln.toml")).unwrap();
    assert!(manifest.contains("name = \"counter_top\""));
    assert!(project.join("src/counter_top.sv").exists());
    assert!(project.join("tests/.gitkeep").exists());
    assert!(project.join(".gitignore").exists());
}

#[test]
fn init_with_explicit_name_overrides_directory() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("not_a_valid_sv_name__no_actually_it_is");
    std::fs::create_dir_all(&project).unwrap();

    kiln()
        .args(["init", "--name", "explicit_name"])
        .current_dir(&project)
        .assert()
        .success();

    let manifest = std::fs::read_to_string(project.join("Kiln.toml")).unwrap();
    assert!(manifest.contains("name = \"explicit_name\""));
    assert!(project.join("src/explicit_name.sv").exists());
}

#[test]
fn init_refuses_when_manifest_exists() {
    let tmp = tempdir().unwrap();
    let project = tmp.path().join("already_inited");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("Kiln.toml"), "[package]\nname=\"x\"\n").unwrap();

    kiln()
        .args(["init", "--name", "fresh"])
        .current_dir(&project)
        .assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}
