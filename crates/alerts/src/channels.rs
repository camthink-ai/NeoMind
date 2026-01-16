//! Built-in notification channel implementations.
//!
//! This module provides concrete implementations of the `AlertChannel` trait
//! for common notification methods.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;

use edge_ai_core::alerts::{Alert, AlertChannel, AlertError, Result as CoreResult};

use super::error::{Error, Result};

/// Convert core AlertError to local Error
impl From<AlertError> for Error {
    fn from(err: AlertError) -> Self {
        match err {
            AlertError::ChannelDisabled(n) => Error::ChannelDisabled(n),
            AlertError::ChannelNotFound(n) => Error::NotFound(n),
            AlertError::SendFailed(m) => Error::SendError(m),
            AlertError::InvalidConfiguration(m) => Error::Validation(m),
            AlertError::Other(e) => Error::Other(e),
        }
    }
}

// ============================================================================
// Built-in Channel Implementations
// ============================================================================

/// In-memory channel for testing.
#[derive(Debug, Clone)]
pub struct MemoryChannel {
    name: String,
    enabled: bool,
    alerts: Arc<std::sync::Mutex<Vec<Alert>>>,
}

impl MemoryChannel {
    /// Create a new memory channel.
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            alerts: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Create a disabled memory channel.
    pub fn disabled(name: String) -> Self {
        Self {
            name,
            enabled: false,
            alerts: Arc::new(std::sync::Mutex::new(Vec::new())),
        }
    }

    /// Get all alerts sent to this channel.
    pub fn get_alerts(&self) -> Vec<Alert> {
        self.alerts.lock().unwrap().clone()
    }

    /// Clear all alerts.
    pub fn clear(&self) {
        self.alerts.lock().unwrap().clear();
    }

    /// Get the count of alerts.
    pub fn count(&self) -> usize {
        self.alerts.lock().unwrap().len()
    }

    /// Enable the channel.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the channel.
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

#[async_trait]
impl AlertChannel for MemoryChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "memory"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, alert: &Alert) -> CoreResult<()> {
        if !self.enabled {
            return Err(AlertError::ChannelDisabled(self.name.clone()));
        }
        self.alerts.lock().unwrap().push(alert.clone());
        Ok(())
    }
}

/// Console channel for printing alerts to stdout.
#[derive(Debug, Clone)]
pub struct ConsoleChannel {
    name: String,
    enabled: bool,
    include_details: bool,
}

impl ConsoleChannel {
    /// Create a new console channel.
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            include_details: true,
        }
    }

    /// Set whether to include detailed information.
    pub fn with_details(mut self, include: bool) -> Self {
        self.include_details = include;
        self
    }

    /// Create a disabled console channel.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Enable the channel.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the channel.
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

#[async_trait]
impl AlertChannel for ConsoleChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "console"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, alert: &Alert) -> CoreResult<()> {
        if !self.enabled {
            return Err(AlertError::ChannelDisabled(self.name.clone()));
        }

        println!("=== {} ===", alert.severity);
        println!("时间: {}", alert.timestamp);
        println!("标题: {}", alert.title);
        println!("消息: {}", alert.message);
        println!("来源: {}", alert.source);

        if self.include_details {
            if alert.metadata.get("tags").is_some() {
                println!("标签: {:?}", alert.metadata.get("tags"));
            }
            println!("状态: {}", alert.status);
        }

        println!("================");

        Ok(())
    }
}

// ============================================================================
// Webhook Channel (feature-gated)
// ============================================================================

/// Webhook channel for sending alerts via HTTP POST.
#[cfg(feature = "webhook")]
#[derive(Debug, Clone)]
pub struct WebhookChannel {
    name: String,
    enabled: bool,
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

#[cfg(feature = "webhook")]
impl WebhookChannel {
    /// Create a new webhook channel.
    pub fn new(name: String, url: String) -> Self {
        Self {
            name,
            enabled: true,
            url,
            headers: HashMap::new(),
            client: reqwest::Client::new(),
        }
    }

    /// Add a header to the webhook request.
    pub fn with_header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Set headers for the webhook request.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Disable the channel.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Enable the channel.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the channel.
    pub fn disable(&mut self) {
        self.enabled = false;
    }
}

#[cfg(feature = "webhook")]
#[async_trait]
impl AlertChannel for WebhookChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "webhook"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, alert: &Alert) -> CoreResult<()> {
        if !self.enabled {
            return Err(AlertError::ChannelDisabled(self.name.clone()));
        }

