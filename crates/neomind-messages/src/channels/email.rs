//! Email notification channel.

#[cfg(feature = "email")]
use async_trait::async_trait;

#[cfg(feature = "email")]
use std::sync::Arc;

#[cfg(feature = "email")]
use super::super::{Error, Message, Result};
#[cfg(feature = "email")]
use super::MessageChannel;

/// Escape HTML special characters to prevent XSS and display issues
#[cfg(feature = "email")]
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

/// Wrapper for SmtpTransport that implements Debug and Clone.
#[cfg(feature = "email")]
#[derive(Clone)]
struct SmtpTransportHandle {
    inner: Arc<std::sync::Mutex<lettre::SmtpTransport>>,
}

#[cfg(feature = "email")]
impl std::fmt::Debug for SmtpTransportHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmtpTransportHandle").finish()
    }
}

/// How a TLS SMTP session is established. Port 465 speaks TLS from the first
/// byte (implicit TLS / SMTPS); 587 and 25 start plaintext and upgrade via
/// STARTTLS. lettre models these as `Tls::Wrapper` and `Tls::Required`
/// respectively — this used to hardcode `Required`, which made every 465
/// server (smtp.qq.com, smtp.163.com, Gmail SSL) fail the handshake while
/// waiting for a plaintext greeting that never comes.
#[cfg(feature = "email")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SmtpTlsMode {
    /// Implicit TLS (`Tls::Wrapper`) — port 465 convention.
    ImplicitTls,
    /// STARTTLS (`Tls::Required`) — ports 587/25 convention.
    StartTls,
}

#[cfg(feature = "email")]
fn tls_mode_for_port(smtp_port: u16) -> SmtpTlsMode {
    if smtp_port == 465 {
        SmtpTlsMode::ImplicitTls
    } else {
        SmtpTlsMode::StartTls
    }
}

/// Cap on any single SMTP exchange (connect, handshake, DATA). lettre has no
/// default timeout, and send() runs on a blocking thread awaited by the
/// delivery loop — without this a hung server stalls every alert behind it.
#[cfg(feature = "email")]
const SMTP_TIMEOUT_SECS: u64 = 30;

/// Build the SMTP transport from config.
#[cfg(feature = "email")]
fn build_smtp_transport(
    smtp_server: &str,
    smtp_port: u16,
    username: String,
    password: String,
    use_tls: bool,
) -> std::result::Result<lettre::SmtpTransport, Error> {
    let creds = lettre::transport::smtp::authentication::Credentials::new(username, password);

    if use_tls {
        use lettre::transport::smtp::client::Tls;
        use lettre::transport::smtp::client::TlsParametersBuilder;

        let tls_params = TlsParametersBuilder::new(smtp_server.to_string())
            .build()
            .map_err(|e| Error::SendFailed(format!("Failed to build TLS params: {}", e)))?;

        let tls = match tls_mode_for_port(smtp_port) {
            SmtpTlsMode::ImplicitTls => Tls::Wrapper(tls_params),
            SmtpTlsMode::StartTls => Tls::Required(tls_params),
        };

        Ok(lettre::SmtpTransport::relay(smtp_server)
            .map_err(|e| Error::SendFailed(format!("Invalid SMTP server: {}", e)))?
            .port(smtp_port)
            .tls(tls)
            .credentials(creds)
            .timeout(Some(std::time::Duration::from_secs(SMTP_TIMEOUT_SECS)))
            .build())
    } else {
        Ok(lettre::SmtpTransport::builder_dangerous(smtp_server)
            .port(smtp_port)
            .credentials(creds)
            .timeout(Some(std::time::Duration::from_secs(SMTP_TIMEOUT_SECS)))
            .build())
    }
}

/// Email channel for sending messages via SMTP.
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
    /// Cached SMTP transport for reuse.
    transport: Option<SmtpTransportHandle>,
}

