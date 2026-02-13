//! Hex analyzer for detecting and interpreting hexadecimal data.
//!
//! This module provides heuristic-based detection of hex-encoded values
//! and attempts to interpret them meaningfully.

use serde_json::Value;
use std::collections::HashSet;

/// Field name hints that suggest hex-encoded data
pub const FIELD_HEX_HINTS: &[&str] = &[
    "raw",
    "hex",
    "payload",
    "data",
    "packet",
    "buffer",
    "bytes",
    "binary",
    "state",
    "status",
    "register",
    "value_raw",
    "original",
    "encoded",
    "frame",
    "message",
    "command",
    "response",
];

/// Probability level that a value is hex-encoded
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum HexProbability {
    /// Unlikely to be hex - treat as regular value
    Unlikely = 0,
    /// Possibly hex - flag for manual review
    Possible = 1,
    /// Likely hex - provide dual interpretation
    Likely = 2,
}

impl HexProbability {
    /// Get numeric score
    pub fn score(&self) -> u8 {
        match self {
            HexProbability::Unlikely => 0,
            HexProbability::Possible => 50,
            HexProbability::Likely => 80,
        }
    }
}

/// Information about a hex-encoded value
#[derive(Debug, Clone)]
pub struct HexInfo {
    /// Whether this is detected as hex
    pub is_hex: bool,
    /// Original hex string
    pub original_hex: String,
    /// Decoded integer value (if applicable)
    pub decoded_integer: Option<u64>,
    /// Decoded bytes interpretation
    pub as_bytes: Option<Vec<u8>>,
    /// Display hint for UI
    pub display_hint: String,
    /// Probability score
    pub probability: HexProbability,
}

/// Metric interpretation with hex information
#[derive(Debug, Clone)]
pub struct MetricInterpretation {
    /// Original path
    pub path: String,
    /// Original value
    pub original_value: Value,
    /// Hex analysis result
    pub hex_info: Option<HexInfo>,
    /// Suggested data type
    pub suggested_type: SuggestedType,
    /// Suggested display name
    pub suggested_name: Option<String>,
}

/// Suggested type for the metric
#[derive(Debug, Clone)]
pub enum SuggestedType {
    /// Regular integer
    Integer,
    /// Regular float
    Float,
    /// Hex-encoded integer
    HexInteger { decoded: u64 },
    /// Raw hex data (bytes)
    HexBytes,
    /// Boolean
    Boolean,
    /// String
    String,
    /// Unknown
    Unknown,
}

/// Hex analyzer for detecting and interpreting hex values
pub struct HexAnalyzer {
    /// Hex patterns to look for
    hex_patterns: HashSet<String>,
}

impl HexAnalyzer {
    /// Create a new hex analyzer
    pub fn new() -> Self {
        let mut hex_patterns = HashSet::new();
        for hint in FIELD_HEX_HINTS {
            hex_patterns.insert(hint.to_string());
        }

        Self { hex_patterns }
    }

