//! Server self-upgrade (web-triggered, Linux/systemd deployments).
//!
//! Two-phase design, split by privilege boundary:
//!
//! 1. **Stage** (this process, `neomind` user): the API downloads the new
//!    release into a writable staging dir under the data dir, verifies the
//!    downloaded binary reports the target version, then writes a trigger
//!    file. The API process itself cannot replace `/usr/local/bin/neomind`:
//!    it runs as the sandboxed `neomind` user under `ProtectSystem=full`
//!    (read-only mounts), and its `NoNewPrivileges=true` flag means even a
//!    `sudo` child could never gain root — only a systemd-started unit
//!    escapes both.
//!
//! 2. **Apply** (`neomind-upgrade-apply.service`, root, no sandbox): started
//!    by the `neomind-upgrade-apply.path` unit, which watches the trigger
//!    file with inotify (no sudoers/polkit needed — writing a file in the
//!    data dir is the only capability required). It runs
//!    `neomind upgrade --apply-staged --yes`, which reads the staged
//!    manifest, backs up + atomically swaps the binaries, swaps the web dir,
//!    restarts the service, and cleans the staging dir.
//!
//! `common` holds the shared release/semver/download primitives used by both
//! phases and by the interactive CLI path; `env` detects the deployment
//! environment; `service` is the staged orchestration state machine exposed
//! via the admin API (`/api/system/upgrade*` in `handlers::system`).

pub mod common;
pub mod env;
pub mod service;
