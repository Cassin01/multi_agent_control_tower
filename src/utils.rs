use sha2::{Digest, Sha256};
use std::path::Path;

const BRANCH_NAME_MAX_LEN: usize = 50;

pub fn sanitize_branch_name(input: &str) -> String {
    let sanitized: String = input
        .to_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect();

    let collapsed = sanitized
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");

    let truncated = if collapsed.len() > BRANCH_NAME_MAX_LEN {
        &collapsed[..BRANCH_NAME_MAX_LEN]
    } else {
        &collapsed
    };

    let result = truncated.trim_end_matches('-').to_string();

    if result.is_empty() {
        "unnamed".to_string()
    } else {
        result
    }
}

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

/// Compute a deterministic 8-char hex hash from an absolute path.
/// This is the canonical hash used to derive session names from project paths.
pub fn compute_path_hash(path: &Path) -> String {
    let abs_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let path_str = abs_path.to_string_lossy();

    let mut hasher = Sha256::new();
    hasher.update(path_str.as_bytes());
    let result = hasher.finalize();

    hex::encode(&result[..4])
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

    #[test]
    fn sanitize_branch_name_simple() {
        assert_eq!(
            sanitize_branch_name("add-auth"),
            "add-auth",
            "sanitize_branch_name: simple hyphenated name should pass through"
        );
    }

    #[test]
    fn sanitize_branch_name_spaces_to_hyphens() {
        assert_eq!(
            sanitize_branch_name("add user auth"),
            "add-user-auth",
            "sanitize_branch_name: spaces should become hyphens"
        );
    }

    #[test]
    fn sanitize_branch_name_uppercase_to_lowercase() {
        assert_eq!(
            sanitize_branch_name("Add Auth"),
            "add-auth",
            "sanitize_branch_name: uppercase should become lowercase"
        );
    }

    #[test]
    fn sanitize_branch_name_special_chars_removed() {
        assert_eq!(
            sanitize_branch_name("feat: add auth!"),
            "feat-add-auth",
            "sanitize_branch_name: special characters should be removed"
        );
    }

    #[test]
    fn sanitize_branch_name_consecutive_hyphens_collapsed() {
        assert_eq!(
            sanitize_branch_name("add  --  auth"),
            "add-auth",
            "sanitize_branch_name: consecutive hyphens should be collapsed"
        );
    }

    #[test]
    fn sanitize_branch_name_leading_trailing_hyphens_stripped() {
        assert_eq!(
            sanitize_branch_name("--add-auth--"),
            "add-auth",
            "sanitize_branch_name: leading/trailing hyphens should be stripped"
        );
    }

    #[test]
    fn sanitize_branch_name_truncates_long_input() {
        let long = "a".repeat(100);
        let result = sanitize_branch_name(&long);
        assert!(
            result.len() <= 50,
            "sanitize_branch_name: should truncate to max 50 chars, got {}",
            result.len()
        );
    }

    #[test]
    fn sanitize_branch_name_empty_returns_unnamed() {
        assert_eq!(
            sanitize_branch_name(""),
            "unnamed",
            "sanitize_branch_name: empty input should return 'unnamed'"
        );
    }

    #[test]
    fn sanitize_branch_name_only_special_chars_returns_unnamed() {
        assert_eq!(
            sanitize_branch_name("!@#$%"),
            "unnamed",
            "sanitize_branch_name: only special chars should return 'unnamed'"
        );
    }

    #[test]
    fn sanitize_branch_name_underscores_preserved() {
        assert_eq!(
            sanitize_branch_name("add_user_auth"),
            "add_user_auth",
            "sanitize_branch_name: underscores should be preserved"
        );
    }

    #[test]
    fn sanitize_branch_name_dots_preserved() {
        assert_eq!(
            sanitize_branch_name("fix v1.2"),
            "fix-v1.2",
            "sanitize_branch_name: dots should be preserved for version numbers"
        );
    }

    #[test]
    fn compute_path_hash_is_deterministic() {
        let hash1 = compute_path_hash(std::path::Path::new("/tmp/test"));
        let hash2 = compute_path_hash(std::path::Path::new("/tmp/test"));
        assert_eq!(
            hash1, hash2,
            "compute_path_hash: same path should produce same hash"
        );
    }

    #[test]
    fn compute_path_hash_differs_for_different_paths() {
        let hash1 = compute_path_hash(std::path::Path::new("/tmp/project1"));
        let hash2 = compute_path_hash(std::path::Path::new("/tmp/project2"));
        assert_ne!(
            hash1, hash2,
            "compute_path_hash: different paths should produce different hashes"
        );
    }

    #[test]
    fn compute_path_hash_is_8_chars() {
        let hash = compute_path_hash(std::path::Path::new("/tmp/test"));
        assert_eq!(
            hash.len(),
            8,
            "compute_path_hash: hash should be 8 hex characters"
        );
    }
}
