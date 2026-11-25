//! Token estimation for context window management.
//!
//! Provides accurate token counting for Chinese, English, and code content.

/// Estimate token count for a text string.
///
/// Heuristic rates. NOTE (2026-11-11): the earlier "accurate" calibration
/// (CJK 1.0/char, digits 1.5/char) regressed the 138-case gate to 88/138
/// from 99/138 — inflated CLI-output estimates over-compacted history and
/// caused re-work. These conservative rates are load-bearing as compaction
/// headroom; don't re-calibrate without real trial wire data + a gate run.
/// - Chinese characters: ~1.8 tokens each
/// - English: ~0.25 tokens per character
/// - Digits: ~0.3 tokens each
/// - Other: ~0.5 tokens each
/// - Global ×1.1 safety margin
pub fn estimate_tokens(text: &str) -> usize {
    let mut tokens = 0f64;

    for line in text.lines() {
        let chinese_count = line.chars().filter(|c| is_chinese(*c)).count() as f64;
        let english_count = line.chars().filter(|c| c.is_ascii_alphabetic()).count() as f64;
        let number_count = line.chars().filter(|c| c.is_ascii_digit()).count() as f64;
        let special_count = line.chars().filter(|c| !c.is_alphanumeric()).count() as f64;

        tokens += chinese_count * 1.8;
        tokens += english_count * 0.25;
        tokens += number_count * 0.3;
        tokens += special_count * 0.5;
    }

    (tokens * 1.1).ceil() as usize
}

/// Calibrated token estimator for compact JSON tool-definition schemas.
///
/// The general `estimate_tokens` prices every punctuation/whitespace char at
/// `0.5 * 1.1 = 0.55` tokens, which inflates dense JSON schemas (`":"`, `","`,
/// `{"type":"string"}`) by ~40% vs real BPE tokenizers (Qwen/Llama/MiniCPM),
/// tripping the 8K `prompt_overhead >= max_context` gate and making frontend
/// context-usage breakdowns exceed measured `prompt_tokens`.
///
/// Do NOT reuse this for history compression — keep [`estimate_tokens`] there.
pub fn estimate_json_schema_tokens(json_str: &str) -> usize {
    let mut tokens = 0f64;
    for ch in json_str.chars() {
        if is_chinese(ch) {
            tokens += 1.6;
        } else if ch.is_ascii_alphanumeric() {
            tokens += 0.25;
        } else {
            tokens += 0.22;
        }
    }
    (tokens * 1.08).ceil() as usize
}

/// Check if a character is a Chinese/Japanese/Korean character.
fn is_chinese(c: char) -> bool {
    let cp = c as u32;
    // CJK Unified Ideographs
    (0x4E00..=0x9FFF).contains(&cp) ||
    // CJK Extension A
    (0x3400..=0x4DBF).contains(&cp) ||
    // CJK Compatibility Ideographs
    (0xF900..=0xFAFF).contains(&cp) ||
    // Fullwidth forms
    (0xFF00..=0xFFEF).contains(&cp) ||
    // Hiragana, Katakana
    (0x3040..=0x309F).contains(&cp) ||
    (0x30A0..=0x30FF).contains(&cp)
}

/// Truncate to the longest prefix whose `estimate_tokens` is ≤ `max_tokens`.
///
/// `estimate_tokens` is monotonically non-decreasing in prefix length (every
/// char contributes a positive weight), so a binary search over the char split
/// point bounds it in ~log(n) measurements. The canonical token-based truncation
/// primitive — used wherever content must be capped by token budget rather than
/// char count (memory snapshot, knowledge files).
pub fn truncate_to_tokens(s: &str, max_tokens: usize) -> String {
    if estimate_tokens(s) <= max_tokens {
        return s.to_string();
    }
    let total_chars = s.chars().count();
    // Fast path: if the content is far over budget, trim to a conservative
    // char ceiling first (≈3.5 chars/token is an upper bound — CJK is ~1.8,
    // ASCII ~4 — so the prefix can't drop below the token budget). This keeps
    // the binary search below operating on a small string instead of scanning
    // a 20K-char CJK file ~15 times per call.
    let start = if total_chars > max_tokens * 4 {
        max_tokens * 4
    } else {
        total_chars
    };
    let mut lo: usize = 0;
    let mut hi: usize = start;
    while lo < hi {
        let mid = lo + (hi - lo).div_ceil(2);
        if estimate_tokens(split_at_char(s, mid)) <= max_tokens {
            lo = mid;
        } else {
            hi = mid - 1;
        }
    }
    split_at_char(s, lo).to_string()
}

/// Prefix of `s` containing the first `n` Unicode scalar values (UTF-8 safe).
fn split_at_char(s: &str, n: usize) -> &str {
    if n == 0 {
        return "";
    }
    match s.char_indices().nth(n) {
        Some((idx, _)) => &s[..idx],
        None => s,
    }
}
