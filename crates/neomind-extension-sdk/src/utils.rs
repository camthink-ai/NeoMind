//! Utility functions for extension development
//!
//! This module provides WASM-compatible utility functions that work
//! seamlessly across both Native and WASM targets.

// ============================================================================
// Timestamp Functions
// ============================================================================

/// Get current timestamp in milliseconds since Unix epoch
///
/// This function is WASM-compatible and uses the appropriate method
/// for each target:
/// - Native: Uses `chrono::Utc::now().timestamp_millis()`
/// - WASM: Uses `js_sys::Date::now()`
pub fn current_timestamp_ms() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Utc::now().timestamp_millis()
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as i64
    }
}

/// Get current timestamp in seconds since Unix epoch
pub fn current_timestamp_secs() -> i64 {
    current_timestamp_ms() / 1000
}

/// Get current timestamp as ISO 8601 string
///
/// Returns a string like "2024-01-15T10:30:00.000Z"
pub fn current_timestamp_iso() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Utc::now().to_rfc3339()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // Use js_sys::Date for WASM
        let date = js_sys::Date::new_0();
        date.to_iso_string().as_string().unwrap_or_default()
    }
}

// ============================================================================
// Async Helpers
// ============================================================================

/// Block on an async future, compatible with both Native and WASM
///
/// This is a convenience wrapper around the appropriate blocking executor:
/// - Native: Uses tokio runtime
/// - WASM: Uses pollster
///
/// # Note
/// For WASM, this uses a single-threaded executor (pollster).
/// The future should not spawn additional tasks or use tokio-specific features.
///
/// # Example
/// ```rust,ignore
/// use neomind_extension_sdk::utils::block_on;
///
/// let result = block_on(async {
///     // Your async code here
///     42
/// });
/// assert_eq!(result, 42);
/// ```
pub fn block_on<F: std::future::Future>(future: F) -> F::Output {
    #[cfg(not(target_arch = "wasm32"))]
    {
        tokio::runtime::Runtime::new()
            .expect("Failed to create tokio runtime")
            .block_on(future)
    }
    #[cfg(target_arch = "wasm32")]
    {
        pollster::block_on(future)
    }
}

// ============================================================================
// Random Number Generation
// ============================================================================

/// Generate a random UUID v4
///
/// This function is WASM-compatible and uses the appropriate method
/// for each target.
pub fn random_uuid() -> String {
    #[cfg(not(target_arch = "wasm32"))]
    {
        uuid::Uuid::new_v4().to_string()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // Use Web Crypto API for WASM
        let array = js_sys::Uint8Array::new_with_length(16);
        js_sys::Reflect::get(&js_sys::global(), &js_sys::JsString::from("crypto"))
            .ok()
            .and_then(|crypto| {
                js_sys::Reflect::get(&crypto, &js_sys::JsString::from("getRandomValues"))
                    .ok()
                    .and_then(|func| {
                        let func: js_sys::Function = func.into();
                        func.call1(&crypto, &array).ok()
                    })
            })
            .map(|_| {
                // Format as UUID v4
                let bytes = array.to_vec();
                format!(
                    "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
                    bytes[0], bytes[1], bytes[2], bytes[3],
                    bytes[4], bytes[5],
                    bytes[6], bytes[7],
                    bytes[8], bytes[9],
                    bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15]
                )
            })
            .unwrap_or_else(|| {
                // Fallback: generate a simple random string
                format!("{}-{}-{}-{}-{}",
                    random_u32(),
                    random_u32(),
                    random_u32(),
                    random_u32(),
                    random_u32()
                )
            })
    }
}

/// Generate a random u32
///
/// This function is WASM-compatible.
pub fn random_u32() -> u32 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rand::random()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // Use Math.random() for WASM (not cryptographically secure, but works)
        (js_sys::Math::random() * u32::MAX as f64) as u32
    }
}

/// Generate a random u64
pub fn random_u64() -> u64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rand::random()
    }
    #[cfg(target_arch = "wasm32")]
    {
        ((js_sys::Math::random() * u32::MAX as f64) as u64) << 32
            | (js_sys::Math::random() * u32::MAX as f64) as u64
    }
}

/// Generate a random f64 between 0.0 and 1.0
pub fn random_f64() -> f64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rand::random()
    }
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Math::random()
    }
}

// ============================================================================
// Formatting Utilities
// ============================================================================

/// Format bytes as human-readable size
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;
    const TB: u64 = GB * 1024;

    if bytes >= TB {
        format!("{:.2} TB", bytes as f64 / TB as f64)
    } else if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} B", bytes)
    }
}

