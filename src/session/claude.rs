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
        agents_file: Option<&Path>,
        settings_file: Option<&Path>,
    ) -> Result<()> {
        let mut args = vec!["--dangerously-skip-permissions".to_string()];

        if let Some(file) = instruction_file {
            args.push("--append-system-prompt".to_string());
            args.push(format!(
                "\"$(cat {})\"",
                shell_single_quote(&file.display().to_string())
            ));
        }

        if let Some(file) = agents_file {
            args.push("--agents".to_string());
            args.push(format!(
                "\"$(cat {})\"",
                shell_single_quote(&file.display().to_string())
            ));
        }

        if let Some(file) = settings_file {
            args.push("--settings".to_string());
            args.push(format!(
                "\"$(cat {})\"",
                shell_single_quote(&file.display().to_string())
            ));
        }

        let claude_cmd = format!(
            "cd {} && claude {}",
            shell_single_quote(working_dir),
            args.join(" ")
        );

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

    pub async fn capture_full_history(&self, expert_id: u32) -> Result<String> {
        self.tmux.capture_full_history(expert_id).await
    }

    pub async fn send_keys_with_enter(&self, expert_id: u32, keys: &str) -> Result<()> {
        self.tmux.send_keys_with_enter(expert_id, keys).await
    }

    pub async fn resize_pane(&self, window_id: u32, width: u16, height: u16) -> Result<()> {
        self.tmux.resize_pane(window_id, width, height).await
    }

    pub async fn send_exit(&self, expert_id: u32) -> Result<()> {
        self.send_keys_with_enter(expert_id, "/exit").await
    }

    /// Check whether the foreground process in the pane is a shell (not claude).
    /// Returns `true` if a shell prompt is detected (claude has exited).
    pub async fn is_shell_foreground(&self, expert_id: u32) -> Result<bool> {
        match self.tmux.get_pane_current_command(expert_id).await? {
            Some(cmd) => {
                let cmd_lower = cmd.to_lowercase();
                let cmd_basename = std::path::Path::new(&cmd_lower)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&cmd_lower);
                let shell_names = ["bash", "zsh", "fish", "sh", "dash", "ksh"];
                Ok(shell_names.contains(&cmd_basename))
            }
            None => Ok(false),
        }
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

fn shell_single_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[derive(Clone)]
    struct MockTmuxSender {
        sent_keys: Arc<Mutex<Vec<(u32, String)>>>,
        capture_response: Arc<Mutex<String>>,
        pane_command_response: Arc<Mutex<Option<String>>>,
    }

    impl MockTmuxSender {
        fn new() -> Self {
            Self {
                sent_keys: Arc::new(Mutex::new(Vec::new())),
                capture_response: Arc::new(Mutex::new(String::new())),
                pane_command_response: Arc::new(Mutex::new(None)),
            }
        }

        fn with_capture_response(self, response: &str) -> Self {
            *self.capture_response.lock().unwrap() = response.to_string();
            self
        }

        fn with_pane_command(self, cmd: &str) -> Self {
            *self.pane_command_response.lock().unwrap() = Some(cmd.to_string());
            self
        }

        fn sent_keys(&self) -> Vec<(u32, String)> {
            self.sent_keys.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl TmuxSender for MockTmuxSender {
        async fn send_keys(&self, window_id: u32, keys: &str) -> Result<()> {
            self.sent_keys
                .lock()
                .unwrap()
                .push((window_id, keys.to_string()));
            Ok(())
        }

        async fn capture_pane(&self, _window_id: u32) -> Result<String> {
            Ok(self.capture_response.lock().unwrap().clone())
        }

        async fn get_pane_current_command(&self, _window_id: u32) -> Result<Option<String>> {
            Ok(self.pane_command_response.lock().unwrap().clone())
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
            .launch_claude(
                0,
                "/tmp/workdir",
                Some(instruction_file.as_path()),
                None,
                None,
            )
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

        manager
            .launch_claude(0, "/tmp/workdir", None, None, None)
            .await
            .unwrap();

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
    async fn launch_claude_with_agents_file() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        let agents_file = std::path::PathBuf::from("/tmp/agents.json");
        manager
            .launch_claude(0, "/tmp/workdir", None, Some(agents_file.as_path()), None)
            .await
            .unwrap();

        let keys = mock.sent_keys();
        let cmd = keys
            .iter()
            .find(|(_, k)| k.contains("claude"))
            .map(|(_, k)| k.as_str())
            .expect("launch_claude: should send a claude command");
        assert!(
            cmd.contains("--agents"),
            "launch_claude: should include --agents flag when agents_file is provided"
        );
        assert!(
            cmd.contains("/tmp/agents.json"),
            "launch_claude: should include agents file path"
        );
    }

    #[tokio::test]
    async fn launch_claude_with_both() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        let instruction_file = std::path::PathBuf::from("/tmp/instructions.txt");
        let agents_file = std::path::PathBuf::from("/tmp/agents.json");
        manager
            .launch_claude(
                0,
                "/tmp/workdir",
                Some(instruction_file.as_path()),
                Some(agents_file.as_path()),
                None,
            )
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
            cmd.contains("--agents"),
            "launch_claude: should include --agents flag"
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
            "send_clear: should send /clear to correct window"
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
        assert!(
            ready,
            "wait_for_ready: should return true when window contains 'bypass permissions'"
        );
    }

    #[tokio::test]
    async fn wait_for_ready_returns_false_on_timeout() {
        let mock = MockTmuxSender::new().with_capture_response("some other output");
        let manager = create_mock_manager(mock);

        let ready = manager.wait_for_ready(0, 1).await.unwrap();
        assert!(
            !ready,
            "wait_for_ready: should return false when window never shows ready prompt"
        );
    }

    #[tokio::test]
    async fn capture_full_history_delegates_to_sender() {
        let mock = MockTmuxSender::new().with_capture_response("full history content");
        let manager = create_mock_manager(mock);

        let result = manager.capture_full_history(0).await.unwrap();
        assert_eq!(
            result, "full history content",
            "capture_full_history: should delegate to tmux sender"
        );
    }

    #[tokio::test]
    async fn resize_pane_delegates_to_sender() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        manager.resize_pane(2, 80, 24).await.unwrap();
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
            keys.iter().any(|(_, k)| k == "C-l"),
            "send_keys_with_enter: should send C-l to clear line"
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

    // --- Task 12.1: Resize atomicity tests (P6: Resize Atomicity) ---

    /// A mock that selectively fails `resize_pane` for specified window IDs.
    #[derive(Clone)]
    struct SelectiveFailSender {
        fail_ids: Arc<Mutex<Vec<u32>>>,
        resized_ids: Arc<Mutex<Vec<u32>>>,
    }

    impl SelectiveFailSender {
        fn new(fail_ids: Vec<u32>) -> Self {
            Self {
                fail_ids: Arc::new(Mutex::new(fail_ids)),
                resized_ids: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn resized_ids(&self) -> Vec<u32> {
            let mut ids = self.resized_ids.lock().unwrap().clone();
            ids.sort();
            ids
        }
    }

    #[async_trait::async_trait]
    impl TmuxSender for SelectiveFailSender {
        async fn send_keys(&self, _window_id: u32, _keys: &str) -> Result<()> {
            Ok(())
        }

        async fn capture_pane(&self, _window_id: u32) -> Result<String> {
            Ok(String::new())
        }

        async fn resize_pane(&self, window_id: u32, _width: u16, _height: u16) -> Result<()> {
            if self.fail_ids.lock().unwrap().contains(&window_id) {
                return Err(anyhow::anyhow!("resize failed for window {}", window_id));
            }
            self.resized_ids.lock().unwrap().push(window_id);
            Ok(())
        }
    }

    #[tokio::test]
    async fn resize_all_panes_parallel_all_succeed() {
        let sender = SelectiveFailSender::new(vec![]);
        let manager = ClaudeManager::with_sender(sender.clone());

        let resize_futures: Vec<_> = (0..4u32)
            .map(|id| {
                let m = &manager;
                async move {
                    if let Err(e) = m.resize_pane(id, 80, 24).await {
                        tracing::warn!("resize failed for {}: {}", id, e);
                    }
                }
            })
            .collect();
        futures::future::join_all(resize_futures).await;

        assert_eq!(
            sender.resized_ids(),
            vec![0, 1, 2, 3],
            "resize_all: all 4 panes should be resized"
        );
    }

    #[tokio::test]
    async fn resize_one_failure_does_not_block_others() {
        let sender = SelectiveFailSender::new(vec![1]);
        let manager = ClaudeManager::with_sender(sender.clone());

        let resize_futures: Vec<_> = (0..4u32)
            .map(|id| {
                let m = &manager;
                async move {
                    if let Err(e) = m.resize_pane(id, 80, 24).await {
                        tracing::warn!("resize failed for {}: {}", id, e);
                    }
                }
            })
            .collect();
        futures::future::join_all(resize_futures).await;

        assert_eq!(
            sender.resized_ids(),
            vec![0, 2, 3],
            "resize_one_fail: experts 0, 2, 3 should succeed despite expert 1 failing"
        );
    }

    #[tokio::test]
    async fn resize_multiple_failures_do_not_block() {
        let sender = SelectiveFailSender::new(vec![0, 2]);
        let manager = ClaudeManager::with_sender(sender.clone());

        let resize_futures: Vec<_> = (0..4u32)
            .map(|id| {
                let m = &manager;
                async move {
                    if let Err(e) = m.resize_pane(id, 80, 24).await {
                        tracing::warn!("resize failed for {}: {}", id, e);
                    }
                }
            })
            .collect();
        futures::future::join_all(resize_futures).await;

        assert_eq!(
            sender.resized_ids(),
            vec![1, 3],
            "resize_multi_fail: experts 1, 3 should succeed despite experts 0, 2 failing"
        );
    }

    #[tokio::test]
    async fn launch_claude_with_settings_file() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        let settings_file = std::path::PathBuf::from("/tmp/settings.json");
        manager
            .launch_claude(0, "/tmp/workdir", None, None, Some(settings_file.as_path()))
            .await
            .unwrap();

        let keys = mock.sent_keys();
        let cmd = keys
            .iter()
            .find(|(_, k)| k.contains("claude"))
            .map(|(_, k)| k.as_str())
            .expect("launch_claude: should send a claude command");
        assert!(
            cmd.contains("--settings"),
            "launch_claude: should include --settings flag when settings_file is provided"
        );
        assert!(
            cmd.contains("/tmp/settings.json"),
            "launch_claude: should include settings file path"
        );
    }

    #[tokio::test]
    async fn launch_claude_with_all_three_files() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        let instruction_file = std::path::PathBuf::from("/tmp/instructions.txt");
        let agents_file = std::path::PathBuf::from("/tmp/agents.json");
        let settings_file = std::path::PathBuf::from("/tmp/settings.json");
        manager
            .launch_claude(
                0,
                "/tmp/workdir",
                Some(instruction_file.as_path()),
                Some(agents_file.as_path()),
                Some(settings_file.as_path()),
            )
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
            cmd.contains("--agents"),
            "launch_claude: should include --agents flag"
        );
        assert!(
            cmd.contains("--settings"),
            "launch_claude: should include --settings flag"
        );
    }

    #[tokio::test]
    async fn launch_claude_quotes_paths_for_shell_safety() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock.clone());

        let instruction_file = std::path::PathBuf::from("/tmp/o'hara/instructions.txt");
        manager
            .launch_claude(
                0,
                "/tmp/work dir/it's",
                Some(instruction_file.as_path()),
                None,
                None,
            )
            .await
            .unwrap();

        let keys = mock.sent_keys();
        let cmd = keys
            .iter()
            .find(|(_, k)| k.contains("claude"))
            .map(|(_, k)| k.as_str())
            .expect("launch_claude: should send a claude command");

        assert!(
            cmd.contains("cd '/tmp/work dir/it'\\''s' && claude"),
            "launch_claude: should safely quote working dir"
        );
        assert!(
            cmd.contains("$(cat '/tmp/o'\\''hara/instructions.txt')"),
            "launch_claude: should safely quote file paths"
        );
    }

    #[tokio::test]
    async fn resize_all_fail_no_panic() {
        let sender = SelectiveFailSender::new(vec![0, 1, 2, 3]);
        let manager = ClaudeManager::with_sender(sender.clone());

        let resize_futures: Vec<_> = (0..4u32)
            .map(|id| {
                let m = &manager;
                async move {
                    if let Err(e) = m.resize_pane(id, 80, 24).await {
                        tracing::warn!("resize failed for {}: {}", id, e);
                    }
                }
            })
            .collect();
        futures::future::join_all(resize_futures).await;

        assert!(
            sender.resized_ids().is_empty(),
            "resize_all_fail: no panes should be resized when all fail"
        );
    }

    #[tokio::test]
    async fn is_shell_foreground_returns_true_for_bash() {
        let mock = MockTmuxSender::new().with_pane_command("bash");
        let manager = create_mock_manager(mock);
        assert!(
            manager.is_shell_foreground(0).await.unwrap(),
            "is_shell_foreground: should return true for bash"
        );
    }

    #[tokio::test]
    async fn is_shell_foreground_returns_true_for_zsh() {
        let mock = MockTmuxSender::new().with_pane_command("zsh");
        let manager = create_mock_manager(mock);
        assert!(
            manager.is_shell_foreground(0).await.unwrap(),
            "is_shell_foreground: should return true for zsh"
        );
    }

    #[tokio::test]
    async fn is_shell_foreground_returns_false_for_claude() {
        let mock = MockTmuxSender::new().with_pane_command("claude");
        let manager = create_mock_manager(mock);
        assert!(
            !manager.is_shell_foreground(0).await.unwrap(),
            "is_shell_foreground: should return false for claude"
        );
    }

    #[tokio::test]
    async fn is_shell_foreground_returns_false_for_node() {
        let mock = MockTmuxSender::new().with_pane_command("node");
        let manager = create_mock_manager(mock);
        assert!(
            !manager.is_shell_foreground(0).await.unwrap(),
            "is_shell_foreground: should return false for node"
        );
    }

    #[tokio::test]
    async fn is_shell_foreground_returns_false_for_ssh() {
        let mock = MockTmuxSender::new().with_pane_command("ssh");
        let manager = create_mock_manager(mock);
        assert!(
            !manager.is_shell_foreground(0).await.unwrap(),
            "is_shell_foreground: should return false for ssh (not a shell)"
        );
    }

    #[tokio::test]
    async fn is_shell_foreground_returns_false_when_none() {
        let mock = MockTmuxSender::new();
        let manager = create_mock_manager(mock);
        assert!(
            !manager.is_shell_foreground(0).await.unwrap(),
            "is_shell_foreground: should return false when no command detected"
        );
    }
}
