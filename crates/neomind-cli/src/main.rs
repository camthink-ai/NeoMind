//! Command-line interface for NeoMind.

use std::net::SocketAddr;
use std::io::Read;

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
        #[arg(long, default_value = "0.0.0.0")]
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
    /// Check system health and status.
    Health,
    /// View system logs.
    Logs {
        /// Number of lines to show (default: 50).
        #[arg(long, default_value_t = 50)]
        tail: usize,
        /// Follow log output (like tail -f).
        #[arg(long)]
        follow: bool,
        /// Filter by log level (ERROR, WARN, INFO, DEBUG).
        #[arg(long)]
        level: Option<String>,
        /// Show logs from the last duration (e.g., "1h", "30m").
        #[arg(long)]
        since: Option<String>,
    },
    /// Extension management commands.
    Extension {
        #[command(subcommand)]
        extension_cmd: ExtensionCommand,
    },
}

/// Extension subcommands.
#[derive(Subcommand, Debug)]
enum ExtensionCommand {
   /// Validate a .nep extension package.
    Validate {
        /// Path to the .nep file.
        #[arg(required = true)]
        path: std::path::PathBuf,
        /// Show detailed output.
        #[arg(short, long)]
        verbose: bool,
    },
    /// List installed extensions.
    List {
        /// Show detailed information.
        #[arg(short, long)]
        verbose: bool,
    },
    /// Show extension information.
    Info {
        /// Extension ID or .nep file path.
        #[arg(required = true)]
        id_or_path: String,
    },
    /// Install a .nep extension package.
    Install {
        /// Path to the .nep file or URL.
        #[arg(required = true)]
        package: String,
    },
    /// Uninstall an extension.
    Uninstall {
        /// Extension ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new extension scaffold.
    Create {
        /// Extension ID (lowercase, hyphens only).
        #[arg(required = true)]
        name: String,
        /// Extension type.
        #[arg(short, long, default_value = "tool")]
        extension_type: String,
        /// Output directory.
        #[arg(short, long)]
        output: Option<std::path::PathBuf>,
    },
}

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
        Command::Health => run_health().await,
        Command::Logs { tail, follow, level, since } => run_logs(tail, follow, level, since).await,
        Command::Extension { extension_cmd } => run_extension_cmd(extension_cmd).await,
    }
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
        return Ok(LlmBackend::Ollama { endpoint, model, capabilities: None });
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

