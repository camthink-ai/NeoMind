//! Command-line interface for NeoMind.

use std::io::Read;
use std::net::SocketAddr;
use std::path::Path;
use std::time::SystemTime;

use anyhow::Result;
use clap::Parser;
use neomind_agent::{LlmBackend, SessionManager};
use neomind_core::config::{
    endpoints, env_vars, models, normalize_ollama_endpoint, normalize_openai_endpoint,
};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::Layer;

// Clap command types now live in neomind_cli_ops::dispatch::commands
use neomind_cli_ops::dispatch::commands::*;

mod self_update;

// Jemalloc global allocator (Linux only): glibc malloc's per-thread arenas
// fragment over time and don't return freed memory to the OS (server RSS
// climbed to 4-6 GB over days). jemalloc packs allocations tightly and
// releases freed pages promptly. macOS/Windows use their own allocators
// (not glibc) so they don't have this problem.
#[cfg(target_os = "linux")]
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;

// Custom runtime with increased worker threads for better concurrent performance
// Default is num_cpus, but we use more to handle block_in_place alternatives
#[tokio::main(flavor = "multi_thread", worker_threads = 16)]
async fn main() -> Result<()> {
    // Install extension panic hook to catch and log extension panics
    // This provides better error messages and prevents some panics from crashing the server
    // IMPORTANT: This cannot catch all panic types (e.g., foreign exceptions from C/C++ code)
    neomind_core::extension::safety::install_extension_panic_hook();

    let args = Args::parse();

    // Initialize logging
    let json_logging = std::env::var("NEOMIND_LOG_JSON")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(false);

    // Build the env filter for log level control
    // -v/--verbose sets debug level; RUST_LOG env var takes precedence
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        if args.verbose {
            tracing_subscriber::EnvFilter::new("neomind=debug")
                .add_directive(tracing::Level::DEBUG.into())
        } else {
            tracing_subscriber::EnvFilter::new("neomind=info")
                .add_directive(tracing::Level::INFO.into())
                .add_directive(tracing::Level::WARN.into())
        }
    });

    // For serve command: dual output (stdout + file); for others: stdout only
    let file_logging = matches!(args.command, Command::Serve { .. });

    if file_logging {
        let log_dir = Path::new("data/logs");
        let file_appender = tracing_appender::rolling::daily(log_dir, "neomind.log");

        let stdout_layer = if json_logging {
            tracing_subscriber::fmt::layer()
                .json()
                .with_target(true)
                .with_filter(env_filter.clone())
                .boxed()
        } else {
            tracing_subscriber::fmt::layer()
                .with_target(false)
                .with_thread_ids(false)
                .with_file(false)
                .with_line_number(false)
                .compact()
                .with_level(false)
                .with_filter(env_filter.clone())
                .boxed()
        };

        // Never write ANSI color codes into the on-disk log: `fmt::layer()`
        // defaults `with_ansi` to true (no TTY detection, unlike `fmt()`), so
        // without this every line would be wrapped in `\x1b[...m` escapes and
        // the /api/logs/download archive would ship them verbatim.
        let file_layer = tracing_subscriber::fmt::layer()
            .with_target(true)
            .with_writer(file_appender)
            .with_ansi(false)
            .with_filter(env_filter);

        tracing_subscriber::registry()
            .with(stdout_layer)
            .with(file_layer)
            .init();
    } else if json_logging {
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .with_target(true)
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_thread_ids(false)
            .with_file(false)
            .with_line_number(false)
            .compact()
            .with_level(false)
            .init();
    }

    // Run the appropriate command
    match args.command {
        Command::Serve { host, port } => run_server(host, port).await,
        Command::Prompt {
            prompt,
            max_tokens: _,
            temperature: _,
        } => run_prompt(&prompt).await,
        Command::Chat { session } => run_chat(session).await,
        Command::ListModels { endpoint } => list_models(endpoint).await,
        Command::Health => run_health().await,
        Command::Logs {
            tail,
            follow,
            level,
            since,
        } => run_logs(tail, follow, level, since).await,
        Command::Extension { extension_cmd } => run_extension_cmd(extension_cmd).await,
        Command::CheckUpdate => run_check_update().await,
        Command::Upgrade { version, yes } => self_update::run_upgrade(version, yes).await,
        Command::Uninstall { purge, yes } => self_update::run_uninstall(purge, yes).await,
        Command::ApiKey { key_cmd } => run_api_key_cmd(key_cmd).await,
        Command::Llm { llm_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_llm_cmd(llm_cmd).await)
        }
        Command::Device { device_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_device_cmd(device_cmd).await)
        }
        Command::Dashboard { dashboard_cmd } => print_result(
            neomind_cli_ops::dispatch::handlers::run_dashboard_cmd(dashboard_cmd).await,
        ),
        Command::Rule { rule_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_rule_cmd(rule_cmd).await)
        }
        Command::Transform { transform_cmd } => print_result(
            neomind_cli_ops::dispatch::handlers::run_transform_cmd(transform_cmd).await,
        ),
        Command::Agent { agent_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_agent_cmd(agent_cmd).await)
        }
        Command::Message { message_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_message_cmd(message_cmd).await)
        }
        Command::Push { push_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_push_cmd(push_cmd).await)
        }
        Command::Widget { widget_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_widget_cmd(widget_cmd).await)
        }
        Command::System { system_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_system_cmd(system_cmd).await)
        }
        Command::Config { config_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_config_cmd(config_cmd).await)
        }
        Command::Data { data_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_data_cmd(data_cmd).await)
        }
        Command::Connector { connector_cmd } => print_result(
            neomind_cli_ops::dispatch::handlers::run_connector_cmd(connector_cmd).await,
        ),
        Command::Settings { settings_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_settings_cmd(settings_cmd).await)
        }
        Command::Login { data_dir, force } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_login_cmd(data_dir, force).await)
        }
        Command::Logout => {
            print_result(neomind_cli_ops::dispatch::handlers::run_logout_cmd().await)
        }
        Command::Whoami => {
            print_result(neomind_cli_ops::dispatch::handlers::run_whoami_cmd().await)
        }
        Command::User { user_cmd } => {
            print_result(neomind_cli_ops::dispatch::handlers::run_user_cmd(user_cmd).await)
        }
    }
}

