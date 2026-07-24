// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod update;

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::tray::TrayIconEvent;
use tauri::{image::Image, AppHandle, Emitter, Listener, Manager};
use tokio::runtime::Runtime;
use tracing::info;

// Global state for the Axum server
struct ServerState {
    runtime: Arc<Mutex<Option<Runtime>>>,
    server_thread: Arc<Mutex<Option<std::thread::JoinHandle<()>>>>,
}

impl ServerState {}

impl Drop for ServerState {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.runtime.lock() {
            if let Some(rt) = guard.take() {
                rt.shutdown_background();
            }
        }
    }
}

/// Get the application data directory
fn get_app_data_dir(app_handle: &AppHandle) -> PathBuf {
    match app_handle.path().app_data_dir() {
        Ok(dir) => {
            // Create directory if needed
            let _ = fs::create_dir_all(&dir);
            dir
        }
        Err(_) => {
            // Fallback to home directory
            env::var("HOME")
                .or_else(|_| env::var("USERPROFILE"))
                .map(PathBuf::from)
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".neomind")
        }
    }
}

/// Clean up log files older than 7 days.
fn cleanup_old_logs(log_dir: &std::path::Path) {
    let max_age_secs: i64 = 7 * 24 * 60 * 60; // 7 days
    let now = chrono::Utc::now();

    let mut removed = 0u32;
    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                if !filename.starts_with("neomind.log") {
                    continue;
                }
                if let Some(date_str) = filename.strip_prefix("neomind.log.") {
                    if let Ok(file_date) =
                        chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d")
                    {
                        let file_datetime = file_date
                            .and_time(chrono::NaiveTime::default())
                            .and_utc();
                        if (now - file_datetime).num_seconds() > max_age_secs {
                            let _ = fs::remove_file(&path);
                            removed += 1;
                        }
                    }
                }
            }
        }
    }
    if removed > 0 {
        tracing::info!("Log cleanup: removed {} old file(s)", removed);
    }
}

/// Move any `neomind.log.*` files from the legacy `<app_data>/logs/`
/// directory (used by app versions ≤ 0.9.2) into the canonical
/// `<app_data>/data/logs/` location. Files in the destination that already
/// exist with the same name are kept (the legacy copy is deleted to avoid
/// double-archiving). Idempotent — safe to call on every startup.
fn migrate_legacy_log_dir(app_data_dir: &std::path::Path, dest_dir: &std::path::Path) {
    let legacy_dir = app_data_dir.join("logs");
    if legacy_dir == *dest_dir || !legacy_dir.is_dir() {
        return;
    }
    let entries = match fs::read_dir(&legacy_dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut moved = 0u32;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n,
            None => continue,
        };
        if !name.starts_with("neomind.log") {
            continue;
        }
        let dest = dest_dir.join(name);
        // rename (move) is atomic on the same filesystem. If the destination
        // already exists (re-running migration), just remove the legacy copy.
        // On cross-filesystem rename failures (rare, but possible if the user
        // mounted `<app_data>` across volumes), fall back to copy + delete.
        match fs::rename(&path, &dest) {
            Ok(_) => moved += 1,
            Err(_) => {
                if dest.exists() {
                    let _ = fs::remove_file(&path);
                } else if fs::copy(&path, &dest).is_ok() {
                    let _ = fs::remove_file(&path);
                    moved += 1;
                }
            }
        }
    }
    if moved > 0 {
        tracing::info!(
            "Migrated {} legacy log file(s) from {} to {}",
            moved,
            legacy_dir.display(),
            dest_dir.display()
        );
        // Best-effort cleanup of the now-empty legacy directory. Remove only
        // if empty (fs::remove_dir fails harmlessly if anything remains).
        let _ = fs::remove_dir(&legacy_dir);
    }
}

/// Show the main window
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.set_ignore_cursor_events(false);
    }
}

