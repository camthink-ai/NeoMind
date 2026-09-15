//! Shared time parsing utilities and trait definitions.

use async_trait::async_trait;
use serde_json::Value;

/// Parse relative time range string to seconds.
/// Supports: "30min", "1h", "6h", "1d", "3d", "1w", "2w", "1m"/"1mo" (month), "3m"
pub fn parse_time_range(input: &str) -> Option<i64> {
    let s = input.trim().to_lowercase();
    let (num, unit) = if let Some(rest) = s.strip_suffix("min") {
        (rest.parse::<i64>().ok()?, "min")
    } else if let Some(rest) = s.strip_suffix('h') {
        (rest.parse::<i64>().ok()?, "h")
    } else if let Some(rest) = s.strip_suffix('d') {
        (rest.parse::<i64>().ok()?, "d")
    } else if let Some(rest) = s.strip_suffix('w') {
        (rest.parse::<i64>().ok()?, "w")
    } else if let Some(rest) = s.strip_suffix("mo") {
        (rest.parse::<i64>().ok()?, "mo") // explicit month suffix
    } else {
        let rest = s.strip_suffix('m')?;
        (rest.parse::<i64>().ok()?, "m") // m = months (use min for minutes)
    };
    Some(match unit {
        "min" => num * 60,
        "h" => num * 3600,
        "d" => num * 86400,
        "w" => num * 7 * 86400,
        "m" => num * 30 * 86400,  // m = months
        "mo" => num * 30 * 86400, // mo = months (explicit)
        _ => return None,
    })
}

/// Trait for transform storage operations.
///
/// Used by API-level tools that need to interact with the automation store
/// for transform-specific operations.
#[async_trait]
pub trait TransformStore: Send + Sync {
    /// Save a transform (create or update). Takes serde_json::Value representing the full Automation.
    async fn save_transform(&self, data: Value) -> std::result::Result<String, String>;
    /// Get a transform by ID. Returns None if not found, Err if not a transform.
    async fn get_transform(&self, id: &str) -> std::result::Result<Option<Value>, String>;
    /// List all transforms.
    async fn list_transforms(&self) -> std::result::Result<Vec<Value>, String>;
    /// Delete a transform by ID. Returns false if not found, Err if not a transform.
    async fn delete_transform(&self, id: &str) -> std::result::Result<bool, String>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_time_range() {
        assert_eq!(parse_time_range("30min"), Some(30 * 60));
        assert_eq!(parse_time_range("1h"), Some(3600));
        assert_eq!(parse_time_range("6h"), Some(6 * 3600));
        assert_eq!(parse_time_range("1d"), Some(86400));
        assert_eq!(parse_time_range("3d"), Some(3 * 86400));
        assert_eq!(parse_time_range("1w"), Some(7 * 86400));
        assert_eq!(parse_time_range("2w"), Some(14 * 86400));
        assert_eq!(parse_time_range("1m"), Some(30 * 86400));
        assert_eq!(parse_time_range("1mo"), Some(30 * 86400));
        assert_eq!(parse_time_range("3m"), Some(90 * 86400));
        assert_eq!(parse_time_range("invalid"), None);
    }
}