/// Format duration in milliseconds to human-readable string
pub fn format_duration_ms(ms: u64) -> String {
    const SECOND: u64 = 1000;
    const MINUTE: u64 = SECOND * 60;
    const HOUR: u64 = MINUTE * 60;
    const DAY: u64 = HOUR * 24;

    if ms >= DAY {
        format!("{:.1}d", ms as f64 / DAY as f64)
    } else if ms >= HOUR {
        format!("{:.1}h", ms as f64 / HOUR as f64)
    } else if ms >= MINUTE {
        format!("{:.1}m", ms as f64 / MINUTE as f64)
    } else if ms >= SECOND {
        format!("{:.1}s", ms as f64 / SECOND as f64)
    } else {
        format!("{}ms", ms)
    }
}

/// Format a number with thousand separators
pub fn format_number(n: i64) -> String {
    let s = n.to_string();
    let mut result = String::new();
    let chars: Vec<char> = s.chars().collect();
    for (i, c) in chars.iter().enumerate() {
        if i > 0 && (chars.len() - i) % 3 == 0 {
            result.push(',');
        }
        result.push(*c);
    }
    result
}

// ============================================================================
// String Utilities
// ============================================================================

/// Truncate a string to a maximum length, adding "..." if truncated
pub fn truncate_string(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else if max_len <= 3 {
        "...".to_string()
    } else {
        format!("{}...", &s[..max_len - 3])
    }
}

/// Check if a string is a valid identifier (alphanumeric and underscores)
pub fn is_valid_identifier(s: &str) -> bool {
    !s.is_empty()
        && s.chars().next().map(|c| c.is_alphabetic() || c == '_').unwrap_or(false)
        && s.chars().all(|c| c.is_alphanumeric() || c == '_')
}

// ============================================================================
// Sleep Functions
// ============================================================================

/// Sleep for a specified duration in milliseconds
///
/// # Note
/// For WASM, this is a blocking sleep using a busy-wait loop.
/// For better performance in WASM, consider using async patterns instead.
pub fn sleep_ms(ms: u64) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    #[cfg(target_arch = "wasm32")]
    {
        // WASM doesn't support blocking sleep
        // Use async patterns instead for WASM
        let start = current_timestamp_ms();
        while (current_timestamp_ms() - start) < ms as i64 {
            // Busy wait - not ideal but works for short durations
            // In production, use async/await patterns
        }
    }
}

// ============================================================================
// Environment Detection
// ============================================================================

/// Check if running in WASM environment
pub fn is_wasm() -> bool {
    cfg!(target_arch = "wasm32")
}

/// Check if running in native environment
pub fn is_native() -> bool {
    !cfg!(target_arch = "wasm32")
}

/// Get the current target architecture name
pub fn target_arch() -> &'static str {
    #[cfg(target_arch = "wasm32")]
    { "wasm32" }
    #[cfg(target_arch = "x86_64")]
    { "x86_64" }
    #[cfg(target_arch = "aarch64")]
    { "aarch64" }
    #[cfg(target_arch = "arm")]
    { "arm" }
    #[cfg(not(any(target_arch = "wasm32", target_arch = "x86_64", target_arch = "aarch64", target_arch = "arm")))]
    { "unknown" }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
        assert_eq!(format_bytes(1024 * 1024 * 1024), "1.00 GB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(1000), "1.0s");
        assert_eq!(format_duration_ms(60000), "1.0m");
        assert_eq!(format_duration_ms(3600000), "1.0h");
    }

    #[test]
    fn test_format_number() {
        assert_eq!(format_number(0), "0");
        assert_eq!(format_number(100), "100");
        assert_eq!(format_number(1000), "1,000");
        assert_eq!(format_number(1000000), "1,000,000");
    }

    #[test]
    fn test_truncate_string() {
        assert_eq!(truncate_string("hello", 10), "hello");
        assert_eq!(truncate_string("hello world", 8), "hello...");
        assert_eq!(truncate_string("hi", 2), "hi");
    }

    #[test]
    fn test_is_valid_identifier() {
        assert!(is_valid_identifier("hello"));
        assert!(is_valid_identifier("_hello"));
        assert!(is_valid_identifier("hello_world"));
        assert!(is_valid_identifier("hello123"));
        assert!(!is_valid_identifier(""));
        assert!(!is_valid_identifier("123hello"));
        assert!(!is_valid_identifier("hello-world"));
    }

    #[test]
    fn test_timestamp() {
        let ts = current_timestamp_ms();
        assert!(ts > 0);
        assert!(ts > 1700000000000); // After 2023
    }

    #[test]
    fn test_random() {
        let r1 = random_u32();
        let r2 = random_u32();
        // Very unlikely to be equal
        assert_ne!(r1, r2);
    }

    #[test]
    fn test_environment() {
        #[cfg(target_arch = "wasm32")]
        {
            assert!(is_wasm());
            assert!(!is_native());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            assert!(!is_wasm());
            assert!(is_native());
        }
    }
}