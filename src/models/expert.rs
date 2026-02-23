use chrono::{DateTime, Utc};
use ratatui::style::Color;
use serde::{Deserialize, Serialize};

use super::message::ExpertId;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpertState {
    #[default]
    Idle,
    Busy,
}

impl ExpertState {
    pub fn symbol(&self) -> &'static str {
        match self {
            ExpertState::Idle => "○",
            ExpertState::Busy => "●",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            ExpertState::Idle => Color::Gray,
            ExpertState::Busy => Color::Green,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            ExpertState::Idle => "Waiting for input",
            ExpertState::Busy => "Working",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Analyst,
    Developer,
    Reviewer,
    Coordinator,
    Specialist(String),
}

impl Role {
    pub fn specialist(name: impl Into<String>) -> Self {
        Self::Specialist(name.into())
    }

    pub fn matches(&self, other: &str) -> bool {
        match self {
            Role::Analyst => other.eq_ignore_ascii_case("analyst"),
            Role::Developer => other.eq_ignore_ascii_case("developer"),
            Role::Reviewer => other.eq_ignore_ascii_case("reviewer"),
            Role::Coordinator => other.eq_ignore_ascii_case("coordinator"),
            Role::Specialist(name) => name.eq_ignore_ascii_case(other),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            Role::Analyst => "analyst",
            Role::Developer => "developer",
            Role::Reviewer => "reviewer",
            Role::Coordinator => "coordinator",
            Role::Specialist(name) => name,
        }
    }
}

impl std::fmt::Display for Role {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertInfo {
    pub id: ExpertId,
    pub name: String,
    pub role: Role,
    pub tmux_session: String,
    #[serde(alias = "tmux_pane")]
    pub tmux_window: String,
    pub state: ExpertState,
    pub last_activity: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worktree_path: Option<String>,
}

impl ExpertInfo {
    pub fn new(
        id: ExpertId,
        name: String,
        role: Role,
        tmux_session: String,
        tmux_window: String,
    ) -> Self {
        Self {
            id,
            name,
            role,
            tmux_session,
            tmux_window,
            state: ExpertState::default(),
            last_activity: Utc::now(),
            worktree_path: None,
        }
    }

    pub fn set_state(&mut self, state: ExpertState) {
        self.state = state;
        self.last_activity = Utc::now();
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, ExpertState::Idle)
    }

    #[allow(dead_code)]
    pub fn is_busy(&self) -> bool {
        matches!(self.state, ExpertState::Busy)
    }

    #[allow(dead_code)]
    pub fn matches_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    #[allow(dead_code)]
    pub fn matches_role(&self, role: &str) -> bool {
        self.role.matches(role)
    }

    pub fn set_worktree_path(&mut self, path: Option<String>) {
        self.worktree_path = path;
    }

    pub fn same_worktree(&self, other: &ExpertInfo) -> bool {
        self.worktree_path == other.worktree_path
    }

    #[allow(dead_code)]
    pub fn update_activity(&mut self) {
        self.last_activity = Utc::now();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expert_info_new_creates_with_defaults() {
        let expert = ExpertInfo::new(
            0,
            "architect".to_string(),
            Role::Analyst,
            "session-1".to_string(),
            "0".to_string(),
        );

        assert_eq!(expert.id, 0);
        assert_eq!(expert.name, "architect");
        assert_eq!(expert.role, Role::Analyst);
        assert_eq!(expert.tmux_session, "session-1");
        assert_eq!(expert.tmux_window, "0");
        assert_eq!(expert.state, ExpertState::Idle);
    }

    #[test]
    fn expert_state_transitions() {
        let mut expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );

        // Start idle
        assert!(expert.is_idle());
        assert!(!expert.is_busy());

        // Set to busy
        expert.set_state(ExpertState::Busy);
        assert!(expert.is_busy());
        assert!(!expert.is_idle());

        // Set back to idle
        expert.set_state(ExpertState::Idle);
        assert!(expert.is_idle());
        assert!(!expert.is_busy());
    }

    #[test]
    fn expert_name_matching() {
        let expert = ExpertInfo::new(
            0,
            "Backend-Expert".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );

        assert!(expert.matches_name("Backend-Expert"));
        assert!(expert.matches_name("backend-expert"));
        assert!(expert.matches_name("BACKEND-EXPERT"));
        assert!(!expert.matches_name("Frontend"));
    }

