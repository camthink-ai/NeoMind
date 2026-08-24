//! Clap command definitions shared between the \`neomind\` binary and the
//! in-process dispatcher. Moved verbatim from the binary so that both the
//! real CLI and the agent-side dispatch parse the exact same argument surface.

use clap::{Parser, Subcommand};

/// NeoMind AI Agent - Run LLMs on edge devices.
#[derive(Parser, Debug)]
#[command(name = "neomind")]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Action to perform.
    #[command(subcommand)]
    pub command: Command,

    /// Model path or identifier.
    #[arg(short, long, global = true)]
    pub model: Option<String>,

    /// Verbose output.
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

/// Available commands.
#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
pub enum Command {
    /// Start the web server (API on :9375, swagger at /api/docs).
    ///
    /// Example: `neomind serve --port 9375`
    Serve {
        /// Host to bind to.
        #[arg(long, default_value = "0.0.0.0")]
        host: String,
        /// Port to bind to.
        #[arg(short, long, default_value_t = 9375)]
        port: u16,
    },
    /// Run a single prompt and exit (non-interactive).
    ///
    /// Example: `neomind prompt "Summarize the latest sensor readings"`
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
    ///
    /// Resume an existing session with --session <ID>.
    ///
    /// Example: `neomind chat`
    /// Example: `neomind chat --session sess-001`
    Chat {
        /// Session ID to resume (optional).
        #[arg(short, long)]
        session: Option<String>,
    },
    /// List available models from Ollama.
    ///
    /// Prefer `llm models` for the same result under a clearer namespace.
    ///
    /// Example: `neomind list-models --endpoint http://localhost:11434`
    ListModels {
        /// Ollama endpoint.
        #[arg(long, default_value = "http://localhost:11434")]
        endpoint: String,
    },
    /// Check system health and status.
    ///
    /// Example: `neomind health`
    Health,
    /// View system logs.
    ///
    /// Example: `neomind logs --tail 100 --follow`
    /// Example: `neomind logs --level ERROR --since 1h`
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
    /// Check for updates.
    ///
    /// Example: `neomind check-update`
    CheckUpdate,
    /// Self-upgrade the NeoMind server binary (and web frontend) to the latest
    /// release. Downloads the host-arch server tarball, verifies its version,
    /// backs up the current binary, swaps it in, and restarts the systemd
    /// service if one is running. Linux only.
    ///
    /// Examples: `neomind upgrade`, `neomind upgrade --version 0.9.11 --yes`
    Upgrade {
        /// Specific version to upgrade to (default: latest release).
        #[arg(long)]
        version: Option<String>,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// Uninstall NeoMind: stop + disable the systemd service and remove the
    /// binary + service unit. `--purge` also deletes the data and web dirs.
    /// Linux only.
    ///
    /// Examples: `neomind uninstall`, `neomind uninstall --purge --yes`
    Uninstall {
        /// Also remove the data directory and web files.
        #[arg(long)]
        purge: bool,
        /// Skip the confirmation prompt.
        #[arg(long)]
        yes: bool,
    },
    /// LLM backend management commands.
    Llm {
        #[command(subcommand)]
        llm_cmd: LlmCommand,
    },
    /// Extension management commands.
    Extension {
        #[command(subcommand)]
        extension_cmd: ExtensionCommand,
    },
    /// API key management commands.
    ApiKey {
        #[command(subcommand)]
        key_cmd: ApiKeyCommand,
    },
    /// Device management commands.
    Device {
        #[command(subcommand)]
        device_cmd: DeviceCommand,
    },
    /// Dashboard management commands.
    Dashboard {
        #[command(subcommand)]
        dashboard_cmd: DashboardCommand,
    },
    /// Rule management commands.
    Rule {
        #[command(subcommand)]
        rule_cmd: RuleCommand,
    },
    /// Transform management commands.
    Transform {
        #[command(subcommand)]
        transform_cmd: TransformCommand,
    },
    /// Agent management commands.
    Agent {
        #[command(subcommand)]
        agent_cmd: AgentCommand,
    },
    /// Message management commands.
    Message {
        #[command(subcommand)]
        message_cmd: MessageCommand,
    },
    /// Data push management commands.
    ///
    /// Forward device metrics and extension outputs to external systems.
    /// Target types: webhook, mqtt. Schedule types: event (real-time), interval.
    Push {
        #[command(subcommand)]
        push_cmd: PushCommand,
    },
    /// Widget management commands.
    Widget {
        #[command(subcommand)]
        widget_cmd: WidgetCommand,
    },
    /// System information and infrastructure.
    System {
        #[command(subcommand)]
        system_cmd: SystemCommand,
    },
    /// Backup / restore the full system configuration.
    ///
    /// Example: `neomind config export > backup.json`
    /// Example: `neomind config import backup.json` (reads the file)
    Config {
        #[command(subcommand)]
        config_cmd: ConfigCommand,
    },
    /// Data connector management (MQTT, webhook, HTTP, etc.).
    Connector {
        #[command(subcommand)]
        connector_cmd: ConnectorCommand,
    },
    /// System settings (timezone, data retention).
    Settings {
        #[command(subcommand)]
        settings_cmd: SettingsCommand,
    },
    /// Log in by reading a key from the server's auth DB and saving it locally.
    ///
    /// After `neomind login`, the CLI works from any working directory without
    /// needing `NEOMIND_API_KEY`. Mirrors `gh auth login`.
    ///
    /// Example: `neomind login`
    /// Example: `neomind login --data-dir /var/lib/neomind --force`
    Login {
        /// Server data directory (auto-detected if omitted).
        #[arg(long)]
        data_dir: Option<String>,
        /// Re-fetch and overwrite the credential even if already logged in.
        #[arg(long)]
        force: bool,
    },
    /// Remove the locally saved API key credential.
    ///
    /// After logout the CLI falls back to NEOMIND_API_KEY env var or fails auth.
    ///
    /// Example: `neomind logout`
    Logout,
    /// Show the current API key and validate it against the server.
    ///
    /// Example: `neomind whoami`
    Whoami,
    /// User account management (local-only, offline).
    User {
        #[command(subcommand)]
        user_cmd: UserCommand,
    },
}

/// User account subcommands.
#[derive(Subcommand, Debug)]
pub enum UserCommand {
    /// Reset a user's password (offline).
    ///
    /// Rewrites the stored bcrypt hash without verifying the old password.
    /// Requires shell/filesystem access to the server data directory.
    ///
    /// Example: `neomind user reset-password admin --data-dir /var/lib/neomind`
    ResetPassword {
        /// Username whose password to reset.
        username: String,
        /// Server data directory (auto-detected if omitted).
        #[arg(long)]
        data_dir: Option<String>,
    },
}

/// API key subcommands.
#[derive(Subcommand, Debug)]
pub enum ApiKeyCommand {
    /// Create a new API key.
    ///
    /// Generates an API key for authenticating external requests.
    /// Example: `neomind api-key create --name my-app`
    Create {
        /// Name for the key.
        #[arg(short, long, default_value = "default")]
        name: String,
        /// Data directory path.
        #[arg(long, default_value = "data")]
        data_dir: String,
    },
    /// List all API keys.
    ///
    /// Shows all registered API key names (values are masked).
    /// Example: `neomind api-key list`
    List {
        /// Data directory path.
        #[arg(long, default_value = "data")]
        data_dir: String,
    },
    /// Delete an API key by name.
    ///
    /// Removes the key immediately; all requests using it will be rejected.
    /// Example: `neomind api-key delete my-app`
    Delete {
        /// Key name to delete.
        name: String,
        /// Data directory path.
        #[arg(long, default_value = "data")]
        data_dir: String,
    },
}

/// LLM backend subcommands.
#[derive(Subcommand, Debug)]
pub enum LlmCommand {
    /// List configured LLM backends.
    ///
    /// Shows all registered LLM backend instances with their ID, type, and model.
    /// Use the ID as --llm-backend value in agent create/update.
    ///
    /// Example: `neomind llm list`
    List {},
    /// Get LLM backend details.
    ///
    /// Shows full backend config including endpoint, model, and parameters.
    ///
    /// Example: `neomind llm get ollama-default`
    Get {
        /// Backend ID.
        #[arg(required = true)]
        id: String,
    },
    /// List available models from Ollama.
    ///
    /// Queries the Ollama server for models that can be pulled and used.
    /// Use model names when configuring LLM backends.
    ///
    /// Example: `neomind llm models`
    Models {
        /// Ollama endpoint.
        #[arg(long, default_value = "http://localhost:11434")]
        endpoint: String,
    },
    /// Create a new LLM backend.
    ///
    /// Registers a new LLM backend instance for use by agents.
    /// Backend types: ollama, llamacpp (local runners) and openai, anthropic
    /// (the two cloud protocols). Any cloud vendor (Qwen/DeepSeek/GLM/xAI...)
    /// or OpenAI-compatible server (vLLM, OpenRouter, LM Studio) rides
    /// --type openai with its own --endpoint. Legacy vendor type values
    /// (qwen/deepseek/glm/xai/google/minimax) still parse for back-compat.
    ///
    /// Workflow:
    ///   1. `llm list` — see existing backends
    ///   2. `llm models` — find available model names (Ollama)
    ///   3. `llm create --name local --type ollama --endpoint http://localhost:11434 --model qwen3.5:4b`
    ///   4. `llm test <ID>` — verify connection
    ///   5. `llm activate <ID>` — set as default
    ///
    /// Example: `neomind llm create --name my-llm --type openai --endpoint https://api.openai.com/v1 --model gpt-4.1-mini --api-key sk-xxx`
    Create {
        /// Backend display name.
        #[arg(short, long)]
        name: String,
        /// Backend type: ollama | llamacpp | openai | anthropic (legacy vendor
        /// values qwen/deepseek/glm/xai/google/minimax accepted for back-compat).
        #[arg(short, long)]
        r#type: String,
        /// API endpoint URL.
        ///   Ollama: http://localhost:11434
        ///   llama.cpp: http://127.0.0.1:8080 (no /v1 — the client appends its own path)
        ///   OpenAI: https://api.openai.com/v1 (with /v1)
        ///   Anthropic: https://api.anthropic.com (/v1 auto-appended when missing)
        #[arg(short, long)]
        endpoint: String,
        /// Model name.
        ///   Ollama: qwen3.5:4b, llama3:8b, etc.
        ///   OpenAI: gpt-4.1-mini, gpt-4.1, etc.
        #[arg(short, long)]
        model: String,
        /// API key (required for cloud providers, not for ollama/llamacpp).
        #[arg(short, long)]
        api_key: Option<String>,
        /// Temperature (0.0 - 2.0). Default: 0.7.
        #[arg(long)]
        temperature: Option<f64>,
    },
    /// Update LLM backend configuration.
    ///
    /// Modify endpoint, model, or other settings. Changes apply immediately.
    ///
    /// Example: `neomind llm update my-llm --model qwen3.5:8b`
    Update {
        /// Backend ID.
        #[arg(required = true)]
        id: String,
        /// New display name.
        #[arg(short, long)]
        name: Option<String>,
        /// New model name.
        #[arg(short, long)]
        model: Option<String>,
        /// New endpoint URL.
        #[arg(short, long)]
        endpoint: Option<String>,
        /// New API key.
        #[arg(short, long)]
        api_key: Option<String>,
        /// New temperature.
        #[arg(long)]
        temperature: Option<f64>,
    },
    /// Delete an LLM backend.
    ///
    /// Removes the backend. Agents using this backend will fail on next execution.
    /// Check agent usage first: `agent list` and look for llm_backend_id.
    ///
    /// Example: `neomind llm delete my-llm`
    Delete {
        /// Backend ID.
        #[arg(required = true)]
        id: String,
    },
    /// Activate an LLM backend (set as default).
    ///
    /// Sets the backend as the system default. New agents will use this backend
    /// unless overridden with --llm-backend.
    ///
    /// Example: `neomind llm activate my-llm`
    Activate {
        /// Backend ID.
        #[arg(required = true)]
        id: String,
    },
    /// Test LLM backend connection.
    ///
    /// Sends a test request to verify the backend is reachable and the model is available.
    /// Run after create or update to verify settings.
    ///
    /// Example: `neomind llm test my-llm`
    Test {
        /// Backend ID.
        #[arg(required = true)]
        id: String,
    },
}

/// Extension subcommands.
#[derive(Subcommand, Debug)]
pub enum ExtensionCommand {
    /// Validate a .nep extension package.
    ///
    /// Checks manifest, binary compatibility, and schema before installing.
    /// Always validate before `extension install` to catch issues early.
    /// Use -v for detailed output including all metrics and commands.
    /// Example: `neomind extension validate ./my-extension.nep -v`
    Validate {
        /// Path to the .nep file.
        #[arg(required = true)]
        path: std::path::PathBuf,
        /// Show detailed output.
        #[arg(short, long)]
        verbose: bool,
    },
    /// List installed extensions.
    ///
    /// Shows all installed extensions with their ID, version, and status.
    /// Use -v for detailed metrics and commands info.
    /// Example: `neomind extension list -v`
    List {
        /// Show detailed information.
        #[arg(short, long)]
        verbose: bool,
    },
    /// Show extension information.
    ///
    /// Displays manifest details: version, description, metrics, commands.
    /// `info` is a visible alias for backward compatibility.
    /// Example: `neomind extension get weather-forecast`
    #[command(visible_alias = "info")]
    Get {
        /// Extension ID or .nep file path.
        #[arg(required = true)]
        id_or_path: String,
    },
    /// Install a .nep extension package.
    ///
    /// Installs from a local file path. The extension is loaded immediately.
    /// Example: `neomind extension install ./weather-forecast-v2.nep`
    Install {
        /// Path to the .nep file or URL.
        #[arg(required = true)]
        package: String,
    },
    /// Uninstall an extension.
    ///
    /// Stops the extension process and removes all files. This is irreversible.
    /// Example: `neomind extension uninstall weather-forecast`
    Uninstall {
        /// Extension ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new extension scaffold.
    ///
    /// Generates a complete extension project with Cargo.toml, lib.rs, and manifest.
    /// Example: `neomind extension create my-extension --extension-type tool -o ./extensions`
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
    /// Get extension health status.
    ///
    /// Shows process status, uptime, last error, and resource usage.
    /// Example: `neomind extension status weather-forecast`
    Status {
        /// Extension ID.
        #[arg(required = true)]
        id: String,
    },
    /// Get extension logs.
    ///
    /// Retrieves recent log output from the extension's isolated process.
    /// Example: `neomind extension logs weather-forecast --lines 50`
    Logs {
        /// Extension ID.
        #[arg(required = true)]
        id: String,
        /// Number of log lines to show.
        #[arg(short, long)]
        lines: Option<usize>,
    },
    /// Build an extension from source.
    ///
    /// Compiles the extension in release mode and produces a .nep package.
    ///
    /// Workflow:
    ///   1. `extension create my-extension` — scaffold
    ///   2. Edit code in the generated project
    ///   3. `extension build ./my-extension` — compile
    ///   4. `extension validate ./my-extension.nep` — check
    ///   5. `extension install ./my-extension.nep` — deploy
    ///
    ///   Example: `neomind extension build ./extensions/weather-forecast`
    Build {
        /// Extension directory path.
        #[arg(required = true)]
        path: std::path::PathBuf,
    },
    /// Install from marketplace.
    ///
    /// Downloads and installs an extension from the marketplace registry.
    /// Example: `neomind extension market-install weather-forecast --version 2.0.0`
    MarketInstall {
        /// Extension ID in marketplace.
        #[arg(required = true)]
        extension_id: String,
        /// Version to install (optional, defaults to latest).
        #[arg(long)]
        version: Option<String>,
    },
    /// List marketplace extensions.
    ///
    /// Shows all extensions available in the marketplace registry.
    /// Example: `neomind extension market-list`
    MarketList,
    /// Reload an extension.
    ///
    /// Restarts the extension process from its installed files.
    /// Useful after config changes or when the extension is unresponsive.
    /// Example: `neomind extension reload weather-forecast`
    Reload {
        /// Extension ID.
        #[arg(required = true)]
        id: String,
    },
    /// Get or update extension configuration.
    ///
    /// Without --set: shows current config.
    /// With --set: updates config (JSON format).
    /// Example: `neomind extension config weather-forecast`
    /// Example: `neomind extension config weather-forecast --set '{"city":"Beijing"}'`
    Config {
        /// Extension ID.
        #[arg(required = true)]
        id: String,
        /// Set config value (JSON). Omit to view current config.
        #[arg(long)]
        set: Option<String>,
    },
}

/// Device subcommands.
#[derive(Subcommand, Debug)]
pub enum DeviceCommand {
    /// List all devices, grouped by type.
    ///
    /// Returns devices grouped by device type with metric field names and
    /// an example device's current values per type. No need to call
    /// `device get <ID>` for deep inspection.
    /// Use --device-type or --status to filter results.
    ///
    /// Workflow: Use this for discovery — find device IDs, metric names,
    /// and current values in one command. Then use `device get <ID>` for
    /// deep inspection of a specific device.
    ///
    /// Example: `neomind device list --device-type temp_sensor --status online`
    List {
        /// Filter by device type.
        #[arg(short, long)]
        device_type: Option<String>,
        /// Filter by status.
        #[arg(short, long)]
        status: Option<String>,
    },
    /// Get device details (metadata + metrics + commands).
    ///
    /// Returns full device info: metadata, connection config, current metric
    /// values, and available commands. Pass `--metric` to return only that one
    /// metric's current value — avoids pulling all metrics (e.g. AI-camera
    /// inference fields) when you want a single reading like battery level.
    ///
    /// Workflow: Use `device list` for overview, then `device get <ID>` for detail.
    ///
    /// Example: `neomind device get device-001`
    /// Example: `neomind device get device-001 --metric values.battery`
    Get {
        /// Device ID.
        #[arg(required = true)]
        id: String,
        /// Return only this metric's current value (e.g. `values.battery`).
        /// Omit to get all metrics.
        #[arg(long)]
        metric: Option<String>,
    },
    /// Create a new device.
    ///
    /// First run `neomind device types list` to see available device types,
    /// or use any custom name. Default adapter is "mqtt".
    ///
    /// Workflow:
    ///   1. `neomind device types list` — pick a device type
    ///   2. `neomind device create --name "My Sensor" --device-type temp_sensor --adapter-type mqtt --id my-sensor-01`
    ///   3. Send data via MQTT to the configured topic, or use `device write-metric`
    ///   4. Verify with `device get <ID>`
    Create {
        /// Device name.
        #[arg(short, long)]
        name: String,
        /// Device type name. Run `neomind device types list` to see built-in types,
        /// or use any custom name (e.g., "sensor", "camera", "switch").
        #[arg(short, long)]
        device_type: String,
        /// Adapter type: mqtt (default) | webhook.
        #[arg(short, long, default_value = "mqtt")]
        adapter_type: String,
        /// Optional explicit device ID. Auto-generated if omitted. Use this when
        /// the user provides a specific identifier (e.g. `cam-office`) — never
        /// silently swap it for an auto-generated one.
        #[arg(long = "id", alias = "device-id")]
        device_id: Option<String>,
        /// Connection config JSON. For MQTT: '{"topic":"sensor/data"}'.
        /// For webhook: omit (auto-generated URL).
        #[arg(short, long)]
        config: Option<String>,
    },
    /// Update device.
    ///
    /// Modify device name or connection config.
    ///
    /// Workflow: Find ID with `device list`, update fields as needed.
    /// To change MQTT topic: `device update <ID> --config '{"topic":"new/topic"}'`.
    ///
    /// Example: `neomind device update device-001 --name "Kitchen Sensor"`
    Update {
        /// Device ID.
        #[arg(required = true)]
        id: String,
        /// New name.
        #[arg(short, long)]
        name: Option<String>,
        /// Connection config JSON.
        #[arg(short, long)]
        config: Option<String>,
    },
    /// Delete device.
    ///
    /// Removes the device and ALL associated telemetry data. This is irreversible.
    /// Consider exporting data first with `device history` if needed.
    ///
    /// Example: `neomind device delete device-001`
    Delete {
        /// Device ID.
        #[arg(required = true)]
        id: String,
    },
    /// (Alias for device get) Get device details and current metrics.
    ///
    /// Identical to `device get <ID>`. Use `device get` instead.
    /// Kept for backward compatibility.
    #[command(hide = true)]
    Latest {
        /// Device ID.
        #[arg(required = true)]
        id: String,
    },
    /// Get telemetry history.
    ///
    /// Retrieves time-series metric data. Use --time-range to specify period
    /// (1h, 24h, 7d, 30d). Use --compress for AI-friendly compressed series
    /// that retains trends and allows up to 90 days of data.
    ///
    /// Workflow:
    ///   1. `device history <ID>` — all metrics, last 24h
    ///   2. `device history <ID> --metric temperature --time-range 7d` — specific metric
    ///   3. `device history <ID> --compress` — compact for AI consumption
    ///
    /// Example: `neomind device history device-001 --metric temperature --time-range 24h`
    History {
        /// Device ID.
        #[arg(required = true)]
        id: String,
        /// Metric name to filter (optional, returns all metrics if omitted).
        #[arg(long)]
        metric: Option<String>,
        /// Time range: "1h", "24h", "7d", "30d" (default: 24h).
        #[arg(short, long)]
        time_range: Option<String>,
        /// AI compression mode: lossless adaptive series (kept/fluctuated).
        /// Allows up to 90 days. Designed for AI consumption, not frontend charts.
        /// Use --compress=true to enable or --compress=false to disable.
        #[arg(long)]
        compress: Option<bool>,
        /// Max data points per metric (API range 1-5000, default 100). Caps
        /// payload size when you only need recent points.
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Send control command.
    ///
    /// Sends a command to a device (e.g., toggle switch, set speed, reboot).
    /// Check available commands first: `neomind device get <ID>` shows supported commands.
    ///
    /// Workflow:
    ///   1. `device get <ID>` — check available commands
    ///   2. `device control <ID> <command> --params '<json>'`
    ///
    /// Example: `neomind device control <ID> toggle --params '{"state":true}'`
    #[command(alias = "command")]
    Control {
        /// Device ID.
        #[arg(required = true)]
        id: String,
        /// Command name (e.g., "toggle", "set_speed", "reboot").
        #[arg(required = true)]
        command: String,
        /// Command parameters JSON. Example: '{"state":true}'
        #[arg(short, long)]
        params: Option<String>,
    },
    /// Device type management.
    Types {
        #[command(subcommand)]
        type_cmd: DeviceTypeCommand,
    },
    /// Write a metric data point.
    ///
    /// Manually push a metric value to a device. Useful for testing or
    /// feeding data from custom sources. The metric name must match the
    /// device type definition.
    ///
    /// Workflow:
    ///   1. `device types get <type>` — check valid metric names
    ///   2. `device write-metric <ID> --metric temp --value 23.5`
    ///   3. `device get <ID>` — verify the value was recorded
    ///
    /// Example: `neomind device write-metric <ID> --metric temperature --value 25.5`
    WriteMetric {
        /// Device ID.
        #[arg(required = true)]
        id: String,
        /// Metric name (must match device type definition).
        #[arg(long)]
        metric: String,
        /// Value (number, string, or "true"/"false").
        #[arg(long)]
        value: String,
        /// Timestamp in milliseconds (defaults to now).
        #[arg(long)]
        timestamp: Option<i64>,
    },
    /// Get webhook URL for a device.
    ///
    /// Returns the full URL for pushing data to this device via HTTP POST.
    /// Only available for webhook adapter devices.
    /// Example: `neomind device webhook-url <ID>`
    WebhookUrl {
        /// Device ID.
        #[arg(required = true)]
        id: String,
    },
    /// Manage device auto-discovery drafts.
    ///
    /// When unknown devices send data via MQTT/Webhook, they appear as drafts
    /// awaiting approval. Use these commands to review, approve, or reject them.
    ///
    /// Workflow:
    ///   1. `device drafts list` — see pending devices
    ///   2. `device drafts get <ID>` — inspect sample data
    ///   3. `device drafts approve <ID> --name "My Device" --type temp_sensor` — register
    ///   4. Or `device drafts reject <ID>` — discard
    ///
    /// Example: `neomind device drafts list`
    Drafts {
        #[command(subcommand)]
        draft_cmd: DraftCommand,
    },
}

/// Device draft subcommands.
#[derive(Subcommand, Debug)]
pub enum DraftCommand {
    /// List pending device drafts.
    ///
    /// Shows all unapproved devices that have sent data but aren't registered yet.
    /// Example: `neomind device drafts list`
    List {},
    /// Get draft details including sample data.
    ///
    /// Shows the device's auto-detected metrics and recent data samples.
    /// Example: `neomind device drafts get <DEVICE_ID>`
    Get {
        /// Device ID of the draft.
        #[arg(required = true)]
        id: String,
    },
    /// Approve a device draft and register it.
    ///
    /// Converts the draft into a registered device. You can override the name and type.
    /// After approval, the device starts receiving data normally.
    /// Example: `neomind device drafts approve <ID> --name "Temperature Sensor" --type temp_sensor`
    Approve {
        /// Device ID of the draft.
        #[arg(required = true)]
        id: String,
        /// Device display name.
        #[arg(long)]
        name: Option<String>,
        /// Device type to assign.
        #[arg(long)]
        r#type: Option<String>,
    },
    /// Reject and discard a device draft.
    ///
    /// Removes the draft and its sample data. The device can re-send data
    /// to create a new draft if auto-discovery is still enabled.
    /// Example: `neomind device drafts reject <ID>`
    Reject {
        /// Device ID of the draft.
        #[arg(required = true)]
        id: String,
    },
    /// View or configure auto-discovery settings.
    ///
    /// Without flags: shows current config.
    /// With flags: updates config values.
    ///
    /// Settings:
    ///   --enabled true/false     Enable/disable auto-discovery
    ///   --auto-approve true/false  Auto-approve new drafts
    ///   --max-samples <N>        Max samples to keep per draft
    ///
    /// Example: `neomind device drafts config --enabled true --auto-approve false`
    Config {
        /// Enable or disable auto-discovery.
        #[arg(long)]
        enabled: Option<bool>,
        /// Auto-approve new drafts without manual review.
        #[arg(long)]
        auto_approve: Option<bool>,
        /// Maximum data samples to keep per draft.
        #[arg(long)]
        max_samples: Option<u32>,
    },
}

/// Device type subcommands.
#[derive(Subcommand, Debug)]
pub enum DeviceTypeCommand {
    /// List all device types.
    ///
    /// Shows all registered device types including built-in and custom ones.
    /// Use this before `device create` to pick the right --device-type value.
    ///
    /// Example: `neomind device types list`
    List,
    /// Get device type details.
    ///
    /// Shows metrics, commands, and sample data for a device type.
    /// Use this to discover valid metric names for `device write-metric`
    /// or available commands for `device control`.
    ///
    /// Example: `neomind device types get temp_sensor`
    Get {
        /// Type ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new device type.
    ///
    /// Example:
    ///   neomind device types create --name "TempSensor" --metrics '[{"name":"temp","display_name":"Temperature","data_type":"Float","unit":"°C"}]'
    ///   neomind device types create --id temp_humidity --name "TempHumidity Sensor" --metrics '[{"name":"temp","display_name":"Temperature","data_type":"Float","unit":"°C"},{"name":"rh","display_name":"Humidity","data_type":"String","unit":"%"}]'
    Create {
        /// Unique type ID (auto-generated from name if omitted, e.g., "TempSensor" -> "temp_sensor").
        #[arg(short, long)]
        id: Option<String>,
        /// Type display name (e.g., "Temperature Sensor").
        #[arg(short, long)]
        name: String,
        /// Metrics JSON array. Each: {"name":"temp","display_name":"Temperature","data_type":"Float","unit":"°C"}
        #[arg(long)]
        metrics: String,
        /// Commands JSON array (optional). Each: {"id":"on","name":"Turn On","params":[]}
        #[arg(long)]
        commands: Option<String>,
    },
    /// Delete device type.
    ///
    /// Removes the type template. Devices already using this type will continue
    /// to work, but new devices cannot be created with this type.
    /// Check usage first: `neomind device list --device-type <type>`.
    ///
    /// Example: `neomind device types delete temp_sensor`
    Delete {
        /// Type ID.
        #[arg(required = true)]
        id: String,
    },
}

/// Dashboard subcommands.
#[derive(Subcommand, Debug)]
pub enum DashboardCommand {
    /// List all dashboards.
    ///
    /// Shows dashboard ID, name, description, and component count.
    /// Set `NEOMIND_JSON=1` for structured output.
    ///
    /// Workflow: Use this to find dashboard IDs for get/update/delete/share commands.
    ///
    /// Example: `neomind dashboard list`
    List {},
    /// Get dashboard details.
    ///
    /// Shows full dashboard config including layout and all widget components.
    /// Use `NEOMIND_JSON=1` to get the exact format needed for `dashboard update --components`.
    ///
    /// Example: `neomind dashboard get dash-001`
    Get {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new dashboard.
    ///
    /// Creates a dashboard (optionally with components in one shot via
    /// --components). To add widgets to an EXISTING dashboard, use
    /// `dashboard add-components` (append mode).
    ///
    /// Workflow:
    ///   1. `neomind dashboard create --name "My Dashboard"` (optionally `--components '[...]'`)
    ///   2. `neomind widget list` — see available widget types
    ///   3. `neomind dashboard add-components <ID> --components '[...]'` — add widgets later
    Create {
        /// Dashboard name.
        #[arg(short, long)]
        name: String,
        /// Dashboard description.
        #[arg(short, long)]
        description: Option<String>,
        /// Layout configuration JSON (optional, auto-generated if omitted).
        #[arg(short, long)]
        layout: Option<String>,
        /// Components JSON array (optional) — create the dashboard AND its
        /// widgets in one shot. Same component shape as `add-components`.
        #[arg(short, long)]
        components: Option<String>,
    },
    /// Update dashboard.
    ///
    /// Modify name, description, or layout. For changing ONE component's
    /// fields, prefer `dashboard update-component`.
    /// --components REPLACES ALL components and requires the explicit
    /// `--replace-all` confirmation flag (without it the command fails with
    /// guidance — most "update component" intents want `add-components` or
    /// `update-component` instead).
    /// Workflow: `dashboard get <ID>` → edit JSON → `dashboard update <ID> --replace-all --components '...'`
    Update {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
        /// New name.
        #[arg(short, long)]
        name: Option<String>,
        /// New description.
        #[arg(short, long)]
        description: Option<String>,
        /// New layout configuration JSON.
        #[arg(short, long)]
        layout: Option<String>,
        /// Dashboard components JSON array. WARNING: Replaces ALL existing components.
        /// Get current with `dashboard get <ID>`, modify the array, then pass back.
        /// Each component: {"type":"widget-type","data_source":{"..."},"display":{...},"config":{...}}
        #[arg(short, long)]
        components: Option<String>,
        /// REQUIRED together with --components: confirms you want to replace
        /// the entire component array (destructive).
        #[arg(long)]
        replace_all: bool,
    },
    /// Update ONE component of a dashboard (PREFERRED for tweaks).
    ///
    /// Deep-merges a partial JSON object into the existing component —
    /// objects merge recursively, scalars/arrays replace. Only the fields
    /// you pass change; id and type are immutable.
    /// NO need to re-send the full component or remove+re-add.
    ///
    /// Examples:
    ///   change a binding field:
    ///   `dashboard update-component <ID> --component-id c3 --set '{"data_source":{"timeWindow":{"type":"last_6hours"}}}'`
    ///   retitle + resize:
    ///   `dashboard update-component <ID> --component-id c3 --set '{"title":"温度","position":{"w":6}}'`
    UpdateComponent {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
        /// Component ID (from `dashboard get <ID>`).
        #[arg(short, long)]
        component_id: String,
        /// Partial component JSON, deep-merged into the component.
        #[arg(short, long)]
        set: String,
    },
    /// Add components to dashboard (append mode).
    ///
    /// Appends new widgets without replacing existing ones.
    /// This is the RECOMMENDED way to add widgets.
    ///
    /// Example: `neomind dashboard add-components <ID> --components '[{"id":"c1","type":"value-card",...}]'`
    AddComponents {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
        /// JSON array of new components to append.
        #[arg(short, long)]
        components: String,
    },
    /// Remove components from dashboard by ID.
    ///
    /// Removes specific widgets by their component IDs.
    ///
    /// Example: `neomind dashboard remove-components <ID> --ids '["c1","c2"]'`
    RemoveComponents {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
        /// JSON array of component IDs to remove.
        #[arg(short, long)]
        ids: String,
    },
    /// Delete dashboard.
    ///
    /// Removes the dashboard and all its widget configurations. This is irreversible.
    /// Shared links will stop working immediately.
    ///
    /// Example: `neomind dashboard delete dash-001`
    Delete {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
    },
    /// Share dashboard.
    ///
    /// Generates a shareable link for the dashboard. Use --public=true for open access
    /// or --expires to set a time-limited link. Defaults to private (--public=false).
    /// Example: `neomind dashboard share dash-001 --public=true --expires "2025-12-31"`
    Share {
        /// Dashboard ID.
        #[arg(required = true)]
        id: String,
        /// Make public (use --public=true or --public=false). Defaults to false.
        #[arg(short, long)]
        public: Option<bool>,
        /// Expiration date/time.
        #[arg(short, long)]
        expires: Option<String>,
    },
}

/// Rule subcommands.
#[derive(Subcommand, Debug)]
pub enum RuleCommand {
    /// List all rules.
    ///
    /// Shows rule ID, name, status (enabled/disabled), and trigger count.
    /// Use this to find rule IDs for get/update/enable/disable commands.
    ///
    /// Example: `neomind rule list`
    List,
    /// Get rule details.
    ///
    /// Shows the full rule definition, condition, actions, and execution stats.
    /// Use this to inspect a rule before modifying or testing it.
    ///
    /// Example: `neomind rule get rule-001`
    Get {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new rule.
    ///
    /// Uses JSON format for rule definition. Must include name, condition, and actions.
    /// Example: `neomind rule create --body '{"name":"Alert","condition":{...},"actions":[...]}'`
    Create {
        /// Rule definition as JSON string.
        /// Required fields: name, condition (optional for schedule/manual), actions.
        /// Conditions: {"condition_type":"comparison","source":"device:sensor1:temp","operator":"greater_than","threshold":30}
        /// Actions: [{"type":"notify","message":"Too hot","severity":"critical"}]
        #[arg(short, long)]
        body: String,
    },
    /// Update rule.
    ///
    /// Modify rule using JSON format. Only included fields are updated.
    /// Test first with `rule test <ID> --input '...'` to verify new conditions.
    ///
    /// Example: `neomind rule update rule-001 --body '{"name":"New Name"}'`
    Update {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
        /// Updated rule definition as JSON string.
        #[arg(short, long)]
        body: String,
    },
    /// Delete rule.
    ///
    /// Removes the rule permanently. This is irreversible.
    /// Consider disabling first with `rule disable <ID>` if unsure.
    ///
    /// Example: `neomind rule delete rule-001`
    Delete {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
    },
    /// Enable rule.
    ///
    /// Activates a disabled rule so it starts evaluating conditions again.
    /// After enabling, verify with `rule list` to confirm status change.
    ///
    /// Example: `neomind rule enable rule-001`
    Enable {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
    },
    /// Disable rule.
    ///
    /// Pauses rule evaluation without deleting it. Can be re-enabled later.
    /// Prefer this over deleting if you want to temporarily stop a rule.
    ///
    /// Example: `neomind rule disable rule-001`
    Disable {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
    },
    /// Test rule.
    ///
    /// Evaluates a rule against sample input data without triggering actions.
    /// Input must be a JSON object with metric values matching device field names.
    /// Use before enabling a new rule to verify it behaves correctly.
    ///
    /// Workflow:
    ///   1. `rule test <ID> --input '{"temperature": 32}'` — should trigger
    ///   2. `rule test <ID> --input '{"temperature": 20}'` — should not trigger
    ///   3. `rule enable <ID>` — activate if tests pass
    ///
    /// Example: `neomind rule test rule-001 --input '{"temperature": 32}'`
    Test {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
        /// Input data JSON.
        #[arg(short, long)]
        input: String,
    },
    /// Get rule execution history.
    ///
    /// Shows recent rule evaluations with timestamps, input data, and results.
    /// Useful for debugging why a rule did or did not trigger.
    ///
    /// Example: `neomind rule history rule-001`
    History {
        /// Rule ID.
        #[arg(required = true)]
        id: String,
    },
}

/// Transform subcommands.
#[derive(Subcommand, Debug)]
pub enum TransformCommand {
    /// List all transforms.
    ///
    /// Shows transform ID, name, scope, and enabled status.
    /// Use this to find transform IDs for get/update/delete commands.
    ///
    /// Example: `neomind transform list`
    List,
    /// Get transform details.
    ///
    /// Shows the full JavaScript code, input/output mapping, and execution stats.
    /// Use this to inspect code before modifying with `transform update`.
    ///
    /// Example: `neomind transform get transform-001`
    Get {
        /// Transform ID.
        #[arg(required = true)]
        id: String,
    },
    /// Recent execution records of a transform.
    ///
    /// Every run (triggered by incoming device data) is recorded with
    /// status/error/output summary — use this to debug a transform that
    /// produces no metrics or fails. Records are kept for 30 days.
    ///
    /// Example: `neomind transform executions transform-001 --limit 20`
    Executions {
        /// Transform ID.
        #[arg(required = true)]
        id: String,
        /// Max records to show (default 10).
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Create a new transform.
    ///
    /// Code receives `input` object with metric data, must return transformed value.
    /// Example: --code 'return input * 1.8 + 32' (Celsius to Fahrenheit).
    Create {
        /// Transform name.
        #[arg(short, long)]
        name: String,
        /// Scope: "global" (all devices) or "device_type:TypeName" or "device:DeviceId".
        /// Most transforms use "global".
        #[arg(short, long)]
        scope: String,
        /// JavaScript transform code. Use `input` to access the value.
        /// Example: 'return input * 1.8 + 32'
        #[arg(short, long)]
        code: String,
        /// Output prefix for virtual metrics.
        #[arg(short, long)]
        output_prefix: Option<String>,
        /// Description.
        #[arg(short, long)]
        description: Option<String>,
        /// Whether to enable immediately.
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Update transform.
    ///
    /// Modify transform code, scope, or enabled status. Changes take effect immediately.
    /// Test new code first with `transform test-code` before applying.
    ///
    /// Example: `neomind transform update transform-001 --code 'return input * 2' --enabled true`
    Update {
        /// Transform ID.
        #[arg(required = true)]
        id: String,
        /// New name.
        #[arg(short, long)]
        name: Option<String>,
        /// New description.
        #[arg(short, long)]
        description: Option<String>,
        /// New JavaScript code.
        #[arg(short, long)]
        code: Option<String>,
        /// New scope.
        #[arg(short, long)]
        scope: Option<String>,
        /// New output prefix.
        #[arg(short, long)]
        output_prefix: Option<String>,
        /// Enable/disable.
        #[arg(long)]
        enabled: Option<bool>,
    },
    /// Enable a transform.
    ///
    /// Reactivates a paused transform. Virtual metrics resume immediately.
    /// Equivalent to `transform update <ID> --enabled true`.
    ///
    /// Example: `neomind transform enable transform-001`
    Enable {
        /// Transform ID.
        #[arg(required = true)]
        id: String,
    },
    /// Disable a transform.
    ///
    /// Pauses transformation without deleting it. Virtual metrics stop updating.
    /// Prefer this over deleting for temporary pauses.
    ///
    /// Example: `neomind transform disable transform-001`
    Disable {
        /// Transform ID.
        #[arg(required = true)]
        id: String,
    },
    /// Delete transform.
    ///
    /// Removes the transform and its virtual metrics permanently. This is irreversible.
    /// Dashboards using the virtual metrics will show errors.
    ///
    /// Example: `neomind transform delete transform-001`
    Delete {
        /// Transform ID.
        #[arg(required = true)]
        id: String,
    },
    /// List virtual metrics from transforms.
    ///
    /// Shows all metrics produced by transforms with their data source mappings.
    /// Use this to discover available data source IDs for dashboards or rules.
    ///
    /// Example: `neomind transform metrics`
    Metrics,
    /// Test transform code.
    ///
    /// Evaluates JavaScript code against sample input without creating a transform.
    /// Always test before creating: write code, test, then `transform create`.
    ///
    /// Workflow:
    ///   1. `transform test-code --code 'return input * 1.8 + 32' --input '{"value": 100}'`
    ///   2. Verify output is correct
    ///   3. `transform create --name "Celsius to Fahrenheit" --scope global --code 'return input * 1.8 + 32'`
    ///
    /// Example: `neomind transform test-code --code 'return input * 1.8 + 32' --input '{"value": 100}'`
    TestCode {
        /// Transform code (JavaScript).
        #[arg(short, long)]
        code: String,
        /// Input data JSON.
        #[arg(short, long)]
        input: String,
    },
    /// List transform data sources.
    ///
    /// Shows available data source IDs and their types (device, extension, etc.).
    /// Example: `neomind transform data-sources`
    DataSources,
}

/// Agent subcommands.
#[derive(Subcommand, Debug)]
pub enum AgentCommand {
    /// List all agents.
    ///
    /// Shows agent ID, name, status, schedule type, and last execution time.
    /// Use this to find agent IDs for all other agent commands.
    ///
    /// Example: `neomind agent list`
    List,
    /// Get agent details.
    ///
    /// Shows full agent config: prompt, schedule, LLM backend, and resources.
    /// Use this to inspect an agent before updating or debugging.
    ///
    /// Example: `neomind agent get agent-001`
    Get {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new agent.
    ///
    /// Creates an agent in paused state. You MUST activate it with
    /// `agent control <ID> active` after creation.
    ///
    /// Schedule is required. Use --schedule-type and --schedule-config together:
    ///   - interval: `--schedule-type interval --schedule-config "300"` (every 5 min)
    ///   - cron:     `--schedule-type cron --schedule-config "0 8 * * *"` (daily 8am)
    ///   - event:    `--schedule-type event` (triggered by device data)
    ///
    /// Workflow:
    ///   1. `agent create --name "Monitor" --prompt "Check sensors" --schedule-type interval --schedule-config "300"`
    ///   2. `agent control <ID> active` — start the agent
    ///   3. `agent latest-execution <ID>` — check results
    Create {
        /// Agent name (1-100 chars).
        #[arg(short, long)]
        name: String,
        /// What the agent should do (natural language, 1-10000 chars). Required.
        #[arg(short, long)]
        prompt: String,
        /// Description (0-500 chars).
        #[arg(short, long)]
        description: Option<String>,
        /// Schedule type: interval | cron | event. Required.
        ///   - interval: runs every N seconds (--schedule-config "300" = every 5min)
        ///   - cron:     runs on schedule (--schedule-config "0 8 * * *" = daily 8am)
        ///   - event:    runs when device data arrives (--event-filter optional)
        #[arg(long)]
        schedule_type: Option<String>,
        /// Schedule config: interval seconds or cron expression.
        /// Example: "300" (5min interval) or "0 8 * * *" (daily 8am).
        #[arg(long)]
        schedule_config: Option<String>,
        /// Shortcut: --every "5m" = interval/300, "1h" = interval/3600, "30s" = interval/30
        /// Overrides --schedule-type and --schedule-config.
        #[arg(long)]
        every: Option<String>,
        /// Event filter JSON for event schedule type. Format: '{"sources":[{"type":"device","id":"sensor-001"}]}'
        /// Use "all" as id to match any source of that type. Add "field" to match a specific metric.
        #[arg(long)]
        event_filter: Option<String>,
        /// Timezone for cron schedule (e.g., "Asia/Shanghai", "UTC"). Default: system timezone.
        #[arg(long)]
        timezone: Option<String>,
        /// LLM backend ID. Run `neomind system info` to see available backends.
        #[arg(short, long)]
        llm_backend: Option<String>,
        /// System prompt for the LLM (overrides default agent system prompt).
        #[arg(short, long)]
        system_prompt: Option<String>,
        /// Execution mode: "free" (multi-round tool calling) or "focused" (single-pass with bound resources).
        /// "focused" requires --resources or --device-ids. Default: "free".
        #[arg(long)]
        execution_mode: Option<String>,
        /// Device IDs to bind (comma-separated). Used in focused mode.
        /// Example: --device-ids "device-001,device-002"
        #[arg(long)]
        device_ids: Option<String>,
        /// Resources JSON array (unified format). Each: {"resource_id":"...","resource_type":"device|extension","name":"..."}
        /// Prefer this over --device-ids for new agents.
        /// Example: --resources '[{"resource_id":"device-001","resource_type":"device","name":"Temp Sensor"}]'
        #[arg(long)]
        resources: Option<String>,
        /// Metrics to bind (JSON array). Each: {"device_id":"...","metric_name":"...","display_name":"..."}
        /// Example: --metrics '[{"device_id":"sensor-001","metric_name":"temperature","display_name":"Temperature"}]'
        #[arg(long)]
        metrics: Option<String>,
        /// Commands to bind (JSON array). Each: {"device_id":"...","command_name":"...","display_name":"...","parameters":{}}
        /// Example: --commands '[{"device_id":"switch-001","command_name":"toggle","display_name":"Toggle","parameters":{}}]'
        #[arg(long)]
        commands: Option<String>,
        /// Enable tool chaining (agent can call multiple tools in sequence). Default: false.
        #[arg(long)]
        enable_tool_chaining: Option<bool>,
        /// Maximum tool chain depth (1-20). Only used when --enable-tool-chaining is true. Default: 3.
        #[arg(long)]
        max_chain_depth: Option<usize>,
        /// Agent priority (0-255, higher = more important). Default: 128.
        #[arg(long)]
        priority: Option<u8>,
        /// Context window size (number of conversation turns to keep). Default: 10.
        #[arg(long)]
        context_window_size: Option<usize>,
    },
    /// Update agent.
    ///
    /// Modify agent configuration. Changes apply to the next scheduled execution.
    /// Check current config first with `agent get <ID>`.
    ///
    /// Example: `neomind agent update agent-001 --prompt "Monitor temperature and alert if above 30"`
    Update {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
        /// New name.
        #[arg(short, long)]
        name: Option<String>,
        /// New user prompt.
        #[arg(short, long)]
        prompt: Option<String>,
        /// New description.
        #[arg(short, long)]
        description: Option<String>,
        /// LLM backend ID.
        #[arg(short, long)]
        llm_backend: Option<String>,
        /// System prompt.
        #[arg(short, long)]
        system_prompt: Option<String>,
        /// New schedule type: interval | cron | event.
        #[arg(long)]
        schedule_type: Option<String>,
        /// New schedule config (seconds for interval, cron expression for cron).
        #[arg(long)]
        schedule_config: Option<String>,
        /// New execution mode: "free" or "focused".
        #[arg(long)]
        execution_mode: Option<String>,
        /// New device IDs (comma-separated). Replaces existing bindings.
        #[arg(long)]
        device_ids: Option<String>,
        /// New resources JSON array. Replaces existing resources.
        #[arg(long)]
        resources: Option<String>,
        /// Metrics to bind (JSON array). Each: {"device_id":"...","metric_name":"...","display_name":"..."}
        /// Example: --metrics '[{"device_id":"sensor-001","metric_name":"temperature","display_name":"Temperature"}]'
        #[arg(long)]
        metrics: Option<String>,
        /// Commands to bind (JSON array). Each: {"device_id":"...","command_name":"...","display_name":"...","parameters":{}}
        /// Example: --commands '[{"device_id":"switch-001","command_name":"toggle","display_name":"Toggle","parameters":{}}]'
        #[arg(long)]
        commands: Option<String>,
        /// Enable/disable tool chaining.
        #[arg(long)]
        enable_tool_chaining: Option<bool>,
        /// Max tool chain depth.
        #[arg(long)]
        max_chain_depth: Option<usize>,
        /// Agent priority (0-255).
        #[arg(long)]
        priority: Option<u8>,
        /// Context window size.
        #[arg(long)]
        context_window_size: Option<usize>,
    },
    /// Delete agent.
    ///
    /// Removes the agent and ALL execution history. This is irreversible.
    /// Consider pausing first with `agent control <ID> paused` if unsure.
    ///
    /// Example: `neomind agent delete agent-001`
    Delete {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
    },
    /// Control agent status.
    ///
    /// Status values: "active" (start running) or "paused" (stop).
    /// New agents are created in paused state — you MUST run this to start them.
    ///
    /// Workflow:
    ///   - Start: `agent control <ID> active`
    ///   - Stop:  `agent control <ID> paused`
    ///   - Verify: `agent list` or `agent get <ID>`
    Control {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
        /// Status (active or paused).
        #[arg(required = true)]
        status: String,
    },
    /// Invoke agent with input.
    ///
    /// Runs a one-time agent execution with custom input, bypassing the schedule.
    /// Returns the agent's response directly. Useful for testing or ad-hoc queries.
    ///
    /// Workflow: `agent invoke <ID> "your question"` → see immediate response.
    ///
    /// Example: `neomind agent invoke agent-001 "Check all sensor readings"`
    Invoke {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
        /// Input prompt.
        #[arg(required = true)]
        input: String,
    },
    /// Get agent memory.
    ///
    /// Shows the execution journal (recent outcomes) and knowledge files index.
    /// The journal tracks recent execution results; knowledge files store
    /// persistent notes the agent has created about its environment.
    ///
    /// Example: `neomind agent memory agent-001`
    Memory {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
    },
    /// Clear agent memory.
    ///
    /// Resets the execution journal and removes all knowledge files.
    /// Use this when the agent has accumulated stale or incorrect knowledge.
    ///
    /// Example: `neomind agent clear-memory agent-001`
    ClearMemory {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
    },
    /// Get agent execution history.
    ///
    /// Shows past execution records with timestamps, status, and duration.
    /// Use --limit and --offset for pagination.
    /// For just the latest result, use `agent latest-execution` instead.
    ///
    /// Example: `neomind agent executions agent-001 --limit 10`
    Executions {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
        /// Limit number of results.
        #[arg(short, long)]
        limit: Option<usize>,
        /// Offset for pagination.
        #[arg(short, long)]
        offset: Option<usize>,
    },
    /// Get latest agent execution.
    ///
    /// Shows the most recent execution result including tool calls and response.
    /// Quick way to check if the agent is working correctly.
    ///
    /// Example: `neomind agent latest-execution agent-001`
    LatestExecution {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
    },
    /// Get agent conversation (messages).
    ///
    /// Shows the full conversation thread between the agent and LLM.
    /// Use --limit to control the number of messages returned.
    /// Useful for debugging: see exactly what the agent sent and received.
    ///
    /// Example: `neomind agent conversation agent-001 --limit 20`
    Conversation {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
        /// Limit number of messages.
        #[arg(short, long)]
        limit: Option<usize>,
    },
    /// Send message to agent.
    ///
    /// Sends an inline message to an active agent. The agent processes it
    /// in its next execution cycle. Use --message-type to categorize
    /// (instruction, correction, etc.).
    ///
    /// Workflow:
    ///   - Correction: `agent send-message <ID> "The threshold should be 35 not 30" --message-type correction`
    ///   - Instruction: `agent send-message <ID> "Focus on temperature sensors only" --message-type instruction`
    ///
    /// Example: `neomind agent send-message agent-001 "Increase alert threshold to 35"`
    SendMessage {
        /// Agent ID.
        #[arg(required = true)]
        id: String,
        /// Message content.
        #[arg(required = true)]
        message: String,
        /// Message type (e.g., instruction, correction).
        #[arg(long)]
        message_type: Option<String>,
    },
}

/// Message subcommands.
#[derive(Subcommand, Debug)]
pub enum MessageCommand {
    /// List messages.
    ///
    /// Shows system notifications, alerts, and user messages.
    /// Use --severity or --status to filter. Default limit is 20.
    ///
    /// Workflow: `message list` → `message get <ID>` → `message read <ID>` (acknowledge).
    ///
    /// Example: `neomind message list --severity critical --limit 50`
    List {
        /// Limit number of results.
        #[arg(short, long)]
        limit: Option<usize>,
        /// Offset for pagination.
        #[arg(short, long)]
        offset: Option<usize>,
        /// Filter by severity.
        #[arg(long)]
        severity: Option<String>,
        /// Filter by status.
        #[arg(long)]
        status: Option<String>,
    },
    /// Get message details.
    ///
    /// Shows full message content, metadata, and acknowledgment status.
    /// Use this to read the full body of truncated messages from `message list`.
    ///
    /// Example: `neomind message get msg-001`
    Get {
        /// Message ID.
        #[arg(required = true)]
        id: String,
    },
    /// Send a new message.
    ///
    /// Creates a system message with severity level. Supports markdown content.
    /// Severity levels: info, warning, critical, emergency.
    ///
    /// Workflow: `message send --title "Alert" --body "Check sensor #3" --severity warning`
    /// Messages appear in the UI notification center and can trigger rules.
    ///
    /// Example: `neomind message send --title "Deploy Notice" --body "Version 2.0 deployed" --severity info`
    Send {
        /// Message title.
        #[arg(short, long)]
        title: String,
        /// Message content (supports markdown).
        #[arg(short, long)]
        body: String,
        /// Severity level: info | warning | critical | emergency.
        #[arg(short, long, default_value = "info")]
        severity: String,
        /// Source.
        #[arg(long)]
        source: Option<String>,
    },
    /// Acknowledge/read a message.
    ///
    /// Marks a message as read. Used to clear unread notifications.
    /// Batch acknowledge: list IDs from `message list`, then read each one.
    ///
    /// Example: `neomind message read msg-001`
    #[command(visible_alias = "ack")]
    Read {
        /// Message ID.
        #[arg(required = true)]
        id: String,
    },
    /// Delete a message by ID.
    ///
    /// Example: `neomind message delete msg-001`
    Delete {
        /// Message ID.
        #[arg(required = true)]
        id: String,
    },
    /// List message channels.
    ///
    /// Shows all notification channels (webhook, email, etc.) and their status.
    /// Example: `neomind message channel-list`
    ChannelList,
    /// Get channel details.
    ///
    /// Shows channel type, config, and delivery status.
    /// Example: `neomind message channel-get slack-alerts`
    ChannelGet {
        /// Channel name.
        #[arg(required = true)]
        name: String,
    },
    /// List available channel types.
    ///
    /// Shows channel types that can be created (webhook, email, telegram, wecom, dingtalk, slack, feishu).
    /// Example: `neomind message channel-types`
    ChannelTypes,
    /// Get config schema and examples for a channel type.
    ///
    /// Shows required/optional fields and full config JSON examples.
    /// Run before `channel-create` to know what --config needs.
    /// Example: `neomind message channel-type-schema telegram`
    ChannelTypeSchema {
        /// Channel type (webhook, email, telegram, wecom, dingtalk, slack, feishu).
        #[arg(required = true)]
        channel_type: String,
    },
    /// Create a message channel.
    ///
    /// Workflow:
    ///   1. `message channel-types` — see available types
    ///   2. `message channel-type-schema <TYPE>` — get config fields & examples
    ///   3. `message channel-create --name <N> --type <TYPE> --config '<JSON>'`
    ///   4. `message channel-test <NAME>` — verify it works
    ///
    /// Config examples by type:
    ///   webhook:  '{"url":"https://example.com/webhook","headers":{"Authorization":"Bearer TOKEN"},"timeout_secs":30}'
    ///   email:    '{"smtp_server":"smtp.example.com","smtp_port":587,"username":"user","password":"pass","from_address":"noreply@example.com","use_tls":true}'
    ///   telegram: '{"token":"123456:ABCdefGHIjklMNO","chat_id":"-1001234567890"}'
    ///   wecom:    '{"key":"xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"}'
    ///   dingtalk: '{"access_token":"xxxx","secret":"SECxxxx"}'
    ///   slack:    '{"webhook_url":"https://hooks.slack.com/services/T00/B00/xxx"}'
    ///   feishu:   '{"hook_id":"xxxxxxxx","secret":"optional_sign_secret"}'
    ///
    /// Example: `neomind message channel-create --name "alerts" --type webhook --config '{"url":"https://hooks.slack.com/..."}'`
    ChannelCreate {
        /// Channel name (unique identifier).
        #[arg(long)]
        name: String,
        /// Channel type. Run `channel-types` to see available types.
        #[arg(long, visible_alias = "type")]
        channel_type: String,
        /// Channel config as JSON. Run `channel-type-schema <TYPE>` for field details.
        #[arg(long)]
        config: String,
        /// Enable the channel on creation (default: enabled).
        #[arg(long, default_value_t = true)]
        enabled: bool,
    },
    /// Update channel configuration.
    ///
    /// Updates the channel's configuration. Does not change the channel type.
    /// Example: `neomind message channel-update --name "alerts" --config '{"url":"https://new-url.com"}'`
    ChannelUpdate {
        /// Channel name.
        #[arg(long)]
        name: String,
        /// New config (JSON string).
        #[arg(long)]
        config: String,
    },
    /// Delete a message channel.
    ///
    /// Removes the channel. Pending messages will not be delivered.
    /// Example: `neomind message channel-delete slack-alerts`
    ChannelDelete {
        /// Channel name.
        #[arg(required = true)]
        name: String,
    },
    /// Test a message channel.
    ///
    /// Sends a test message through the channel to verify configuration.
    /// Example: `neomind message channel-test slack-alerts`
    ChannelTest {
        /// Channel name.
        #[arg(required = true)]
        name: String,
    },
}

/// Push subcommands.
#[derive(Subcommand, Debug)]
pub enum PushCommand {
    /// List push targets.
    ///
    /// Shows all data push targets and their status.
    /// Example: `neomind push list`
    List,
    /// Get push target details.
    ///
    /// Shows target config, schedule, delivery stats.
    /// Example: `neomind push get <ID>`
    Get {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a push target.
    ///
    /// Forwards device metrics / extension outputs to an external system.
    ///
    /// Target types & config:
    ///   webhook: '{"url":"https://example.com/api","headers":{"Authorization":"Bearer TOKEN"}}'
    ///   mqtt:    '{"broker":"tcp://broker:1883","topic":"neomind/data","username":"user","password":"pass"}'
    ///
    /// Example: `neomind push create --name my-webhook --type webhook --config '{"url":"https://httpbin.org/post"}'`
    Create {
        /// Target name (unique identifier).
        #[arg(long)]
        name: String,
        /// Target type (webhook, mqtt).
        #[arg(long, visible_alias = "target-type")]
        #[arg(short = 't', long = "type", hide = true)]
        target_type: Option<String>,
        /// Target config as JSON.
        #[arg(long)]
        config: Option<String>,
        /// Schedule type: event (real-time) or interval (every 60s). Default: event.
        #[arg(long)]
        schedule: Option<String>,
        /// Comma-separated source patterns to filter (e.g., "device:sensor-001:temperature").
        #[arg(long)]
        sources: Option<String>,
    },
    /// Update a push target.
    ///
    /// Updates target name, config, or enabled status.
    /// Example: `neomind push update <ID> --config '{"url":"https://new-url.com"}'`
    Update {
        /// Target ID.
        #[arg(required = true)]
        id: String,
        /// New name.
        #[arg(long)]
        name: Option<String>,
        /// New target config as JSON (target-specific: webhook url/headers,
        /// mqtt broker/topic). NOTE: data_filter/schedule/template are NOT
        /// config — use --sources / --schedule / --template instead.
        #[arg(long)]
        config: Option<String>,
        /// Enable or disable.
        #[arg(long)]
        enabled: Option<bool>,
        /// Comma-separated source patterns (TOP-LEVEL filter, e.g.
        /// "device:sensor-001:"). Only matching DataSourceIds are pushed.
        #[arg(long)]
        sources: Option<String>,
        /// Schedule: "event" (real-time) or "interval" (every 60s).
        #[arg(long)]
        schedule: Option<String>,
        /// Handlebars template as a JSON string (transforms each payload).
        #[arg(long)]
        template: Option<String>,
        /// Only push when the value changes. Requires --sources (the whole
        /// data_filter is replaced, so patterns must be supplied with it).
        #[arg(long)]
        only_changes: Option<bool>,
    },
    /// Delete a push target.
    ///
    /// Removes the target and its delivery history.
    /// Example: `neomind push delete <ID>`
    Delete {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Start a push target (hidden alias for `push enable`).
    ///
    /// Enables real-time or scheduled data forwarding.
    /// Prefer `push enable <ID>` for consistency with other domains.
    /// Kept for backward compatibility.
    #[command(hide = true)]
    Start {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Stop a push target (hidden alias for `push disable`).
    ///
    /// Pauses data forwarding without deleting the target.
    /// Prefer `push disable <ID>` for consistency with other domains.
    /// Kept for backward compatibility.
    #[command(hide = true)]
    Stop {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Enable a push target (alias for `push start`).
    ///
    /// Unified form across all domains. Same effect as `push start <ID>`.
    ///
    /// Example: `neomind push enable <ID>`
    Enable {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Disable a push target (alias for `push stop`).
    ///
    /// Unified form across all domains. Same effect as `push stop <ID>`.
    ///
    /// Example: `neomind push disable <ID>`
    Disable {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Test a push target.
    ///
    /// Sends a test payload to verify the target works.
    /// Example: `neomind push test <ID>`
    Test {
        /// Target ID.
        #[arg(required = true)]
        id: String,
    },
    /// Show delivery logs for a target.
    ///
    /// Example: `neomind push logs <ID> --limit 20`
    Logs {
        /// Target ID.
        #[arg(required = true)]
        id: String,
        /// Max log entries to return.
        #[arg(long, default_value_t = 20)]
        limit: usize,
    },
    /// Show push statistics.
    ///
    /// Displays aggregated delivery stats across all targets.
    /// Example: `neomind push stats`
    Stats,
}

/// Widget subcommands.
#[derive(Subcommand, Debug)]
pub enum WidgetCommand {
    /// List installed widgets.
    ///
    /// Shows widget ID, name, type, and version.
    /// Example: `neomind widget list`
    List {},
    /// Get widget details.
    ///
    /// Shows widget manifest, config schema, and supported data sources.
    /// Example: `neomind widget get line-chart`
    Get {
        /// Widget ID.
        #[arg(required = true)]
        id: String,
    },
    /// Get widget bundle.
    ///
    /// Returns the compiled JavaScript bundle for a widget.
    /// Example: `neomind widget bundle line-chart`
    Bundle {
        /// Widget ID.
        #[arg(required = true)]
        id: String,
    },
    /// Scaffold a new widget component (generates manifest.json + bundle.js).
    ///
    /// Creates a widget project with all required files.
    /// Widget types: chart, gauge, stat, table, image, custom.
    /// Example: `neomind widget create "My Chart" --widget-type chart -o ./my-widgets`
    Create {
        /// Widget display name.
        #[arg(required = true)]
        name: String,
        /// Widget type: chart, gauge, stat, table, image, custom.
        #[arg(long, default_value = "custom")]
        widget_type: String,
        /// Output directory (defaults to widget ID).
        #[arg(long)]
        output: Option<String>,
        /// Scaffold AND install in one step — register the widget immediately
        /// after scaffolding, no separate `widget install` needed.
        #[arg(long)]
        install: bool,
    },
    /// Install widget from a scaffolded directory or .zip package.
    ///
    /// Accepts a directory containing manifest.json + bundle.js (as `widget
    /// create` produces) OR a .zip package. Or use `widget create --install`
    /// to scaffold+install in one step.
    /// Example: `neomind widget install ./my-chart/`
    Install {
        /// Path to widget directory or .zip file.
        #[arg(required = true)]
        file: String,
    },
    /// Uninstall widget.
    ///
    /// Removes the widget. Dashboards using this widget will show an error.
    /// Example: `neomind widget uninstall my-chart`
    Uninstall {
        /// Widget ID.
        #[arg(required = true)]
        id: String,
    },
    /// List marketplace widgets.
    ///
    /// Shows all widgets available in the marketplace registry.
    /// Example: `neomind widget market-list`
    MarketList {},
    /// Install widget from marketplace.
    ///
    /// Downloads and installs a widget from the marketplace registry.
    /// Example: `neomind widget market-install line-chart --version 1.2.0`
    MarketInstall {
        /// Widget ID.
        #[arg(required = true)]
        id: String,
        /// Version (optional).
        #[arg(long)]
        version: Option<String>,
    },
}

/// System information subcommands.
#[derive(Subcommand, Debug)]
pub enum SystemCommand {
    /// Show system infrastructure info (MQTT broker, webhook URL, network).
    ///
    /// Returns MQTT broker address, webhook base URL, local IP, and API endpoint.
    /// Use this to discover connection endpoints for devices and connectors.
    ///
    /// Example: `neomind system info`
    #[command(alias = "status")]
    Info {},
}

/// Config subcommands (backup / restore).
#[derive(Subcommand, Debug)]
pub enum ConfigCommand {
    /// Export the full system configuration as JSON.
    ///
    /// Pipe to a file for backup: `neomind config export > backup.json`
    Export {},
    /// Import a configuration from a JSON file (overwrites matching domains).
    ///
    /// Example: `neomind config import backup.json`
    Import {
        /// Path to a JSON file produced by `config export`.
        #[arg(required = true)]
        file: String,
    },
    /// Validate a configuration JSON without applying it.
    Validate {
        /// Path to a JSON file to check.
        #[arg(required = true)]
        file: String,
    },
}

/// System settings subcommands (timezone, data retention).
#[derive(Subcommand, Debug)]
pub enum SettingsCommand {
    /// Get the current global timezone.
    ///
    /// Returns the IANA timezone used for cron schedule evaluation and timestamp display.
    ///
    /// Example: `neomind settings timezone`
    Timezone {},
    /// Set the global timezone (IANA format, e.g. Asia/Shanghai).
    ///
    /// Affects cron schedule evaluation and displayed timestamps. Run `settings timezones`
    /// to see valid values.
    ///
    /// Example: `neomind settings set-timezone Asia/Shanghai`
    SetTimezone {
        /// Timezone in IANA format (e.g. "Asia/Shanghai", "UTC").
        #[arg(required = true)]
        timezone: String,
    },
    /// List available timezones.
    ///
    /// Shows all IANA timezone identifiers accepted by `settings set-timezone`.
    ///
    /// Example: `neomind settings timezones`
    Timezones {},
    /// Get data retention configuration.
    ///
    /// Returns whether automatic cleanup is enabled, the cleanup interval, and
    /// retention limits for telemetry and image data. Change with `settings set-retention`.
    ///
    /// Example: `neomind settings retention`
    Retention {},
    /// Update data retention configuration.
    ///
    /// Controls automatic cleanup of telemetry data in `data/telemetry.redb`.
    /// Example: `neomind settings set-retention --enabled --interval-hours 1 --default-retention 168`
    SetRetention {
        /// Enable or disable automatic retention cleanup.
        #[arg(long)]
        enabled: bool,
        /// Cleanup interval in hours (must be greater than 0).
        #[arg(long)]
        interval_hours: u64,
        /// Default retention limit in hours for telemetry points.
        #[arg(long)]
        default_retention: Option<u64>,
        /// Retention limit in hours for image data.
        #[arg(long)]
        image_retention: Option<u64>,
    },
    /// Trigger a manual data cleanup now.
    ///
    /// Immediately applies retention rules to delete expired telemetry/image data,
    /// without waiting for the next scheduled cleanup. Review limits with
    /// `settings retention` first.
    ///
    /// Example: `neomind settings cleanup`
    Cleanup {},
}

/// Data connector subcommands (MQTT, webhook, HTTP, etc.).
#[derive(Subcommand, Debug)]
pub enum ConnectorCommand {
    /// List all data connectors.
    ///
    /// Shows connector ID, name, host, port, type, and connection status.
    /// Example: `neomind connector list`
    List {},
    /// Get connector details and connection status.
    ///
    /// Shows connection state, subscriptions, and message statistics.
    /// Example: `neomind connector get connector-001`
    Get {
        /// Connector ID.
        #[arg(required = true)]
        id: String,
    },
    /// Create a new data connector.
    ///
    /// Registers an external data source for data ingestion.
    /// Currently supports MQTT connectors; more types coming soon.
    ///
    /// Workflow:
    ///   1. `connector create --name "Factory MQTT" --host 192.168.1.100 --port 1883`
    ///   2. `connector test <ID>` — verify connectivity
    ///   3. Devices sending to the connector's topics will auto-appear in NeoMind
    ///
    ///   Example: `neomind connector create --type mqtt --name "Factory MQTT" --host 192.168.1.100 --port 1883 --topics "sensor/#,device/#"`
    Create {
        /// Connector type (mqtt, webhook, http). Default: mqtt.
        #[arg(long, default_value = "mqtt")]
        connector_type: String,
        /// Connector display name.
        #[arg(long)]
        name: String,
        /// Connector hostname or IP.
        #[arg(long)]
        host: String,
        /// Connector port (default: 1883).
        #[arg(long, default_value_t = 1883)]
        port: u16,
        /// Enable/disable TLS (use --tls=true or --tls=false). Defaults to false.
        #[arg(long)]
        tls: Option<bool>,
        /// Username for authentication.
        #[arg(long)]
        username: Option<String>,
        /// Password for authentication.
        #[arg(long)]
        password: Option<String>,
        /// Comma-separated topic subscriptions (default: # for all).
        #[arg(long)]
        topics: Option<String>,
    },
    /// Update connector configuration.
    ///
    /// Modify connector settings. Use --disable to stop, omit to keep current values.
    /// Example: `neomind connector update connector-001 --topics "sensor/+/data" --password "newpass"`
    Update {
        /// Connector ID.
        #[arg(required = true)]
        id: String,
        /// Connector display name.
        #[arg(long)]
        name: Option<String>,
        /// Connector hostname or IP.
        #[arg(long)]
        host: Option<String>,
        /// Connector port.
        #[arg(long)]
        port: Option<u16>,
        /// Enable/disable TLS (use --tls=true or --tls=false).
        #[arg(long)]
        tls: Option<bool>,
        /// Username for authentication.
        #[arg(long)]
        username: Option<String>,
        /// Password for authentication.
        #[arg(long)]
        password: Option<String>,
        /// Comma-separated topic subscriptions.
        #[arg(long)]
        topics: Option<String>,
        /// Disable the connector (use --disable=true or --disable=false). Prefer `connector enable/disable` subcommands.
        #[arg(long)]
        disable: Option<bool>,
    },
    /// Delete a connector.
    ///
    /// Removes the connector and disconnects. All subscriptions are lost.
    /// Example: `neomind connector delete connector-001`
    Delete {
        /// Connector ID.
        #[arg(required = true)]
        id: String,
    },
    /// Enable a connector.
    ///
    /// Reactivates a disabled connector. Subscriptions resume.
    /// Prefer this over `connector update <ID>` (without `--disable`) for clarity.
    ///
    /// Example: `neomind connector enable connector-001`
    Enable {
        /// Connector ID.
        #[arg(required = true)]
        id: String,
    },
    /// Disable a connector.
    ///
    /// Stops the connection without deleting the connector.
    /// Prefer this over `connector update <ID> --disable`.
    ///
    /// Example: `neomind connector disable connector-001`
    Disable {
        /// Connector ID.
        #[arg(required = true)]
        id: String,
    },
    /// Test connector connectivity with real MQTT handshake.
    ///
    /// Attempts to connect, subscribe, and publish a test message.
    /// Run this after creating or updating a connector to verify settings.
    /// If it fails, check host, port, credentials, and firewall rules.
    /// Example: `neomind connector test connector-001`
    Test {
        /// Connector ID.
        #[arg(required = true)]
        id: String,
    },
    /// List all MQTT topic subscriptions.
    ///
    /// Shows all active topic subscriptions across all connectors.
    /// Example: `neomind connector subscriptions`
    Subscriptions {},
    /// Subscribe to a custom MQTT topic.
    ///
    /// Adds a new topic subscription to the embedded broker.
    /// Supports MQTT wildcards: + (single level) and # (multi level).
    /// Example: `neomind connector subscribe --topic "factory/+/temperature" --qos 1`
    Subscribe {
        /// Topic pattern to subscribe to.
        #[arg(long)]
        topic: String,
        /// QoS level (0, 1, or 2). Default: 1.
        #[arg(long, default_value_t = 1)]
        qos: u8,
    },
    /// Unsubscribe from an MQTT topic.
    ///
    /// Removes a topic subscription from the embedded broker.
    /// Example: `neomind connector unsubscribe --topic "factory/+/temperature"`
    Unsubscribe {
        /// Topic to unsubscribe from.
        #[arg(long)]
        topic: String,
    },
}

/// Parse human duration like "30s", "5m", "1h", "2d" to seconds
pub fn parse_duration(s: &str) -> u64 {
    let s = s.trim();
    if let Some(num) = s.strip_suffix('s') {
        num.parse::<u64>().unwrap_or(30)
    } else if let Some(num) = s.strip_suffix('m') {
        num.parse::<u64>().unwrap_or(1) * 60
    } else if let Some(num) = s.strip_suffix('h') {
        num.parse::<u64>().unwrap_or(1) * 3600
    } else if let Some(num) = s.strip_suffix('d') {
        num.parse::<u64>().unwrap_or(1) * 86400
    } else {
        s.parse::<u64>().unwrap_or(300)
    }
}

#[cfg(test)]
mod unify_enable_disable_tests {
    //! Smoke tests for the unified `<domain> enable|disable <id>` subcommand pattern.
    //! Each domain (transform/push/connector) should parse identically.

    use super::{Args, Command};
    use clap::Parser;

    fn parse(argv: &str) -> Command {
        // try_parse_from treats the first item as the program name (bin_name)
        Args::try_parse_from(["neomind"].into_iter().chain(argv.split_whitespace()))
            .unwrap()
            .command
    }

    #[test]
    fn transform_enable_disable_parse() {
        let cmd = parse("transform enable transform-001");
        let Command::Transform { transform_cmd } = cmd else {
            panic!("expected Transform, got {cmd:?}");
        };
        match transform_cmd {
            super::TransformCommand::Enable { id } => assert_eq!(id, "transform-001"),
            other => panic!("expected Transform::Enable, got {other:?}"),
        }
        let cmd = parse("transform disable transform-001");
        let Command::Transform { transform_cmd } = cmd else {
            panic!("expected Transform, got {cmd:?}");
        };
        match transform_cmd {
            super::TransformCommand::Disable { id } => assert_eq!(id, "transform-001"),
            other => panic!("expected Transform::Disable, got {other:?}"),
        }
    }

    #[test]
    fn push_enable_disable_parse() {
        let cmd = parse("push enable push-001");
        let Command::Push { push_cmd } = cmd else {
            panic!("expected Push, got {cmd:?}");
        };
        match push_cmd {
            super::PushCommand::Enable { id } => assert_eq!(id, "push-001"),
            other => panic!("expected Push::Enable, got {other:?}"),
        }
        let cmd = parse("push disable push-001");
        let Command::Push { push_cmd } = cmd else {
            panic!("expected Push, got {cmd:?}");
        };
        match push_cmd {
            super::PushCommand::Disable { id } => assert_eq!(id, "push-001"),
            other => panic!("expected Push::Disable, got {other:?}"),
        }
    }

    #[test]
    fn connector_enable_disable_parse() {
        let cmd = parse("connector enable conn-001");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Enable { id } => assert_eq!(id, "conn-001"),
            other => panic!("expected Connector::Enable, got {other:?}"),
        }
        let cmd = parse("connector disable conn-001");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Disable { id } => assert_eq!(id, "conn-001"),
            other => panic!("expected Connector::Disable, got {other:?}"),
        }
    }

    /// P1.1: connector update --tls=false and --disable=true must work
    /// (previously plain `bool` rejected value form, only accepted flag presence).
    #[test]
    fn connector_update_bool_flags_accept_false_value() {
        let cmd = parse("connector update conn-001 --tls=false");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Update { tls, id, .. } => {
                assert_eq!(id, "conn-001");
                assert_eq!(tls, Some(false));
            }
            other => panic!("expected Connector::Update, got {other:?}"),
        }
        let cmd = parse("connector update conn-001 --disable=true");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Update { disable, id, .. } => {
                assert_eq!(id, "conn-001");
                assert_eq!(disable, Some(true));
            }
            other => panic!("expected Connector::Update, got {other:?}"),
        }
    }

    /// Batch 1 C1: connector create --tls=false must work (was plain `bool`).
    /// Symmetric with the update path fix from P1.1.
    #[test]
    fn connector_create_tls_accepts_explicit_value() {
        // Explicit false
        let cmd = parse("connector create --name x --host h --tls=false");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Create { tls, .. } => {
                assert_eq!(tls, Some(false));
            }
            other => panic!("expected Connector::Create, got {other:?}"),
        }

        // Explicit true
        let cmd = parse("connector create --name x --host h --tls=true");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Create { tls, .. } => {
                assert_eq!(tls, Some(true));
            }
            other => panic!("expected Connector::Create, got {other:?}"),
        }

        // Omitted → None (handler defaults to false)
        let cmd = parse("connector create --name x --host h");
        let Command::Connector { connector_cmd } = cmd else {
            panic!("expected Connector, got {cmd:?}");
        };
        match connector_cmd {
            super::ConnectorCommand::Create { tls, .. } => {
                assert_eq!(tls, None);
            }
            other => panic!("expected Connector::Create, got {other:?}"),
        }
    }

    /// Batch 1 Concern 1: dashboard share --public=false and device history
    /// --compress=false must work (Option<bool> sweep).
    #[test]
    fn option_bool_sweep_accepts_false_value() {
        // dashboard share --public=false
        let cmd = parse("dashboard share dash-001 --public=false");
        let Command::Dashboard { dashboard_cmd } = cmd else {
            panic!("expected Dashboard, got {cmd:?}");
        };
        match dashboard_cmd {
            super::DashboardCommand::Share { public, id, .. } => {
                assert_eq!(id, "dash-001");
                assert_eq!(public, Some(false));
            }
            other => panic!("expected Dashboard::Share, got {other:?}"),
        }

        // device history --compress=false
        let cmd = parse("device history dev-001 --compress=false");
        let Command::Device { device_cmd } = cmd else {
            panic!("expected Device, got {cmd:?}");
        };
        match device_cmd {
            super::DeviceCommand::History { compress, id, .. } => {
                assert_eq!(id, "dev-001");
                assert_eq!(compress, Some(false));
            }
            other => panic!("expected Device::History, got {other:?}"),
        }
    }

    /// Batch 1 Concern 3: extension get is primary; info is visible alias.
    #[test]
    fn extension_get_is_primary_info_is_alias() {
        // Both `get` and `info` must parse to the same variant.
        let cmd_get = parse("extension get weather-forecast");
        let cmd_info = parse("extension info weather-forecast");
        match (cmd_get, cmd_info) {
            (
                Command::Extension {
                    extension_cmd: super::ExtensionCommand::Get { id_or_path: id1 },
                },
                Command::Extension {
                    extension_cmd: super::ExtensionCommand::Get { id_or_path: id2 },
                },
            ) => {
                assert_eq!(id1, "weather-forecast");
                assert_eq!(id2, "weather-forecast");
            }
            other => panic!("expected Extension::Get for both, got {other:?}"),
        }
    }

    /// Batch 1 Concern 8: message ack is a visible alias for read.
    #[test]
    fn message_ack_is_alias_for_read() {
        let cmd = parse("message ack msg-001");
        let Command::Message { message_cmd } = cmd else {
            panic!("expected Message, got {cmd:?}");
        };
        match message_cmd {
            super::MessageCommand::Read { id } => {
                assert_eq!(id, "msg-001");
            }
            other => panic!("expected Message::Read, got {other:?}"),
        }
    }
}
