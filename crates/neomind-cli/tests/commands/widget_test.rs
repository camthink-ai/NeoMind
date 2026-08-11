//! Tests for the `widget` command and its subcommands.

use assert_cmd::Command;
use predicates::prelude::*;

/// Test widget command help.
#[test]
fn test_widget_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Widget management commands"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("get"))
        .stdout(predicate::str::contains("bundle"))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("uninstall"))
        .stdout(predicate::str::contains("market-list"))
        .stdout(predicate::str::contains("market-install"));
}

/// Test widget list help.
#[test]
fn test_widget_list_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("list").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("List installed widgets"));
}

/// Test widget get help.
#[test]
fn test_widget_get_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("get").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Get widget details"));
}

/// Test widget bundle help.
#[test]
fn test_widget_bundle_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("bundle").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Get widget bundle"));
}

/// Test widget install help.
#[test]
fn test_widget_install_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("install").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Install widget from"));
}

/// Test widget uninstall help.
#[test]
fn test_widget_uninstall_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("uninstall").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Uninstall widget"));
}

/// Test widget market-list help.
#[test]
fn test_widget_market_list_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("market-list").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("List marketplace widgets"));
}

/// Test widget market-install help.
#[test]
fn test_widget_market_install_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("widget").arg("market-install").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Install widget from marketplace"));
}