/// Helper: run a cli-ops data handler, format its output, return Ok(()) on success.
fn print_result(
    result: anyhow::Result<(
        neomind_cli_ops::types::CliResponse,
        neomind_cli_ops::types::OutputFormat,
    )>,
) -> Result<()> {
    let (resp, fmt) = result?;
    neomind_cli_ops::output::format_output(&resp, fmt);
    Ok(())
}

/// Initialize LLM backend from available config sources.
async fn init_llm_backend(session_manager: &SessionManager) -> Result<()> {
    // Try config.toml, then llm_config.json, then environment variables
    let backend = load_llm_backend_from_env()?;
    // Only set as default for new sessions
    session_manager.set_default_llm_backend(backend).await;
    Ok(())
}

/// Load LLM backend from environment variables.
fn load_llm_backend_from_env() -> Result<LlmBackend> {
    // Check for Ollama
    if let Ok(endpoint) = std::env::var(env_vars::OLLAMA_ENDPOINT) {
        let endpoint = normalize_ollama_endpoint(endpoint);
        let model = std::env::var(env_vars::LLM_MODEL)
            .unwrap_or_else(|_| models::OLLAMA_DEFAULT.to_string());
        eprintln!("Using Ollama: endpoint={}, model={}", endpoint, model);
        return Ok(LlmBackend::Ollama {
            endpoint,
            model,
            capabilities: None,
        });
    }

    // Check for OpenAI
    if let Ok(api_key) = std::env::var(env_vars::OPENAI_API_KEY) {
        let endpoint = std::env::var(env_vars::OPENAI_ENDPOINT)
            .unwrap_or_else(|_| endpoints::OPENAI.to_string());
        let endpoint = normalize_openai_endpoint(endpoint);
        let model = std::env::var(env_vars::LLM_MODEL)
            .unwrap_or_else(|_| models::OPENAI_DEFAULT.to_string());
        eprintln!("Using OpenAI: endpoint={}, model={}", endpoint, model);
        return Ok(LlmBackend::OpenAi {
            api_key,
            endpoint,
            model,
            capabilities: None,
        });
    }

    Err(anyhow::anyhow!(
        "No LLM backend configured. Set OLLAMA_ENDPOINT or OPENAI_API_KEY, or configure via the Web UI (http://localhost:9375)."
    ))
}

/// Run a single prompt.
async fn run_prompt(prompt: &str) -> Result<()> {
    println!("NeoMind Edge AI - Prompt Mode");
    println!("==============================\n");
    println!("Prompt: {}", prompt);
    println!("\nGenerating response...\n");

    // Create session manager
    let session_manager = SessionManager::new()
        .map_err(|e| anyhow::anyhow!("Failed to create session manager: {}", e))?;

    // Initialize LLM backend
    init_llm_backend(&session_manager).await?;

    // Create a temporary session
    let session_id = session_manager
        .create_session()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to create session: {}", e))?;

    // Process the prompt
    let response = session_manager
        .process_message(&session_id, prompt)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to process message: {}", e))?;

    println!("{}", response.message.content);
    println!("\nProcessing time: {}ms", response.processing_time_ms);

    // Clean up the temporary session
    let _ = session_manager.remove_session(&session_id).await;

    Ok(())
}