    /// Analyze a single value for hex characteristics
    pub fn analyze_value(&self, path: &str, value: &Value) -> HexInfo {
        let original_hex: String = match value {
            Value::String(s) => s.clone(),
            Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    // Check if it looks like a hex number (negative could be two's complement)
                    if i > 255 {
                        format!("{:X}", i)
                    } else {
                        return HexInfo {
                            is_hex: false,
                            original_hex: String::new(),
                            decoded_integer: None,
                            as_bytes: None,
                            display_hint: format!("{}", n),
                            probability: HexProbability::Unlikely,
                        };
                    }
                } else if let Some(f) = n.as_f64() {
                    // Floats are not hex
                    return HexInfo {
                        is_hex: false,
                        original_hex: String::new(),
                        decoded_integer: None,
                        as_bytes: None,
                        display_hint: format!("{}", f),
                        probability: HexProbability::Unlikely,
                    };
                } else {
                    // Number but not i64 or f64 (shouldn't happen)
                    return HexInfo {
                        is_hex: false,
                        original_hex: String::new(),
                        decoded_integer: None,
                        as_bytes: None,
                        display_hint: format!("{:?}", value),
                        probability: HexProbability::Unlikely,
                    };
                }
            }
            _ => {
                return HexInfo {
                    is_hex: false,
                    original_hex: String::new(),
                    decoded_integer: None,
                    as_bytes: None,
                    display_hint: format!("{:?}", value),
                    probability: HexProbability::Unlikely,
                };
            }
        };

        let probability = self.assess_probability(&original_hex, path);

        if probability == HexProbability::Unlikely {
            let display_hint = original_hex.clone();
            return HexInfo {
                is_hex: false,
                original_hex,
                decoded_integer: None,
                as_bytes: None,
                display_hint,
                probability,
            };
        }

        // Try to interpret the hex
        let (decoded_integer, as_bytes, display_hint) = self.interpret_hex(&original_hex);

        HexInfo {
            is_hex: true,
            original_hex,
            decoded_integer,
            as_bytes,
            display_hint,
            probability,
        }
    }

    /// Assess probability that a string is hex-encoded
    fn assess_probability(&self, value: &str, path: &str) -> HexProbability {
        if value.is_empty() {
            return HexProbability::Unlikely;
        }

        let mut score = 0;

        // 1. Field name hint (up to 40 points)
        let path_lower = path.to_lowercase();
        for hint in &self.hex_patterns {
            if path_lower.contains(hint) {
                score += 40;
                break;
            }
        }

        // 2. Format checks (up to 40 points)
        let chars: Vec<char> = value.chars().collect();
        let valid_hex_count = chars
            .iter()
            .filter(|c: &&char| c.is_ascii_hexdigit())
            .count();

        if valid_hex_count == chars.len() {
            score += 30; // All valid hex chars
        } else if valid_hex_count as f64 / chars.len() as f64 > 0.9 {
            score += 15; // Mostly valid hex chars
        }

        // 3. Length characteristics (up to 20 points)
        let len = value.len();
        if len >= 4 && len.is_multiple_of(2) {
            score += 10; // Even length, typical for hex
        }
        if len >= 8 {
            score += 5; // Longer values more likely to be hex-encoded
        }

        // 4. Pattern checks (up to 10 points)
        if value.starts_with("0x") || value.starts_with("0X") {
            score += 15; // Explicit hex prefix
        }

        // 5. Value range check
        if let Ok(int_val) = u64::from_str_radix(value, 16) {
            // If it decodes to a reasonable integer value
            if int_val > 255 && int_val < 1_000_000_000 {
                score += 10; // In the "plausible integer range"
            }
        }

        // Convert score to probability
        match score {
            ..0 => HexProbability::Unlikely,
            0..40 => HexProbability::Unlikely,
            40..70 => HexProbability::Possible,
            70.. => HexProbability::Likely,
        }
    }

    /// Interpret a hex string to extract meaning
    fn interpret_hex(&self, hex_str: &str) -> (Option<u64>, Option<Vec<u8>>, String) {
        // Remove 0x prefix if present
        let clean_hex = hex_str.trim_start_matches("0x").trim_start_matches("0X");

        // Try to decode as integer
        let decoded_integer = u64::from_str_radix(clean_hex, 16).ok();

        // Try to decode as bytes
        let as_bytes = if clean_hex.len().is_multiple_of(2) {
            hex_to_bytes(clean_hex)
        } else {
            None
        };

        // Build display hint
        let display_hint = if let Some(int_val) = decoded_integer {
            format!("0x{} ({})", clean_hex, int_val)
        } else if let Some(bytes) = &as_bytes {
            format!("0x{} ({} bytes)", clean_hex, bytes.len())
        } else {
            format!("0x{} (invalid hex)", clean_hex)
        };

        (decoded_integer, as_bytes, display_hint)
    }

    /// Analyze a metric and provide interpretation
    pub fn interpret_metric(
        &self,
        path: &str,
        value: &Value,
        stats: &ValueStats,
    ) -> MetricInterpretation {
        let hex_info = self.analyze_value(path, value);

        let suggested_type = if hex_info.is_hex {
            if let Some(int_val) = hex_info.decoded_integer {
                // Check if decoded value is reasonable
                if int_val < 100_000
                    && !stats
                        .unit_hint
                        .as_ref()
                        .map(|u| u.contains("°C"))
                        .unwrap_or(false)
                {
                    SuggestedType::HexInteger { decoded: int_val }
                } else {
                    SuggestedType::HexBytes
                }
            } else {
                SuggestedType::HexBytes
            }
        } else {
            match value {
                Value::Number(n) => {
                    if n.is_i64() || n.is_u64() {
                        SuggestedType::Integer
                    } else {
                        SuggestedType::Float
                    }
                }
                Value::Bool(_) => SuggestedType::Boolean,
                Value::String(_s) => {
                    if stats.boolean_like {
                        SuggestedType::Boolean
                    } else {
                        SuggestedType::String
                    }
                }
                _ => SuggestedType::Unknown,
            }
        };

        // Suggest name for hex fields
        let suggested_name = if hex_info.is_hex {
            self.suggest_hex_name(path, &hex_info)
        } else {
            None
        };

        MetricInterpretation {
            path: path.to_string(),
            original_value: value.clone(),
            hex_info: Some(hex_info),
            suggested_type,
            suggested_name,
        }
    }

    /// Suggest a name for a hex field based on its characteristics
    fn suggest_hex_name(&self, path: &str, hex_info: &HexInfo) -> Option<String> {
        let base_name = extract_field_name(path);

        if let Some(_int_val) = hex_info.decoded_integer {
            // If it decodes to a reasonable integer, use that
            Some(format!("{}_decoded", base_name))
        } else if let Some(bytes) = &hex_info.as_bytes {
            if bytes.len() <= 4 {
                Some(format!("{}_u32", base_name))
            } else if bytes.len() <= 8 {
                Some(format!("{}_u64", base_name))
            } else {
                Some(format!("{}_bytes", base_name))
            }
        } else {
            None
        }
    }

    /// Check if a value looks like a hex string (quick check)
    pub fn is_hex_string(value: &str) -> bool {
        if value.is_empty() || value.len() > 1024 {
            return false;
        }

        // Must have even length (unless odd length is allowed)
        if !value.len().is_multiple_of(2) && !value.starts_with("0x") {
            return false;
        }

        let clean = value.trim_start_matches("0x").trim_start_matches("0X");
        clean.chars().all(|c: char| c.is_ascii_hexdigit())
    }

    /// Try to convert a hex string to bytes
    pub fn hex_to_bytes(hex_str: &str) -> Option<Vec<u8>> {
        let clean = hex_str.trim_start_matches("0x").trim_start_matches("0X");

        if !clean.len().is_multiple_of(2) {
            return None;
        }

        (0..clean.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&clean[i..i + 2], 16).ok())
            .collect()
    }
}

