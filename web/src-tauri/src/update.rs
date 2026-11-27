//! Update module for handling application updates
//!
//! Provides Tauri commands for checking, downloading, and installing updates
//! using the Tauri updater plugin.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager, Window};
use tauri_plugin_updater::UpdaterExt;

/// Update information returned to the frontend
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateInfo {
    /// Whether an update is available
    pub available: bool,
    /// The new version number (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// Release notes/body (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
    /// Release date (if available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
}

impl UpdateInfo {
    /// Create an UpdateInfo indicating no update is available
    pub fn none() -> Self {
        Self {
            available: false,
            version: None,
            body: None,
            date: None,
        }
    }
}

/// Update download progress
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateProgress {
    /// Total bytes to download
    pub total: u64,
    /// Bytes downloaded so far
    pub current: u64,
    /// Progress as a percentage (0-100)
    pub progress: f64,
}

/// Manages the cached update check result to avoid redundant network requests.
pub struct UpdateCache(pub Mutex<Option<tauri_plugin_updater::Update>>);

/// Check for available updates
///
/// This command checks the configured update endpoint for a new version
/// and returns information about any available update.
/// The result is cached so `download_and_install` can reuse it.
#[tauri::command]
#[allow(unused_variables, unreachable_code)] // unreachable_code: early return in debug mode is intentional
pub async fn check_update(app: AppHandle) -> Result<UpdateInfo, String> {
    // In development mode, skip update checks to avoid network errors
    #[cfg(debug_assertions)]
    {
        return Ok(UpdateInfo::none());
    }

    let response = app
        .updater()
        .map_err(|e| format!("Updater not initialized: {}", e))?
        .check()
        .await
        .map_err(|e| format!("Failed to check for updates: {}", e))?;

    // Cache the raw update response for download_and_install to reuse
    let cache = app.state::<UpdateCache>();
    if let Ok(mut guard) = cache.0.lock() {
        *guard = response.clone();
    }

    if let Some(update) = response {
        // Defensive check: if remote version matches current version, skip.
        // normalize strips: leading 'v', surrounding whitespace, and any
        // build metadata after '+' or pre-release after '-' (e.g.
        // "0.8.24+build.1" or "0.8.24-beta" → "0.8.24"). This is the
        // last-resort safety net so the update dialog doesn't re-appear on
        // the very version we just installed (the primary guard is the
        // frontend's localStorage marker — see useUpdateCheck.ts).
        //
        // Use the compile-time version (env!("CARGO_PKG_VERSION")) rather
        // than app.config().version: the latter can return None/empty in
        // some Tauri 2.x builds, which makes this comparison always fail
        // ("" != "<remote>") and the update dialog re-appears on every
        // launch once the localStorage marker is consumed/lost. The Cargo
        // version is baked in at build time and always matches the release
        // tag (both bumped together).
        let current_version = env!("CARGO_PKG_VERSION").to_string();
        tracing::info!(
            current_version = %current_version,
            remote_version = %update.version,
            "check_update: installed vs remote version"
        );
        let normalize = |v: &str| {
            v.trim_start_matches('v')
                .trim()
                .split(['+', '-'])
                .next()
                .unwrap_or("")
                .trim()
                .to_string()
        };
        if normalize(&update.version) == normalize(&current_version) {
            return Ok(UpdateInfo::none());
        }

        Ok(UpdateInfo {
            available: true,
            version: Some(update.version.clone()),
            body: update.body.clone(),
            date: update.date.as_ref().map(|d| d.to_string()),
        })
    } else {
        Ok(UpdateInfo::none())
    }
}

