use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum EffortLevel {
    Simple,
    #[default]
    Medium,
    Complex,
    Critical,
}

impl EffortLevel {
    pub fn all() -> [EffortLevel; 4] {
        [
            EffortLevel::Simple,
            EffortLevel::Medium,
            EffortLevel::Complex,
            EffortLevel::Critical,
        ]
    }

    pub fn next(&self) -> EffortLevel {
        match self {
            EffortLevel::Simple => EffortLevel::Medium,
            EffortLevel::Medium => EffortLevel::Complex,
            EffortLevel::Complex => EffortLevel::Critical,
            EffortLevel::Critical => EffortLevel::Simple,
        }
    }

    pub fn prev(&self) -> EffortLevel {
        match self {
            EffortLevel::Simple => EffortLevel::Critical,
            EffortLevel::Medium => EffortLevel::Simple,
            EffortLevel::Complex => EffortLevel::Medium,
            EffortLevel::Critical => EffortLevel::Complex,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EffortConfig {
    pub level: EffortLevel,
    pub max_tool_calls: u32,
    pub max_files_modified: Option<u32>,
    pub duration_hint: String,
    pub scope_boundary: Option<String>,
}

impl EffortConfig {
    pub fn from_level(level: EffortLevel) -> Self {
        match level {
            EffortLevel::Simple => Self {
                level,
                max_tool_calls: 10,
                max_files_modified: Some(3),
                duration_hint: "15m".into(),
                scope_boundary: None,
            },
            EffortLevel::Medium => Self {
                level,
                max_tool_calls: 25,
                max_files_modified: Some(7),
                duration_hint: "45m".into(),
                scope_boundary: None,
            },
            EffortLevel::Complex => Self {
                level,
                max_tool_calls: 50,
                max_files_modified: Some(15),
                duration_hint: "2h".into(),
                scope_boundary: None,
            },
            EffortLevel::Critical => Self {
                level,
                max_tool_calls: 100,
                max_files_modified: None,
                duration_hint: "4h".into(),
                scope_boundary: None,
            },
        }
    }

    pub fn with_scope_boundary(mut self, boundary: String) -> Self {
        self.scope_boundary = Some(boundary);
        self
    }

    pub fn with_max_tool_calls(mut self, max: u32) -> Self {
        self.max_tool_calls = max;
        self
    }

    pub fn with_max_files_modified(mut self, max: Option<u32>) -> Self {
        self.max_files_modified = max;
        self
    }
}

impl Default for EffortConfig {
    fn default() -> Self {
        Self::from_level(EffortLevel::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_level_default_is_medium() {
        assert_eq!(EffortLevel::default(), EffortLevel::Medium);
    }

    #[test]
    fn effort_level_next_cycles_correctly() {
        assert_eq!(EffortLevel::Simple.next(), EffortLevel::Medium);
        assert_eq!(EffortLevel::Medium.next(), EffortLevel::Complex);
        assert_eq!(EffortLevel::Complex.next(), EffortLevel::Critical);
        assert_eq!(EffortLevel::Critical.next(), EffortLevel::Simple);
    }

    #[test]
    fn effort_level_prev_cycles_correctly() {
        assert_eq!(EffortLevel::Simple.prev(), EffortLevel::Critical);
        assert_eq!(EffortLevel::Medium.prev(), EffortLevel::Simple);
        assert_eq!(EffortLevel::Complex.prev(), EffortLevel::Medium);
        assert_eq!(EffortLevel::Critical.prev(), EffortLevel::Complex);
    }

    #[test]
    fn effort_config_from_simple_has_correct_defaults() {
        let config = EffortConfig::from_level(EffortLevel::Simple);
        assert_eq!(config.max_tool_calls, 10);
        assert_eq!(config.max_files_modified, Some(3));
        assert_eq!(config.duration_hint, "15m");
    }

    #[test]
    fn effort_config_from_critical_has_unlimited_files() {
        let config = EffortConfig::from_level(EffortLevel::Critical);
        assert_eq!(config.max_tool_calls, 100);
        assert_eq!(config.max_files_modified, None);
        assert_eq!(config.duration_hint, "4h");
    }

    #[test]
    fn effort_config_serializes_to_yaml() {
        let config = EffortConfig::from_level(EffortLevel::Medium);
        let yaml = serde_yaml::to_string(&config).unwrap();
        assert!(yaml.contains("level: medium"));
        assert!(yaml.contains("max_tool_calls: 25"));
    }

    #[test]
    fn effort_config_deserializes_from_yaml() {
        let yaml = r#"
level: complex
max_tool_calls: 50
max_files_modified: 15
duration_hint: "2h"
scope_boundary: null
"#;
        let config: EffortConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.level, EffortLevel::Complex);
        assert_eq!(config.max_tool_calls, 50);
    }

    #[test]
    fn effort_config_builder_methods_work() {
        let config = EffortConfig::from_level(EffortLevel::Simple)
            .with_scope_boundary("src/".to_string())
            .with_max_tool_calls(20)
            .with_max_files_modified(Some(5));

        assert_eq!(config.scope_boundary, Some("src/".to_string()));
        assert_eq!(config.max_tool_calls, 20);
        assert_eq!(config.max_files_modified, Some(5));
    }
}
