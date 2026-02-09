use anyhow::Result;
use std::path::Path;
use tokio::time::{sleep, Duration};

use super::{TmuxManager, TmuxSender};

#[derive(Clone)]
pub struct ClaudeManager<T: TmuxSender = TmuxManager> {
    tmux: T,
}

impl ClaudeManager {
    pub fn new(session_name: String) -> Self {
        Self {
            tmux: TmuxManager::new(session_name),
        }
    }
}

impl<T: TmuxSender> ClaudeManager<T> {
    #[allow(dead_code)]
    pub fn with_sender(sender: T) -> Self {
        Self { tmux: sender }
    }

    pub async fn launch_claude(
        &self,
        expert_id: u32,
        working_dir: &str,
        instruction_file: Option<&Path>,
    ) -> Result<()> {
        let mut args = vec!["--dangerously-skip-permissions".to_string()];

        if let Some(file) = instruction_file {
            args.push("--append-system-prompt".to_string());
            args.push(format!("\"$(cat '{}')\"", file.display()));
        }

        let claude_cmd = format!("cd {} && claude {}", working_dir, args.join(" "));

        self.tmux
            .send_keys_with_enter(expert_id, &claude_cmd)
            .await?;

        Ok(())
    }

    pub async fn send_keys(&self, expert_id: u32, keys: &str) -> Result<()> {
        self.tmux.send_keys(expert_id, keys).await
    }

    pub async fn capture_pane_with_escapes(&self, expert_id: u32) -> Result<String> {
        self.tmux.capture_pane_with_escapes(expert_id).await
    }

    pub async fn send_keys_with_enter(&self, expert_id: u32, keys: &str) -> Result<()> {
        self.tmux.send_keys_with_enter(expert_id, keys).await
    }

    pub async fn send_exit(&self, expert_id: u32) -> Result<()> {
        self.send_keys_with_enter(expert_id, "/exit").await
    }

    #[allow(dead_code)]
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

    #[allow(dead_code)]
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
    use std::sync::{Arc, Mutex};

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

    fn create_mock_manager(mock: MockTmuxSender) -> ClaudeManager<MockTmuxSender> {
        ClaudeManager::with_sender(mock)
    }

    #[test]
    fn claude_manager_creates_with_session_name() {
        let manager = ClaudeManager::new("test-session".to_string());
        assert_eq!(manager.tmux.session_name(), "test-session");
    }

    #[tokio::test]
    async fn launch_claude_with_instruction_file() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        let instruction_file = std::path::PathBuf::from("/tmp/instructions.txt");
        manager
            .launch_claude(0, "/tmp/workdir", Some(instruction_file.as_path()))
            .await
            .unwrap();

        let keys = mock.sent_keys();
        let cmd = keys
            .iter()
            .find(|(_, k)| k.contains("claude"))
            .map(|(_, k)| k.as_str())
            .expect("launch_claude: should send a claude command");
        assert!(
            cmd.contains("--append-system-prompt"),
            "launch_claude: should include --append-system-prompt flag"
        );
        assert!(
            cmd.contains("--dangerously-skip-permissions"),
            "launch_claude: should include --dangerously-skip-permissions flag"
        );
    }

    #[tokio::test]
    async fn launch_claude_without_instruction_file() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        manager.launch_claude(0, "/tmp/workdir", None).await.unwrap();

        let keys = mock.sent_keys();
        let cmd = keys
            .iter()
            .find(|(_, k)| k.contains("claude"))
            .map(|(_, k)| k.as_str())
            .expect("launch_claude: should send a claude command");
        assert!(
            !cmd.contains("--append-system-prompt"),
            "launch_claude: should not include --append-system-prompt when no instruction file"
        );
        assert!(
            cmd.contains("--dangerously-skip-permissions"),
            "launch_claude: should include --dangerously-skip-permissions flag"
        );
    }

    #[tokio::test]
    async fn send_exit_sends_exit_command() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

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
        let manager = create_mock_manager(mock.clone());

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
        let manager = create_mock_manager(mock.clone());

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
        let manager = create_mock_manager(mock.clone());

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
        let manager = create_mock_manager(mock);

        let ready = manager.wait_for_ready(0, 2).await.unwrap();
        assert!(ready, "wait_for_ready: should return true when pane contains 'bypass permissions'");
    }

    #[tokio::test]
    async fn wait_for_ready_returns_false_on_timeout() {
        let mock = MockTmuxSender::new().with_capture_response("some other output");
        let manager = create_mock_manager(mock);

        let ready = manager.wait_for_ready(0, 1).await.unwrap();
        assert!(!ready, "wait_for_ready: should return false when pane never shows ready prompt");
    }

    #[tokio::test]
    async fn send_keys_delegates_to_sender() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        manager.send_keys(3, "test-keys").await.unwrap();

        let keys = mock.sent_keys();
        assert_eq!(keys, vec![(3, "test-keys".to_string())]);
    }

    #[tokio::test]
    async fn send_keys_with_enter_uses_default_trait_behavior() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

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
