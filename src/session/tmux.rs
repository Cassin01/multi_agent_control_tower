use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use std::process::Stdio;
use tokio::process::Command;

use crate::config::Config;

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_name: String,
    pub project_path: String,
    pub num_experts: u32,
    pub created_at: DateTime<Utc>,
}

pub struct TmuxManager {
    session_name: String,
}

impl TmuxManager {
    pub fn new(session_name: String) -> Self {
        Self { session_name }
    }

    pub fn from_config(config: &Config) -> Self {
        Self::new(config.session_name())
    }

    #[allow(dead_code)]
    pub fn session_name(&self) -> &str {
        &self.session_name
    }

    pub async fn session_exists(&self) -> bool {
        Command::new("tmux")
            .args(["has-session", "-t", &self.session_name])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .await
            .map(|s| s.success())
            .unwrap_or(false)
    }

    pub async fn create_session(&self, num_panes: u32, working_dir: &str) -> Result<()> {
        Command::new("tmux")
            .args([
                "new-session",
                "-d",
                "-s",
                &self.session_name,
                "-c",
                working_dir,
            ])
            .output()
            .await
            .context("Failed to create tmux session")?;

        for i in 1..num_panes {
            Command::new("tmux")
                .args([
                    "split-window",
                    "-t",
                    &format!("{}:0", self.session_name),
                    "-c",
                    working_dir,
                ])
                .output()
                .await
                .context(format!("Failed to create pane {}", i))?;

            Command::new("tmux")
                .args([
                    "select-layout",
                    "-t",
                    &format!("{}:0", self.session_name),
                    "tiled",
                ])
                .output()
                .await?;
        }

        Ok(())
    }

    pub async fn set_env(&self, key: &str, value: &str) -> Result<()> {
        Command::new("tmux")
            .args(["setenv", "-t", &self.session_name, key, value])
            .output()
            .await
            .context(format!("Failed to set env {}", key))?;
        Ok(())
    }

    pub async fn get_env(&self, key: &str) -> Result<Option<String>> {
        let output = Command::new("tmux")
            .args(["showenv", "-t", &self.session_name, key])
            .output()
            .await
            .context(format!("Failed to get env {}", key))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(value) = stdout.strip_prefix(&format!("{}=", key)) {
                return Ok(Some(value.trim().to_string()));
            }
        }

        Ok(None)
    }

    pub async fn send_keys(&self, pane_id: u32, keys: &str) -> Result<()> {
        Command::new("tmux")
            .args([
                "send-keys",
                "-t",
                &format!("{}:0.{}", self.session_name, pane_id),
                keys,
            ])
            .output()
            .await
            .context(format!("Failed to send keys to pane {}", pane_id))?;
        Ok(())
    }

    pub async fn send_keys_with_enter(&self, pane_id: u32, keys: &str) -> Result<()> {
        self.send_keys(pane_id, keys).await?;
        self.send_keys(pane_id, "Enter").await?;
        Ok(())
    }

    pub async fn capture_pane(&self, pane_id: u32) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &format!("{}:0.{}", self.session_name, pane_id),
                "-p",
            ])
            .output()
            .await
            .context(format!("Failed to capture pane {}", pane_id))?;

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    pub async fn kill_session(&self) -> Result<()> {
        Command::new("tmux")
            .args(["kill-session", "-t", &self.session_name])
            .output()
            .await
            .context("Failed to kill tmux session")?;
        Ok(())
    }

    pub async fn set_pane_title(&self, pane_id: u32, title: &str) -> Result<()> {
        Command::new("tmux")
            .args([
                "select-pane",
                "-t",
                &format!("{}:0.{}", self.session_name, pane_id),
                "-T",
                title,
            ])
            .output()
            .await
            .context(format!("Failed to set pane title for pane {}", pane_id))?;
        Ok(())
    }

    pub async fn list_all_macot_sessions() -> Result<Vec<SessionInfo>> {
        let output = Command::new("tmux")
            .args(["list-sessions", "-F", "#{session_name}"])
            .output()
            .await?;

        if !output.status.success() {
            return Ok(Vec::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut sessions = Vec::new();

        for line in stdout.lines() {
            if line.starts_with("macot-") {
                let manager = TmuxManager::new(line.to_string());

                let project_path = manager
                    .get_env("MACOT_PROJECT_PATH")
                    .await?
                    .unwrap_or_else(|| "unknown".to_string());

                let num_experts = manager
                    .get_env("MACOT_NUM_EXPERTS")
                    .await?
                    .and_then(|s| s.parse().ok())
                    .unwrap_or(0);

                let created_at = manager
                    .get_env("MACOT_CREATED_AT")
                    .await?
                    .and_then(|s| DateTime::parse_from_rfc3339(&s).ok())
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(Utc::now);

                sessions.push(SessionInfo {
                    session_name: line.to_string(),
                    project_path,
                    num_experts,
                    created_at,
                });
            }
        }

        Ok(sessions)
    }

    pub async fn init_session_metadata(
        &self,
        project_path: &str,
        num_experts: u32,
    ) -> Result<()> {
        self.set_env("MACOT_PROJECT_PATH", project_path).await?;
        self.set_env("MACOT_NUM_EXPERTS", &num_experts.to_string())
            .await?;
        self.set_env("MACOT_CREATED_AT", &Utc::now().to_rfc3339())
            .await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tmux_manager_new_sets_session_name() {
        let manager = TmuxManager::new("test-session".to_string());
        assert_eq!(manager.session_name(), "test-session");
    }

    #[test]
    fn tmux_manager_from_config_uses_config_session_name() {
        use std::path::PathBuf;

        let config = Config::default().with_project_path(PathBuf::from("/tmp/test"));
        let manager = TmuxManager::from_config(&config);

        assert!(manager.session_name().starts_with("macot-"));
    }
}