/// Properly shutdown the server before exiting
fn clean_shutdown(app_handle: &AppHandle) {
    // Try to get server state and shutdown
    if let Some(state) = app_handle.try_state::<ServerState>() {
        // Shutdown the tokio runtime
        if let Ok(mut guard) = state.runtime.lock() {
            if let Some(rt) = guard.take() {
                rt.shutdown_timeout(tokio::time::Duration::from_secs(2));
            }
        }
        // The server thread will be joined when ServerState is dropped
    }
}

/// Create and set up the system tray menu
fn create_tray_menu(app: &tauri::App) -> Result<tauri::tray::TrayIcon, Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "Show", true, None::<String>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<String>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<String>)?;

    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    let app_handle = app.handle().clone();
    let app_handle_for_tray = app_handle.clone();

    // Load tray icon from embedded resource
    let tray_icon = Image::from_bytes(include_bytes!("../icons/icon.png"))?;

    // Build tray icon with proper Windows support
    let tray = TrayIconBuilder::new()
        .icon(tray_icon)
        .menu(&menu)
        .show_menu_on_left_click(false) // Only show menu on right-click
        .tooltip("NeoMind - Edge AI Platform") // Add tooltip for better UX
        .on_tray_icon_event(move |_app, event| match event {
            // Only handle left-click events - right-click shows the context menu automatically
            TrayIconEvent::Click { button, .. } => {
                // Only show window on left-click (right-click shows menu automatically)
                if button == tauri::tray:: MouseButton::Left {
                    show_main_window(&app_handle_for_tray);
                }
                // Right-click is handled automatically by Tauri to show the menu
            }
            TrayIconEvent::DoubleClick { button, .. } => {
                // Only show window on left-double-click
                if button == tauri::tray:: MouseButton::Left {
                    show_main_window(&app_handle_for_tray);
                }
            }
            _ => {}
        })
        .on_menu_event(move |_app, event| match event.id.as_ref() {
            "show" => {
                show_main_window(&app_handle);
            }
            "hide" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "quit" => {
                app_handle.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(tray)
}

// Global state for the tray icon
struct TrayState {
    _tray: Option<tauri::tray::TrayIcon>,
}

fn start_axum_server(
    state: tauri::State<ServerState>,
    app_handle: &AppHandle,
) -> Result<(), String> {
    // Initialize tracing with dual output (stdout + file)
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));

    // Log to file under the canonical data dir: <app_data>/data/logs/
    // This matches NEOMIND_DATA_DIR (set in setup()) and the storage layout
    // convention in CLAUDE.md, so the API's /api/logs/download handler
    // (which reads state.data_dir.join("logs")) finds the same files.
    let log_dir = get_app_data_dir(app_handle).join("data").join("logs");
    let _ = fs::create_dir_all(&log_dir);
    // One-time migration: move any logs written by previous versions to the
    // legacy `<app_data>/logs/` path into the canonical `<app_data>/data/logs/`
    // so users don't lose their 7-day history. Idempotent — if the source dir
    // is empty or already migrated, this is a no-op.
    migrate_legacy_log_dir(&get_app_data_dir(app_handle), &log_dir);
    let file_appender = tracing_appender::rolling::daily(&log_dir, "neomind.log");

    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;
    use tracing_subscriber::Layer;

    let stdout_layer = tracing_subscriber::fmt::layer()
        .with_target(false)
        .compact()
        .with_filter(filter.clone())
        .boxed();

    let file_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_writer(file_appender)
        .with_filter(filter);

    tracing_subscriber::registry()
        .with(stdout_layer)
        .with(file_layer)
        .init();

    // Startup cleanup of old log files
    cleanup_old_logs(&log_dir);

    // Clone the Arc before moving into the closure
    let runtime_arc = Arc::clone(&state.runtime);
    let server_thread = Arc::clone(&state.server_thread);
    // Clone the AppHandle so the server thread can emit a "backend-start-failed"
    // event if start_server() errors (e.g. port 9375 already in use). Without
    // this the failure is only eprintln'd to stderr, the Tauri window stays open
    // with no backend, and the user stares at an endless "Reconnecting" with no
    // clue why. Symmetric to the "backend-ready" event emitted on success.
    let app_handle_for_errors = app_handle.clone();

    let thread_handle = std::thread::spawn(move || {
        let rt = match runtime_arc.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => {
                eprintln!("Failed to acquire runtime lock");
                return;
            }
        };

        let Some(rt) = rt else {
            eprintln!("Runtime not available");
            return;
        };

        rt.block_on(async move {
            if let Err(e) = edge_api::start_server().await {
                let err_str = e.to_string();
                eprintln!("Failed to start server: {}", err_str);
                // Surface the failure to the frontend so the user sees a clear
                // error page instead of an endless "Reconnecting". Detect the
                // common case (port already in use) to give targeted guidance.
                let port_conflict = err_str.contains("AddrInUse")
                    || err_str.to_lowercase().contains("address already in use");
                let _ = app_handle_for_errors.emit(
                    "backend-start-failed",
                    serde_json::json!({
                        "error": err_str,
                        "port_conflict": port_conflict,
                    }),
                );
            }
        });
    });

    if let Ok(mut guard) = server_thread.lock() {
        *guard = Some(thread_handle);
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Create runtime with proper error handling
    let rt = match Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("Failed to create runtime: {}", e);
            std::process::exit(1);
        }
    };

    let server_state = ServerState {
        runtime: Arc::new(Mutex::new(Some(rt))),
        server_thread: Arc::new(Mutex::new(None)),
    };

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_dialog::init())
        // Notification plugin - system tray notifications
        .plugin(tauri_plugin_notification::init())
        // Updater plugin - handles application updates
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Single instance plugin - prevents multiple app instances
        // When a second instance is launched, focus the existing window
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }));

    // BLE plugin - native Bluetooth LE for device provisioning
    // Graceful fallback: if BLE adapter is unavailable, the app works fine without it
    let builder = match std::panic::catch_unwind(tauri_plugin_blec::init) {
        Ok(plugin) => builder.plugin(plugin),
        Err(e) => {
            eprintln!("BLE plugin init skipped (non-fatal): {:?}", e);
            builder
        }
    };

    builder
        .manage(server_state)
        .manage(update::UpdateCache(std::sync::Mutex::new(None)))
        .invoke_handler(tauri::generate_handler![
            update::check_update,
            update::download_and_install,
            update::get_app_version,
            update::relaunch_app,
            update::show_update_notification,
        ])
        .setup(setup_app)
        .build(tauri::generate_context!())
        .unwrap_or_else(|e| {
            eprintln!("Failed to build Tauri application: {}", e);
            std::process::exit(1);
        })
        .run(|app_handle, event| {
            match event {
                #[cfg(target_os = "macos")]
                tauri::RunEvent::Reopen { .. } => {
                    show_main_window(app_handle);
                }
                // Handle exit request from OS (taskbar right-click, Alt+F4, etc.)
                tauri::RunEvent::ExitRequested { .. } => {
                    // Perform clean shutdown before exiting
                    clean_shutdown(app_handle);
                    // Allow the exit to proceed
                }
                _ => {}
            }
        });
}