/// Run interactive chat mode.
async fn run_chat(session_id: Option<String>) -> Result<()> {
    println!("NeoMind Edge AI - Chat Mode");
    println!("===========================\n");

    // Create session manager
    let session_manager = SessionManager::new()
        .map_err(|e| anyhow::anyhow!("Failed to create session manager: {}", e))?;

    // Initialize LLM backend
    init_llm_backend(&session_manager).await?;

    // Use existing session or create new one
    let session_id = if let Some(sid) = session_id {
        println!("Resuming session: {}", sid);
        sid
    } else {
        let sid = session_manager
            .create_session()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create session: {}", e))?;
        println!("New session: {}", sid);
        sid
    };

    println!("\nType your message and press Enter to send.");
    println!("Type 'quit' or 'exit' to quit.");
    println!("Type 'clear' to clear conversation history.\n");

    let mut input = String::new();
    let stdin = std::io::stdin();

    loop {
        print!("> ");
        use std::io::Write;
        std::io::stdout().flush()?;

        input.clear();
        stdin.read_line(&mut input)?;

        let input = input.trim();

        if input.is_empty() {
            continue;
        }

        if input == "quit" || input == "exit" {
            println!("Goodbye!");
            break;
        }

        if input == "clear" {
            if session_manager.clear_history(&session_id).await.is_ok() {
                println!("Conversation history cleared.\n");
            }
            continue;
        }

        println!();

        // Process with streaming events
        match session_manager
            .process_message_events(&session_id, input)
            .await
        {
            Ok(mut stream) => {
                use futures::stream::StreamExt;
                let mut response_text = String::new();

                while let Some(event) = stream.next().await {
                    match event {
                        neomind_agent::AgentEvent::Thinking { content } => {
                            if !content.is_empty() {
                                print!("\x1b[90m[Thinking: {}...]\x1b[0m", content.trim());
                                use std::io::Write;
                                std::io::stdout().flush()?;
                            }
                        }
                        neomind_agent::AgentEvent::Content { content } => {
                            if !response_text.is_empty() && response_text != content {
                                // Print incremental content
                                let added =
                                    content.strip_prefix(&response_text).unwrap_or(&content);
                                print!("{}", added);
                                use std::io::Write;
                                std::io::stdout().flush()?;
                            }
                            response_text = content;
                        }
                        neomind_agent::AgentEvent::ToolCallStart { tool, .. } => {
                            println!("\n\n[Calling tool: {}]", tool);
                        }
                        neomind_agent::AgentEvent::ToolCallEnd { tool, success, .. } => {
                            if success {
                                println!("[Tool {} completed]\n", tool);
                            } else {
                                println!("[Tool {} failed]\n", tool);
                            }
                        }
                        neomind_agent::AgentEvent::Error { message } => {
                            eprintln!("\nError: {}", message);
                        }
                        neomind_agent::AgentEvent::Intent {
                            display_name,
                            confidence,
                            ..
                        } => {
                            println!(
                                "\n[Intent: {} (confidence: {:.0}%)]",
                                display_name,
                                confidence.unwrap_or(0.0) * 100.0
                            );
                        }
                        neomind_agent::AgentEvent::Plan { step, stage } => {
                            println!("[Plan: {} - {}]", stage, step);
                        }
                        neomind_agent::AgentEvent::Progress { message, .. } => {
                            println!("[Progress: {}]", message);
                        }
                        neomind_agent::AgentEvent::Heartbeat { .. } => {
                            // Ignore heartbeat events in CLI
                        }
                        neomind_agent::AgentEvent::Warning { message } => {
                            eprintln!("[Warning] {}", message);
                        }
                        neomind_agent::AgentEvent::IntermediateEnd => {
                            // Intermediate end - more content coming, don't break
                            println!("[Continuing...]");
                        }
                        neomind_agent::AgentEvent::End { .. } => {
                            break;
                        }
                    }
                }

                println!(); // Final newline

                // Persist history
                let _ = session_manager.persist_history(&session_id).await;
            }
            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }

    Ok(())
}

/// List available models from Ollama.
async fn list_models(endpoint: String) -> Result<()> {
    println!("Available Models from Ollama:");
    println!("==============================\n");

    let url = format!("{}/api/tags", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::new();

    let response = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to connect to Ollama: {}", e))?;

    if !response.status().is_success() {
        eprintln!("Ollama returned status: {}", response.status());
        eprintln!("\nMake sure Ollama is running at: {}", endpoint);
        return Ok(());
    }

    #[derive(serde::Deserialize)]
    struct OllamaModelsResponse {
        models: Vec<OllamaModel>,
    }

    #[derive(serde::Deserialize)]
    struct OllamaModel {
        name: String,
        size: Option<u64>,
    }

    let ollama_response: OllamaModelsResponse = response
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to parse response: {}", e))?;

    if ollama_response.models.is_empty() {
        println!("No models found. Pull a model using: ollama pull <model>");
    } else {
        for model in &ollama_response.models {
            println!("  {}", model.name);
            if let Some(size) = model.size {
                let size_gb = size as f64 / (1024.0 * 1024.0 * 1024.0);
                println!("    Size: {:.2} GB", size_gb);
            }
        }
    }

    println!("\nTo use a model, set LLM_MODEL environment variable:");
    println!("  export LLM_MODEL=<model_name>");

    Ok(())
}

/// Check for updates by comparing current version with latest GitHub release.
async fn run_check_update() -> Result<()> {
    use neomind_core::brand::APP_VERSION;

    println!("NeoMind {}", APP_VERSION);
    println!("Checking for updates...\n");

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .user_agent(neomind_core::brand::user_agent())
        .build()?;

    let response = client
        .get("https://api.github.com/repos/camthink-ai/NeoMind/releases/latest")
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("Failed to check for updates: {}", e))?;

    if !response.status().is_success() {
        eprintln!("Failed to fetch release info (HTTP {})", response.status());
        return Ok(());
    }

    let release: serde_json::Value = response.json().await?;
    let latest_tag = release["tag_name"]
        .as_str()
        .unwrap_or("unknown")
        .trim_start_matches('v');

    let current = semver(APP_VERSION);
    let latest = semver(latest_tag);

    if current[0] < latest[0]
        || (current[0] == latest[0] && current[1] < latest[1])
        || (current[0] == latest[0] && current[1] == latest[1] && current[2] < latest[2])
    {
        println!("Update available: {} → v{}", APP_VERSION, latest_tag);

        if let Some(notes) = release["body"].as_str() {
            println!();
            // Show first 5 lines of release notes
            for line in notes.lines().take(5) {
                println!("  {}", line.trim());
            }
            let total_lines = notes.lines().count();
            if total_lines > 5 {
                println!("  ... ({} more lines)", total_lines - 5);
            }
        }

        if let Some(url) = release["html_url"].as_str() {
            println!("\nDownload: {}", url);
        }

        // Show relevant download assets
        if let Some(assets) = release["assets"].as_array() {
            let os_type = if cfg!(target_os = "macos") {
                "darwin"
            } else if cfg!(target_os = "linux") {
                "linux"
            } else {
                "windows"
            };
            let arch_type = if cfg!(target_arch = "aarch64") {
                "arm64"
            } else {
                "amd64"
            };

            let server_asset = assets.iter().find(|a| {
                a["name"]
                    .as_str()
                    .map(|n| n.contains(&format!("server-{}", os_type)) && n.contains(arch_type))
                    .unwrap_or(false)
            });

            if let Some(asset) = server_asset {
                if let (Some(name), Some(size)) = (asset["name"].as_str(), asset["size"].as_u64()) {
                    let size_mb = size as f64 / (1024.0 * 1024.0);
                    println!(
                        "\nServer binary for your platform: {} ({:.1} MB)",
                        name, size_mb
                    );
                }
            }

            println!("\nUpdate command:");
            println!(
                "  curl -fsSL https://raw.githubusercontent.com/camthink-ai/NeoMind/main/scripts/install.sh | sh"
            );
        }
    } else {
        println!("Already up to date (v{})", APP_VERSION);
    }

    Ok(())
}

/// Parse a semver string like "0.6.6" into [u32, u32, u32].
fn semver(v: &str) -> [u32; 3] {
    let parts: Vec<u32> = v.split('.').filter_map(|s| s.parse().ok()).collect();
    match parts.as_slice() {
        [a, b, c] => [*a, *b, *c],
        [a, b] => [*a, *b, 0],
        [a] => [*a, 0, 0],
        _ => [0, 0, 0],
    }
}

/// Run the web server.
async fn run_server(host: String, port: u16) -> Result<()> {
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid address: {}:{}", host, port))?;

    // Startup cleanup of old log files
    cleanup_old_logs();

    // Spawn periodic log cleanup (every 24 hours)
    tokio::spawn(async {
        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(24 * 60 * 60));
        loop {
            interval.tick().await;
            cleanup_old_logs();
        }
    });

    neomind_api::run(addr).await
}

/// Clean up log files older than 7 days in data/logs/.
fn cleanup_old_logs() {
    use std::fs;

    let log_dir = Path::new("data/logs");
    if !log_dir.exists() {
        return;
    }

    let max_age_secs: i64 = 7 * 24 * 60 * 60; // 7 days
    let now = chrono::Utc::now();

    let mut removed = 0u32;
    let mut kept = 0u32;

    if let Ok(entries) = fs::read_dir(log_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let should_remove = if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                if let Some(date_str) = filename.strip_prefix("neomind.log.") {
                    // Parse date from filename: neomind.log.YYYY-MM-DD
                    if let Ok(file_date) = chrono::NaiveDate::parse_from_str(date_str, "%Y-%m-%d") {
                        let file_datetime =
                            file_date.and_time(chrono::NaiveTime::default()).and_utc();
                        (now - file_datetime).num_seconds() > max_age_secs
                    } else {
                        // Non-date suffix, fall back to mtime
                        is_file_older_than(&path, max_age_secs)
                    }
                } else {
                    // Not a rotated log file (e.g., current neomind.log), skip
                    false
                }
            } else {
                false
            };

            if should_remove {
                if fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            } else {
                kept += 1;
            }
        }
    }

    if removed > 0 {
        tracing::info!(
            "Log cleanup: removed {} old file(s), kept {}",
            removed,
            kept
        );
    }
}

