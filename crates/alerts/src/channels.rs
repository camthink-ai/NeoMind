//! Notification channels for sending alerts.
//!
//! This module defines the channel types and built-in channel implementations.

use std::collections::HashMap;

use super::alert::Alert;
use super::error::{Error, Result};

/// Channel type enum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChannelType {
    Memory,
    Console,
    #[cfg(feature = "webhook")]
    Webhook,
    #[cfg(feature = "email")]
    Email,
}

/// Notification channel.
#[derive(Debug, Clone)]
pub enum NotificationChannel {
    Memory(MemoryChannel),
    Console(ConsoleChannel),
    #[cfg(feature = "webhook")]
    Webhook(WebhookChannel),
    #[cfg(feature = "email")]
    Email(EmailChannel),
}

impl NotificationChannel {
    /// Get the channel name.
    pub fn name(&self) -> &str {
        match self {
            Self::Memory(ch) => ch.name(),
            Self::Console(ch) => ch.name(),
            #[cfg(feature = "webhook")]
            Self::Webhook(ch) => ch.name(),
            #[cfg(feature = "email")]
            Self::Email(ch) => ch.name(),
        }
    }

    /// Get the channel type.
    pub fn channel_type(&self) -> ChannelType {
        match self {
            Self::Memory(_) => ChannelType::Memory,
            Self::Console(_) => ChannelType::Console,
            #[cfg(feature = "webhook")]
            Self::Webhook(_) => ChannelType::Webhook,
            #[cfg(feature = "email")]
            Self::Email(_) => ChannelType::Email,
        }
    }

    /// Check if the channel is enabled.
    pub fn is_enabled(&self) -> bool {
        match self {
            Self::Memory(ch) => ch.is_enabled(),
            Self::Console(ch) => ch.is_enabled(),
            #[cfg(feature = "webhook")]
            Self::Webhook(ch) => ch.is_enabled(),
            #[cfg(feature = "email")]
            Self::Email(ch) => ch.is_enabled(),
        }
    }

    /// Send an alert through this channel.
    pub async fn send(&self, alert: &Alert) -> Result<()> {
        match self {
            Self::Memory(ch) => ch.send(alert).await,
            Self::Console(ch) => ch.send(alert).await,
            #[cfg(feature = "webhook")]
            Self::Webhook(ch) => ch.send(alert).await,
            #[cfg(feature = "email")]
            Self::Email(ch) => ch.send(alert).await,
        }
    }
}

/// In-memory channel for testing.
#[derive(Debug, Clone)]
pub struct MemoryChannel {
    name: String,
    enabled: bool,
    alerts: std::sync::Arc<std::sync::Mutex<Vec<Alert>>>,
}

impl MemoryChannel {
    /// Create a new memory channel.
    pub fn new(name: String) -> Self {
        Self {
            name,
            enabled: true,
            alerts: std::sync::Arc::new(std::sync::Mutex::new(Vec::new())),
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

    /// Get the channel name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Send an alert.
    pub async fn send(&self, alert: &Alert) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
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

    /// Disable the channel.
    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    /// Get the channel name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Send an alert.
    pub async fn send(&self, alert: &Alert) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        println!("=== {} ===", alert.severity);
        println!("时间: {}", alert.timestamp.format("%Y-%m-%d %H:%M:%S UTC"));
        println!("标题: {}", alert.title);
        println!("消息: {}", alert.message);
        println!("来源: {}", alert.source);

        if self.include_details {
            if !alert.tags.is_empty() {
                println!("标签: {}", alert.tags.join(", "));
            }
            if alert.occurrence_count > 1 {
                println!("发生次数: {}", alert.occurrence_count);
            }
            println!("状态: {}", alert.status);
        }

        println!("================");

        Ok(())
    }
}

#[cfg(feature = "webhook")]
#[derive(Debug, Clone)]
pub struct WebhookChannel {
    name: String,
    enabled: bool,
    url: String,
    headers: HashMap<String, String>,
    client: reqwest::Client,
}

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

