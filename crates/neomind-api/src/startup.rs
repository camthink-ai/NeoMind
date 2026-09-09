//! Unified startup logging and console output formatting.

use std::sync::OnceLock;

/// ANSI color codes for terminal output.
const ANSI_RESET: &str = "\x1b[0m";
const ANSI_BOLD: &str = "\x1b[1m";
const ANSI_DIM: &str = "\x1b[2m";
const ANSI_GREEN: &str = "\x1b[32m";
const ANSI_BLUE: &str = "\x1b[34m";
const ANSI_YELLOW: &str = "\x1b[33m";
const ANSI_RED: &str = "\x1b[31m";
const ANSI_CYAN: &str = "\x1b[36m";
const ANSI_GRAY: &str = "\x1b[90m";

/// Whether colors are enabled (disabled in CI/logs, can be forced via env var).
fn colors_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        // Check if explicitly disabled
        if std::env::var("NO_COLOR").is_ok() {
            return false;
        }
        // Check if explicitly enabled
        if std::env::var("NEOMIND_COLOR")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(false)
        {
            return true;
        }
        // Auto-detect: enable if we have a TTY
        atty::is(atty::Stream::Stderr) // Use Stderr to avoid conflicts with stdout redirection
    })
}

/// Wrap text in color if colors are enabled.
fn color(s: impl AsRef<str>, ansi: &str) -> String {
    if colors_enabled() {
        format!("{}{}{}", ansi, s.as_ref(), ANSI_RESET)
    } else {
        s.as_ref().to_string()
    }
}

/// Startup phase tracker for organized console output.
pub struct StartupLogger {
    phase: StartupPhase,
    quiet: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum StartupPhase {
    Banner,
    Initialization,
    Configuration,
    Services,
    Ready,
}

impl StartupLogger {
    /// Create a new startup logger.
    pub fn new() -> Self {
        Self {
            phase: StartupPhase::Banner,
            quiet: false,
        }
    }

    /// Create a quiet startup logger (minimal output).
    pub fn quiet() -> Self {
        Self {
            phase: StartupPhase::Banner,
            quiet: true,
        }
    }

    /// Print the startup banner.
    pub fn banner(&mut self) {
        if self.quiet {
            return;
        }
        self.phase = StartupPhase::Banner;

        println!();
        println!(
            "{}",
            color("┌─────────────────────────────────────────┐", ANSI_CYAN)
        );
        println!(
            "{}{}{}",
            color("│ ", ANSI_CYAN),
            color("NeoMind Edge AI Agent", ANSI_BOLD),
            color("                       │", ANSI_CYAN)
        );
        println!(
            "{}{}{}",
            color("│ ", ANSI_CYAN),
            color("Edge AI Agent - Web Server", ANSI_DIM),
            color("                    │", ANSI_CYAN)
        );
        println!(
            "{}",
            color("└─────────────────────────────────────────┘", ANSI_CYAN)
        );
        // A server built without the `static` feature still runs the API
        // fine, but every UI route serves a "not embedded" page — this
        // bit operators MORE than once (they saw a broken-looking page
        // and assumed the server was broken). Say it loudly here too.
        #[cfg(not(feature = "static"))]
        {
            println!(
                "{} {}",
                color("⚠", ANSI_YELLOW),
                color(
                    "Web UI not embedded — API only. \
                     Rebuild with --features static to serve the dashboard.",
                    ANSI_YELLOW
                )
            );
            println!();
        }
        #[cfg(feature = "static")]
        println!();
    }

    /// Transition to initialization phase.
    pub fn phase_init(&mut self) {
        if self.quiet {
            return;
        }
        if self.phase != StartupPhase::Initialization {
            println!(
                "{} {} {}",
                color("›", ANSI_BOLD),
                color("Initialization", ANSI_BLUE),
                color("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", ANSI_DIM)
            );
            self.phase = StartupPhase::Initialization;
        }
    }

    /// Transition to configuration phase.
    pub fn phase_config(&mut self) {
        if self.quiet {
            return;
        }
        if self.phase != StartupPhase::Configuration {
            println!(
                "{} {} {}",
                color("›", ANSI_BOLD),
                color("Configuration", ANSI_BLUE),
                color("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", ANSI_DIM)
            );
            self.phase = StartupPhase::Configuration;
        }
    }

