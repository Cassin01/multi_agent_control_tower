use anyhow::Result;
use regex::Regex;
use tokio::time::{sleep, Duration};

use super::{TmuxManager, TmuxSender};
use crate::context::ContextStore;

#[derive(Clone)]
pub struct ClaudeManager {
    tmux: TmuxManager,
    context_store: ContextStore,
}

impl ClaudeManager {
    pub fn new(session_name: String, context_store: ContextStore) -> Self {
        Self {
            tmux: TmuxManager::new(session_name),
            context_store,
        }
    }

    pub async fn launch_claude(
        &self,
        expert_id: u32,
        session_hash: &str,
        working_dir: &str,
    ) -> Result<()> {
        let mut args = vec!["--dangerously-skip-permissions".to_string()];

        if let Some(ctx) = self
            .context_store
            .load_expert_context(session_hash, expert_id)
            .await?
        {
            if let Some(session_id) = ctx.claude_session.session_id {
                args.push("--resume".to_string());
                args.push(session_id);
            }
        }

        let claude_cmd = format!("cd {} && claude {}", working_dir, args.join(" "));

        self.tmux
            .send_keys_with_enter(expert_id, &claude_cmd)
            .await?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn capture_session_id(&self, expert_id: u32) -> Result<Option<String>> {
        sleep(Duration::from_secs(2)).await;

        let content = self.tmux.capture_pane(expert_id).await?;

        let re = Regex::new(r"Session:\s*([a-zA-Z0-9_-]+)")?;
        if let Some(caps) = re.captures(&content) {
            return Ok(Some(caps[1].to_string()));
        }

        let re_alt = Regex::new(r"session[:\s]+([a-f0-9-]{36})")?;
        if let Some(caps) = re_alt.captures(&content) {
            return Ok(Some(caps[1].to_string()));
        }

        Ok(None)
    }

    pub async fn send_keys(&self, expert_id: u32, keys: &str) -> Result<()> {
        self.tmux.send_keys(expert_id, keys).await
    }

    pub async fn send_keys_with_enter(&self, expert_id: u32, keys: &str) -> Result<()> {
        self.tmux.send_keys_with_enter(expert_id, keys).await
    }

    pub async fn send_exit(&self, expert_id: u32) -> Result<()> {
        self.send_keys_with_enter(expert_id, "/exit").await
    }

    pub async fn send_clear(&self, expert_id: u32) -> Result<()> {
        self.send_keys_with_enter(expert_id, "/clear").await
    }

    pub async fn wait_for_ready(&self, expert_id: u32, timeout_secs: u64) -> Result<bool> {
        let start = std::time::Instant::now();
        let timeout = Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            let content = self.tmux.capture_pane(expert_id).await?;

            if content.contains("bypass permissions") {
                return Ok(true);
            }

            sleep(Duration::from_millis(500)).await;
        }

        Ok(false)
    }

    pub async fn send_instruction(&self, expert_id: u32, instruction: &str) -> Result<()> {
        for chunk in instruction.as_bytes().chunks(200) {
            let chunk_str = String::from_utf8_lossy(chunk);
            self.send_keys(expert_id, &chunk_str).await?;
            sleep(Duration::from_millis(50)).await;
        }
        self.send_keys(expert_id, "Enter").await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use tempfile::TempDir;

    #[allow(dead_code)]
    fn create_test_manager() -> (ClaudeManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let context_store = ContextStore::new(temp_dir.path().to_path_buf());
        let manager = ClaudeManager::new("test-session".to_string(), context_store);
        (manager, temp_dir)
    }

    #[test]
    fn claude_manager_creates_with_session_name() {
        let context_store = ContextStore::new(PathBuf::from("/tmp"));
        let manager = ClaudeManager::new("test-session".to_string(), context_store);
        assert_eq!(manager.tmux.session_name(), "test-session");
    }

    #[test]
    fn session_id_regex_matches_uuid() {
        let re = Regex::new(r"(?i)session[:\s]+([a-f0-9-]{36})").unwrap();
        let content = "session: a1b2c3d4-e5f6-7890-abcd-ef1234567890";
        assert!(re.is_match(content));
    }

    #[test]
    fn session_id_regex_matches_simple_format() {
        let re = Regex::new(r"Session:\s*([a-zA-Z0-9_-]+)").unwrap();
        let content = "Session: my-session-123";
        let caps = re.captures(content).unwrap();
        assert_eq!(&caps[1], "my-session-123");
    }
}
