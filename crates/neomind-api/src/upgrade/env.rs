//! Deployment-environment detection for the self-upgrade feature.
//!
//! The web-triggered upgrade only makes sense on Linux hosts installed via
//! `scripts/install.sh` (systemd unit + sudoers helper). Docker containers
//! upgrade by replacing the image; macOS/Windows bare-metal re-runs the
//! installer. The check endpoint reports which case applies so the frontend
//! can show the right hint instead of a broken button.

use std::path::Path;

/// Where this server is running, as far as upgrade support is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeploymentEnv {
    /// Docker/container deployment — upgrade = replace the image.
    Docker,
    /// Linux + systemd `neomind` unit — web upgrade path applies.
    Systemd,
    /// Anything else (dev workstations, launchd, raw binaries).
    Unsupported,
}

/// Detect the deployment environment.
///
/// Docker: `/.dockerenv` exists in every docker container, or the Dockerfile
/// sets `NEOMIND_IN_DOCKER=1` as a belt-and-suspenders marker (also covers
/// rare container runtimes that don't mount the marker file).
/// Systemd: Linux + `/run/systemd/system` (booted under systemd) + either an
/// active `neomind` unit or the unit file on disk (inactive-but-installed
/// still allows upgrade — the apply step runs the binary directly).
pub fn detect() -> DeploymentEnv {
    if Path::new("/.dockerenv").exists()
        || std::env::var("NEOMIND_IN_DOCKER")
            .map(|v| v == "1")
            .unwrap_or(false)
    {
        return DeploymentEnv::Docker;
    }
    if cfg!(target_os = "linux") && Path::new("/run/systemd/system").is_dir() {
        let unit_on_disk = Path::new("/etc/systemd/system/neomind.service").exists();
        let unit_active = std::process::Command::new("systemctl")
            .args(["is-active", "--quiet", "neomind"])
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if unit_on_disk || unit_active {
            return DeploymentEnv::Systemd;
        }
    }
    DeploymentEnv::Unsupported
}

impl DeploymentEnv {
    pub fn as_str(self) -> &'static str {
        match self {
            DeploymentEnv::Docker => "docker",
            DeploymentEnv::Systemd => "systemd",
            DeploymentEnv::Unsupported => "unsupported",
        }
    }

    /// Human/operator-facing hint shown by the frontend when web upgrade
    /// cannot run in this environment.
    pub fn upgrade_hint(self) -> Option<&'static str> {
        match self {
            DeploymentEnv::Docker => Some("Docker 部署请升级镜像：docker compose pull && docker compose up -d\nFor Docker deployments, upgrade by pulling a new image: docker compose pull && docker compose up -d"),
            DeploymentEnv::Unsupported => Some("仅支持 Linux systemd 部署的在线升级；其他环境请重新运行安装脚本\nWeb upgrade supports Linux systemd installs only; other environments should re-run scripts/install.sh"),
            DeploymentEnv::Systemd => None,
        }
    }
}

/// Whether the privileged apply step is wired up on this host: both the
/// helper service unit and its path unit (the trigger watcher) must exist —
/// both are written by `scripts/install.sh`. Deployments that predate the
/// installer writing them need one re-run of install.sh to enable
/// web-triggered upgrades.
pub fn helper_available() -> bool {
    Path::new("/etc/systemd/system/neomind-upgrade-apply.service").exists()
        && Path::new("/etc/systemd/system/neomind-upgrade-apply.path").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_strings_are_stable() {
        // These leak into the HTTP API + frontend logic; don't rename casually.
        assert_eq!(DeploymentEnv::Docker.as_str(), "docker");
        assert_eq!(DeploymentEnv::Systemd.as_str(), "systemd");
        assert_eq!(DeploymentEnv::Unsupported.as_str(), "unsupported");
    }

    #[test]
    fn only_systemd_has_no_hint() {
        assert!(DeploymentEnv::Docker.upgrade_hint().is_some());
        assert!(DeploymentEnv::Unsupported.upgrade_hint().is_some());
        assert!(DeploymentEnv::Systemd.upgrade_hint().is_none());
    }
}
