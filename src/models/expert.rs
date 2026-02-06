use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::message::ExpertId;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExpertState {
    Idle,
    Busy,
    Offline,
}

impl Default for ExpertState {
    fn default() -> Self {
        Self::Offline
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
    pub tmux_pane: String,
    pub state: ExpertState,
    pub last_activity: DateTime<Utc>,
}

impl ExpertInfo {
    pub fn new(
        id: ExpertId,
        name: String,
        role: Role,
        tmux_session: String,
        tmux_pane: String,
    ) -> Self {
        Self {
            id,
            name,
            role,
            tmux_session,
            tmux_pane,
            state: ExpertState::default(),
            last_activity: Utc::now(),
        }
    }

    pub fn set_state(&mut self, state: ExpertState) {
        self.state = state;
        self.last_activity = Utc::now();
    }

    pub fn is_idle(&self) -> bool {
        matches!(self.state, ExpertState::Idle)
    }

    pub fn is_busy(&self) -> bool {
        matches!(self.state, ExpertState::Busy)
    }

    pub fn is_offline(&self) -> bool {
        matches!(self.state, ExpertState::Offline)
    }

    pub fn matches_name(&self, name: &str) -> bool {
        self.name.eq_ignore_ascii_case(name)
    }

    pub fn matches_role(&self, role: &str) -> bool {
        self.role.matches(role)
    }

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
        assert_eq!(expert.tmux_pane, "0");
        assert_eq!(expert.state, ExpertState::Offline);
        assert!(expert.is_offline());
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

        // Start offline
        assert!(expert.is_offline());
        assert!(!expert.is_idle());
        assert!(!expert.is_busy());

        // Set to idle
        expert.set_state(ExpertState::Idle);
        assert!(expert.is_idle());
        assert!(!expert.is_offline());
        assert!(!expert.is_busy());

        // Set to busy
        expert.set_state(ExpertState::Busy);
        assert!(expert.is_busy());
        assert!(!expert.is_idle());
        assert!(!expert.is_offline());
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
        assert!(yaml.contains("state: offline"));
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
    fn expert_state_default_is_offline() {
        assert_eq!(ExpertState::default(), ExpertState::Offline);
    }
}