impl Default for HexAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to convert hex string to bytes
pub fn hex_to_bytes(hex_str: &str) -> Option<Vec<u8>> {
    HexAnalyzer::hex_to_bytes(hex_str)
}

/// Check if a string value looks like hex
pub fn is_hex_string(value: &str) -> bool {
    HexAnalyzer::is_hex_string(value)
}

/// Value statistics from sample analysis
#[derive(Debug, Clone)]
pub struct ValueStats {
    /// Minimum value (for numeric types)
    pub min: Option<f64>,
    /// Maximum value (for numeric types)
    pub max: Option<f64>,
    /// Most common string value
    pub most_common_string: Option<String>,
    /// String length range
    pub string_length: Option<(usize, usize)>,
    /// Whether values appear to be hex-encoded
    pub hex_like: bool,
    /// Whether values appear to be boolean-like
    pub boolean_like: bool,
    /// Unit/hint from value patterns
    pub unit_hint: Option<String>,
}

/// Compute statistics from a list of values
pub fn compute_stats(values: &[Value]) -> ValueStats {
    if values.is_empty() {
        return ValueStats {
            min: None,
            max: None,
            most_common_string: None,
            string_length: None,
            hex_like: false,
            boolean_like: false,
            unit_hint: None,
        };
    }

    let mut numeric_values: Vec<f64> = Vec::new();
    let mut string_values: Vec<String> = Vec::new();
    let mut hex_count = 0;
    let mut boolean_count = 0;
    let mut string_lengths: Vec<usize> = Vec::new();

    for val in values {
        match val {
            Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    numeric_values.push(f);
                }
            }
            Value::String(s) => {
                string_values.push(s.clone());
                string_lengths.push(s.len());
                if HexAnalyzer::is_hex_string(s) {
                    hex_count += 1;
                }
            }
            Value::Bool(_) => {
                boolean_count += 1;
            }
            _ => {}
        }
    }

    let min = numeric_values.iter().cloned().reduce(f64::min);
    let max = numeric_values.iter().cloned().reduce(f64::max);

    let most_common_string = if !string_values.is_empty() {
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for s in &string_values {
            *counts.entry(s.clone()).or_insert(0) += 1;
        }
        counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(s, _)| s)
    } else {
        None
    };

    let string_length = if !string_lengths.is_empty() {
        let min_len = *string_lengths.iter().min().unwrap();
        let max_len = *string_lengths.iter().max().unwrap();
        Some((min_len, max_len))
    } else {
        None
    };

    let hex_like =
        !string_values.is_empty() && (hex_count as f64) / (string_values.len() as f64) > 0.5;
    let boolean_like = !values.is_empty() && (boolean_count as f64) / (values.len() as f64) > 0.8;

    // Detect unit hints from values
    let unit_hint = detect_unit_hint(&numeric_values, &string_values);

    ValueStats {
        min,
        max,
        most_common_string,
        string_length,
        hex_like,
        boolean_like,
        unit_hint,
    }
}

