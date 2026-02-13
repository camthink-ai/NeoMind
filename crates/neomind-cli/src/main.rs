//! Command-line interface for NeoMind.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use neomind_agent::{LlmBackend, SessionManager};
use neomind_core::config::{
    endpoints, env_vars, models, normalize_ollama_endpoint, normalize_openai_endpoint,
};

/// NeoMind AI Agent - Run LLMs on edge devices.
#[derive(Parser, Debug)]
#[command(name = "neomind")]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Action to perform.
    #[command(subcommand)]
    command: Command,

    /// Model path or identifier.
    #[arg(short, long, global = true)]
    model: Option<String>,

    /// Verbose output.
    #[arg(short, long, global = true)]
    verbose: bool,
}

/// Available commands.
#[derive(Subcommand, Debug)]
enum Command {
    /// Start the web server.
    Serve {
        /// Host to bind to.
        #[arg(long, default_value = "127.0.0.1")]
        host: String,
        /// Port to bind to.
        #[arg(short, long, default_value_t = 9375)]
        port: u16,
    },
    /// Run a single prompt and exit.
    Prompt {
        /// The prompt to process.
        prompt: String,
        /// Maximum tokens to generate.
        #[arg(long, default_value_t = usize::MAX)]
        max_tokens: usize,
        /// Temperature.
        #[arg(short, long, default_value_t = 0.7)]
        temperature: f32,
    },
    /// Chat mode (interactive REPL with session persistence).
    Chat {
        /// Session ID to resume (optional).
        #[arg(short, long)]
        session: Option<String>,
    },
    /// List available models from Ollama.
    ListModels {
        /// Ollama endpoint.
        #[arg(long, default_value = "http://localhost:11434")]
        endpoint: String,
    },
    /// Plugin management commands.
    Plugin {
        #[command(subcommand)]
        plugin_cmd: PluginCommand,
    },
}

/// Plugin subcommands.
#[derive(Subcommand, Debug)]
enum PluginCommand {
    /// Validate a plugin file (WASM or native).
    Validate {
        /// Path to the plugin file.
        #[arg(required = true)]
        path: std::path::PathBuf,
        /// Show detailed output.
        #[arg(short, long)]
        verbose: bool,
    },
    /// Create a new plugin scaffold.
    Create {
        /// Plugin ID (lowercase, hyphens only).
        #[arg(required = true)]
        name: String,
        /// Plugin type.
        #[arg(short, long, default_value = "tool")]
        plugin_type: String,
        /// Output directory.
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },
    /// List discovered plugins.
    List {
        /// Plugin directory to scan.
        #[arg(short, long)]
        dir: Option<std::path::PathBuf>,
        /// Plugin type filter.
        #[arg(short, long)]
        ty: Option<String>,
    },
    /// Show plugin metadata.
    Info {
        /// Path to the plugin file.
        #[arg(required = true)]
        path: std::path::PathBuf,
    },
}

// Custom runtime with increased worker threads for better concurrent performance
// Default is num_cpus, but we use more to handle block_in_place alternatives
#[tokio::main(flavor = "multi_thread", worker_threads = 16)]
async fn main() -> Result<()> {
    // Set up panic hook to catch panics before they abort
    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("\n=== PANIC ===");
        if let Some(location) = panic_info.location() {
            eprintln!(
                "Location: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        } else {
            eprintln!("Location: <unknown>");
        }
        eprintln!("Message: {}", panic_info);
        eprintln!("==============\n");
    }));

    let args = Args::parse();

    // Initialize logging
    let _log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    // Check if JSON logging is requested (for production/container environments)
    let json_logging = std::env::var("NEOMIND_LOG_JSON")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(false);

    // Build the env filter for log level control
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        tracing_subscriber::EnvFilter::new("neomind=info")
            .add_directive(tracing::Level::INFO.into())
            .add_directive(tracing::Level::WARN.into())
    });

    if json_logging {
        // JSON format for production/container environments
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(env_filter)
            .with_target(true)
            .init();
    } else {
        // Human-readable format for development - clean and compact
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
        Command::Plugin { plugin_cmd } => run_plugin_cmd(plugin_cmd).await,
    }
}