/// Check if a file is older than the given number of seconds (by mtime).
fn is_file_older_than(path: &Path, max_age_secs: i64) -> bool {
    let now = SystemTime::now();
    path.metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|mtime| now.duration_since(mtime).ok())
        .map(|age| age.as_secs() as i64 > max_age_secs)
        .unwrap_or(false)
}

/// Run health check command.
async fn run_health() -> Result<()> {
    println!("NeoMind System Health Check");
    println!("==========================\n");

    // Check if server is running
    println!("🔍 Checking server status...");
    let api_base = std::env::var("NEOMIND_API_BASE")
        .unwrap_or_else(|_| "http://localhost:9375/api".to_string());
    let check_url = format!("{}/health", api_base);
    match reqwest::get(&check_url).await {
        Ok(resp) if resp.status().is_success() => {
            println!("  ✅ Server is running");

            // Get detailed health status
            if let Ok(health_json) = resp.json::<serde_json::Value>().await {
                if let Some(status) = health_json.get("status").and_then(|v| v.as_str()) {
                    println!("  Status: {}", status);
                }
            }
        }
        Ok(resp) => {
            println!("  ⚠️  Server returned status: {}", resp.status());
        }
        Err(e) => {
            println!("  ❌ Server is not responding: {}", e);
            println!("  Hint: Start the server with 'neomind serve'");
        }
    }

    println!();

    // Check database files
    println!("🔍 Checking databases...");
    let data_dir = std::path::PathBuf::from("./data");
    if data_dir.exists() {
        let db_files = [
            "telemetry.redb",
            "sessions.redb",
            "devices.redb",
            "extensions.redb",
        ];
        for db in &db_files {
            let db_path = data_dir.join(db);
            if db_path.exists() {
                let metadata = std::fs::metadata(&db_path)?;
                let size_kb = metadata.len() / 1024;
                println!("  ✅ {} ({} KB)", db, size_kb);
            } else {
                println!("  ⚠️  {} not found (will be created on first use)", db);
            }
        }
    } else {
        println!("  ℹ️  Data directory not found (will be created on first use)");
    }

    println!();

    // Check LLM backend (use authenticated API client)
    println!("🔍 Checking LLM backend...");
    let api_client = neomind_cli_ops::ApiClient::new();
    match neomind_cli_ops::llm::list_backends(&api_client).await {
        Ok(response) => {
            if response.success {
                if let Some(data) = &response.data {
                    // Navigate past the CliResponse -> API response double-wrap
                    let inner = data.get("data").unwrap_or(data);
                    let count = inner.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let active_id = inner.get("active_id").and_then(|v| v.as_str());
                    if count > 0 {
                        println!("  ✅ {} LLM backend(s) configured", count);
                        if let Some(id) = active_id {
                            println!("  ✅ Active backend: {}", id);
                        } else {
                            println!("  ⚠️  No active backend selected");
                            println!(
                                "  Hint: Use 'neomind llm activate <id>' to activate a backend"
                            );
                        }
                    } else {
                        println!("  ⚠️  No LLM backend configured");
                        println!("  Hint: Use 'neomind llm add' to add an LLM backend");
                    }
                }
            } else {
                println!("  ⚠️  Could not query LLM backends");
            }
        }
        Err(e) => {
            println!("  ⚠️  Could not query LLM backends: {}", e);
        }
    }

    println!();

    // Check extensions directory
    println!("🔍 Checking extensions...");
    let extensions_dir = std::path::PathBuf::from("./extensions");
    if extensions_dir.exists() {
        let entries = std::fs::read_dir(&extensions_dir)?;
        let count = entries.filter_map(|e| e.ok()).count();
        println!("  ✅ Extensions directory found ({} items)", count);
    } else {
        println!("  ℹ️  Extensions directory not found");
    }

    println!();
    println!("Health check complete.");

    Ok(())
}

/// Run logs command.
async fn run_logs(
    tail: usize,
    follow: bool,
    level: Option<String>,
    _since: Option<String>,
) -> Result<()> {
    use std::fs::File;
    use std::io::{BufRead, BufReader, Seek};

    // Search log directories: project data/logs/, macOS Tauri, Linux Tauri, Windows Tauri
    let home = std::env::var("HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    let appdata = std::env::var("APPDATA").unwrap_or_default();

    let log_dirs = [
        std::path::PathBuf::from("data/logs"),
        // Current Tauri path (≥ 0.9.2): <app_data>/data/logs/
        // macOS
        std::path::PathBuf::from(&home)
            .join("Library/Application Support/com.neomind.neomind/data/logs"),
        // Linux
        std::path::PathBuf::from(&home).join(".local/share/com.neomind.neomind/data/logs"),
        // Windows
        std::path::PathBuf::from(&appdata).join("com.neomind.neomind/data/logs"),
        // Legacy Tauri path (≤ 0.9.1): <app_data>/logs/ — kept for one release
        // so users upgrading from older versions can still read pre-migration
        // logs via `neomind logs` before restarting the desktop app.
        std::path::PathBuf::from(&home)
            .join("Library/Application Support/com.neomind.neomind/logs"),
        std::path::PathBuf::from(&home).join(".local/share/com.neomind.neomind/logs"),
        std::path::PathBuf::from(&appdata).join("com.neomind.neomind/logs"),
        // Fallback: ~/.neomind/logs/
        std::path::PathBuf::from(&home).join(".neomind/logs"),
    ];

    let log_dir = log_dirs.iter().find(|d| d.exists()).ok_or_else(|| {
        let paths: String = log_dirs
            .iter()
            .filter(|d| !d.as_os_str().is_empty())
            .map(|d| format!("  {}", d.display()))
            .collect::<Vec<_>>()
            .join("\n");
        anyhow::anyhow!(
            "Log directory not found. Searched in:\n{}\n\
                 Hint: Start the server with 'neomind serve' or run the Tauri desktop app.",
            paths
        )
    })?;

    // Find the newest log file
    let log_path = find_newest_log(log_dir)?;

    if follow {
        // Follow mode (like tail -f)
        println!(
            "Following log file: {} (Ctrl+C to stop)\n",
            log_path.display()
        );

        let file = File::open(&log_path)?;
        let mut reader = BufReader::new(file);
        reader.seek(std::io::SeekFrom::End(0))?;

        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) => {
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                }
                Ok(_) => {
                    if matches_level(&line, &level) {
                        print!("{}", line);
                    }
                    line.clear();
                }
                Err(e) => {
                    eprintln!("Error reading log: {}", e);
                    break;
                }
            }
        }
    } else {
        // Tail mode - show last N lines
        let file = File::open(&log_path)?;
        let reader = BufReader::new(file);

        let lines: Vec<String> = reader
            .lines()
            .map_while(Result::ok)
            .filter(|l| matches_level(l, &level))
            .collect();

        let start = if lines.len() > tail {
            lines.len() - tail
        } else {
            0
        };

        println!(
            "Log file: {} (showing last {} lines)\n",
            log_path.display(),
            lines.len() - start
        );

        for line in lines.iter().skip(start) {
            println!("{}", line);
        }
    }

    Ok(())
}

