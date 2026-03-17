// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::tray::TrayIconEvent;
use tauri::{image::Image, AppHandle, Listener, Manager};
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

fn start_axum_server(state: tauri::State<ServerState>) -> Result<(), String> {
    // Clone the Arc before moving into the closure
    let runtime_arc = Arc::clone(&state.runtime);
    let server_thread = Arc::clone(&state.server_thread);

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

        rt.block_on(async {
            if let Err(e) = edge_api::start_server().await {
                eprintln!("Failed to start server: {}", e);
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

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        // Single instance plugin - prevents multiple app instances
        // When a second instance is launched, focus the existing window
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main_window(app);
        }))
        .manage(server_state)
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
    if let Ok(tray) = create_tray_menu(app) {
        app.manage(TrayState { _tray: Some(tray) });
    }

    // Handle window close event
    // On Windows: close button quits the app (user expectation)
    // On macOS/Linux: close button minimizes to tray
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
                    // On macOS/Linux, minimize to tray
                    api.prevent_close();
                    let _ = window_clone.hide();
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
    if let Err(e) = start_axum_server(state) {
        eprintln!("Failed to start server: {}", e);
    }

    Ok(())
}

fn main() {
    run()
}