    /// Transition to services phase.
    pub fn phase_services(&mut self) {
        if self.quiet {
            return;
        }
        if self.phase != StartupPhase::Services {
            println!(
                "{} {} {}",
                color("›", ANSI_BOLD),
                color("Services", ANSI_BLUE),
                color("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━", ANSI_DIM)
            );
            self.phase = StartupPhase::Services;
        }
    }

    /// Transition to ready phase.
    pub fn phase_ready(&mut self) {
        if self.quiet {
            return;
        }
        if self.phase != StartupPhase::Ready {
            println!();
            println!(
                "{} {}",
                color("✓", ANSI_GREEN),
                color("Server ready", ANSI_BOLD)
            );
            self.phase = StartupPhase::Ready;
        }
    }

    /// Log an informational message with icon.
    pub fn info(&self, message: &str) {
        if self.quiet {
            return;
        }
        println!("  {} {}", color("●", ANSI_BLUE), message);
    }

    /// Log a success message with icon.
    pub fn success(&self, message: &str) {
        if self.quiet {
            return;
        }
        println!("  {} {}", color("✓", ANSI_GREEN), message);
    }

    /// Log a warning message with icon.
    pub fn warning(&self, message: &str) {
        if self.quiet {
            return;
        }
        println!("  {} {}", color("⚠", ANSI_YELLOW), message);
    }

    /// Log an error message with icon.
    pub fn error(&self, message: &str) {
        if self.quiet {
            return;
        }
        println!("  {} {}", color("✗", ANSI_RED), message);
    }

    /// Log a detail message (indented, dim).
    pub fn detail(&self, message: &str) {
        if self.quiet {
            return;
        }
        println!("    {}", color(message, ANSI_GRAY));
    }

    /// Print API key banner.
    pub fn api_key_banner(&self, key: &str, name: &str) {
        if self.quiet {
            return;
        }
        println!();
        println!(
            "{}",
            color(
                "  ╔═════════════════════════════════════════════════════╗",
                ANSI_YELLOW
            )
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_YELLOW),
            color("⚠ DEFAULT API KEY GENERATED", ANSI_BOLD),
            color("                               ║", ANSI_YELLOW)
        );
        println!(
            "{}",
            color(
                "  ╠═════════════════════════════════════════════════════╣",
                ANSI_YELLOW
            )
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_YELLOW),
            color("Key:", ANSI_BOLD),
            color(format!(" {:44} ", key), ANSI_CYAN)
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_YELLOW),
            color("Name:", ANSI_BOLD),
            color(format!(" {:43} ", name), ANSI_DIM)
        );
        println!(
            "{}",
            color(
                "  ╠═════════════════════════════════════════════════════╣",
                ANSI_YELLOW
            )
        );
        println!(
            "{} {}",
            color("  ║", ANSI_YELLOW),
            color(
                "  Save this key! You'll need it to access the API. ",
                ANSI_DIM
            )
        );
        println!(
            "{} {}",
            color("  ║", ANSI_YELLOW),
            color(
                "  Use it in the frontend login dialog.              ",
                ANSI_DIM
            )
        );
        println!(
            "{}",
            color(
                "  ╚══════════════════════════════════════════════════════╝",
                ANSI_YELLOW
            )
        );
        println!();
    }

    /// Print default admin user banner.
    pub fn admin_user_banner(&self, username: &str, password: &str) {
        if self.quiet {
            return;
        }
        println!();
        println!(
            "{}",
            color(
                "  ╔═════════════════════════════════════════════════════╗",
                ANSI_BLUE
            )
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_BLUE),
            color("👤 DEFAULT ADMIN USER CREATED", ANSI_BOLD),
            color("                          ║", ANSI_BLUE)
        );
        println!(
            "{}",
            color(
                "  ╠═════════════════════════════════════════════════════╣",
                ANSI_BLUE
            )
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_BLUE),
            color("Username:", ANSI_BOLD),
            color(format!(" {:39} ", username), ANSI_CYAN)
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_BLUE),
            color("Password:", ANSI_BOLD),
            color(format!(" {:39} ", password), ANSI_CYAN)
        );
        println!(
            "{}",
            color(
                "  ╠═════════════════════════════════════════════════════╣",
                ANSI_BLUE
            )
        );
        println!(
            "{} {}",
            color("  ║", ANSI_BLUE),
            color(
                "  ⚠ Please change the password after first login!  ",
                ANSI_DIM
            )
        );
        println!(
            "{}",
            color(
                "  ╚══════════════════════════════════════════════════════╝",
                ANSI_BLUE
            )
        );
        println!();
    }

    /// Print server ready info with URL.
    pub fn ready_info(&self, addr: &str) {
        if self.quiet {
            return;
        }
        println!();
        println!(
            "{}",
            color(
                "  ╔═════════════════════════════════════════════════════╗",
                ANSI_GREEN
            )
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_GREEN),
            color("✓ Server is running!", ANSI_BOLD),
            color("                                ║", ANSI_GREEN)
        );
        println!(
            "{}",
            color(
                "  ╠═════════════════════════════════════════════════════╣",
                ANSI_GREEN
            )
        );
        println!(
            "{} {} {}",
            color("  ║", ANSI_GREEN),
            color("Local:", ANSI_BOLD),
            color(format!("  http://{}                     ", addr), ANSI_CYAN)
        );
        println!(
            "{}",
            color(
                "  ╚══════════════════════════════════════════════════════╝",
                ANSI_GREEN
            )
        );
        println!();
        println!(
            "{} {}",
            color("Press", ANSI_BOLD),
            color("Ctrl+C to stop.", ANSI_DIM)
        );
        println!();
    }

    /// Log service startup.
    pub fn service(&self, name: &str, status: ServiceStatus) {
        if self.quiet {
            return;
        }
        let (icon, color_code) = match status {
            ServiceStatus::Started => ("✓", ANSI_GREEN),
            ServiceStatus::Warning => ("⚠", ANSI_YELLOW),
            ServiceStatus::Error => ("✗", ANSI_RED),
            ServiceStatus::Disabled => ("○", ANSI_GRAY),
        };
        println!("    {} {:30}", color(icon, color_code), name);
    }
}

