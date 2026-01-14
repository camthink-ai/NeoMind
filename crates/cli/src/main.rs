//! Command-line interface for Edge AI Agent.

use std::net::SocketAddr;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};
use edge_ai_agent::{SessionManager, LlmBackend};
use edge_ai_core::config::{endpoints, models, env_vars,
                            normalize_ollama_endpoint, normalize_openai_endpoint};

/// Edge AI Agent - Run LLMs on edge devices.
#[derive(Parser, Debug)]
#[command(name = "edge-ai")]
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
        #[arg(short, long, default_value_t = 3000)]
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
    let args = Args::parse();

    // Initialize logging
    let _log_level = if args.verbose {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    // Check if JSON logging is requested (for production/container environments)
    let json_logging = std::env::var("NEOTALK_LOG_JSON")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(false);

    // Build the env filter for log level control
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| {
            tracing_subscriber::EnvFilter::new("edge_ai=info")
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
        Command::Serve { host, port } => {
            run_server(host, port).await
        }
        Command::Prompt {
            prompt,
            max_tokens: _,
            temperature: _,
        } => {
            run_prompt(&prompt).await
        }
        Command::Chat { session } => {
            run_chat(session).await
        }
        Command::ListModels { endpoint } => {
            list_models(endpoint).await
        }
        Command::Plugin { plugin_cmd } => {
            run_plugin_cmd(plugin_cmd).await
        }
    }
}

/// Initialize LLM backend from available config sources.
async fn init_llm_backend(session_manager: &SessionManager) -> Result<()> {
    // Try config.toml, then llm_config.json, then environment variables
    let backend = load_llm_backend_from_env()?;
    session_manager.set_llm_backend(backend).await
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
        return Ok(LlmBackend::OpenAi { api_key, endpoint, model });
    }

    Err(anyhow::anyhow!(
        "No LLM backend configured. Set OLLAMA_ENDPOINT or OPENAI_API_KEY environment variable."
    ))
}

/// Run a single prompt.
async fn run_prompt(prompt: &str) -> Result<()> {
    println!("Edge AI Agent - Prompt Mode");
    println!("============================\n");
    println!("Prompt: {}", prompt);
    println!("\nGenerating response...\n");

    // Create session manager
    let session_manager = SessionManager::new()
        .map_err(|e| anyhow::anyhow!("Failed to create session manager: {}", e))?;

    // Initialize LLM backend
    init_llm_backend(&session_manager).await?;

    // Create a temporary session
    let session_id = session_manager.create_session().await
        .map_err(|e| anyhow::anyhow!("Failed to create session: {}", e))?;

    // Process the prompt
    let response = session_manager.process_message(&session_id, prompt).await
        .map_err(|e| anyhow::anyhow!("Failed to process message: {}", e))?;

    println!("{}", response.message.content);
    println!("\nProcessing time: {}ms", response.processing_time_ms);

    // Clean up the temporary session
    let _ = session_manager.remove_session(&session_id).await;

    Ok(())
}