/// Find the newest log file in the log directory.
fn find_newest_log(log_dir: &Path) -> Result<std::path::PathBuf> {
    let mut newest: Option<(std::path::PathBuf, SystemTime)> = None;

    for entry in std::fs::read_dir(log_dir)? {
        let entry = entry?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        // Only consider neomind log files
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            if !name.starts_with("neomind.log") {
                continue;
            }
        }
        let mtime = path.metadata().ok().and_then(|m| m.modified().ok());
        if let Some(mtime) = mtime {
            if newest.as_ref().is_none_or(|(_, prev)| mtime > *prev) {
                newest = Some((path, mtime));
            }
        }
    }

    newest
        .map(|(p, _)| p)
        .ok_or_else(|| anyhow::anyhow!("No log files found in data/logs/"))
}

/// Check if a log line matches the requested level filter.
fn matches_level(line: &str, level: &Option<String>) -> bool {
    match level {
        Some(lvl) => line.contains(lvl) || line.to_uppercase().contains(&lvl.to_uppercase()),
        None => true,
    }
}

/// Run extension management commands.
async fn run_extension_cmd(cmd: ExtensionCommand) -> Result<()> {
    // Local-only subcommands (validate/install/uninstall/create/build/info) are
    // handled here in the binary — they touch the local filesystem and print
    // directly to stdout. API subcommands are delegated to the cli-ops handler
    // (which is the same code path used by the in-process dispatcher).
    match &cmd {
        ExtensionCommand::Validate { .. } => {
            if let ExtensionCommand::Validate { path, verbose } = cmd {
                return validate_nep_package(&path, verbose).await;
            }
            unreachable!()
        }
        ExtensionCommand::Install { .. } => {
            if let ExtensionCommand::Install { package } = cmd {
                return install_extension(&package).await;
            }
            unreachable!()
        }
        ExtensionCommand::Uninstall { .. } => {
            if let ExtensionCommand::Uninstall { id } = cmd {
                return uninstall_extension(&id).await;
            }
            unreachable!()
        }
        ExtensionCommand::Create { .. } => {
            if let ExtensionCommand::Create {
                name,
                extension_type,
                output,
            } = cmd
            {
                create_extension_scaffold(&name, &extension_type, output)?;
                return Ok(());
            }
            unreachable!()
        }
        ExtensionCommand::Build { .. } => {
            if let ExtensionCommand::Build { path } = cmd {
                build_extension(&path)?;
                return Ok(());
            }
            unreachable!()
        }
        ExtensionCommand::Get { .. } => {
            // Get has local fallback logic, handled separately
            if let ExtensionCommand::Get { id_or_path } = cmd {
                return show_extension_info(&id_or_path).await;
            }
            unreachable!()
        }
        // API commands — delegate to cli-ops handler (shared with in-process dispatch).
        _ => print_result(neomind_cli_ops::dispatch::handlers::run_extension_cmd(cmd).await),
    }
}

/// Validate a .nep extension package.
async fn validate_nep_package(path: &std::path::PathBuf, verbose: bool) -> Result<()> {
    use std::fs::File;
    use zip::ZipArchive;

    if !path.exists() {
        anyhow::bail!("Extension package not found: {}", path.display());
    }

    if path.extension().is_none_or(|e| e != "nep") {
        anyhow::bail!(
            "Invalid extension package. Expected .nep file, got: {}",
            path.extension().unwrap_or_default().display()
        );
    }

    println!("Validating .nep package: {}", path.display());
    println!();

    // Open the ZIP archive
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;

    // Check for manifest.json - collect names first to avoid borrow issues
    let manifest_names: Vec<String> = archive
        .file_names()
        .filter(|n| n.ends_with("manifest.json"))
        .map(|s| s.to_string())
        .collect();

    if manifest_names.is_empty() {
        println!("❌ Validation FAILED");
        println!("   Missing manifest.json in package");
        std::process::exit(1);
    }

    // Read and parse manifest
    let manifest_path = &manifest_names[0];
    let mut manifest_file = archive.by_name(manifest_path)?;
    let mut manifest_content = String::new();
    manifest_file.read_to_string(&mut manifest_content)?;

    let manifest: serde_json::Value = serde_json::from_str(&manifest_content)
        .map_err(|e| anyhow::anyhow!("Failed to parse manifest.json: {}", e))?;

    // Validate required fields
    let required_fields = ["id", "name", "version", "format_version"];
    let mut missing = Vec::new();

    for field in &required_fields {
        if manifest.get(field).is_none() {
            missing.push(*field);
        }
    }

    if !missing.is_empty() {
        println!("❌ Validation FAILED");
        println!("   Missing required fields: {}", missing.join(", "));
        std::process::exit(1);
    }

    // Display package info
    println!("✅ Validation PASSED");
    println!();
    println!(
        "ID:              {}",
        manifest["id"].as_str().unwrap_or("unknown")
    );
    println!(
        "Name:            {}",
        manifest["name"].as_str().unwrap_or("unknown")
    );
    println!(
        "Version:         {}",
        manifest["version"].as_str().unwrap_or("unknown")
    );
    println!(
        "Format Version:  {}",
        manifest["format_version"].as_str().unwrap_or("unknown")
    );

    if let Some(abi) = manifest.get("abi_version").and_then(|v| v.as_u64()) {
        println!("ABI Version:     {}", abi);
    }

    if let Some(desc) = manifest.get("description").and_then(|v| v.as_str()) {
        println!("Description:     {}", desc);
    }

    // List binaries
    if let Some(binaries) = manifest.get("binaries").and_then(|v| v.as_object()) {
        println!();
        println!("Binaries:");
        for (platform, path) in binaries {
            println!("  {}: {}", platform, path.as_str().unwrap_or("unknown"));
        }
    }

    if verbose {
        // manifest_file is no longer used, drop it to release archive borrow
        drop(manifest_file);

        println!();
        println!("--- Verbose Details ---");
        println!("Package size:    {} bytes", path.metadata()?.len());
        println!("Package path:    {}", path.display());
        println!("Files in package: {}", archive.len());

        println!();
        println!("Package contents:");
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            println!("  {}", file.name());
        }
    }

    Ok(())
}

