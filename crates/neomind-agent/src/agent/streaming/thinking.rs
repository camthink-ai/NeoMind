/// Clean up repetitive thinking content by removing excessive repeated phrases
/// This preserves the core thinking while removing the repetitive noise
pub fn cleanup_thinking_content(thinking: &str) -> String {
    // Chars, not bytes: `.len()` counted ~3 bytes per CJK char, so a short
    // 100-char Chinese reasoning block (300 bytes) passed this "short text,
    // leave alone" gate and got the repetition rewrite applied to it.
    if thinking.chars().count() < 200 {
        return thinking.to_string();
    }

    let mut result = thinking.to_string();

    // Pass 1: Remove immediate repetitions — bounded to max 4 iterations
    // (handles patterns like "可能可能可能可能" -> "可能" in O(4*N) instead of unbounded)
    let patterns = [
        ("可能可能", "可能"),
        ("或者或者", "或者"),
        ("也许也许", "也许"),
        ("温度温度", "温度"),
        ("。。", "。"),
        ("，，", "，"),
        ("??", "?"),
        ("  ", " "),
    ];

    for _ in 0..4 {
        let before = result.len();
        for (pattern, replacement) in &patterns {
            result = result.replace(pattern, replacement);
        }
        if result.len() == before {
            break; // No more reductions possible
        }
    }

    // Pass 2: Limit consecutive occurrences of common filler words
    // Using character-based iteration to avoid UTF-8 issues
    let filler_words = [
        ("可能", 3), // Max 3 consecutive "可能"
        ("或者", 2), // Max 2 consecutive "或者"
        ("也许", 2),
    ];

    for (word, max_consecutive) in filler_words {
        let chars: Vec<char> = result.chars().collect();
        let mut new_result = String::new();
        let mut consecutive = 0;
        let mut last_was_word = false;
        let mut char_idx = 0;

        while char_idx < chars.len() {
            // Check if the word starts at this position
            let word_chars: Vec<char> = word.chars().collect();
            let matches = if char_idx + word_chars.len() <= chars.len() {
                chars[char_idx..char_idx + word_chars.len()] == word_chars[..]
            } else {
                false
            };

            if matches {
                if last_was_word {
                    consecutive += 1;
                    if consecutive <= max_consecutive {
                        for &ch in &word_chars {
                            new_result.push(ch);
                        }
                    }
                } else {
                    consecutive = 1;
                    last_was_word = true;
                    for &ch in &word_chars {
                        new_result.push(ch);
                    }
                }
                char_idx += word_chars.len();
            } else {
                consecutive = 0;
                last_was_word = false;
                new_result.push(chars[char_idx]);
                char_idx += 1;
            }
        }
        result = new_result;
    }

    // Pass 3: If still too long, truncate with ellipsis at char boundary
    if result.chars().count() > 500 {
        let _char_count = result.chars().count();
        // Take first 500 chars and add ellipsis
        result = result.chars().take(500).collect::<String>();
        result.push_str("...");
    }

    result
}