/// Download and install an available update
///
/// Uses the cached check result from `check_update` when available,
/// falling back to a fresh check if the cache is empty.
/// Progress events are emitted to the frontend via "update-progress" events.
#[tauri::command]
pub async fn download_and_install(
    app: AppHandle,
    window: Window,
) -> Result<String, String> {
    // Try to use cached update from the last check_update call
    let cached = {
        let cache = app.state::<UpdateCache>();
        cache.0.lock().ok().and_then(|mut guard| guard.take())
    };

    let response = match cached {
        Some(update) => update,
        None => {
            // Fallback: fresh check if cache was empty (e.g. app restarted)
            let check_response = app
                .updater()
                .map_err(|e| format!("Updater not initialized: {}", e))?
                .check()
                .await
                .map_err(|e| format!("Failed to check for updates: {}", e))?;
            match check_response {
                Some(update) => {
                    // Defensive: skip if remote version matches current
                    let current_version = env!("CARGO_PKG_VERSION").to_string();
                    let normalize = |v: &str| {
                        v.trim_start_matches('v')
                            .trim()
                            .split(['+', '-'])
                            .next()
                            .unwrap_or("")
                            .trim()
                            .to_string()
                    };
                    if normalize(&update.version) == normalize(&current_version) {
                        return Err("Already on the latest version".to_string());
                    }
                    update
                }
                None => return Err("No update available".to_string()),
            }
        }
    };

    // Track cumulative downloaded bytes across callback invocations
    let downloaded = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(0));

    // Download and install with progress reporting
    // The callback receives (chunk_length, content_length) for each chunk
    response
        .download_and_install(
            {
                let downloaded = downloaded.clone();
                let window = window.clone();
                move |chunk_length, content_length| {
                    let total = content_length.unwrap_or(0);
                    let current = downloaded.fetch_add(chunk_length as u64, std::sync::atomic::Ordering::Relaxed) + chunk_length as u64;
                    let progress = if total > 0 {
                        ((current as f64 / total as f64) * 100.0).min(100.0)
                    } else {
                        0.0
                    };

                    let _ = window.emit(
                        "update-progress",
                        UpdateProgress {
                            total,
                            current,
                            progress,
                        },
                    );
                }
            },
            || {
                // on_download_finish callback - optional cleanup
            },
        )
        .await
        .map_err(|e| format!("Failed to download and install update: {}", e))?;

    Ok("Update downloaded successfully. Please restart the application.".to_string())
}

/// Get the current application version
///
/// Uses the compile-time `CARGO_PKG_VERSION` (baked from src-tauri/Cargo.toml,
/// bumped alongside the release tag). `app.config().version` can return None/empty
/// in some Tauri 2.x builds — same reason check_update was migrated off it.
#[tauri::command]
pub async fn get_app_version(_app: AppHandle) -> Result<String, String> {
    Ok(env!("CARGO_PKG_VERSION").to_string())
}

/// Restart the application
///
/// Triggers graceful shutdown of the embedded server (flushing telemetry,
/// stopping extensions, DataPush, IM bridges, device adapters, and llama-server)
/// before calling `request_restart()`.
#[tauri::command]
pub async fn relaunch_app(app: AppHandle) {
    let app_for_shutdown = app.clone();
    let _ = tokio::task::spawn_blocking(move || {
        crate::clean_shutdown(&app_for_shutdown);
    })
    .await;
    app.request_restart();
}

/// Show a system notification for available updates
///
/// This command displays a system tray notification when an update is available.
#[tauri::command]
pub async fn show_update_notification(
    app: AppHandle,
    title: String,
    body: String,
) -> Result<(), String> {
    use tauri_plugin_notification::NotificationExt;

    app.notification()
        .builder()
        .title(&title)
        .body(&body)
        .show()
        .map_err(|e| format!("Failed to show notification: {}", e))?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_update_info_none() {
        let info = UpdateInfo::none();
        assert!(!info.available);
        assert!(info.version.is_none());
        assert!(info.body.is_none());
        assert!(info.date.is_none());
    }

    #[test]
    fn test_update_info_with_data() {
        let info = UpdateInfo {
            available: true,
            version: Some("0.6.0".to_string()),
            body: Some("New features".to_string()),
            date: Some("2025-03-18".to_string()),
        };
        assert!(info.available);
        assert_eq!(info.version, Some("0.6.0".to_string()));
    }
}