#[cfg(feature = "email")]
impl EmailChannel {
    pub fn new(
        name: String,
        smtp_server: String,
        smtp_port: u16,
        username: String,
        password: String,
        from_address: String,
    ) -> Self {
        let transport = build_smtp_transport(
            &smtp_server,
            smtp_port,
            username.clone(),
            password.clone(),
            true,
        )
        .ok()
        .map(|t| SmtpTransportHandle {
            inner: Arc::new(std::sync::Mutex::new(t)),
        });

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
            transport,
        }
    }

    pub fn add_recipient(mut self, address: String) -> Self {
        self.to_addresses.push(address);
        self
    }

    pub fn with_recipients(mut self, addresses: Vec<String>) -> Self {
        self.to_addresses = addresses;
        self
    }

    /// Set recipients (for updating after creation)
    pub fn set_recipients_internal(&mut self, addresses: Vec<String>) {
        self.to_addresses = addresses;
    }

    /// Get current recipients
    pub fn get_recipients_internal(&self) -> &[String] {
        &self.to_addresses
    }

    pub fn without_tls(mut self) -> Self {
        self.use_tls = false;
        self.transport = build_smtp_transport(
            &self.smtp_server,
            self.smtp_port,
            self.username.clone(),
            self.password.clone(),
            false,
        )
        .ok()
        .map(|t| SmtpTransportHandle {
            inner: Arc::new(std::sync::Mutex::new(t)),
        });
        self
    }

    pub fn disabled(mut self) -> Self {
        self.enabled = false;
        self
    }

    pub fn enable(&mut self) {
        self.enabled = true;
    }

    pub fn disable(&mut self) {
        self.enabled = false;
    }

    fn build_email_body(&self, message: &Message) -> String {
        // Build message content section
        let message_content = html_escape(&message.message);

        // Severity colors using orange accent theme
        let (severity_color, severity_bg, severity_border) = match message.severity {
            super::super::MessageSeverity::Info => ("#28a745", "#e8f5e9", "#28a745"),
            super::super::MessageSeverity::Warning => ("#e67e22", "#fff3e0", "#e67e22"),
            super::super::MessageSeverity::Critical => ("#e74c3c", "#ffebee", "#e74c3c"),
            super::super::MessageSeverity::Emergency => ("#c0392b", "#fce4ec", "#c0392b"),
        };

        format!(
            r#"<!DOCTYPE html>
<html>
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{ font-family: Arial, Helvetica, sans-serif; background-color: #f8f9fa; color: #333; line-height: 1.6; }}
        .email-container {{ max-width: 600px; margin: 0 auto; background-color: #ffffff; border-radius: 8px; overflow: hidden; box-shadow: 0 2px 8px rgba(0,0,0,0.1); }}
        .header {{ background-color: #1a1a1a; color: #ffffff; padding: 24px 32px; text-align: center; }}
        .header h1 {{ font-size: 24px; font-weight: bold; margin-bottom: 4px; }}
        .header p {{ font-size: 12px; color: #888; letter-spacing: 2px; text-transform: uppercase; }}
        .content {{ padding: 32px; }}
        .message-card {{ background-color: {}; border-left: 4px solid {}; border-radius: 6px; padding: 24px; margin-bottom: 24px; }}
        .message-header {{ display: flex; align-items: center; justify-content: space-between; margin-bottom: 16px; flex-wrap: wrap; gap: 12px; }}
        .message-title {{ font-size: 20px; font-weight: bold; color: #1a1a1a; flex: 1; }}
        .severity-badge {{ display: inline-block; padding: 4px 12px; border-radius: 4px; font-size: 12px; font-weight: bold; background-color: {}; color: #ffffff; }}
        .meta-info {{ margin-bottom: 16px; }}
        .meta-row {{ display: flex; margin-bottom: 8px; font-size: 14px; }}
        .meta-label {{ color: #888; width: 80px; flex-shrink: 0; }}
        .meta-value {{ color: #333; font-weight: 500; }}
        .message-body {{ font-size: 15px; color: #555; line-height: 1.7; padding: 16px; background-color: #ffffff; border-radius: 4px; }}
        .footer {{ background-color: #1a1a1a; color: #888; padding: 24px 32px; text-align: center; }}
        .footer p {{ font-size: 13px; margin-bottom: 8px; }}
        .footer a {{ color: #e67e22; text-decoration: none; }}
        .divider {{ height: 1px; background-color: #eee; margin: 16px 0; }}
    </style>
</head>
<body style="background-color: #f8f9fa; padding: 20px;">
    <div class="email-container">
        <div class="header">
            <h1>NeoMind</h1>
            <p>Edge AI Platform</p>
        </div>
        <div class="content">
            <div class="message-card">
                <div class="message-header">
                    <span class="message-title">{}</span>
                </div>
                <div style="margin-bottom: 12px;">
                    <span class="severity-badge" style="background-color: {};">{}</span>
                </div>
                <div class="meta-info">
                    <div class="meta-row">
                        <span class="meta-label">Time:</span>
                        <span class="meta-value">{}</span>
                    </div>
                    <div class="meta-row">
                        <span class="meta-label">Source:</span>
                        <span class="meta-value">{}</span>
                    </div>
                </div>
                <div class="divider"></div>
                <div class="message-body">
                    {}
                </div>
            </div>
        </div>
        <div class="footer">
            <p>This email was automatically sent by NeoMind Platform</p>
            <p>© 2024 NeoMind Edge AI Platform. All rights reserved.</p>
        </div>
    </div>
</body>
</html>"#,
            severity_bg,
            severity_border,
            severity_color,
            // [html-injection] title and source carry user-controlled strings
            // (rule names, device names) and used to be interpolated raw —
            // a name containing <img onerror=...> rendered as live HTML in
            // recipients' clients. Only severity/timestamp are trusted here.
            html_escape(&message.title),
            severity_color,
            message.severity.as_str(),
            message.timestamp.format("%Y-%m-%d %H:%M:%S"),
            html_escape(&message.source),
            message_content,
        )
    }
}

#[cfg(feature = "email")]
#[async_trait]
impl MessageChannel for EmailChannel {
    fn name(&self) -> &str {
        &self.name
    }

    fn channel_type(&self) -> &str {
        "email"
    }

    fn is_enabled(&self) -> bool {
        self.enabled
    }

    async fn send(&self, message: &Message) -> Result<()> {
        if !self.enabled {
            return Err(Error::ChannelDisabled(self.name.clone()));
        }

        if self.to_addresses.is_empty() {
            return Err(Error::SendFailed("No recipients configured".to_string()));
        }

        let html_body = self.build_email_body(message);
        let subject = format!("[{}] {}", message.severity, message.title);

        let from_mailbox: lettre::message::Mailbox = self
            .from_address
            .parse()
            .map_err(|e| Error::InvalidConfiguration(format!("Invalid from address: {}", e)))?;

        let mut email_builder = lettre::Message::builder()
            .from(from_mailbox.clone())
            .subject(subject);

        for to_addr in &self.to_addresses {
            let mailbox: lettre::message::Mailbox = to_addr
                .parse()
                .map_err(|e| Error::InvalidConfiguration(format!("Invalid to address: {}", e)))?;
            email_builder = email_builder.to(mailbox);
        }

        let email = email_builder
            .multipart(
                lettre::message::MultiPart::alternative()
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(lettre::message::header::ContentType::TEXT_PLAIN)
                            .body(format!("{}\n\n{}", message.title, message.message)),
                    )
                    .singlepart(
                        lettre::message::SinglePart::builder()
                            .header(lettre::message::header::ContentType::TEXT_HTML)
                            .body(html_body),
                    ),
            )
            .map_err(|e| Error::SendFailed(format!("Failed to build email: {}", e)))?;

        let smtp_server = self.smtp_server.clone();
        let smtp_port = self.smtp_port;
        let username = self.username.clone();
        let password = self.password.clone();
        let use_tls = self.use_tls;
        let transport = self.transport.clone();

        tokio::task::spawn_blocking(move || {
            if let Some(handle) = transport {
                // Reuse cached transport
                let guard = handle.inner.lock().unwrap();
                lettre::Transport::send(&*guard, &email)
                    .map_err(|e| Error::SendFailed(format!("Failed to send email: {}", e)))?;
            } else {
                // Fallback: build transport on-the-fly (should not normally happen)
                let mailer =
                    build_smtp_transport(&smtp_server, smtp_port, username, password, use_tls)?;
                lettre::Transport::send(&mailer, &email)
                    .map_err(|e| Error::SendFailed(format!("Failed to send email: {}", e)))?;
            }

            Ok::<(), Error>(())
        })
        .await
        .map_err(|e| Error::SendFailed(format!("Task join error: {}", e)))?
    }

    fn set_recipients(&mut self, recipients: Vec<String>) {
        self.to_addresses = recipients;
    }

    fn get_recipients(&self) -> Vec<String> {
        self.to_addresses.clone()
    }
}

/// Factory for creating email channels.
#[cfg(feature = "email")]
pub struct EmailChannelFactory;

#[cfg(feature = "email")]
impl super::ChannelFactory for EmailChannelFactory {
    fn channel_type(&self) -> &str {
        "email"
    }

    fn create(&self, config: &serde_json::Value) -> Result<std::sync::Arc<dyn MessageChannel>> {
        let smtp_server = config
            .get("smtp_server")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing smtp_server".to_string()))?;

        // Support both number and string types for smtp_port
        let smtp_port = config
            .get("smtp_port")
            .and_then(|v| {
                if let Some(n) = v.as_u64() {
                    Some(n)
                } else if let Some(s) = v.as_str() {
                    s.parse::<u64>().ok()
                } else {
                    None
                }
            })
            .ok_or_else(|| {
                Error::InvalidConfiguration("Missing or invalid smtp_port".to_string())
            })? as u16;

        let username = config
            .get("username")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing username".to_string()))?
            .to_string();

        let password = config
            .get("password")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing password".to_string()))?
            .to_string();

        let from_address = config
            .get("from_address")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::InvalidConfiguration("Missing from_address".to_string()))?
            .to_string();

        let name = config
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("email")
            .to_string();

        let mut channel = EmailChannel::new(
            name,
            smtp_server.to_string(),
            smtp_port,
            username,
            password,
            from_address,
        );

        if let Some(recipients) = config.get("recipients") {
            if let Some(arr) = recipients.as_array() {
                for addr in arr {
                    if let Some(str_addr) = addr.as_str() {
                        channel = channel.add_recipient(str_addr.to_string());
                    }
                }
            }
        }

        if !config
            .get("use_tls")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            channel = channel.without_tls();
        }

        if !config
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(true)
        {
            channel = channel.disabled();
        }

        Ok(std::sync::Arc::new(channel))
    }
}

#[cfg(feature = "email")]
#[cfg(test)]
mod tests {
    use super::*;

    /// 465 is the implicit-TLS (SMTPS) convention and must map to
    /// `Tls::Wrapper`; 587/25 use STARTTLS and must map to `Tls::Required`.
    /// Sending STARTTLS to a 465 endpoint waits for a plaintext greeting
    /// that never arrives — every QQ/163-style mail config used to fail.
    #[test]
    fn test_tls_mode_by_port() {
        assert_eq!(tls_mode_for_port(465), SmtpTlsMode::ImplicitTls);
        assert_eq!(tls_mode_for_port(587), SmtpTlsMode::StartTls);
        assert_eq!(tls_mode_for_port(25), SmtpTlsMode::StartTls);
    }
}