/// Initialize LLM backend from available config sources.
async fn init_llm_backend(session_manager: &SessionManager) -> Result<()> {
    // Try config.toml, then llm_config.json, then environment variables
    let backend = load_llm_backend_from_env()?;
    session_manager
        .set_llm_backend(backend)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to set LLM backend: {}", e))?;
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
        return Ok(LlmBackend::Ollama { endpoint, model });
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
        });
    }

    Err(anyhow::anyhow!(
        "No LLM backend configured. Set OLLAMA_ENDPOINT or OPENAI_API_KEY environment variable."
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
            if let Ok(_) = session_manager.clear_history(&session_id).await {
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
                        neomind_agent::AgentEvent::End => {
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

/// Run the web server.
async fn run_server(host: String, port: u16) -> Result<()> {
    let addr: SocketAddr = format!("{}:{}", host, port)
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid address: {}:{}", host, port))?;

    neomind_api::run(addr).await
}

/// Run plugin management commands.
async fn run_plugin_cmd(cmd: PluginCommand) -> Result<()> {
    use neomind_core::extension::{loader::native::NativeExtensionLoader, WasmExtensionLoader};

    match cmd {
        PluginCommand::Validate { path, verbose } => {
            // Detect extension type by file extension
            let is_native = path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| matches!(e, "so" | "dylib" | "dll"));

            if is_native {
                // Use NativeExtensionLoader for .so/.dylib/.dll files
                let loader = NativeExtensionLoader::new();

                match loader.load(&path) {
                    Ok(loaded) => {
                        let ext_guard = loaded.extension.read().await;
                        let metadata = ext_guard.metadata();

                        println!("Extension Validation: PASSED");
                        println!();
                        println!("ID:              {}", metadata.id);
                        println!("Name:            {}", metadata.name);
                        println!("Version:         {}", metadata.version);

                        if verbose {
                            println!("\n--- Verbose Details ---\n");
                            println!("Extension path: {}", path.display());
                            println!("Current directory: {}", std::env::current_dir()?.display());
                            if let Some(file_path) = &metadata.file_path {
                                println!("File path:       {}", file_path.display());
                            }
                        }

                        std::process::exit(0);
                    }
                    Err(e) => {
                        println!("Extension Validation: FAILED");
                        println!();
                        println!("Error: {}", e);
                        println!();
                        println!("Make sure:");
                        println!("  1. The extension file exists");
                        println!("  2. The file has .so/.dylib/.dll extension");
                        println!("  3. The extension is built for V2 ABI");

                        std::process::exit(1);
                    }
                }
            } else {
                // Use WasmExtensionLoader for .wasm files
                let loader = WasmExtensionLoader::new()?;

                // Try to load the extension
                let result = loader.load(&path).await;

                match result {
                    Ok(loaded) => {
                        // Get metadata from the loaded extension
                        let ext = loaded.extension.blocking_read();
                        let metadata = ext.metadata();

                        println!("Extension Validation: PASSED");
                        println!();
                        println!("ID:              {}", metadata.id);
                        println!("Name:            {}", metadata.name);
                        println!("Version:         {}", metadata.version);

                        if verbose {
                            println!("\n--- Verbose Details ---\n");
                            println!("Extension path: {}", path.display());
                            println!("Current directory: {}", std::env::current_dir()?.display());
                            if let Some(file_path) = &metadata.file_path {
                                println!("File path:       {}", file_path.display());
                            }
                        }

                        std::process::exit(0);
                    }
                    Err(e) => {
                        println!("Extension Validation: FAILED");
                        println!();
                        println!("Error: {}", e);
                        println!();
                        println!("Make sure:");
                        println!("  1. The extension file exists");
                        println!("  2. The file has .wasm extension");
                        println!("  3. A sidecar .json file exists with metadata (optional)");

                        std::process::exit(1);
                    }
                } // Close match result
            } // Close else block
        } // Close Validate match arm

        PluginCommand::Create {
            name,
            plugin_type,
            output,
        } => {
            create_plugin_scaffold(&name, &plugin_type, output)?;
            Ok(())
        }

        PluginCommand::List { dir, ty } => {
            list_plugins(dir, ty).await?;
            Ok(())
        }

        PluginCommand::Info { path } => {
            show_plugin_info(&path).await?;
            Ok(())
        }
    }
}

/// Create a new plugin scaffold.
fn create_plugin_scaffold(name: &str, plugin_type: &str, _output: Option<PathBuf>) -> Result<()> {
    let valid_types = [
        "tool",
        "llm_backend",
        "storage_backend",
        "device_adapter",
        "integration",
        "alert_channel",
        "rule_engine",
        "workflow_engine",
    ];

    if !valid_types.contains(&plugin_type) {
        anyhow::bail!(
            "Invalid plugin type '{}'. Valid types: {}",
            plugin_type,
            valid_types.join(", ")
        );
    }

    println!("Creating plugin: {} (type: {})", name, plugin_type);
    println!();
    println!("Please use the shell script for full scaffold generation:");
    println!("  ./plugins/create-plugin.sh {} {}", name, plugin_type);
    println!();
    println!("Or manually create the plugin structure:");
    println!("  mkdir -p plugins/{}", name);
    println!("  cd plugins/{}", name);
    println!("  cargo init --lib");
    println!("  # Add edge-ai-core dependency and implement Plugin trait");

    Ok(())
}

/// List discovered plugins.
async fn list_plugins(dir: Option<PathBuf>, ty: Option<String>) -> Result<()> {
    use std::fs;

    let search_dirs = if let Some(d) = dir {
        vec![d]
    } else {
        vec![
            PathBuf::from("./plugins"),
            PathBuf::from("/var/lib/neomind/plugins"),
        ]
    };

    println!("Discovered Plugins");
    println!("==================\n");

    let mut found_count = 0;

    for search_dir in &search_dirs {
        if !search_dir.exists() {
            continue;
        }

        let entries = fs::read_dir(search_dir)?;

        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();

            // Check for WASM plugins
            if path.extension().is_some_and(|e| e == "wasm") {
                let plugin_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");

                if let Some(ref filter_type) = ty {
                    let json_path = path.with_extension("json");
                    if let Ok(content) = fs::read_to_string(&json_path) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            if json
                                .get("type")
                                .and_then(|v| v.as_str())
                                .is_none_or(|t| t != filter_type)
                            {
                                continue;
                            }
                        }
                    }
                }

                println!("  WASM: {}", plugin_name);
                println!("        Path: {}", path.display());
                let json_path = path.with_extension("json");
                if json_path.exists() {
                    println!("        Metadata: {}", json_path.display());
                }
                println!();
                found_count += 1;
            }

            // Check for native plugins (.so, .dylib, .dll)
            if let Some(ext) = path.extension() {
                match ext.to_str() {
                    Some("so") | Some("dylib") | Some("dll") => {
                        let plugin_name = path.file_stem().and_then(|s| s.to_str()).unwrap_or("?");

                        println!("  Native: {}", plugin_name);
                        println!("          Path: {}", path.display());
                        println!();
                        found_count += 1;
                    }
                    _ => {}
                }
            }
        }
    }

    if found_count == 0 {
        println!("  No plugins found.");
        println!();
        println!("  Searched in:");
        for d in &search_dirs {
            println!("    - {}", d.display());
        }
    } else {
        println!("Total: {} plugin(s)", found_count);
    }

    Ok(())
}

