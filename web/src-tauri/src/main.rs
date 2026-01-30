// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::Manager;
use std::sync::Mutex;
use tokio::runtime::Runtime;

// Global state for the Axum server handle
struct ServerState {
    runtime: Mutex<Option<Runtime>>,
    server_handle: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

// Tauri commands for frontend communication
#[tauri::command]
fn show_window(window: tauri::Window) {
    if let Some(webview) = window.get_webview_window("main") {
        let _ = webview.show();
        let _ = webview.set_focus();
    }
}

#[tauri::command]
fn hide_window(window: tauri::Window) {
    if let Some(webview) = window.get_webview_window("main") {
        let _ = webview.hide();
    }
}

#[tauri::command]
fn is_window_visible(window: tauri::Window) -> bool {
    window
        .get_webview_window("main")
        .map(|w| w.is_visible().unwrap_or(false))
        .unwrap_or(false)
}

#[tauri::command]
async fn get_server_port() -> Result<usize, String> {
    // Return the port where Axum server is running
    // In production, this should be read from config or state
    Ok(3000)
}

#[tauri::command]
fn open_devtools(window: tauri::Window) {
    if let Some(webview) = window.get_webview_window("main") {
        webview.open_devtools();
    }
}

fn create_tray_menu(app: &tauri::App) -> Result<(), Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "Show", true, None::<String>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<String>)?;
    let dev = MenuItem::with_id(app, "devtools", "DevTools", true, None::<String>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<String>)?;

    let menu = Menu::with_items(app, &[&show, &hide, &dev, &quit])?;

    let _tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "hide" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "devtools" => {
                if let Some(window) = app.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(())
}

fn start_axum_server(state: tauri::State<ServerState>) -> Result<(), String> {
    let runtime = state.runtime.lock().unwrap();
    let runtime = runtime.as_ref().ok_or("Runtime not initialized")?;

    // Start the Axum server in the background
    // This uses the existing edge-api server setup
    let handle = runtime.spawn(async move {
        // Import and start the existing server
        if let Err(e) = edge_api::start_server().await {
            eprintln!("Failed to start server: {}", e);
        }
    });

    *state.server_handle.lock().unwrap() = Some(handle);
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = Runtime::new().map_err(|e| format!("Failed to create runtime: {}", e)).unwrap();
    let server_state = ServerState {
        runtime: Mutex::new(Some(rt)),
        server_handle: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .manage(server_state)
        .invoke_handler(tauri::generate_handler![
            show_window,
            hide_window,
            is_window_visible,
            get_server_port,
            open_devtools,
        ])
        .setup(|app| {
            // Create system tray
            if let Err(e) = create_tray_menu(app) {
                eprintln!("Failed to create tray: {}", e);
            }

            // Start Axum server in background
            let state = app.state::<ServerState>();
            if let Err(e) = start_axum_server(state) {
                eprintln!("Failed to start Axum server: {}", e);
            }

            // Handle window close event - minimize to tray instead of quitting
            let window = app.get_webview_window("main").unwrap();
            let window_clone = window.clone();
            window.on_window_event(move |event| match event {
                tauri::WindowEvent::CloseRequested { api, .. } => {
                    // Prevent the window from closing
                    api.prevent_close();
                    // Hide the window instead
                    let _ = window_clone.hide();
                }
                _ => {}
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn main() {
    run()
}
