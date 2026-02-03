/// Truncates a string to max_chars characters, appending "..." if truncated.
/// Safe for UTF-8 multi-byte characters (e.g., Japanese text).
pub fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncate_at = max_chars.saturating_sub(3);
        let byte_index = s
            .char_indices()
            .nth(truncate_at)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..byte_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_str_short_string() {
        assert_eq!(truncate_str("short", 20), "short");
    }

    #[test]
    fn truncate_str_exact_length() {
        assert_eq!(truncate_str("hello", 5), "hello");
    }

    #[test]
    fn truncate_str_long_string() {
        let long = "A".repeat(100);
        let result = truncate_str(&long, 60);
        assert!(result.chars().count() <= 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_str_specific_truncation() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn truncate_str_utf8_safe() {
        let japanese = "日本語のテストテキストです。これは非常に長いテキストで切り詰められます。";
        let result = truncate_str(japanese, 20);
        assert!(result.chars().count() <= 20);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn truncate_str_japanese_short() {
        let japanese = "こんにちは世界";
        assert_eq!(truncate_str(japanese, 10), japanese);
        assert_eq!(truncate_str(japanese, 5), "こん...");
    }
}
