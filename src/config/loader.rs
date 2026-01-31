use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertConfig {
    pub name: String,
    pub color: String,
}

impl Default for ExpertConfig {
    fn default() -> Self {
        Self {
            name: "expert".to_string(),
            color: "white".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimeoutConfig {
    pub agent_ready: u64,
    pub task_completion: u64,
    pub graceful_shutdown: u64,
}

impl Default for TimeoutConfig {
    fn default() -> Self {
        Self {
            agent_ready: 30,
            task_completion: 600,
            graceful_shutdown: 10,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub num_experts: u32,
    pub session_prefix: String,
    pub experts: Vec<ExpertConfig>,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    #[serde(skip)]
    pub project_path: PathBuf,
    #[serde(skip)]
    pub queue_path: PathBuf,
    #[serde(skip)]
    pub instructions_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            num_experts: 4,
            session_prefix: "macot".to_string(),
            experts: vec![
                ExpertConfig {
                    name: "architect".to_string(),
                    color: "red".to_string(),
                },
                ExpertConfig {
                    name: "frontend".to_string(),
                    color: "blue".to_string(),
                },
                ExpertConfig {
                    name: "backend".to_string(),
                    color: "green".to_string(),
                },
                ExpertConfig {
                    name: "tester".to_string(),
                    color: "yellow".to_string(),
                },
            ],
            timeouts: TimeoutConfig::default(),
            project_path: PathBuf::new(),
            queue_path: PathBuf::new(),
            instructions_path: PathBuf::new(),
        }
    }
}

impl Config {
    pub fn load(config_path: Option<PathBuf>) -> Result<Self> {
        let path = config_path.unwrap_or_else(Self::default_config_path);

        if path.exists() {
            let content = std::fs::read_to_string(&path)
                .with_context(|| format!("Failed to read config file: {:?}", path))?;
            let config: Config = serde_yaml::from_str(&content)
                .with_context(|| format!("Failed to parse config file: {:?}", path))?;
            Ok(config)
        } else {
            Ok(Config::default())
        }
    }

    pub fn default_config_path() -> PathBuf {
        if let Some(config_path) = std::env::var_os("MACOT_CONFIG") {
            PathBuf::from(config_path)
        } else {
            dirs::config_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join("macot")
                .join("config.yaml")
        }
    }

    pub fn with_project_path(mut self, project_path: PathBuf) -> Self {
        self.queue_path = project_path.join("queue");
        self.instructions_path = project_path.join("instructions");
        self.project_path = project_path;
        self
    }

    pub fn with_num_experts(mut self, num_experts: u32) -> Self {
        self.num_experts = num_experts;
        while self.experts.len() < num_experts as usize {
            let idx = self.experts.len();
            self.experts.push(ExpertConfig {
                name: format!("expert{}", idx),
                color: "white".to_string(),
            });
        }
        self.experts.truncate(num_experts as usize);
        self
    }

    pub fn get_expert(&self, id: u32) -> Option<&ExpertConfig> {
        self.experts.get(id as usize)
    }

    pub fn get_expert_by_name(&self, name: &str) -> Option<(u32, &ExpertConfig)> {
        self.experts
            .iter()
            .enumerate()
            .find(|(_, e)| e.name.eq_ignore_ascii_case(name))
            .map(|(i, e)| (i as u32, e))
    }

    pub fn session_hash(&self) -> String {
        use sha2::{Digest, Sha256};

        let abs_path = self
            .project_path
            .canonicalize()
            .unwrap_or_else(|_| self.project_path.clone());
        let path_str = abs_path.to_string_lossy();

        let mut hasher = Sha256::new();
        hasher.update(path_str.as_bytes());
        let result = hasher.finalize();

        hex::encode(&result[..4])
    }

    pub fn session_name(&self) -> String {
        format!("{}-{}", self.session_prefix, self.session_hash())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn config_default_has_four_experts() {
        let config = Config::default();
        assert_eq!(config.num_experts, 4);
        assert_eq!(config.experts.len(), 4);
        assert_eq!(config.experts[0].name, "architect");
        assert_eq!(config.experts[1].name, "frontend");
    }

    #[test]
    fn config_with_num_experts_adjusts_list() {
        let config = Config::default().with_num_experts(2);
        assert_eq!(config.num_experts, 2);
        assert_eq!(config.experts.len(), 2);
    }

    #[test]
    fn config_with_num_experts_expands_list() {
        let config = Config::default().with_num_experts(6);
        assert_eq!(config.num_experts, 6);
        assert_eq!(config.experts.len(), 6);
        assert_eq!(config.experts[4].name, "expert4");
        assert_eq!(config.experts[5].name, "expert5");
    }

    #[test]
    fn config_get_expert_returns_correct_expert() {
        let config = Config::default();
        let expert = config.get_expert(0).unwrap();
        assert_eq!(expert.name, "architect");
    }

    #[test]
    fn config_get_expert_returns_none_for_invalid_id() {
        let config = Config::default();
        assert!(config.get_expert(100).is_none());
    }

    #[test]
    fn config_get_expert_by_name_case_insensitive() {
        let config = Config::default();

        let (id, expert) = config.get_expert_by_name("ARCHITECT").unwrap();
        assert_eq!(id, 0);
        assert_eq!(expert.name, "architect");

        let (id, _) = config.get_expert_by_name("Frontend").unwrap();
        assert_eq!(id, 1);
    }

    #[test]
    fn config_session_hash_is_deterministic() {
        let config1 = Config::default().with_project_path(PathBuf::from("/tmp/test"));
        let config2 = Config::default().with_project_path(PathBuf::from("/tmp/test"));

        assert_eq!(config1.session_hash(), config2.session_hash());
    }

    #[test]
    fn config_session_hash_differs_for_different_paths() {
        let config1 = Config::default().with_project_path(PathBuf::from("/tmp/project1"));
        let config2 = Config::default().with_project_path(PathBuf::from("/tmp/project2"));

        assert_ne!(config1.session_hash(), config2.session_hash());
    }

    #[test]
    fn config_session_name_format() {
        let config = Config::default().with_project_path(PathBuf::from("/tmp/test"));
        let name = config.session_name();

        assert!(name.starts_with("macot-"));
        assert_eq!(name.len(), "macot-".len() + 8);
    }

    #[test]
    fn config_loads_from_yaml_file() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        let yaml = r#"
num_experts: 3
session_prefix: "test"
experts:
  - name: "lead"
    color: "cyan"
  - name: "dev"
    color: "magenta"
  - name: "qa"
    color: "yellow"
timeouts:
  agent_ready: 60
  task_completion: 1200
  graceful_shutdown: 20
"#;
        std::fs::write(&config_path, yaml).unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.num_experts, 3);
        assert_eq!(config.session_prefix, "test");
        assert_eq!(config.experts[0].name, "lead");
        assert_eq!(config.timeouts.agent_ready, 60);
    }

    #[test]
    fn config_load_returns_default_when_file_missing() {
        let config = Config::load(Some(PathBuf::from("/nonexistent/config.yaml"))).unwrap();
        assert_eq!(config.num_experts, 4);
        assert_eq!(config.session_prefix, "macot");
    }

    #[test]
    fn config_with_project_path_sets_derived_paths() {
        let config = Config::default().with_project_path(PathBuf::from("/tmp/project"));

        assert_eq!(config.project_path, PathBuf::from("/tmp/project"));
        assert_eq!(config.queue_path, PathBuf::from("/tmp/project/queue"));
        assert_eq!(
            config.instructions_path,
            PathBuf::from("/tmp/project/instructions")
        );
    }

    #[test]
    fn config_serializes_to_yaml() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();

        assert!(yaml.contains("num_experts: 4"));
        assert!(yaml.contains("session_prefix: macot"));
        assert!(yaml.contains("name: architect"));
    }
}
