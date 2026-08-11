//! Tests for the `extension` command and its subcommands.

#![allow(deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::fs::File;
use std::io::Write;
use tempfile::TempDir;

/// Test that extension command requires a subcommand.
#[test]
fn test_extension_requires_subcommand() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension");

    // Clap displays usage on stdout with exit code 2
    cmd.assert().failure().code(2);
}

/// Test extension validate requires path argument.
#[test]
fn test_extension_validate_requires_path() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("validate");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"))
        .stderr(predicate::str::contains("<PATH>"));
}

/// Test extension validate with verbose flag.
#[test]
fn test_extension_validate_verbose_flag() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension")
        .arg("validate")
        .arg("/nonexistent/path.wasm")
        .arg("--verbose");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

/// Test extension create command generates scaffold files.
#[test]
fn test_extension_create_command() {
    let temp_dir = TempDir::new().unwrap();
    let output_path = temp_dir.path().join("test-extension");

    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension")
        .arg("create")
        .arg("test-extension")
        .arg("--output")
        .arg(&output_path);

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Extension created"));

    // Verify key files were generated
    assert!(output_path.join("Cargo.toml").exists());
    assert!(output_path.join("src/lib.rs").exists());
    assert!(output_path.join("manifest.json").exists());
    assert!(output_path.join(".gitignore").exists());
}

/// Test extension create rejects invalid names (spaces, uppercase).
#[test]
fn test_extension_create_invalid_name() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("create").arg("Invalid Name");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("kebab-case"));
}

/// Test extension create rejects directory that already exists.
#[test]
fn test_extension_create_existing_dir() {
    let temp_dir = TempDir::new().unwrap();
    let existing = temp_dir.path().join("my-ext");
    std::fs::create_dir_all(&existing).unwrap();

    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension")
        .arg("create")
        .arg("my-ext")
        .arg("--output")
        .arg(&existing);

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("already exists"));
}

/// Test extension create help shows all valid types.
#[test]
fn test_extension_create_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("create").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Create a new extension scaffold"))
        .stdout(predicate::str::contains("--extension-type"));
}

/// Test extension list command (requires running server).
#[test]
#[ignore]
fn test_extension_list_command() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("list");

    cmd.assert().success();
}

/// Test extension list with verbose flag.
/// Requires a running server (same as `test_extension_list_command`).
#[test]
#[ignore]
fn test_extension_list_verbose() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("list").arg("--verbose");

    cmd.assert().success();
}

/// Test extension info command requires path.
#[test]
fn test_extension_info_requires_id() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("info");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("required"))
        .stderr(predicate::str::contains("<ID_OR_PATH>"));
}

/// Test extension info with nonexistent file.
#[test]
fn test_extension_info_nonexistent_file() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension")
        .arg("info")
        .arg("/tmp/nonexistent-extension-12345.nep");

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("not found"));
}

/// Test extension validate with a fake .nep package.
#[test]
fn test_extension_validate_fake_nep() {
    let temp_dir = TempDir::new().unwrap();
    let fake_nep = temp_dir.path().join("test.nep");

    let mut file = File::create(&fake_nep).unwrap();
    file.write_all(b"fake nep content").unwrap();

    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("validate").arg(&fake_nep);

    cmd.assert()
        .failure()
        .stderr(predicate::str::contains("invalid Zip archive"));
}

/// Test extension install command with nonexistent file.
#[test]
fn test_extension_install_nonexistent() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension")
        .arg("install")
        .arg("/tmp/nonexistent-extension-12345.nep");

    cmd.assert().failure();
}

/// Test extension help shows all subcommands.
#[test]
fn test_extension_help() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("extension").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("Extension management commands"))
        .stdout(predicate::str::contains("validate"))
        .stdout(predicate::str::contains("list"))
        .stdout(predicate::str::contains("info"))
        .stdout(predicate::str::contains("install"))
        .stdout(predicate::str::contains("uninstall"))
        .stdout(predicate::str::contains("create"))
        .stdout(predicate::str::contains("status"))
        .stdout(predicate::str::contains("logs"))
        .stdout(predicate::str::contains("build"));
}