    #[test]
    fn role_matching() {
        let analyst_role = Role::Analyst;
        let specialist_role = Role::specialist("backend");

        assert!(analyst_role.matches("analyst"));
        assert!(analyst_role.matches("ANALYST"));
        assert!(analyst_role.matches("Analyst"));
        assert!(!analyst_role.matches("developer"));

        assert!(specialist_role.matches("backend"));
        assert!(specialist_role.matches("BACKEND"));
        assert!(!specialist_role.matches("frontend"));
    }

    #[test]
    fn expert_role_matching() {
        let expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::specialist("backend"),
            "session".to_string(),
            "0".to_string(),
        );

        assert!(expert.matches_role("backend"));
        assert!(expert.matches_role("BACKEND"));
        assert!(!expert.matches_role("frontend"));
    }

    #[test]
    fn role_display() {
        assert_eq!(Role::Analyst.to_string(), "analyst");
        assert_eq!(Role::Developer.to_string(), "developer");
        assert_eq!(Role::specialist("custom").to_string(), "custom");
    }

    #[test]
    fn expert_activity_update() {
        let mut expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );

        let initial_activity = expert.last_activity;

        // Small delay to ensure timestamp difference
        std::thread::sleep(std::time::Duration::from_millis(1));

        expert.update_activity();
        assert!(expert.last_activity > initial_activity);
    }

    #[test]
    fn expert_serializes_to_yaml() {
        let expert = ExpertInfo::new(
            1,
            "backend-dev".to_string(),
            Role::Developer,
            "macot-session".to_string(),
            "1".to_string(),
        );

        let yaml = serde_yaml::to_string(&expert).unwrap();
        assert!(yaml.contains("id: 1"));
        assert!(yaml.contains("name: backend-dev"));
        assert!(yaml.contains("role: developer"));
        assert!(yaml.contains("state: idle"));
    }

    #[test]
    fn expert_deserializes_from_yaml() {
        let yaml = r#"
id: 2
name: "frontend-expert"
role: developer
tmux_session: "macot-session"
tmux_pane: "2"
state: idle
last_activity: "2024-01-15T10:30:00Z"
"#;

        let expert: ExpertInfo = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(expert.id, 2);
        assert_eq!(expert.name, "frontend-expert");
        assert_eq!(expert.role, Role::Developer);
        assert_eq!(expert.state, ExpertState::Idle);
    }

    #[test]
    fn worktree_path_defaults_to_none() {
        let expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );
        assert!(
            expert.worktree_path.is_none(),
            "worktree_path: should default to None"
        );
    }

    #[test]
    fn set_worktree_path_updates_field() {
        let mut expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );

        expert.set_worktree_path(Some("/path/to/worktree".to_string()));
        assert_eq!(
            expert.worktree_path,
            Some("/path/to/worktree".to_string()),
            "set_worktree_path: should set to Some"
        );

        expert.set_worktree_path(None);
        assert!(
            expert.worktree_path.is_none(),
            "set_worktree_path: should set back to None"
        );
    }

    #[test]
    fn same_worktree_both_none() {
        let a = ExpertInfo::new(
            0,
            "a".to_string(),
            Role::Developer,
            "s".to_string(),
            "0".to_string(),
        );
        let b = ExpertInfo::new(
            1,
            "b".to_string(),
            Role::Developer,
            "s".to_string(),
            "1".to_string(),
        );
        assert!(
            a.same_worktree(&b),
            "same_worktree: (None, None) should be true"
        );
    }

    #[test]
    fn same_worktree_both_same_path() {
        let mut a = ExpertInfo::new(
            0,
            "a".to_string(),
            Role::Developer,
            "s".to_string(),
            "0".to_string(),
        );
        let mut b = ExpertInfo::new(
            1,
            "b".to_string(),
            Role::Developer,
            "s".to_string(),
            "1".to_string(),
        );
        a.set_worktree_path(Some("/worktree/feature-auth".to_string()));
        b.set_worktree_path(Some("/worktree/feature-auth".to_string()));
        assert!(
            a.same_worktree(&b),
            "same_worktree: (Some(X), Some(X)) should be true"
        );
    }

    #[test]
    fn same_worktree_none_vs_some() {
        let a = ExpertInfo::new(
            0,
            "a".to_string(),
            Role::Developer,
            "s".to_string(),
            "0".to_string(),
        );
        let mut b = ExpertInfo::new(
            1,
            "b".to_string(),
            Role::Developer,
            "s".to_string(),
            "1".to_string(),
        );
        b.set_worktree_path(Some("/worktree/feature-auth".to_string()));
        assert!(
            !a.same_worktree(&b),
            "same_worktree: (None, Some(X)) should be false"
        );
        assert!(
            !b.same_worktree(&a),
            "same_worktree: (Some(X), None) should be false"
        );
    }

    #[test]
    fn same_worktree_different_paths() {
        let mut a = ExpertInfo::new(
            0,
            "a".to_string(),
            Role::Developer,
            "s".to_string(),
            "0".to_string(),
        );
        let mut b = ExpertInfo::new(
            1,
            "b".to_string(),
            Role::Developer,
            "s".to_string(),
            "1".to_string(),
        );
        a.set_worktree_path(Some("/worktree/feature-auth".to_string()));
        b.set_worktree_path(Some("/worktree/feature-payments".to_string()));
        assert!(
            !a.same_worktree(&b),
            "same_worktree: (Some(X), Some(Y)) should be false"
        );
    }

    #[test]
    fn same_worktree_is_symmetric() {
        let a = ExpertInfo::new(
            0,
            "a".to_string(),
            Role::Developer,
            "s".to_string(),
            "0".to_string(),
        );
        let mut b = ExpertInfo::new(
            1,
            "b".to_string(),
            Role::Developer,
            "s".to_string(),
            "1".to_string(),
        );
        b.set_worktree_path(Some("/worktree/feature-auth".to_string()));

        assert_eq!(
            a.same_worktree(&b),
            b.same_worktree(&a),
            "same_worktree: should be symmetric"
        );
    }

    #[test]
    fn same_worktree_is_reflexive() {
        let a = ExpertInfo::new(
            0,
            "a".to_string(),
            Role::Developer,
            "s".to_string(),
            "0".to_string(),
        );
        assert!(a.same_worktree(&a), "same_worktree: reflexive with None");

        let mut b = ExpertInfo::new(
            1,
            "b".to_string(),
            Role::Developer,
            "s".to_string(),
            "1".to_string(),
        );
        b.set_worktree_path(Some("/worktree/feature-auth".to_string()));
        assert!(b.same_worktree(&b), "same_worktree: reflexive with Some");
    }

    #[test]
    fn worktree_path_backward_compat_deserialization() {
        // YAML without worktree_path field should deserialize to None
        let yaml = r#"
id: 2
name: "test-expert"
role: developer
tmux_session: "session"
tmux_pane: "0"
state: idle
last_activity: "2024-01-15T10:30:00Z"
"#;
        let expert: ExpertInfo = serde_yaml::from_str(yaml).unwrap();
        assert!(
            expert.worktree_path.is_none(),
            "backward_compat: missing worktree_path should deserialize as None"
        );
    }

    #[test]
    fn worktree_path_serialization_roundtrip() {
        let mut expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );
        expert.set_worktree_path(Some("/worktree/feature".to_string()));

        let yaml = serde_yaml::to_string(&expert).unwrap();
        assert!(
            yaml.contains("worktree_path"),
            "serialization: should include worktree_path when Some"
        );

        let deserialized: ExpertInfo = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(
            deserialized.worktree_path,
            Some("/worktree/feature".to_string()),
            "serialization: roundtrip should preserve worktree_path"
        );
    }

    #[test]
    fn worktree_path_none_omitted_in_serialization() {
        let expert = ExpertInfo::new(
            0,
            "test".to_string(),
            Role::Developer,
            "session".to_string(),
            "0".to_string(),
        );

        let yaml = serde_yaml::to_string(&expert).unwrap();
        assert!(
            !yaml.contains("worktree_path"),
            "serialization: should omit worktree_path when None"
        );
    }

    #[test]
    fn expert_state_default_is_idle() {
        assert_eq!(ExpertState::default(), ExpertState::Idle);
    }

    #[test]
    fn expert_state_symbols_are_unique() {
        let states = [ExpertState::Idle, ExpertState::Busy];
        let symbols: Vec<_> = states.iter().map(|s| s.symbol()).collect();
        let unique: std::collections::HashSet<_> = symbols.iter().collect();
        assert_eq!(
            symbols.len(),
            unique.len(),
            "expert_state_symbols: all symbols should be unique"
        );
    }

    #[test]
    fn expert_state_symbol_values() {
        assert_eq!(ExpertState::Idle.symbol(), "○");
        assert_eq!(ExpertState::Busy.symbol(), "●");
    }

    #[test]
    fn expert_state_color_values() {
        assert_eq!(ExpertState::Idle.color(), Color::Gray);
        assert_eq!(ExpertState::Busy.color(), Color::Green);
    }

    #[test]
    fn expert_state_description_values() {
        assert_eq!(ExpertState::Idle.description(), "Waiting for input");
        assert_eq!(ExpertState::Busy.description(), "Working");
    }
}