/// List installed extensions.
/// Show extension information.
async fn show_extension_info(id_or_path: &str) -> Result<()> {
    use neomind_cli_ops::output::format_output;
    use neomind_cli_ops::types::OutputFormat;

    let path = std::path::PathBuf::from(id_or_path);

    if path.exists() {
        // It's a file path, validate it
        validate_nep_package(&path, true).await?;
        return Ok(());
    }

    // Try API first (shows runtime info: status, commands, metrics)
    let client = neomind_cli_ops::ApiClient::new();
    if let Ok(response) = neomind_cli_ops::extension::get_extension(&client, id_or_path).await {
        let output_format = if std::env::var("NEOMIND_JSON").is_ok() {
            OutputFormat::Json
        } else {
            OutputFormat::Human
        };
        format_output(&response, output_format);
        return Ok(());
    }

    // Fallback: search local filesystem for .nep files
    use std::fs;

    let search_dirs = [
        std::path::PathBuf::from("./data/extensions"),
        std::path::PathBuf::from("./extensions"),
    ];

    let mut found = None;
    'search: for search_dir in &search_dirs {
        if let Ok(entries) = fs::read_dir(search_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let entry_path = entry.path();
                if entry_path.extension().is_some_and(|e| e == "nep") {
                    if let Some(stem) = entry_path.file_stem() {
                        if stem.to_str().unwrap_or("").contains(id_or_path) {
                            found = Some(entry_path);
                            break 'search;
                        }
                    }
                }
            }
        }
    }

    if let Some(found_path) = found {
        validate_nep_package(&found_path, true).await?;
        Ok(())
    } else {
        anyhow::bail!("Extension not found: {}", id_or_path);
    }
}

/// Install an extension from .nep package.
async fn install_extension(package: &str) -> Result<()> {
    let source_path = std::path::PathBuf::from(package);

    if !source_path.exists() {
        anyhow::bail!("Package file not found: {}", package);
    }

    println!("Installing extension from: {}", package);

    // Validate first
    validate_nep_package(&source_path, false).await?;

    // Create target directory
    let target_dir = std::path::PathBuf::from("./data/extensions");
    std::fs::create_dir_all(&target_dir)?;

    let target_path = target_dir.join(source_path.file_name().unwrap());

    // Copy package
    std::fs::copy(&source_path, &target_path)?;

    println!();
    println!("✅ Extension installed successfully!");
    println!("   Location: {}", target_path.display());
    println!();
    println!("Note: The extension will be loaded on next server restart.");
    println!("      Or use the Web UI to load it dynamically.");

    Ok(())
}

/// Uninstall an extension.
async fn uninstall_extension(id: &str) -> Result<()> {
    use std::fs;

    let search_dirs = [
        std::path::PathBuf::from("./data/extensions"),
        std::path::PathBuf::from("./extensions"),
    ];

    let mut found = Vec::new();

    for search_dir in &search_dirs {
        if let Ok(entries) = fs::read_dir(search_dir) {
            for entry in entries.filter_map(|e| e.ok()) {
                let path = entry.path();
                if path.extension().is_some_and(|e| e == "nep") {
                    if let Some(stem) = path.file_stem() {
                        if stem.to_str().unwrap_or("").contains(id) {
                            found.push(path);
                        }
                    }
                }
            }
        }
    }

    if found.is_empty() {
        anyhow::bail!("Extension not found: {}", id);
    }

    if found.len() > 1 {
        println!("Found multiple extensions matching '{}':", id);
        for (i, path) in found.iter().enumerate() {
            println!("  {}. {}", i + 1, path.display());
        }
        anyhow::bail!("Please be more specific");
    }

    let path = &found[0];

    println!("Uninstalling extension: {}", path.display());
    println!("This will delete the extension package.");
    print!("Confirm? [y/N] ");
    use std::io::Write;
    std::io::stdout().flush()?;

    let mut input = String::new();
    std::io::stdin().read_line(&mut input)?;

    if !input.trim().to_lowercase().starts_with('y') {
        println!("Cancelled.");
        return Ok(());
    }

    fs::remove_file(path)?;

    println!("✅ Extension uninstalled successfully!");

    Ok(())
}

