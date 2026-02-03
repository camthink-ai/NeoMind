// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tauri::{AppHandle, Listener, Manager};
use tokio::runtime::Runtime;

use tauri::tray::TrayIconEvent;

// Global state for the Axum server
struct ServerState {
    runtime: Arc<Mutex<Option<Runtime>>>,
    server_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl ServerState {
    fn wait_for_server_ready(&self, timeout_secs: u64) -> bool {
        let max_attempts = timeout_secs * 20;
        for _ in 0..max_attempts {
            if self.check_server_health() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn check_server_health(&self) -> bool {
        std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], 9375)),
            Duration::from_millis(100),
        )
        .is_ok()
    }
}

impl Drop for ServerState {
    fn drop(&mut self) {
        if let Some(rt) = self.runtime.lock().unwrap().take() {
            rt.shutdown_background();
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
    let tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(move |_app, event| match event {
            TrayIconEvent::Click { .. } => {
                show_main_window(&app_handle_for_tray);
            }
            TrayIconEvent::DoubleClick { .. } => {
                show_main_window(&app_handle_for_tray);
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
    let runtime_arc = Arc::clone(&state.runtime);
    let thread_handle = std::thread::spawn(move || {
        let rt = runtime_arc
            .lock()
            .unwrap()
            .take()
            .expect("Runtime not available");
        rt.block_on(async {
            if let Err(e) = edge_api::start_server().await {
                eprintln!("Failed to start server: {}", e);
            }
        });
    });
    *state.server_thread.lock().unwrap() = Some(thread_handle);
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
        server_thread: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .manage(server_state)
        .setup(setup_app)
        .build(tauri::generate_context!())
        .expect("Failed to build Tauri application")
        .run(|app_handle, event| {
            #[cfg(target_os = "macos")]
            if let tauri::RunEvent::Reopen { .. } = event {
                show_main_window(app_handle);
            }
            #[cfg(not(target_os = "macos"))]
            let _ = event;
            #[cfg(not(target_os = "macos"))]
            let _ = app_handle;
        });
}

/// Application setup function
fn setup_app(app: &mut tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    // Get and set up data directory
    let app_data_dir = get_app_data_dir(app.handle());
    fs::create_dir_all(&app_data_dir)?;

    // Change to data directory for relative paths
    let data_dir = app_data_dir.join("data");
    fs::create_dir_all(&data_dir)?;

    // Try to change to data directory, but don't fail if we can't
    let _ = env::set_current_dir(&data_dir);

    // Create tray menu (don't fail if tray creation fails)
    if let Ok(tray) = create_tray_menu(app) {
        app.manage(TrayState { _tray: Some(tray) });
    }

    // Handle window close event
    if let Some(window) = app.get_webview_window("main") {
        let window_clone = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window_clone.hide();
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

    // Wait for server ready
    let state = app.state::<ServerState>();
    if !state.wait_for_server_ready(10) {
        eprintln!("Server did not become ready in time");
    }

    Ok(())
}

fn main() {
    run()
}
