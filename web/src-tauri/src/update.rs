//! Update module for handling application updates
//!
//! Provides Tauri commands for checking, downloading, and installing updates
//! using the Tauri updater plugin.

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Window};
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

/// Check for available updates
///
/// This command checks the configured update endpoint for a new version
/// and returns information about any available update.
#[tauri::command]
#[allow(unused_variables)]
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

    if let Some(update) = response {
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
/// This command downloads the update package and installs it.
/// Progress events are emitted to the frontend via "update-progress" events.
/// After successful download and installation, the app should be restarted.
#[tauri::command]
pub async fn download_and_install(
    app: AppHandle,
    window: Window,
) -> Result<String, String> {
    let response = app
        .updater()
        .map_err(|e| format!("Updater not initialized: {}", e))?
        .check()
        .await
        .map_err(|e| format!("Failed to check for updates: {}", e))?
        .ok_or("No update available")?;

// Download and install with progress reporting
    // The callback receives (chunk_length, content_length)
    response
        .download_and_install(
            |chunk_length, content_length| {
                let total = content_length.unwrap_or(0);
                let progress = if total > 0 {
                    ((chunk_length as f64 / total as f64) * 100.0).min(100.0)
                } else {
                    0.0
                };

                let _ = window.emit(
                    "update-progress",
                    UpdateProgress {
                        total,
                        current: chunk_length as u64,
                        progress,
                    },
                );
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
/// Returns the version string from the Tauri config.
#[tauri::command]
pub async fn get_app_version(app: AppHandle) -> Result<String, String> {
    app.config()
        .version
        .clone()
        .ok_or_else(|| "Failed to get app version".to_string())
}

/// Restart the application
///
/// This command triggers a restart of the application.
/// Should be called after a successful update installation.
#[tauri::command]
pub async fn relaunch_app(app: AppHandle) {
    app.restart();
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