/// Run health check command.
async fn run_health() -> Result<()> {
    
    
    println!("NeoMind System Health Check");
    println!("==========================\n");
    
    // Check if server is running
    println!("🔍 Checking server status...");
    let check_url = "http://localhost:9375/api/health";
    match reqwest::get(check_url).await {
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
        let db_files = ["telemetry.redb", "sessions.redb", "devices.redb", "extensions.redb"];
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
    
    // Check LLM backend
    println!("🔍 Checking LLM backend...");
    if std::env::var("OLLAMA_ENDPOINT").is_ok() || std::env::var("OPENAI_API_KEY").is_ok() {
        if std::env::var("OLLAMA_ENDPOINT").is_ok() {
            let endpoint = std::env::var("OLLAMA_ENDPOINT").unwrap_or_default();
            println!("  ✅ Ollama configured: {}", endpoint);
        }
        if std::env::var("OPENAI_API_KEY").is_ok() {
            println!("  ✅ OpenAI configured");
        }
    } else {
        println!("  ⚠️  No LLM backend configured");
        println!("  Hint: Set OLLAMA_ENDPOINT or OPENAI_API_KEY environment variable");
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
async fn run_logs(tail: usize, follow: bool, level: Option<String>, _since: Option<String>) -> Result<()> {
    use std::io::{BufRead, BufReader};
    use std::fs::File;
    use std::path::Path;
    
    // Find log file
    let log_paths = [
        "./neomind.log",
        "./data/neomind.log",
        "/var/log/neomind.log",
    ];
    
    let log_path = log_paths.iter()
        .find(|p| Path::new(p).exists())
        .ok_or_else(|| anyhow::anyhow!("Log file not found. Searched in: {:?}", log_paths))?;
    
    if follow {
        // Follow mode (like tail -f)
        println!("Following log file: {} (Ctrl+C to stop)\n", log_path);
        
        let file = File::open(log_path)?;
        let _metadata = file.metadata()?;
        let mut reader = BufReader::new(file);
        
        // Seek to end first
        use std::io::Seek;
        reader.seek(std::io::SeekFrom::End(0))?;
        
        // Read new lines
        let mut line = String::new();
        loop {
            match reader.read_line(&mut line) {
                Ok(0) => {
                    // EOF reached, wait for more data
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                    continue;
                }
                Ok(_) => {
                    let should_print = if let Some(ref lvl) = level {
                        line.contains(lvl) || line.to_uppercase().contains(lvl.as_str())
                    } else {
                        true
                    };
                    
                    if should_print {
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
        let file = File::open(log_path)?;
        let reader = BufReader::new(file);
        
        let lines: Vec<String> = reader.lines()
            .filter_map(|l| l.ok())
            .filter(|l| {
                if let Some(ref lvl) = level {
                    l.contains(lvl) || l.to_uppercase().contains(lvl.as_str())
                } else {
                    true
                }
            })
            .collect();
        
        let start = if lines.len() > tail { lines.len() - tail } else { 0 };
        
        println!("Log file: {} (showing last {} lines)\n", log_path, 
                 lines.len() - start);
        
        for line in lines.iter().skip(start) {
            println!("{}", line);
        }
    }
    
    Ok(())
}

/// Run extension management commands.
async fn run_extension_cmd(cmd: ExtensionCommand) -> Result<()> {
    match cmd {
        ExtensionCommand::Validate { path, verbose } => {
            validate_nep_package(&path, verbose).await
        }
        
        ExtensionCommand::List { verbose } => {
            list_extensions(verbose).await
        }
        
        ExtensionCommand::Info { id_or_path } => {
            show_extension_info(&id_or_path).await
        }
        
        ExtensionCommand::Install { package } => {
            install_extension(&package).await
        }
        
        ExtensionCommand::Uninstall { id } => {
            uninstall_extension(&id).await
        }
        
        ExtensionCommand::Create {
            name,
            extension_type,
            output,
        } => {
            create_extension_scaffold(&name, &extension_type, output)?;
            Ok(())
        }
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
        anyhow::bail!("Invalid extension package. Expected .nep file, got: {}", 
                      path.extension().unwrap_or_default().display());
    }
    
    println!("Validating .nep package: {}", path.display());
    println!();
    
    // Open the ZIP archive
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    
    // Check for manifest.json - collect names first to avoid borrow issues
    let manifest_names: Vec<String> = archive.file_names()
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
    println!("ID:              {}", manifest["id"].as_str().unwrap_or("unknown"));
    println!("Name:            {}", manifest["name"].as_str().unwrap_or("unknown"));
    println!("Version:         {}", manifest["version"].as_str().unwrap_or("unknown"));
    println!("Format Version:  {}", manifest["format_version"].as_str().unwrap_or("unknown"));
    
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
async fn list_extensions(verbose: bool) -> Result<()> {
    use std::fs;
    
    let search_dirs = [
        std::path::PathBuf::from("./data/extensions"),
        std::path::PathBuf::from("./extensions"),
    ];
    
    println!("Installed Extensions");
    println!("====================\\n");
    
    let mut found_count = 0;
    
    for search_dir in &search_dirs {
        if !search_dir.exists() {
            continue;
        }
        
        let entries = fs::read_dir(search_dir)?;
        
        for entry in entries.filter_map(|e| e.ok()) {
            let path = entry.path();
            
            // Check for .nep files
            if path.extension().is_some_and(|e| e == "nep") {
                let name = path.file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("?");
                
                println!("📦 {}", name);
                println!("   Path: {}", path.display());
                
                if verbose {
                    // Try to read manifest
                    if let Ok(manifest) = read_nep_manifest(&path) {
                        if let Some(version) = manifest.get("version").and_then(|v| v.as_str()) {
                            println!("   Version: {}", version);
                        }
                        if let Some(desc) = manifest.get("description").and_then(|v| v.as_str()) {
                            println!("   Description: {}", desc);
                        }
                    }
                    println!("   Size: {} bytes", path.metadata()?.len());
                }
                
                println!();
                found_count += 1;
            }
        }
    }
    
    if found_count == 0 {
        println!("No extensions found.");
        println!();
        println!("Searched in:");
        for dir in &search_dirs {
            println!("  - {}", dir.display());
        }
        println!();
        println!("Install extensions using:");
        println!("  neomind extension install <package.nep>");
    } else {
        println!("Total: {} extension(s)", found_count);
    }
    
    Ok(())
}

/// Show extension information.
async fn show_extension_info(id_or_path: &str) -> Result<()> {
    let path = std::path::PathBuf::from(id_or_path);
    
    if path.exists() {
        // It's a file path, validate it
        validate_nep_package(&path, true).await?;
    } else {
        // It's an extension ID, search for it
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
        } else {
            anyhow::bail!("Extension not found: {}", id_or_path);
        }
    }
    
    Ok(())
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

/// Read manifest from .nep package.
fn read_nep_manifest(path: &std::path::PathBuf) -> Result<serde_json::Value> {
    use std::fs::File;
    use zip::ZipArchive;
    
    let file = File::open(path)?;
    let mut archive = ZipArchive::new(file)?;
    
    // Collect file names first to avoid borrowing issues
    let manifest_names: Vec<String> = archive.file_names()
        .filter(|n| n.ends_with("manifest.json"))
        .map(|s| s.to_string())
        .collect();
    
    if let Some(name) = manifest_names.into_iter().next() {
        let manifest_file = archive.by_name(&name)?;
        let mut content = String::new();
        let mut reader = std::io::BufReader::new(manifest_file);
        reader.read_to_string(&mut content)?;
        let manifest: serde_json::Value = serde_json::from_str(&content)?;
        return Ok(manifest);
    }
    
    anyhow::bail!("No manifest.json found in package")
}


/// Create a new extension scaffold.
fn create_extension_scaffold(name: &str, extension_type: &str, _output: Option<std::path::PathBuf>) -> Result<()> {
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

    if !valid_types.contains(&extension_type) {
        anyhow::bail!(
            "Invalid extension type '{}'. Valid types: {}",
            extension_type,
            valid_types.join(", ")
        );
    }

    println!("Creating extension: {} (type: {})", name, extension_type);
    println!();
    println!("Please use the extension SDK for full scaffold generation:");
    println!("  See: https://github.com/camthink-ai/NeoMind-Extensions");
    println!();
    println!("Or manually create the extension structure:");
    println!("  mkdir -p extensions/{}", name);
    println!("  cd extensions/{}", name);
    println!("  cargo init --lib");
    println!("  # Add neomind-extension-sdk dependency and implement Extension trait");

    Ok(())
}