    /// Get the channel name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
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
        {}
        {}
    </div>
</body>
</html>"#,
            alert.severity.to_string().to_lowercase(),
            alert.title,
            alert.timestamp.format("%Y-%m-%d %H:%M:%S UTC"),
            alert.source,
            alert.message,
            if !alert.tags.is_empty() {
                format!("<p><strong>标签:</strong> {}</p>", alert.tags.join(", "))
            } else {
                String::new()
            },
            if alert.occurrence_count > 1 {
                format!("<p><strong>发生次数:</strong> {}</p>", alert.occurrence_count)
            } else {
                String::new()
            }
        )
    }

    /// Send an alert.
    pub async fn send(&self, alert: &Alert) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        if self.to_addresses.is_empty() {
            return Err(Error::SendError("No recipients configured".to_string()));
        }

        // Build email message
        let html_body = self.build_email_body(alert);
        let subject = format!("[{}] {}", alert.severity, alert.title);

        // Parse from address
        let from_mailbox: lettre::message::Mailbox = self.from_address.parse()
            .map_err(|e| Error::SendError(format!("Invalid from address: {}", e)))?;

        // Build email with recipients
        let mut email_builder = lettre::Message::builder()
            .from(from_mailbox.clone())
            .subject(subject);

        // Add all recipients
        for to_addr in &self.to_addresses {
            let mailbox: lettre::message::Mailbox = to_addr.parse()
                .map_err(|e| Error::SendError(format!("Invalid to address: {}", e)))?;
            email_builder = email_builder.to(mailbox);
        }

        // Build multipart email
        let email = email_builder
            .multipart(
                lettre::message::MultiPart::alternative()
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(lettre::message::header::ContentType::TEXT_PLAIN)
                            .body(format!("{}\n\n{}", alert.title, alert.message))
                    )
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(lettre::message::header::ContentType::TEXT_HTML)
                            .body(html_body)
                    )
            )
            .map_err(|e| Error::SendError(format!("Failed to build email: {}", e)))?;

        // Clone data for spawn_blocking
        let smtp_server = self.smtp_server.clone();
        let smtp_port = self.smtp_port;
        let username = self.username.clone();
        let password = self.password.clone();

        // Configure and send via tokio executor
        tokio::task::spawn_blocking(move || {
            // Configure SMTP transport
            let creds = lettre::transport::smtp::authentication::Credentials::new(
                username,
                password,
            );
            let relay = format!("{}:{}", smtp_server, smtp_port);
            let mailer = lettre::SmtpTransport::relay(&relay)
                .map_err(|e| Error::SendError(format!("Invalid SMTP server: {}", e)))?
                .credentials(creds)
                .build();

            // Send email
            lettre::Transport::send(&mailer, &email)
                .map_err(|e| Error::SendError(format!("Failed to send email: {}", e)))?;

            Ok::<(), Error>(())
        })
        .await
        .map_err(|e| Error::SendError(format!("Task join error: {}", e)))?
    }

    /// Send an email with attachments.
    pub async fn send_with_attachments(
        &self,
        subject: &str,
        body: &str,
        _attachments: Vec<EmailAttachment>,
    ) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        if self.to_addresses.is_empty() {
            return Err(Error::SendError("No recipients configured".to_string()));
        }

        // Parse from address
        let from_mailbox: lettre::message::Mailbox = self.from_address.parse()
            .map_err(|e| Error::SendError(format!("Invalid from address: {}", e)))?;

        // Build email with recipients
        let mut email_builder = lettre::Message::builder()
            .from(from_mailbox.clone())
            .subject(subject);

        // Add all recipients
        for to_addr in &self.to_addresses {
            let mailbox: lettre::message::Mailbox = to_addr.parse()
                .map_err(|e| Error::SendError(format!("Invalid to address: {}", e)))?;
            email_builder = email_builder.to(mailbox);
        }

        // Build email body (with attachments support - simplified for now)
        let email = email_builder
            .body(body.to_string())
            .map_err(|e| Error::SendError(format!("Failed to build email: {}", e)))?;

        // Clone data for spawn_blocking
        let smtp_server = self.smtp_server.clone();
        let smtp_port = self.smtp_port;
        let username = self.username.clone();
        let password = self.password.clone();

        // Configure and send via tokio executor
        tokio::task::spawn_blocking(move || {
            // Configure SMTP transport
            let creds = lettre::transport::smtp::authentication::Credentials::new(
                username,
                password,
            );
            let relay = format!("{}:{}", smtp_server, smtp_port);
            let mailer = lettre::SmtpTransport::relay(&relay)
                .map_err(|e| Error::SendError(format!("Invalid SMTP server: {}", e)))?
                .credentials(creds)
                .build();

            // Send email
            lettre::Transport::send(&mailer, &email)
                .map_err(|e| Error::SendError(format!("Failed to send email: {}", e)))?;

            Ok::<(), Error>(())
        })
        .await
        .map_err(|e| Error::SendError(format!("Task join error: {}", e)))?
    }
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

    /// Get the channel name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Check if enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Send an alert.
    pub async fn send(&self, alert: &Alert) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        let mut request = self.client.post(&self.url);

        for (key, value) in &self.headers {
            request = request.header(key, value);
        }

        let response = request
            .json(alert)
            .send()
            .await
            .map_err(|e| Error::SendError(format!("Webhook request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::SendError(format!(
                "Webhook returned error: {}",
                response.status()
            )));
        }

        Ok(())
    }
}

