use anyhow::Result;
use regex::Regex;
use tokio::time::{sleep, Duration};

use super::{TmuxManager, TmuxSender};
use crate::context::ContextStore;

#[derive(Clone)]
pub struct ClaudeManager<T: TmuxSender = TmuxManager> {
    tmux: T,
    context_store: ContextStore,
}

impl ClaudeManager {
    pub fn new(session_name: String, context_store: ContextStore) -> Self {
        Self {
            tmux: TmuxManager::new(session_name),
            context_store,
        }
    }
}

impl<T: TmuxSender> ClaudeManager<T> {
    #[allow(dead_code)]
    pub fn with_sender(sender: T, context_store: ContextStore) -> Self {
        Self {
            tmux: sender,
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
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    #[derive(Clone)]
    struct MockTmuxSender {
        sent_keys: Arc<Mutex<Vec<(u32, String)>>>,
        capture_response: Arc<Mutex<String>>,
    }

    impl MockTmuxSender {
        fn new() -> Self {
            Self {
                sent_keys: Arc::new(Mutex::new(Vec::new())),
                capture_response: Arc::new(Mutex::new(String::new())),
            }
        }

        fn with_capture_response(self, response: &str) -> Self {
            *self.capture_response.lock().unwrap() = response.to_string();
            self
        }

        fn sent_keys(&self) -> Vec<(u32, String)> {
            self.sent_keys.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl TmuxSender for MockTmuxSender {
        async fn send_keys(&self, pane_id: u32, keys: &str) -> Result<()> {
            self.sent_keys
                .lock()
                .unwrap()
                .push((pane_id, keys.to_string()));
            Ok(())
        }

        async fn capture_pane(&self, _pane_id: u32) -> Result<String> {
            Ok(self.capture_response.lock().unwrap().clone())
        }
    }

    fn create_mock_manager(mock: MockTmuxSender) -> (ClaudeManager<MockTmuxSender>, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let context_store = ContextStore::new(temp_dir.path().to_path_buf());
        let manager = ClaudeManager::with_sender(mock, context_store);
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

    #[tokio::test]
    async fn send_exit_sends_exit_command() {
        let mock = MockTmuxSender::new();
        let (manager, _tmp) = create_mock_manager(mock.clone());

        manager.send_exit(0).await.unwrap();

        let keys = mock.sent_keys();
        assert!(
            keys.iter().any(|(_, k)| k == "/exit"),
            "send_exit: should send /exit command"
        );
    }

    #[tokio::test]
    async fn send_clear_sends_clear_command() {
        let mock = MockTmuxSender::new();
        let (manager, _tmp) = create_mock_manager(mock.clone());

        manager.send_clear(2).await.unwrap();

        let keys = mock.sent_keys();
        assert!(
            keys.iter().any(|(id, k)| *id == 2 && k == "/clear"),
            "send_clear: should send /clear to correct pane"
        );
    }

    #[tokio::test]
    async fn send_instruction_chunks_and_sends_enter() {
        let mock = MockTmuxSender::new();
        let (manager, _tmp) = create_mock_manager(mock.clone());

        manager.send_instruction(1, "hello").await.unwrap();

        let keys = mock.sent_keys();
        assert!(
            keys.iter().any(|(_, k)| k == "hello"),
            "send_instruction: should send instruction text"
        );
        assert_eq!(
            keys.last().map(|(_, k)| k.as_str()),
            Some("Enter"),
            "send_instruction: should end with Enter"
        );
    }

    #[tokio::test]
    async fn send_instruction_splits_large_input() {
        let mock = MockTmuxSender::new();
        let (manager, _tmp) = create_mock_manager(mock.clone());

        let large_input = "a".repeat(500);
        manager.send_instruction(0, &large_input).await.unwrap();

        let keys = mock.sent_keys();
        let text_keys: Vec<_> = keys.iter().filter(|(_, k)| k != "Enter").collect();
        assert!(
            text_keys.len() >= 3,
            "send_instruction: should split 500-byte input into multiple chunks"
        );
    }

    #[tokio::test]
    async fn wait_for_ready_returns_true_when_bypass_found() {
        let mock = MockTmuxSender::new().with_capture_response("bypass permissions");
        let (manager, _tmp) = create_mock_manager(mock);

        let ready = manager.wait_for_ready(0, 2).await.unwrap();
        assert!(ready, "wait_for_ready: should return true when pane contains 'bypass permissions'");
    }

    #[tokio::test]
    async fn wait_for_ready_returns_false_on_timeout() {
        let mock = MockTmuxSender::new().with_capture_response("some other output");
        let (manager, _tmp) = create_mock_manager(mock);

        let ready = manager.wait_for_ready(0, 1).await.unwrap();
        assert!(!ready, "wait_for_ready: should return false when pane never shows ready prompt");
    }

    #[tokio::test]
    async fn send_keys_delegates_to_sender() {
        let mock = MockTmuxSender::new();
        let (manager, _tmp) = create_mock_manager(mock.clone());

        manager.send_keys(3, "test-keys").await.unwrap();

        let keys = mock.sent_keys();
        assert_eq!(keys, vec![(3, "test-keys".to_string())]);
    }

    #[tokio::test]
    async fn send_keys_with_enter_uses_default_trait_behavior() {
        let mock = MockTmuxSender::new();
        let (manager, _tmp) = create_mock_manager(mock.clone());

        manager.send_keys_with_enter(1, "my-command").await.unwrap();

        let keys = mock.sent_keys();
        assert!(
            keys.iter().any(|(_, k)| k == "C-u"),
            "send_keys_with_enter: should send C-u to clear line"
        );
        assert!(
            keys.iter().any(|(_, k)| k == "my-command"),
            "send_keys_with_enter: should send the command"
        );
        assert!(
            keys.iter().any(|(_, k)| k == "Enter"),
            "send_keys_with_enter: should send Enter"
        );
    }
}
