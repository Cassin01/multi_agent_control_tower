use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertConfig {
    pub name: String, // Display name only
    pub color: String,
    #[serde(default)]
    pub role: String, // Instruction file name (required for instruction loading)
}

impl Default for ExpertConfig {
    fn default() -> Self {
        Self {
            name: "expert".to_string(),
            color: "white".to_string(),
            role: "general".to_string(),
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
    pub session_prefix: String,
    pub experts: Vec<ExpertConfig>,
    #[serde(default)]
    pub timeouts: TimeoutConfig,
    #[serde(default = "Config::default_role_instructions_path")]
    pub role_instructions_path: PathBuf,
    #[serde(skip)]
    pub project_path: PathBuf,
    #[serde(skip)]
    pub queue_path: PathBuf,
    #[serde(skip)]
    pub core_instructions_path: PathBuf,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            session_prefix: "macot".to_string(),
            experts: vec![
                ExpertConfig {
                    name: "Alyosha".to_string(),
                    color: "red".to_string(),
                    role: "architect".to_string(),
                },
                ExpertConfig {
                    name: "Ilyusha".to_string(),
                    color: "blue".to_string(),
                    role: "frontend".to_string(),
                },
                ExpertConfig {
                    name: "Grigory".to_string(),
                    color: "green".to_string(),
                    role: "backend".to_string(),
                },
                ExpertConfig {
                    name: "Katya".to_string(),
                    color: "yellow".to_string(),
                    role: "tester".to_string(),
                },
            ],
            timeouts: TimeoutConfig::default(),
            role_instructions_path: Self::default_role_instructions_path(),
            project_path: PathBuf::new(),
            queue_path: PathBuf::new(),
            core_instructions_path: PathBuf::new(),
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

    /// Default path for role instructions: ~/.config/macot/instructions/
    pub fn default_role_instructions_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("macot")
            .join("instructions")
    }

    pub fn with_project_path(mut self, project_path: PathBuf) -> Self {
        self.queue_path = project_path.join(".macot");
        self.core_instructions_path = project_path.join("instructions");
        self.project_path = project_path;
        self
    }

    /// Returns the number of experts (derived from experts array length)
    pub fn num_experts(&self) -> u32 {
        self.experts.len() as u32
    }

    pub fn with_num_experts(mut self, num_experts: u32) -> Self {
        while self.experts.len() < num_experts as usize {
            let idx = self.experts.len();
            self.experts.push(ExpertConfig {
                name: format!("expert{}", idx),
                color: "white".to_string(),
                role: format!("expert{}", idx),
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
        crate::utils::compute_path_hash(&self.project_path)
    }

    pub fn session_name(&self) -> String {
        format!("{}-{}", self.session_prefix, self.session_hash())
    }

    /// Resolve expert by ID (u32) or name (case-insensitive)
    pub fn resolve_expert_id(&self, expert: &str) -> Result<u32> {
        use anyhow::bail;

        if let Ok(id) = expert.parse::<u32>() {
            if id < self.experts.len() as u32 {
                return Ok(id);
            }
            bail!(
                "Expert ID {} out of range (0-{})",
                id,
                self.experts.len() - 1
            );
        }

        if let Some((id, _)) = self.get_expert_by_name(expert) {
            return Ok(id);
        }

        bail!("Unknown expert: {}", expert)
    }

    /// Get expert name with fallback to default naming
    pub fn get_expert_name(&self, id: u32) -> String {
        self.get_expert(id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| format!("expert{}", id))
    }

    /// Returns the absolute path to the status marker file for a given expert.
    /// Path format: {queue_path}/status/expert{expert_id}
    pub fn status_file_path(&self, expert_id: u32) -> String {
        self.queue_path
            .join("status")
            .join(format!("expert{}", expert_id))
            .to_string_lossy()
            .into_owned()
    }

    /// Get default role for expert from config
    pub fn get_expert_role(&self, id: u32) -> String {
        self.get_expert(id)
            .map(|e| {
                if e.role.is_empty() {
                    e.name.clone() // Fallback to name if role is empty
                } else {
                    e.role.clone()
                }
            })
            .unwrap_or_else(|| format!("expert{}", id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn config_default_has_four_experts() {
        let config = Config::default();
        assert_eq!(config.num_experts(), 4);
        assert_eq!(config.experts.len(), 4);
        assert_eq!(config.experts[0].name, "Alyosha");
        assert_eq!(config.experts[1].name, "Ilyusha");
    }

    #[test]
    fn config_with_num_experts_adjusts_list() {
        let config = Config::default().with_num_experts(2);
        assert_eq!(config.num_experts(), 2);
        assert_eq!(config.experts.len(), 2);
    }

    #[test]
    fn config_with_num_experts_expands_list() {
        let config = Config::default().with_num_experts(6);
        assert_eq!(config.num_experts(), 6);
        assert_eq!(config.experts.len(), 6);
        assert_eq!(config.experts[4].name, "expert4");
        assert_eq!(config.experts[5].name, "expert5");
    }

    #[test]
    fn config_get_expert_returns_correct_expert() {
        let config = Config::default();
        let expert = config.get_expert(0).unwrap();
        assert_eq!(expert.name, "Alyosha");
    }

    #[test]
    fn config_get_expert_returns_none_for_invalid_id() {
        let config = Config::default();
        assert!(config.get_expert(100).is_none());
    }

    #[test]
    fn config_get_expert_by_name_case_insensitive() {
        let config = Config::default();

        let (id, expert) = config.get_expert_by_name("ALYOSHA").unwrap();
        assert_eq!(id, 0);
        assert_eq!(expert.name, "Alyosha");

        let (id, _) = config.get_expert_by_name("Ilyusha").unwrap();
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
        assert_eq!(config.num_experts(), 3);
        assert_eq!(config.session_prefix, "test");
        assert_eq!(config.experts[0].name, "lead");
        assert_eq!(config.timeouts.agent_ready, 60);
    }

    #[test]
    fn config_load_returns_default_when_file_missing() {
        let config = Config::load(Some(PathBuf::from("/nonexistent/config.yaml"))).unwrap();
        assert_eq!(config.num_experts(), 4);
        assert_eq!(config.session_prefix, "macot");
    }

    #[test]
    fn config_with_project_path_sets_derived_paths() {
        let config = Config::default().with_project_path(PathBuf::from("/tmp/project"));

        assert_eq!(config.project_path, PathBuf::from("/tmp/project"));
        assert_eq!(config.queue_path, PathBuf::from("/tmp/project/.macot"));
        assert_eq!(
            config.core_instructions_path,
            PathBuf::from("/tmp/project/instructions")
        );
    }

    #[test]
    fn config_role_instructions_path_defaults_to_config_dir() {
        let config = Config::default();
        let expected = dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("macot")
            .join("instructions");
        assert_eq!(config.role_instructions_path, expected);
    }

    #[test]
    fn config_serializes_to_yaml() {
        let config = Config::default();
        let yaml = serde_yaml::to_string(&config).unwrap();

        // num_experts is no longer serialized; it's derived from experts.len()
        assert!(!yaml.contains("num_experts"));
        assert!(yaml.contains("session_prefix: macot"));
        assert!(yaml.contains("name: Alyosha"));
    }

    #[test]
    fn config_resolve_expert_id_by_number() {
        let config = Config::default();
        assert_eq!(config.resolve_expert_id("0").unwrap(), 0);
        assert_eq!(config.resolve_expert_id("1").unwrap(), 1);
    }

    #[test]
    fn config_resolve_expert_id_by_name() {
        let config = Config::default();
        assert_eq!(config.resolve_expert_id("Alyosha").unwrap(), 0);
        assert_eq!(config.resolve_expert_id("ILYUSHA").unwrap(), 1);
    }

    #[test]
    fn config_resolve_expert_id_invalid() {
        let config = Config::default();
        assert!(config.resolve_expert_id("99").is_err());
        assert!(config.resolve_expert_id("unknown").is_err());
    }

    #[test]
    fn config_get_expert_name_valid() {
        let config = Config::default();
        assert_eq!(config.get_expert_name(0), "Alyosha");
    }

    #[test]
    fn config_get_expert_name_fallback() {
        let config = Config::default();
        assert_eq!(config.get_expert_name(99), "expert99");
    }

    #[test]
    fn config_expert_has_role_field() {
        let config = Config::default();
        assert_eq!(config.experts[0].role, "architect");
        assert_eq!(config.experts[1].role, "frontend");
        assert_eq!(config.experts[2].role, "backend");
        assert_eq!(config.experts[3].role, "tester");
    }

    #[test]
    fn config_get_expert_role_valid() {
        let config = Config::default();
        assert_eq!(config.get_expert_role(0), "architect");
        assert_eq!(config.get_expert_role(1), "frontend");
    }

    #[test]
    fn config_get_expert_role_fallback() {
        let config = Config::default();
        assert_eq!(config.get_expert_role(99), "expert99");
    }

    #[test]
    fn config_expert_role_serde_with_role() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        let yaml = r#"
session_prefix: "test"
experts:
  - name: "Lead Architect"
    color: "cyan"
    role: "architect"
  - name: "Frontend Dev"
    color: "magenta"
    role: "frontend"
"#;
        std::fs::write(&config_path, yaml).unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.experts[0].name, "Lead Architect");
        assert_eq!(config.experts[0].role, "architect");
        assert_eq!(config.experts[1].name, "Frontend Dev");
        assert_eq!(config.experts[1].role, "frontend");
    }

    #[test]
    fn config_status_file_path_format() {
        let config = Config::default().with_project_path(PathBuf::from("/tmp/project"));
        assert_eq!(
            config.status_file_path(0),
            "/tmp/project/.macot/status/expert0"
        );
        assert_eq!(
            config.status_file_path(3),
            "/tmp/project/.macot/status/expert3"
        );
    }

    #[test]
    fn config_expert_role_serde_without_role_defaults() {
        let temp_dir = TempDir::new().unwrap();
        let config_path = temp_dir.path().join("config.yaml");

        let yaml = r#"
session_prefix: "test"
experts:
  - name: "lead"
    color: "cyan"
"#;
        std::fs::write(&config_path, yaml).unwrap();

        let config = Config::load(Some(config_path)).unwrap();
        assert_eq!(config.experts[0].name, "lead");
        // role defaults to empty string via serde(default), get_expert_role falls back to name
        assert_eq!(config.get_expert_role(0), "lead");
    }
}
