/// Embedded default role instructions.
/// These are used as fallback when user hasn't customized instructions.

pub const DEFAULT_ARCHITECT: &str = include_str!("../../instructions/architect.md");
pub const DEFAULT_BACKEND: &str = include_str!("../../instructions/backend.md");
pub const DEFAULT_FRONTEND: &str = include_str!("../../instructions/frontend.md");
pub const DEFAULT_PLANNER: &str = include_str!("../../instructions/planner.md");
pub const DEFAULT_TESTER: &str = include_str!("../../instructions/tester.md");
pub const DEFAULT_GENERAL: &str = include_str!("../../instructions/general.md");

/// Get embedded default instruction for a role.
/// Returns None if the role has no embedded default.
pub fn get_default(role: &str) -> Option<&'static str> {
    match role {
        "architect" => Some(DEFAULT_ARCHITECT),
        "backend" => Some(DEFAULT_BACKEND),
        "frontend" => Some(DEFAULT_FRONTEND),
        "planner" => Some(DEFAULT_PLANNER),
        "tester" => Some(DEFAULT_TESTER),
        "general" => Some(DEFAULT_GENERAL),
        _ => None,
    }
}

/// List of all default role names.
pub fn default_role_names() -> &'static [&'static str] {
    &["architect", "backend", "frontend", "general", "planner", "tester"]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_default_returns_content_for_known_roles() {
        assert!(get_default("architect").is_some());
        assert!(get_default("backend").is_some());
        assert!(get_default("frontend").is_some());
        assert!(get_default("planner").is_some());
        assert!(get_default("tester").is_some());
        assert!(get_default("general").is_some());
    }

    #[test]
    fn get_default_returns_none_for_unknown_role() {
        assert!(get_default("unknown-role").is_none());
        assert!(get_default("custom").is_none());
    }

    #[test]
    fn default_role_names_contains_all_defaults() {
        let names = default_role_names();
        assert!(names.contains(&"architect"));
        assert!(names.contains(&"backend"));
        assert!(names.contains(&"frontend"));
        assert!(names.contains(&"planner"));
        assert!(names.contains(&"tester"));
        assert!(names.contains(&"general"));
    }

    #[test]
    fn embedded_instructions_are_not_empty() {
        assert!(!DEFAULT_ARCHITECT.is_empty());
        assert!(!DEFAULT_BACKEND.is_empty());
        assert!(!DEFAULT_FRONTEND.is_empty());
        assert!(!DEFAULT_PLANNER.is_empty());
        assert!(!DEFAULT_TESTER.is_empty());
        assert!(!DEFAULT_GENERAL.is_empty());
    }
}
