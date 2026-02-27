//! Utility functions for extension development

/// Get current timestamp in milliseconds
pub fn current_timestamp_ms() -> i64 {
    #[cfg(not(target_arch = "wasm32"))]
    {
        chrono::Utc::now().timestamp_millis()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // For WASM, we'll use a simple approach
        // In practice, this would call a host function
        0
    }
}

/// Get current timestamp in seconds
pub fn current_timestamp_secs() -> i64 {
    current_timestamp_ms() / 1000
}

/// Format bytes as human-readable size
pub fn format_bytes(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1.00 KB");
        assert_eq!(format_bytes(1024 * 1024), "1.00 MB");
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration_ms(500), "500ms");
        assert_eq!(format_duration_ms(1000), "1.0s");
        assert_eq!(format_duration_ms(60000), "1.0m");
    }
}