/// Create a new extension scaffold with a complete, compilable project.
fn create_extension_scaffold(
    name: &str,
    _extension_type: &str,
    output: Option<std::path::PathBuf>,
) -> Result<()> {
    use std::fs;

    // Validate name is kebab-case (lowercase, digits, hyphens, no spaces)
    if !name
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        anyhow::bail!(
            "Extension name must be kebab-case (lowercase letters, digits, and hyphens only). Got: '{}'",
            name
        );
    }
    if name.starts_with('-') || name.ends_with('-') {
        anyhow::bail!(
            "Extension name must not start or end with a hyphen. Got: '{}'",
            name
        );
    }

    // Derive Rust identifiers
    let crate_name = format!("neomind_{}", name.replace('-', "_"));
    // Strip optional "neomind-" prefix for struct name, then PascalCase
    let struct_base = name.strip_prefix("neomind-").unwrap_or(name);
    let struct_name = struct_base
        .split('-')
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                None => String::new(),
                Some(f) => f.to_uppercase().collect::<String>() + chars.as_str(),
            }
        })
        .collect::<String>();

    // Resolve output directory
    let out_dir = output.unwrap_or_else(|| std::path::PathBuf::from(name));
    if out_dir.exists() {
        anyhow::bail!("Output directory already exists: {}", out_dir.display());
    }

    // SDK version — use the version of this binary's SDK dependency
    let sdk_version = "0.6.3";

    // Create directory structure
    let src_dir = out_dir.join("src");
    fs::create_dir_all(&src_dir)?;

    // --- Cargo.toml ---
    let cargo_toml = format!(
        r#"[package]
name = "{crate_name}"
version = "0.1.0"
edition = "2021"

[workspace]

[lib]
crate-type = ["cdylib"]

[dependencies]
neomind-extension-sdk = "{sdk_version}"
async-trait = "0.1"
serde_json = "1.0"
tokio = {{ version = "1", features = ["rt-multi-thread", "sync"] }}
"#,
    );
    fs::write(out_dir.join("Cargo.toml"), cargo_toml)?;

    // --- src/lib.rs ---
    let lib_rs = format!(
        r#"use async_trait::async_trait;
use neomind_extension_sdk::{{
    neomind_export, Extension, ExtensionCommand, ExtensionError, ExtensionMetadata,
    ExtensionMetricValue, MetricDataType, MetricDescriptor, ParamMetricValue,
    ParameterDefinition, Result,
}};
use serde_json::json;

pub struct {struct_name};

impl {struct_name} {{
    pub fn new() -> Self {{
        Self
    }}
}}

impl Default for {struct_name} {{
    fn default() -> Self {{
        Self::new()
    }}
}}

#[async_trait]
impl Extension for {struct_name} {{
    fn metadata(&self) -> &ExtensionMetadata {{
        static META: std::sync::OnceLock<ExtensionMetadata> = std::sync::OnceLock::new();
        META.get_or_init(|| {{
            ExtensionMetadata::new("{name}", "{struct_name}", "0.1.0")
                .with_description("A NeoMind extension")
        }})
    }}

    fn commands(&self) -> Vec<ExtensionCommand> {{
        vec![ExtensionCommand {{
            name: "hello".to_string(),
            display_name: "Hello".to_string(),
            description: "Returns a greeting".to_string(),
            payload_template: String::new(),
            parameters: vec![ParameterDefinition {{
                name: "name".to_string(),
                display_name: "Name".to_string(),
                description: "Who to greet".to_string(),
                param_type: MetricDataType::String,
                required: true,
                default_value: None,
                min: None,
                max: None,
                options: Vec::new(),
            }}],
            fixed_values: std::collections::HashMap::new(),
            samples: vec![json!({{"name": "world"}})],
            parameter_groups: Vec::new(),
        }}]
    }}

    fn metrics(&self) -> Vec<MetricDescriptor> {{
        vec![MetricDescriptor {{
            name: "invocations".to_string(),
            display_name: "Invocations".to_string(),
            data_type: MetricDataType::Integer,
            unit: "count".to_string(),
            min: Some(0.0),
            max: None,
            required: false,
        }}]
    }}

    async fn execute_command(
        &self,
        command: &str,
        args: &serde_json::Value,
    ) -> Result<serde_json::Value> {{
        match command {{
            "hello" => {{
                let name = args
                    .get("name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("world");
                Ok(json!({{"greeting": format!("Hello, {{}}!", name)}}))
            }}
            _ => Err(ExtensionError::CommandNotFound(command.to_string())),
        }}
    }}

    fn produce_metrics(&self) -> Result<Vec<ExtensionMetricValue>> {{
        Ok(vec![ExtensionMetricValue::new(
            "invocations",
            ParamMetricValue::Integer(1),
        )])
    }}

    fn as_any(&self) -> &dyn std::any::Any {{
        self
    }}
}}

neomind_export!({struct_name});
"#,
    );
    fs::write(src_dir.join("lib.rs"), lib_rs)?;

    // --- manifest.json ---
    let manifest = serde_json::json!({
        "format": "neomind-extension-package",
        "abi_version": 3,
        "type": "native",
        "id": name,
        "name": struct_name,
        "version": "0.1.0",
        "capabilities": {
            "commands": ["hello"],
            "metrics": ["invocations"]
        }
    });
    fs::write(
        out_dir.join("manifest.json"),
        serde_json::to_string_pretty(&manifest)?,
    )?;

    // --- .gitignore ---
    fs::write(out_dir.join(".gitignore"), "/target/\n")?;

    println!("Extension created: {}", out_dir.display());
    println!();
    println!("Next steps:");
    println!("  cd {}", name);
    println!("  cargo build");
    println!("  neomind extension build ./{}", name);

    Ok(())
}