/// Channel registry for managing multiple channels.
#[derive(Debug, Clone)]
pub struct ChannelRegistry {
    channels: HashMap<String, NotificationChannel>,
}

impl ChannelRegistry {
    /// Create a new channel registry.
    pub fn new() -> Self {
        Self {
            channels: HashMap::new(),
        }
    }

    /// Add a channel to the registry.
    pub fn add_channel(&mut self, channel: NotificationChannel) {
        self.channels.insert(channel.name().to_string(), channel);
    }

    /// Remove a channel from the registry.
    pub fn remove_channel(&mut self, name: &str) -> bool {
        self.channels.remove(name).is_some()
    }

    /// Get a channel by name.
    pub fn get_channel(&self, name: &str) -> Option<&NotificationChannel> {
        self.channels.get(name)
    }

    /// List all channel names.
    pub fn list_channels(&self) -> Vec<String> {
        self.channels.keys().cloned().collect()
    }

    /// Send an alert to all enabled channels.
    pub async fn send_all(&self, alert: &Alert) -> Vec<(String, Result<()>)> {
        let mut results = Vec::new();

        for (name, channel) in &self.channels {
            if !channel.is_enabled() {
                continue;
            }

            let result = channel.send(alert).await;
            results.push((name.clone(), result));
        }

        results
    }

    /// Send an alert to a specific channel.
    pub async fn send_to(&self, name: &str, alert: &Alert) -> Result<()> {
        if let Some(channel) = self.get_channel(name) {
            channel.send(alert).await
        } else {
            Err(Error::NotFound(format!("Channel not found: {}", name)))
        }
    }
}

impl Default for ChannelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::alert::{Alert, AlertSeverity};

    #[tokio::test]
    async fn test_memory_channel() {
        let channel = MemoryChannel::new("test".to_string());

        let alert = Alert::new(
            AlertSeverity::Warning,
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
    async fn test_console_channel() {
        let channel = ConsoleChannel::new("console".to_string());

        let alert = Alert::new(
            AlertSeverity::Critical,
            "Critical Alert".to_string(),
            "Something bad happened".to_string(),
            "sensor_1".to_string(),
        );

        // Should not panic
        channel.send(&alert).await.unwrap();
    }

    #[tokio::test]
    async fn test_channel_registry() {
        let mut registry = ChannelRegistry::new();

        let channel1 = NotificationChannel::Memory(MemoryChannel::new("channel1".to_string()));
        let channel2 = NotificationChannel::Memory(MemoryChannel::new("channel2".to_string()));

        registry.add_channel(channel1);
        registry.add_channel(channel2);

        assert_eq!(registry.list_channels().len(), 2);

        let alert = Alert::new(
            AlertSeverity::Info,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
        );

        let results = registry.send_all(&alert).await;
        assert_eq!(results.len(), 2);
    }

    #[tokio::test]
    async fn test_send_to_specific_channel() {
        let mut registry = ChannelRegistry::new();

        let channel1 = NotificationChannel::Memory(MemoryChannel::new("channel1".to_string()));
        let channel2 = NotificationChannel::Memory(MemoryChannel::new("channel2".to_string()));

        registry.add_channel(channel1);
        registry.add_channel(channel2);

        let alert = Alert::new(
            AlertSeverity::Info,
            "Test".to_string(),
            "Test".to_string(),
            "test".to_string(),
        );

        registry.send_to("channel1", &alert).await.unwrap();

        // Verify the channel has the alert
        let ch1 = registry.get_channel("channel1").unwrap();
        if let NotificationChannel::Memory(mem_ch) = ch1 {
            assert_eq!(mem_ch.count(), 1);
        } else {
            panic!("Expected MemoryChannel");
        }

        let ch2 = registry.get_channel("channel2").unwrap();
        if let NotificationChannel::Memory(mem_ch) = ch2 {
            assert_eq!(mem_ch.count(), 0);
        } else {
            panic!("Expected MemoryChannel");
        }
    }
}