/// Application setup function
fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Get and set up data directory
    let app_data_dir = get_app_data_dir(app.handle());
    fs::create_dir_all(&app_data_dir)?;

    // Change to app data directory for relative paths (e.g., data/devices.redb)
    let _ = env::set_current_dir(&app_data_dir);

    // Set NEOMIND_DATA_DIR environment variable for consistent path handling
    // This ensures extensions are installed to the correct directory
    let data_dir = app_data_dir.join("data");
    fs::create_dir_all(&data_dir)?;

    // Unified data directory strategy for both development and production
    //
    // CRITICAL: Always use app data directory to ensure extension paths are consistent
    // Extensions are installed to $NEOMIND_DATA_DIR/extensions/, and the API reads from
    // the same location. Using different paths causes "extensions not found" errors.
    //
    // Development mode considerations:
    // - Database files (redb) should still be in app data directory
    // - Use RUST_LOG or environment variables for debugging if needed
    // - Do NOT use ./data in development as it causes path inconsistencies
    env::set_var("NEOMIND_DATA_DIR", &data_dir);

    #[cfg(debug_assertions)]
    {
        info!(
            data_dir = %data_dir.display(),
            extensions_dir = %data_dir.join("extensions").display(),
            "Data directory configured (development mode)"
        );

        // Log a warning if ./data exists (might cause confusion)
        let project_data_dir = std::path::PathBuf::from("./data");
        if project_data_dir.exists() {
            info!(
                "Project ./data directory detected but will NOT be used. All data (including extensions) is stored in app data directory to ensure consistency."
            );
        }
    }

    #[cfg(not(debug_assertions))]
    {
        info!(
            data_dir = %data_dir.display(),
            "Data directory configured (production mode)"
        );
    }

    // Create tray menu (don't fail if tray creation fails)
    // Track success so the close handler below can decide whether
    // hide-to-tray is safe. On Linux WMs without StatusNotifierApplet
    // (e.g. bare i3/sway), tray creation fails; if we still hid the
    // window on close, the user would have no way to bring it back.
    let tray_created = match create_tray_menu(app) {
        Ok(tray) => {
            app.manage(TrayState { _tray: Some(tray) });
            true
        }
        Err(e) => {
            tracing::warn!(error = %e, "Tray icon creation failed; close button will exit instead of hiding");
            false
        }
    };

    // Handle window close event
    // On Windows: close button quits the app (user expectation)
    // On macOS/Linux with tray: close button hides to tray
    // On macOS/Linux without tray: let the close proceed normally
    //   (macOS: app keeps running, Dock click reopens via RunEvent::Reopen;
    //    Linux: app exits, user relaunches — better than an invisible window.)
    if let Some(window) = app.get_webview_window("main") {
        let window_clone = window.clone();
        #[cfg(target_os = "windows")]
        let app_handle = app.handle().clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                #[cfg(target_os = "windows")]
                {
                    // On Windows, close button should quit the app
                    // Use exit(0) to trigger proper ExitRequested event
                    api.prevent_close(); // Let Tauri handle the exit
                    app_handle.exit(0);
                }
                #[cfg(not(target_os = "windows"))]
                {
                    if tray_created {
                        // Tray exists — safe to hide; user can re-open via tray icon.
                        api.prevent_close();
                        let _ = window_clone.hide();
                    }
                    // else: fall through, allow normal close.
                }
            }
        });
    }

    // Listen for Dock/taskbar clicks
    let app_handle = app.handle().clone();
    let handle_for_focus = app_handle.clone();
    let _ = app.listen("tauri://focus", move |_| {
        show_main_window(&handle_for_focus);
    });

    // Start server
    let state = app.state::<ServerState>();
    if let Err(e) = start_axum_server(state, &app_handle) {
        eprintln!("Failed to start server: {}", e);
    }

    // Poll until the backend server is accepting connections, then emit "backend-ready"
    let handle_for_ready = app_handle.clone();
    std::thread::spawn(move || {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_io()
            .enable_time()
            .build()
            .expect("health check runtime");
        rt.block_on(async {
            for _ in 0..50 {
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                if tokio::net::TcpStream::connect("127.0.0.1:9375").await.is_ok() {
                    let _ = handle_for_ready.emit("backend-ready", serde_json::json!({
                        "status": "ready",
                        "port": 9375
                    }));
                    return;
                }
            }
            // Timeout — emit anyway so the frontend doesn't hang
            let _ = handle_for_ready.emit("backend-ready", serde_json::json!({
                "status": "timeout",
                "port": 9375
            }));
        });
    });

    Ok(())
}

fn main() {
    run()
}