/// Build an extension from source.
///
/// Runs `cargo build --release`, then packages the resulting cdylib +
/// manifest.json into a `.nep` zip archive. On success, prints a structured
/// marker line `NEP_PATH=<path>` so downstream tools (and the agent) can
/// extract the package path deterministically.
fn build_extension(path: &std::path::PathBuf) -> Result<()> {
    use std::fs;
    use std::process::Command;

    if !path.exists() {
        anyhow::bail!("Extension path not found: {}", path.display());
    }

    println!("Building extension: {}", path.display());
    println!();

    let cargo_toml = path.join("Cargo.toml");
    if !cargo_toml.exists() {
        anyhow::bail!("Cargo.toml not found in extension path. Is this a Rust project?");
    }

    // 1. Read manifest.json to learn the extension id/version + expected crate name.
    let manifest_path = path.join("manifest.json");
    let manifest = if manifest_path.exists() {
        let raw = fs::read_to_string(&manifest_path)?;
        serde_json::from_str::<serde_json::Value>(&raw).map_err(|e| {
            anyhow::anyhow!(
                "Failed to parse manifest.json: {}. Please fix the JSON syntax.",
                e
            )
        })?
    } else {
        anyhow::bail!(
            "manifest.json not found at {}. Run 'neomind extension create' to scaffold a project first.",
            manifest_path.display()
        );
    };

    let ext_id = manifest
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow::anyhow!("manifest.json missing required 'id' field"))?
        .to_string();
    let ext_version = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("0.1.0")
        .to_string();

    // 2. cargo build --release
    let status = Command::new("cargo")
        .args(["build", "--release"])
        .current_dir(path)
        .status()?;

    if !status.success() {
        anyhow::bail!("Extension build failed (cargo build returned non-zero status)");
    }

    println!("✅ Cargo build succeeded");
    println!();

    // 3. Locate the compiled cdylib in target/release/.
    let target_dir = path.join("target").join("release");
    if !target_dir.exists() {
        anyhow::bail!(
            "target/release not found at {}. Did cargo build succeed?",
            target_dir.display()
        );
    }

    // cdylib extension varies by platform: .so (linux), .dylib (macos), .dll (windows)
    // Rust cdylibs are emitted as lib<name>.so / lib<name>.dylib / <name>.dll.
    // We skip Linux versioned variants like libfoo.so.1 (rare for Rust cdylibs) by
    // requiring the filename stem to contain no dot (i.e. only one extension).
    let lib_exts = ["so", "dylib", "dll"];
    let mut lib_path: Option<std::path::PathBuf> = None;
    for entry in fs::read_dir(&target_dir)? {
        let entry = entry?;
        let p = entry.path();
        let Some(ext) = p.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !lib_exts.contains(&ext) {
            continue;
        }
        // stem = filename without extension (e.g. "libfoo" from "libfoo.dylib")
        let stem = p
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        // Reject Linux versioned libs like libfoo.so.1.0 (stem contains digits+dots after .so)
        // For .so specifically: skip if there are extra dot-separated segments after the first.
        if ext == "so" && stem.contains('.') {
            continue;
        }
        lib_path = Some(p);
        break;
    }
    let lib_path = lib_path.ok_or_else(|| {
        anyhow::anyhow!(
            "No cdylib (.so/.dylib/.dll) found in {}. Ensure Cargo.toml has [lib] crate-type = [\"cdylib\"].",
            target_dir.display()
        )
    })?;

    println!("Found compiled binary: {}", lib_path.display());

    // 4. Determine current platform key for the .nep (e.g. darwin_aarch64).
    let platform = neomind_core::extension::package::detect_platform();
    println!("Detected platform: {}", platform);

    // 5. Package into a .nep zip archive at <ext_dir>/<id>-<version>.nep
    let nep_filename = format!("{}-{}.nep", ext_id, ext_version);
    let nep_path = path.join(&nep_filename);

    let lib_bytes = fs::read(&lib_path)?;
    let lib_name = lib_path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| anyhow::anyhow!("Invalid lib filename"))?
        .to_string();
    let manifest_str = serde_json::to_string_pretty(&manifest)?;

    let file = std::fs::File::create(&nep_path)?;
    let mut writer = zip::ZipWriter::new(file);
    use zip::write::SimpleFileOptions;
    let opts = SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .unix_permissions(0o644);

    writer.start_file("manifest.json", opts)?;
    std::io::Write::write_all(&mut writer, manifest_str.as_bytes())?;

    let bin_archive_path = format!("binaries/{}/{}", platform, lib_name);
    writer.start_file(&bin_archive_path, opts)?;
    std::io::Write::write_all(&mut writer, &lib_bytes)?;

    // Optional frontend/dist directory — include if present.
    let frontend_dir = path.join("frontend").join("dist");
    if frontend_dir.exists() {
        for entry in walk_dir(&frontend_dir)? {
            let rel = entry.strip_prefix(path).unwrap_or(&entry);
            let rel_str = rel.to_string_lossy().replace('\\', "/");
            if entry.is_file() {
                let bytes = fs::read(&entry)?;
                writer.start_file(&rel_str, opts)?;
                std::io::Write::write_all(&mut writer, &bytes)?;
            }
        }
        println!("Included frontend/dist assets");
    }

    writer.finish()?;

    let nep_size = fs::metadata(&nep_path)?.len();
    println!();
    println!("✅ Extension packaged successfully!");
    println!("  Package: {}", nep_path.display());
    println!("  Size:    {} bytes", nep_size);
    println!(
        "  Install: neomind extension install {}",
        nep_path.display()
    );
    println!();
    // Structured marker for downstream parsing (agent, scripts).
    println!("NEP_PATH={}", nep_path.display());

    Ok(())
}

/// Recursively collect all files under `dir`.
fn walk_dir(dir: &std::path::Path) -> Result<Vec<std::path::PathBuf>> {
    let mut out = Vec::new();
    let mut stack: Vec<std::path::PathBuf> = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d)? {
            let entry = entry?;
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    Ok(out)
}

/// Run API key management commands.
/// Run LLM backend management commands.
async fn run_api_key_cmd(cmd: ApiKeyCommand) -> Result<()> {
    match cmd {
        ApiKeyCommand::Create { name, data_dir } => {
            std::fs::create_dir_all(&data_dir)?;

            let auth = neomind_api::auth::AuthState::new_with_data_dir(&data_dir);
            let (key, info) = auth.create_key(name.clone(), vec!["*".to_string()]).await;

            println!("API Key created successfully!");
            println!();
            println!("  Name: {}", info.name);
            println!("  ID:   {}", info.id);
            println!("  Key:  {}", key);
            println!();
            println!("IMPORTANT: Save this key now. It will not be shown again.");
        }
        ApiKeyCommand::List { data_dir } => {
            let auth = neomind_api::auth::AuthState::new_with_data_dir(&data_dir);
            let keys = auth.list_keys().await;

            if keys.is_empty() {
                println!("No API keys found.");
                return Ok(());
            }

            println!(
                "{:<36} {:<20} {:<12} {:<20}",
                "ID", "Name", "Active", "Created"
            );
            println!("{}", "-".repeat(90));
            for (_hash, info) in keys {
                let created = {
                    let days = info.created_at / 86400;
                    let time = info.created_at % 86400;
                    let hours = time / 3600;
                    let minutes = (time % 3600) / 60;
                    format!("{}-{:02}:{:02}", days, hours, minutes)
                };
                println!(
                    "{:<36} {:<20} {:<12} {}",
                    info.id,
                    info.name,
                    if info.active { "yes" } else { "no" },
                    created
                );
            }
        }
        ApiKeyCommand::Delete { name, data_dir } => {
            let auth = neomind_api::auth::AuthState::new_with_data_dir(&data_dir);
            let keys = auth.list_keys().await;
            let target = keys.iter().find(|(_, info)| info.name == name);

            match target {
                Some((hash, info)) => {
                    let removed = auth.delete_key_by_hash(hash).await;
                    if removed {
                        println!("Deleted API key '{}' (ID: {})", name, info.id);
                    } else {
                        anyhow::bail!("Failed to delete API key '{}'", name);
                    }
                }
                None => {
                    anyhow::bail!("API key '{}' not found", name);
                }
            }
        }
    }
    Ok(())
}