/// Detect unit hint from value patterns
fn detect_unit_hint(numeric: &[f64], strings: &[String]) -> Option<String> {
    // Check for temperature ranges
    if let Some(max_temp) = numeric.iter().cloned().reduce(f64::max) {
        if (10.0..=50.0).contains(&max_temp) {
            return Some("°C".to_string());
        } else if max_temp > 50.0 && max_temp <= 120.0 {
            return Some("°F".to_string());
        }
    }

    // Check for percentage ranges (0-100)
    if let Some(max_val) = numeric.iter().cloned().reduce(f64::max)
        && (0.0..=100.0).contains(&max_val)
        && let Some(min_val) = numeric.iter().cloned().reduce(f64::min)
        && min_val >= 0.0
    {
        return Some("%".to_string());
    }

    // Check string values for units
    for s in strings {
        if s.ends_with('%') {
            return Some("%".to_string());
        }
        if s.contains('°') && (s.contains('C') || s.contains('c')) {
            return Some("°C".to_string());
        }
    }

    None
}

/// Extract field name from path (re-exported)
pub fn extract_field_name(path: &str) -> String {
    if let Some(last_dot) = path.rfind('.') {
        path[last_dot + 1..].to_string()
    } else if let Some(bracket) = path.find('[') {
        path[..bracket].to_string()
    } else {
        path.replace("$", "")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_is_hex_string() {
        assert!(is_hex_string("1A3F"));
        assert!(is_hex_string("0x1A3F"));
        assert!(is_hex_string("deadbeef"));
        assert!(!is_hex_string("hello")); // Not all hex chars
        assert!(!is_hex_string("1A3G")); // Invalid char
        assert!(!is_hex_string("123")); // Too short, could be decimal
    }

    #[test]
    fn test_hex_to_bytes() {
        assert_eq!(hex_to_bytes("1A3F"), Some(vec![0x1A, 0x3F]));
        assert_eq!(hex_to_bytes("0x1A3F"), Some(vec![0x1A, 0x3F]));
        assert_eq!(hex_to_bytes("1A3"), None); // Odd length
        assert_eq!(hex_to_bytes("GH12"), None); // Invalid hex
    }

    #[test]
    fn test_analyze_value() {
        let analyzer = HexAnalyzer::new();

        // Hex string
        let info = analyzer.analyze_value("$.raw_data", &json!("deadbeef"));
        assert!(info.is_hex);
        assert_eq!(info.decoded_integer, Some(0xdeadbeef));
        assert!(info.display_hint.contains("3735928559"));

        // Regular number
        let info2 = analyzer.analyze_value("$.temperature", &json!(23.5));
        assert!(!info2.is_hex);
    }

    #[test]
    fn test_assess_probability() {
        let analyzer = HexAnalyzer::new();

        // High probability: field name hint + valid hex + good length
        let prob1 = analyzer.assess_probability("deadbeef", "raw_payload");
        assert!(prob1.score() >= 50);

        // Low probability: no hint, short value
        let prob2 = analyzer.assess_probability("12", "count");
        assert!(prob2.score() < 50);
    }

    #[test]
    fn test_compute_stats() {
        let values = vec![json!("0x1A3F"), json!("0x2B4F"), json!("0x3C5F")];

        let stats = compute_stats(&values);
        assert!(stats.hex_like);
    }
}
