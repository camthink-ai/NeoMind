// Prevents additional console window on Windows in release builds
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use tauri::{AppHandle, Listener, Manager};
use std::env;
use std::fs;
use std::path::PathBuf;
use std::process;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::runtime::Runtime;

// Global state for the Axum server
// We use Arc<Runtime> to keep it alive for the app's lifetime
struct ServerState {
    runtime: Arc<Mutex<Option<Runtime>>>,
    server_thread: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl ServerState {
    // Wait for server to be ready by polling the TCP port
    fn wait_for_server_ready(&self, timeout_secs: u64) -> bool {
        let max_attempts = timeout_secs * 20; // Check every 50ms

        for _ in 0..max_attempts {
            if self.check_server_health() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        false
    }

    fn check_server_health(&self) -> bool {
        // Try to connect to the server's port to verify it's listening
        match std::net::TcpStream::connect_timeout(
            &std::net::SocketAddr::from(([127, 0, 0, 1], 3000)),
            Duration::from_millis(100),
        ) {
            Ok(_) => true,
            Err(_) => false,
        }
    }
}

// Implement safe shutdown for ServerState
impl Drop for ServerState {
    fn drop(&mut self) {
        // Shutdown the runtime gracefully
        if let Some(rt) = self.runtime.lock().unwrap().take() {
            rt.shutdown_background();
        }
    }
}

/// Get the application data directory for storing databases
/// This ensures the desktop app uses its own independent data directory
fn get_app_data_dir(app_handle: &AppHandle) -> Result<PathBuf, Box<dyn std::error::Error>> {
    let data_dir = app_handle.path().app_data_dir()?;
    // Create the directory if it doesn't exist
    fs::create_dir_all(&data_dir)?;
    Ok(data_dir)
}

/// Show the main window with comprehensive state handling
fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Unminimize if minimized (Windows/Linux)
        let _ = window.unminimize();

        // Show the window
        let _ = window.show();

        // Bring to front and focus
        let _ = window.set_focus();

        // Ensure window is not ignoring cursor events
        let _ = window.set_ignore_cursor_events(false);
    }
}

/// Create and set up the system tray menu
fn create_tray_menu(app: &tauri::App) -> Result<tauri::tray::TrayIcon, Box<dyn std::error::Error>> {
    use tauri::menu::{Menu, MenuItem};
    use tauri::tray::TrayIconBuilder;

    let show = MenuItem::with_id(app, "show", "Show", true, None::<String>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<String>)?;
    let dev = MenuItem::with_id(app, "devtools", "DevTools", true, None::<String>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<String>)?;

    let menu = Menu::with_items(app, &[&show, &hide, &dev, &quit])?;

    // Get AppHandle to use in the closure
    let app_handle = app.handle().clone();
    let tray = TrayIconBuilder::new()
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |_app, event| match event.id.as_ref() {
            "show" => {
                show_main_window(&app_handle);
            }
            "hide" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "devtools" => {
                if let Some(window) = app_handle.get_webview_window("main") {
                    window.open_devtools();
                }
            }
            "quit" => {
                process::exit(0);
            }
            _ => {}
        })
        .build(app)?;

    Ok(tray)
}

// Global state for the tray icon
// We need to keep the tray icon alive for the app's lifetime
struct TrayState {
    _tray: Option<tauri::tray::TrayIcon>,
}

fn start_axum_server(state: tauri::State<ServerState>) -> Result<(), String> {
    // Clone the Arc so we can move it into the thread
    let runtime_arc = Arc::clone(&state.runtime);

    // Start the Axum server in a background thread
    // This thread owns the runtime and keeps it alive
    let thread_handle = std::thread::spawn(move || {
        // Take ownership of the runtime
        let rt = runtime_arc.lock().unwrap()
            .take()
            .expect("Runtime not available");

        // Run the server - this blocks until the server stops
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
    let rt = Runtime::new().map_err(|e| format!("Failed to create runtime: {}", e)).unwrap();
    let server_state = ServerState {
        runtime: Arc::new(Mutex::new(Some(rt))),
        server_thread: Mutex::new(None),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(server_state)
        .setup(|app| {
            // Set up independent data directory for desktop app
            // Get the app data directory (e.g., ~/Library/Application Support/com.neomind.neomind/)
            let app_data_dir = match get_app_data_dir(&app.handle()) {
                Ok(dir) => {
                    println!("NeoMind data directory: {}", dir.display());
                    dir
                }
                Err(e) => {
                    eprintln!("Failed to get app data directory: {}", e);
                    // Fallback to current directory
                    env::current_dir().unwrap()
                }
            };

            // Change to the app data directory so all relative paths (like "data/") resolve there
            if let Err(e) = env::set_current_dir(&app_data_dir) {
                eprintln!("Failed to set current directory: {}", e);
            }

            // Ensure data subdirectory exists
            let data_dir = app_data_dir.join("data");
            if let Err(e) = fs::create_dir_all(&data_dir) {
                eprintln!("Failed to create data directory: {}", e);
            } else {
                println!("NeoMind data path: {}", data_dir.display());
            }

            // Create system tray and store it in global state to keep it alive
            let tray = match create_tray_menu(app) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("Failed to create tray: {}", e);
                    return Ok(());
                }
            };
            app.manage(TrayState { _tray: Some(tray) });

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

            // Get AppHandle for event listeners
            let app_handle = app.handle().clone();

            // Listen for toolbar/dock clicks to show window
            // macOS: clicked on dock icon
            // Windows/Linux: clicked on taskbar icon
            let handle_for_focus = app_handle.clone();
            app.listen("tauri://focus", move |_| {
                show_main_window(&handle_for_focus);
            });

            // Also listen for the window focus request
            app.listen("tauri://window-focus", move |_| {
                show_main_window(&app_handle);
            });

            // Start Axum server in background
            let state = app.state::<ServerState>();
            if let Err(e) = start_axum_server(state) {
                eprintln!("Failed to start Axum server: {}", e);
            }

            // Wait for server to be ready (up to 10 seconds)
            let state = app.state::<ServerState>();
            if !state.wait_for_server_ready(10) {
                eprintln!("Server did not become ready in time");
            }

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn main() {
    run()
}