/// Show extension metadata.
async fn show_plugin_info(path: &PathBuf) -> Result<()> {
    use neomind_core::extension::WasmExtensionLoader;

    if !path.exists() {
        anyhow::bail!("Extension file not found: {}", path.display());
    }

    let is_wasm = path.extension().is_some_and(|e| e == "wasm");

    if is_wasm {
        let loader = WasmExtensionLoader::new()?;

        match loader.load(path).await {
            Ok(loaded) => {
                // Get metadata from the loaded extension
                let ext = loaded.extension.blocking_read();
                let metadata = ext.metadata();

                println!("Extension Information");
                println!("======================\n");
                println!("ID:              {}", metadata.id);
                println!("Name:            {}", metadata.name);
                println!("Version:         {}", metadata.version);
                if let Some(description) = &metadata.description {
                    println!("Description:     {}", description);
                }
                if let Some(author) = &metadata.author {
                    println!("Author:          {}", author);
                }

                if let Some(file_path) = &metadata.file_path {
                    println!("\nModule:          {}", file_path.display());
                }

                if let Some(homepage) = &metadata.homepage {
                    println!("Homepage:        {}", homepage);
                }
                if let Some(license) = &metadata.license {
                    println!("License:         {}", license);
                }
            }
            Err(e) => {
                eprintln!("Error loading extension metadata: {}", e);
                eprintln!();
                eprintln!("Make sure:");
                println!("  1. The extension file is valid");
                println!("  2. A sidecar .json file exists with extension metadata");
                println!("  3. Run: neomind plugin validate {}", path.display());
            }
        }
    } else {
        println!("Native Extension");
        println!("==================\n");
        println!("Path: {}", path.display());
        println!();
        println!("Note: Detailed metadata extraction for native extensions");
        println!("      requires loading the extension library.");
        println!("      Use: neomind plugin validate {}", path.display());
    }

    Ok(())
}
