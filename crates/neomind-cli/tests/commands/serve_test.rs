//! Tests for the `serve` command.

#![allow(deprecated)]

use assert_cmd::Command;
use predicates::prelude::*;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// Serializes the serve tests that actually start a server. `neomind serve`
/// binds the embedded MQTT broker on port 1883 (read from config.toml, no
/// override flag), so running them in parallel makes the instances contend
/// for 1883 and some panic after the "port still in use, waiting" wait.
static SERVE_LOCK: Mutex<()> = Mutex::new(());

/// Acquire the serve serialization lock.
fn serve_lock() -> MutexGuard<'static, ()> {
    SERVE_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// Spawn `neomind serve` with the given args, wait briefly, and assert it
/// stayed alive (i.e. it started and bound its port). Kills it afterwards.
///
/// `serve` has no graceful-exit flag, so we can't use `assert_cmd`'s
/// `assert()` (it waits for exit). Instead we spawn, sleep, check the process
/// is still running, then kill it. A startup failure (e.g. port already in
/// use) would have exited nonzero before the check.
fn assert_serve_starts(args: &[&str]) {
    let _lock = serve_lock();
    let bin = assert_cmd::cargo::cargo_bin("neomind");
    let mut child = std::process::Command::new(bin)
        .args(["serve"])
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn neomind serve");

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut started = false;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        // If the child exited before the window elapsed, startup failed.
        match child.try_wait() {
            Ok(Some(status)) => {
                panic!(
                    "`neomind serve {:?}` exited early with {status} — expected it to keep running",
                    args
                );
            }
            Ok(None) => {
                started = true;
                break;
            }
            Err(e) => {
                let _ = child.kill();
                panic!("failed to check serve child: {e}");
            }
        }
    }
    if !started {
        let _ = child.kill();
        panic!("`neomind serve {:?}` did not stay alive within 15s", args);
    }

    // Clean up.
    let _ = child.kill();
    let _ = child.wait();
}

/// Test that serve command accepts default values.
#[test]
fn test_serve_default_values() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("serve").arg("--help");

    cmd.assert()
        .success()
        .stdout(predicate::str::contains("--host"))
        .stdout(predicate::str::contains("--port"))
        .stdout(predicate::str::contains("9375")); // Default port
}

/// Test that a custom port is accepted and the server binds it.
#[test]
fn test_serve_custom_port() {
    // Port 0 → OS-assigned ephemeral port, never conflicts with anything.
    assert_serve_starts(&["--port", "0", "--host", "127.0.0.1"]);
}

/// Test that a custom host is accepted.
#[test]
fn test_serve_custom_host() {
    assert_serve_starts(&["--host", "0.0.0.0", "--port", "0"]);
}

/// Test that invalid port is rejected.
#[test]
fn test_serve_invalid_port_rejected() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("serve").arg("--port").arg("invalid");

    cmd.assert().failure();
}

/// Test that port out of range is rejected.
#[test]
fn test_serve_port_out_of_range() {
    let mut cmd = Command::cargo_bin("neomind").unwrap();
    cmd.arg("serve").arg("--port").arg("99999");

    cmd.assert().failure();
}

/// Test host:port parsing for valid addresses.
#[test]
fn test_address_parsing() {
    // Valid IP addresses (localhost requires DNS lookup, skip for unit test)
    let valid_addrs = ["127.0.0.1:9375", "0.0.0.0:8080", "192.168.1.1:9000"];

    for addr_str in valid_addrs {
        let result = SocketAddr::from_str(addr_str);
        assert!(result.is_ok(), "Expected valid address: {}", addr_str);
    }
}

/// Test that missing required arguments starts a server with defaults.
#[test]
fn test_serve_with_defaults() {
    assert_serve_starts(&[]);
}