        let mut request = self.client.post(&self.url);

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        let response = request
            .json(alert)
            .send()
            .await
            .map_err(|e| AlertError::SendFailed(format!("Webhook request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(AlertError::SendFailed(format!(
                "Webhook returned error: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Factory for creating webhook channels.
#[cfg(feature = "webhook")]
pub struct WebhookChannelFactory;

#[cfg(feature = "webhook")]
impl edge_ai_core::alerts::ChannelFactory for WebhookChannelFactory {
    fn channel_type(&self) -> &str {
        "webhook"
    }

    fn create(&self, config: &serde_json::Value) -> CoreResult<std::sync::Arc<dyn AlertChannel>> {
        let url = config
            .get("url")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AlertError::InvalidConfiguration("Missing url".to_string()))?;

        let mut channel = WebhookChannel::new(
            config
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("webhook")
                .to_string(),
            url.to_string(),
        );

        if let Some(headers) = config.get("headers") {
            if let Some(obj) = headers.as_object() {
                for (key, value) in obj {
                    if let Some(str_val) = value.as_str() {
                        channel = channel.with_header(key.clone(), str_val.to_string());
                    }
                }
            }
        }

        if config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            Ok(Arc::new(channel))
        } else {
            Ok(Arc::new(channel.disabled()))
        }
    }
}

// ============================================================================
// Email Channel (feature-gated)
// ============================================================================

/// Email attachment for sending files with alerts.
#[cfg(feature = "email")]
#[derive(Debug, Clone)]
pub struct EmailAttachment {
    /// Filename for the attachment.
    pub filename: String,
    /// Content of the attachment.
    pub content: Vec<u8>,
    /// MIME type of the attachment.
    pub content_type: String,
}

/// Email channel for sending alerts via SMTP.
#[cfg(feature = "email")]
#[derive(Debug, Clone)]
pub struct EmailChannel {
    name: String,
    enabled: bool,
    smtp_server: String,
    smtp_port: u16,
    username: String,
    password: String,
    from_address: String,
    to_addresses: Vec<String>,
    use_tls: bool,
}

#[cfg(feature = "email")]
impl EmailChannel {
    /// Create a new email channel.
    pub fn new(
        name: String,
        smtp_server: String,
        smtp_port: u16,
        username: String,
        password: String,
        from_address: String,
    ) -> Self {
        Self {
            name,
            enabled: true,
            smtp_server,
            smtp_port,
            username,
            password,
            from_address,
            to_addresses: Vec::new(),
            use_tls: true,
        }
    }

    /// Add a recipient.
    pub fn add_recipient(mut self, address: String) -> Self {
        self.to_addresses.push(address);
        self
    }

    /// Set multiple recipients.
    pub fn set_recipients(mut self, addresses: Vec<String>) -> Self {
        self.to_addresses = addresses;
        self
    }

    /// Disable TLS (not recommended).
    pub fn without_tls(mut self) -> Self {
        self.use_tls = false;
        self
    }

    /// Disable the channel.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Enable the channel.
    pub fn enable(&mut self) {
        self.enabled = true;
    }

    /// Disable the channel.
    pub fn disable(&mut self) {
        self.enabled = false;
    }

    /// Build HTML email body from alert.
    fn build_email_body(&self, alert: &Alert) -> String {
        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <style>
        body {{ font-family: Arial, sans-serif; }}
        .alert {{ padding: 20px; border-radius: 5px; }}
        .severity-info {{ background-color: #d4edda; border-left: 4px solid #28a745; }}
        .severity-warning {{ background-color: #fff3cd; border-left: 4px solid #ffc107; }}
        .severity-critical {{ background-color: #f8d7da; border-left: 4px solid #dc3545; }}
        .severity-emergency {{ background-color: #f5c6cb; border-left: 4px solid #bd2130; }}
        .timestamp {{ color: #6c757d; font-size: 0.9em; }}
        .source {{ font-weight: bold; }}
    </style>
</head>
<body>
    <div class="alert severity-{}">
        <h2>{}</h2>
        <p class="timestamp">时间: {}</p>
        <p><strong>来源:</strong> <span class="source">{}</span></p>
        <p><strong>消息:</strong> {}</p>
    </div>
</body>
</html>"#,
            alert.severity.to_string().to_lowercase(),
            alert.title,
            alert.timestamp,
            alert.source,
            alert.message
        )
    }
}

#[cfg(feature = "email")]
#[async_trait]
impl AlertChannel for EmailChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "email"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, alert: &Alert) -> CoreResult<()> {
        if !self.enabled {
            return Err(AlertError::ChannelDisabled(self.name.clone()));
        }

        if self.to_addresses.is_empty() {
            return Err(AlertError::SendFailed(
                "No recipients configured".to_string(),
            ));
        }

        // Build email message
        let html_body = self.build_email_body(alert);
        let subject = format!("[{}] {}", alert.severity, alert.title);

        // Parse from address
        let from_mailbox: lettre::message::Mailbox = self
            .from_address
            .parse()
            .map_err(|e| AlertError::InvalidConfiguration(format!("Invalid from address: {}", e)))?;

        // Build email with recipients
        let mut email_builder = lettre::Message::builder()
            .from(from_mailbox.clone())
            .subject(subject);

        // Add all recipients
        for to_addr in &self.to_addresses {
            let mailbox: lettre::message::Mailbox = to_addr
                .parse()
                .map_err(|e| AlertError::InvalidConfiguration(format!("Invalid to address: {}", e)))?;
            email_builder = email_builder.to(mailbox);
        }

        // Build multipart email
        let email = email_builder
            .multipart(
                lettre::message::MultiPart::alternative()
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(lettre::message::header::ContentType::TEXT_PLAIN)
                            .body(format!("{}\n\n{}", alert.title, alert.message)),
                    )
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(lettre::message::header::ContentType::TEXT_HTML)
                            .body(html_body),
                    ),
            )
            .map_err(|e| AlertError::SendFailed(format!("Failed to build email: {}", e)))?;

        // Clone data for spawn_blocking
        let smtp_server = self.smtp_server.clone();
        let smtp_port = self.smtp_port;
        let username = self.username.clone();
        let password = self.password.clone();

        // Configure and send via tokio executor
        tokio::task::spawn_blocking(move || {
            // Configure SMTP transport
            let creds =
                lettre::transport::smtp::authentication::Credentials::new(username, password);
            let relay = format!("{}:{}", smtp_server, smtp_port);
            let mailer = lettre::SmtpTransport::relay(&relay)
                .map_err(|e| AlertError::SendFailed(format!("Invalid SMTP server: {}", e)))?
                .credentials(creds)
                .build();

            // Send email
            lettre::Transport::send(&mailer, &email)
                .map_err(|e| AlertError::SendFailed(format!("Failed to send email: {}", e)))?;

            Ok::<(), AlertError>(())
        })
        .await
        .map_err(|e| AlertError::SendFailed(format!("Task join error: {}", e)))?
    }
}

/// Factory for creating email channels.
#[cfg(feature = "email")]
pub struct EmailChannelFactory;

#[cfg(feature = "email")]
impl edge_ai_core::alerts::ChannelFactory for EmailChannelFactory {
    fn channel_type(&self) -> &str {
        "email"
    }

    fn create(&self, config: &serde_json::Value) -> CoreResult<std::sync::Arc<dyn AlertChannel>> {
        let smtp_server = config
            .get("smtp_server")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AlertError::InvalidConfiguration("Missing smtp_server".to_string()))?;

        let smtp_port = config
            .get("smtp_port")
            .and_then(|v| v.as_u64())
            .ok_or_else(|| AlertError::InvalidConfiguration("Missing smtp_port".to_string()))?
            as u16;

        let username = config
            .get("username")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AlertError::InvalidConfiguration("Missing username".to_string()))?
            .to_string();

        let password = config
            .get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AlertError::InvalidConfiguration("Missing password".to_string()))?
            .to_string();

        let from_address = config
            .get("from_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AlertError::InvalidConfiguration("Missing from_address".to_string()))?
            .to_string();

        let mut channel = EmailChannel::new(
            config
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("email")
                .to_string(),
            smtp_server.to_string(),
            smtp_port,
            username,
            password,
            from_address,
        );

        // Add recipients
        if let Some(recipients) = config.get("recipients") {
            if let Some(arr) = recipients.as_array() {
                for addr in arr {
                    if let Some(str_addr) = addr.as_str() {
                        channel = channel.add_recipient(str_addr.to_string());
                    }
                }
            }
        }

        // Configure TLS
        if config.get("use_tls").and_then(|v| v.as_bool()).unwrap_or(true) {
            // TLS is enabled by default
        } else {
            channel = channel.without_tls();
        }

        // Set enabled state
        if config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            Ok(Arc::new(channel))
        } else {
            Ok(Arc::new(channel.disabled()))
        }
    }
}

// ============================================================================
// Built-in Channel Factories
// ============================================================================

/// Factory for creating memory channels.
pub struct MemoryChannelFactory;

impl edge_ai_core::alerts::ChannelFactory for MemoryChannelFactory {
    fn channel_type(&self) -> &str {
        "memory"
    }

    fn create(&self, config: &serde_json::Value) -> CoreResult<std::sync::Arc<dyn AlertChannel>> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("memory")
            .to_string();

        let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

        let channel = if enabled {
            MemoryChannel::new(name)
        } else {
            MemoryChannel::disabled(name)
        };

        Ok(Arc::new(channel))
    }
}

/// Factory for creating console channels.
pub struct ConsoleChannelFactory;

impl edge_ai_core::alerts::ChannelFactory for ConsoleChannelFactory {
    fn channel_type(&self) -> &str {
        "console"
    }

    fn create(&self, config: &serde_json::Value) -> CoreResult<std::sync::Arc<dyn AlertChannel>> {
        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("console")
            .to_string();

        let include_details = config
            .get("include_details")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        let enabled = config.get("enabled").and_then(|v| v.as_bool()).unwrap_or(true);

        let mut channel = ConsoleChannel::new(name).with_details(include_details);

        if !enabled {
            channel = channel.disabled();
        }

        Ok(Arc::new(channel))
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use edge_ai_core::alerts::ChannelFactory;

    #[tokio::test]
    async fn test_memory_channel() {
        let channel = MemoryChannel::new("test".to_string());

        let alert = Alert::new(
            "test-1",
            edge_ai_core::alerts::AlertSeverity::Warning,
            "Test".to_string(),
            "Test message".to_string(),
            "test".to_string(),
        );

        channel.send(&alert).await.unwrap();
        assert_eq!(channel.count(), 1);

        let alerts = channel.get_alerts();
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].title, "Test");
    }

    #[tokio::test]
    async fn test_memory_channel_disabled() {
        let channel = MemoryChannel::disabled("test".to_string());

        let alert = Alert::new(
            "test-1",
            edge_ai_core::alerts::AlertSeverity::Warning,
            "Test".to_string(),
            "Test message".to_string(),
            "test".to_string(),
        );

        let result = channel.send(&alert).await;
        assert!(result.is_err());
        assert_eq!(channel.count(), 0);
    }

    #[tokio::test]
    async fn test_console_channel() {
        let channel = ConsoleChannel::new("console".to_string());

        let alert = Alert::new(
            "test-1",
            edge_ai_core::alerts::AlertSeverity::Critical,
            "Critical Alert".to_string(),
            "Something bad happened".to_string(),
            "sensor_1".to_string(),
        );

        // Should not panic
        channel.send(&alert).await.unwrap();
    }

    #[tokio::test]
    async fn test_memory_channel_factory() {
        let factory = MemoryChannelFactory;

        let config = serde_json::json!({
            "name": "test_memory",
            "enabled": true
        });

        let channel = factory.create(&config).unwrap();
        assert_eq!(channel.name(), "test_memory");
        assert!(channel.is_enabled());
        assert_eq!(channel.channel_type(), "memory");
    }

    #[tokio::test]
    async fn test_console_channel_factory() {
        let factory = ConsoleChannelFactory;

        let config = serde_json::json!({
            "name": "test_console",
            "include_details": false,
            "enabled": true
        });

        let channel = factory.create(&config).unwrap();
        assert_eq!(channel.name(), "test_console");
        assert!(channel.is_enabled());
        assert_eq!(channel.channel_type(), "console");
    }
}