/// Service status for startup logging.
pub enum ServiceStatus {
    Started,
    Warning,
    Error,
    Disabled,
}

impl Default for StartupLogger {
    fn default() -> Self {
        Self::new()
    }
}

/// Convenience function to log structured startup messages.
pub fn log_startup() -> StartupLogger {
    StartupLogger::new()
}

/// Log categories for consistent tagging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogCategory {
    /// Storage/database operations
    Storage,
    /// MQTT/network operations
    Network,
    /// Authentication/security
    Auth,
    /// LLM/AI operations
    AI,
    /// Device operations
    Device,
    /// General system
    System,
}

impl LogCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Storage => "storage",
            Self::Network => "network",
            Self::Auth => "auth",
            Self::AI => "ai",
            Self::Device => "device",
            Self::System => "system",
        }
    }

    pub fn icon(&self) -> &'static str {
        match self {
            Self::Storage => "💾",
            Self::Network => "🌐",
            Self::Auth => "🔑",
            Self::AI => "🤖",
            Self::Device => "🔧",
            Self::System => "⚙",
        }
    }
}

/// Structured logging macro replacement for eprintln!.
///
/// Usage:
/// ```rust,ignore
/// startup_log!(LogCategory::Storage, info, "Event log initialized at {}", path);
/// ```
#[macro_export]
macro_rules! startup_log {
    ($category:expr, $level:ident, $($arg:tt)*) => {
        tracing::$level!(
            category = $crate::startup::LogCategory::as_str(&$category),
            $($arg)*
        );
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_colors_enabled() {
        // Just ensure it doesn't panic
        let _enabled = colors_enabled();
    }

    #[test]
    fn test_color_wrapper() {
        let colored = color("test", ANSI_GREEN);
        // With colors disabled, should just be "test"
        // With colors enabled, should contain ANSI codes
        assert!(colored.contains("test"));
    }

    #[test]
    fn test_startup_logger_creation() {
        let logger = StartupLogger::new();
        let quiet = StartupLogger::quiet();
        // Just ensure they don't panic
        let _ = logger;
        let _ = quiet;
    }
}