/// Run interactive chat mode.
async fn run_chat(session_id: Option<String>) -> Result<()> {
    println!("Edge AI Agent - Chat Mode");
    println!("=========================\n");

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
        let sid = session_manager.create_session().await
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
        match session_manager.process_message_events(&session_id, input).await {
            Ok(mut stream) => {
                use futures::stream::StreamExt;
                let mut response_text = String::new();

                while let Some(event) = stream.next().await {
                    match event {
                        edge_ai_agent::AgentEvent::Thinking { content } => {
                            if !content.is_empty() {
                                print!("\x1b[90m[Thinking: {}...]\x1b[0m", content.trim());
                                use std::io::Write;
                                std::io::stdout().flush()?;
                            }
                        }
                        edge_ai_agent::AgentEvent::Content { content } => {
                            if !response_text.is_empty() && response_text != content {
                                // Print incremental content
                                let added = content.strip_prefix(&response_text).unwrap_or(&content);
                                print!("{}", added);
                                use std::io::Write;
                                std::io::stdout().flush()?;
                            }
                            response_text = content;
                        }
                        edge_ai_agent::AgentEvent::ToolCallStart { tool, .. } => {
                            println!("\n\n[Calling tool: {}]", tool);
                        }
                        edge_ai_agent::AgentEvent::ToolCallEnd { tool, success, .. } => {
                            if success {
                                println!("[Tool {} completed]\n", tool);
                            } else {
                                println!("[Tool {} failed]\n", tool);
                            }
                        }
                        edge_ai_agent::AgentEvent::Error { message } => {
                            eprintln!("\nError: {}", message);
                        }
                        edge_ai_agent::AgentEvent::End => {
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

    let response = client.get(&url)
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
    let addr: SocketAddr = format!("{}:{}", host, port).parse()
        .map_err(|_| anyhow::anyhow!("Invalid address: {}:{}", host, port))?;

    edge_ai_api::run(addr).await
}

/// Run plugin management commands.
async fn run_plugin_cmd(cmd: PluginCommand) -> Result<()> {
    use edge_ai_core::plugin::WasmPluginLoader;

    match cmd {
        PluginCommand::Validate { path, verbose } => {
            let loader = WasmPluginLoader::new();
            let result = loader.validate_plugin(&path);

            println!("{}", result.format_report());

            if verbose {
                println!("\n--- Verbose Details ---\n");
                println!("Plugin path: {}", path.display());
                println!("Current directory: {}", std::env::current_dir()?.display());
                println!("Search paths:");
                for sp in &loader.search_paths {
                    println!("  - {}", sp.display());
                }
            }

            std::process::exit(result.exit_code());
        }

        PluginCommand::Create { name, plugin_type, output } => {
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
        "tool", "llm_backend", "storage_backend",
        "device_adapter", "integration", "alert_channel",
        "rule_engine", "workflow_engine",
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
            PathBuf::from("/var/lib/neotalk/plugins"),
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
            if path.extension().map_or(false, |e| e == "wasm") {
                let plugin_name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?");

                if let Some(ref filter_type) = ty {
                    let json_path = path.with_extension("json");
                    if let Ok(content) = fs::read_to_string(&json_path) {
                        if let Ok(json) = serde_json::from_str::<serde_json::Value>(&content) {
                            if json.get("type")
                                .and_then(|v| v.as_str())
                                .map_or(true, |t| t != filter_type)
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
                        let plugin_name = path.file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or("?");

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

/// Show plugin metadata.
async fn show_plugin_info(path: &PathBuf) -> Result<()> {
    use edge_ai_core::plugin::WasmPluginLoader;

    if !path.exists() {
        anyhow::bail!("Plugin file not found: {}", path.display());
    }

    let is_wasm = path.extension().map_or(false, |e| e == "wasm");

    if is_wasm {
        let loader = WasmPluginLoader::new();

        match loader.load_metadata(path) {
            Ok(loaded) => {
                let meta = loaded.metadata;

                println!("Plugin Information");
                println!("==================\n");
                println!("ID:              {}", meta.base.id);
                println!("Name:            {}", meta.base.name);
                println!("Version:         {}", meta.version);
                println!("Type:            {:?}", meta.plugin_type);
                println!("Description:     {}", meta.base.description);
                println!("Author:          {}", meta.base.author.as_ref().unwrap_or(&"Unknown".to_string()));

                if let Some(homepage) = &meta.homepage {
                    println!("Homepage:        {}", homepage);
                }
                if let Some(repository) = &meta.repository {
                    println!("Repository:      {}", repository);
                }
                if let Some(license) = &meta.license {
                    println!("License:         {}", license);
                }

                println!("\nRequirements:");
                println!("  NeoTalk:        {}", meta.required_neotalk_version);

                if let Some(schema) = &meta.config_schema {
                    println!("\nConfig Schema:");
                    println!("{}", serde_json::to_string_pretty(schema)?);
                }

                println!("\nModule:          {}", path.display());
                println!("Memory Size:     {} MB", loaded.memory_size / (1024 * 1024));
            }
            Err(e) => {
                eprintln!("Error loading plugin metadata: {}", e);
                eprintln!();
                eprintln!("Make sure:");
                println!("  1. The plugin file is valid");
                println!("  2. A sidecar .json file exists with plugin metadata");
                println!("  3. Run: edge-ai plugin validate {}", path.display());
            }
        }
    } else {
        println!("Native Plugin");
        println!("==============\n");
        println!("Path: {}", path.display());
        println!();
        println!("Note: Detailed metadata extraction for native plugins");
        println!("      requires loading the plugin library.");
        println!("      Use: edge-ai plugin validate {}", path.display());
    }

    Ok(())
}
