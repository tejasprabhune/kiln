use assert_cmd::Command;
use predicates::prelude::*;

fn kiln() -> Command {
    Command::cargo_bin("kiln").expect("kiln binary should be built")
}

#[test]
fn version_flag_prints_version() {
    kiln()
        .arg("--version")
        .assert()
        .success()
        .stdout(predicate::str::contains("kiln"))
        .stdout(predicate::str::contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn short_version_flag_prints_version() {
    kiln()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("kiln"));
}

#[test]
fn help_lists_subcommands() {
    kiln()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("new"))
        .stdout(predicate::str::contains("init"));
}

#[test]
fn no_args_shows_help_and_exits_non_zero() {
    kiln().assert().failure();
}
