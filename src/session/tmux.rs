use anyhow::{bail, Context, Result};
use chrono::{DateTime, Utc};
use std::collections::HashMap;
use std::process::{Output, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::process::Command;

use crate::config::Config;

fn check_tmux_output(output: Output, context: &str) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{}: tmux exited with {}: {}",
            context,
            output.status,
            stderr.trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

fn check_tmux_status(output: Output, context: &str) -> Result<()> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "{}: tmux exited with {}: {}",
            context,
            output.status,
            stderr.trim()
        );
    }
    Ok(())
}

fn parse_pane_paths(stdout: &str) -> HashMap<u32, String> {
    let mut paths = HashMap::new();
    for line in stdout.lines() {
        let mut parts = line.splitn(2, '\t');
        let Some(window_str) = parts.next() else {
            continue;
        };
        let Some(path) = parts.next() else {
            continue;
        };
        let Ok(window_id) = window_str.parse::<u32>() else {
            continue;
        };
        let path = path.trim();
        if !path.is_empty() {
            paths.insert(window_id, path.to_string());
        }
    }
    paths
}

static NEXT_BUFFER_ID: AtomicU64 = AtomicU64::new(1);

fn next_tmux_buffer_name(window_id: u32) -> String {
    let id = NEXT_BUFFER_ID.fetch_add(1, Ordering::Relaxed);
    format!("macot-{}-{}-{}", std::process::id(), window_id, id)
}

/// Trait for sending keys to and capturing output from tmux windows.
/// Extracted to allow mocking in tests.
#[async_trait::async_trait]
pub trait TmuxSender: Send + Sync {
    async fn send_keys(&self, window_id: u32, keys: &str) -> Result<()>;
    async fn capture_pane(&self, window_id: u32) -> Result<String>;

    fn pre_enter_delay(&self) -> std::time::Duration {
        std::time::Duration::ZERO
    }

    /// Send text content to a pane. For multiline text, implementations should
    /// use bracketed paste to prevent newlines from acting as Enter keypresses.
    /// Default falls back to send_keys (suitable for mocks and single-line text).
    async fn send_text(&self, window_id: u32, text: &str) -> Result<()> {
        self.send_keys(window_id, text).await
    }

    async fn send_keys_with_enter(&self, window_id: u32, keys: &str) -> Result<()> {
        self.send_keys(window_id, "C-l").await?;
        self.send_text(window_id, keys).await?;
        let delay = self.pre_enter_delay();
        if !delay.is_zero() {
            tokio::time::sleep(delay).await;
        }
        self.send_keys(window_id, "Enter").await?;
        Ok(())
    }

    async fn capture_pane_with_escapes(&self, window_id: u32) -> Result<String> {
        self.capture_pane(window_id).await
    }

    async fn capture_full_history(&self, window_id: u32) -> Result<String> {
        self.capture_pane_with_escapes(window_id).await
    }

    async fn resize_pane(&self, _window_id: u32, _width: u16, _height: u16) -> Result<()> {
        Ok(())
    }

    /// Get the current foreground command running in a tmux pane.
    /// Returns `None` by default (for mocks); real implementations should
    /// query tmux for `pane_current_command`.
    async fn get_pane_current_command(&self, _window_id: u32) -> Result<Option<String>> {
        Ok(None)
    }
}

#[async_trait::async_trait]
impl TmuxSender for TmuxManager {
    async fn send_keys(&self, window_id: u32, keys: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "send-keys",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
                keys,
            ])
            .output()
            .await
            .context(format!("Failed to send keys to window {window_id}"))?;
        check_tmux_status(output, &format!("send-keys to window {window_id}"))
    }

    fn pre_enter_delay(&self) -> std::time::Duration {
        std::time::Duration::from_millis(300)
    }

    async fn send_text(&self, window_id: u32, text: &str) -> Result<()> {
        if !text.contains('\n') {
            return self.send_keys(window_id, text).await;
        }
        let target = format!("{}:{}", self.session_name, window_id);
        let buffer_name = next_tmux_buffer_name(window_id);
        let output = Command::new("tmux")
            .args(["set-buffer", "-b", &buffer_name, "--", text])
            .output()
            .await
            .context("Failed to set tmux buffer")?;
        check_tmux_status(output, "set-buffer")?;

        let output = Command::new("tmux")
            .args([
                "paste-buffer",
                "-d",
                "-p",
                "-b",
                &buffer_name,
                "-t",
                &target,
            ])
            .output()
            .await
            .context(format!("Failed to paste buffer to window {window_id}"))?;
        check_tmux_status(output, &format!("paste-buffer to window {window_id}"))
    }

    async fn capture_pane(&self, window_id: u32) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
                "-p",
            ])
            .output()
            .await
            .context(format!("Failed to capture window {window_id}"))?;
        check_tmux_output(output, &format!("capture-pane {window_id}"))
    }

    async fn capture_pane_with_escapes(&self, window_id: u32) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-e",
                "-p",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
            ])
            .output()
            .await
            .context(format!("Failed to capture window {window_id} with escapes"))?;
        check_tmux_output(output, &format!("capture-pane-with-escapes {window_id}"))
    }

    async fn capture_full_history(&self, window_id: u32) -> Result<String> {
        let output = Command::new("tmux")
            .args([
                "capture-pane",
                "-e",
                "-J",
                "-p",
                "-S",
                "-",
                "-E",
                "-",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
            ])
            .output()
            .await
            .context(format!(
                "Failed to capture full history of window {window_id}"
            ))?;
        check_tmux_output(output, &format!("capture-full-history {window_id}"))
    }

    async fn resize_pane(&self, window_id: u32, width: u16, height: u16) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "resize-pane",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
                "-x",
                &width.to_string(),
                "-y",
                &height.to_string(),
            ])
            .output()
            .await
            .context(format!("Failed to resize window {window_id}"))?;
        check_tmux_status(output, &format!("resize-pane {window_id}"))
    }

    async fn get_pane_current_command(&self, window_id: u32) -> Result<Option<String>> {
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
                "-p",
                "#{pane_current_command}",
            ])
            .output()
            .await
            .context(format!(
                "Failed to get pane_current_command for window {window_id}"
            ))?;

        let stdout = check_tmux_output(
            output,
            &format!("get pane_current_command for window {window_id}"),
        )?;
        let cmd = stdout.trim().to_string();
        if cmd.is_empty() {
            Ok(None)
        } else {
            Ok(Some(cmd))
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub session_name: String,
    pub project_path: String,
    pub num_experts: u32,
    pub created_at: DateTime<Utc>,
}

#[derive(Clone)]
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

    pub async fn create_session(&self, num_windows: u32, working_dir: &str) -> Result<()> {
        let output = Command::new("tmux")
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
        check_tmux_status(output, "new-session")?;

        let output = Command::new("tmux")
            .args([
                "set-option",
                "-t",
                &self.session_name,
                "history-limit",
                "10000",
            ])
            .output()
            .await
            .context("Failed to set history-limit")?;
        check_tmux_status(output, "set history-limit")?;

        for i in 1..num_windows {
            let output = Command::new("tmux")
                .args(["new-window", "-t", &self.session_name, "-c", working_dir])
                .output()
                .await
                .context(format!("Failed to create window {i}"))?;
            check_tmux_status(output, &format!("new-window {i}"))?;
        }

        Ok(())
    }

    pub async fn set_env(&self, key: &str, value: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args(["setenv", "-t", &self.session_name, key, value])
            .output()
            .await
            .context(format!("Failed to set env {key}"))?;
        check_tmux_status(output, &format!("setenv {key}"))
    }

    pub async fn get_env(&self, key: &str) -> Result<Option<String>> {
        let output = Command::new("tmux")
            .args(["showenv", "-t", &self.session_name, key])
            .output()
            .await
            .context(format!("Failed to get env {key}"))?;

        if output.status.success() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if let Some(value) = stdout.strip_prefix(&format!("{key}=")) {
                return Ok(Some(value.trim().to_string()));
            }
        }

        Ok(None)
    }

    pub async fn kill_session(&self) -> Result<()> {
        let output = Command::new("tmux")
            .args(["kill-session", "-t", &self.session_name])
            .output()
            .await
            .context("Failed to kill tmux session")?;
        check_tmux_status(output, "kill-session")
    }

    pub async fn set_pane_title(&self, window_id: u32, title: &str) -> Result<()> {
        let output = Command::new("tmux")
            .args([
                "select-pane",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
                "-T",
                title,
            ])
            .output()
            .await
            .context(format!("Failed to set pane title for window {window_id}"))?;
        check_tmux_status(output, &format!("select-pane {window_id}"))
    }

    #[allow(dead_code)]
    pub async fn get_pane_current_path(&self, window_id: u32) -> Result<Option<String>> {
        let output = Command::new("tmux")
            .args([
                "display-message",
                "-t",
                &format!("{}:{}", self.session_name, window_id),
                "-p",
                "#{pane_current_path}",
            ])
            .output()
            .await
            .context(format!(
                "Failed to get pane_current_path for window {window_id}"
            ))?;

        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if path.is_empty() {
                Ok(None)
            } else {
                Ok(Some(path))
            }
        } else {
            Ok(None)
        }
    }

    /// Get current working directories for all panes in this session.
    /// Key is tmux window index.
    pub async fn get_all_pane_current_paths(&self) -> Result<HashMap<u32, String>> {
        let output = Command::new("tmux")
            .args([
                "list-panes",
                "-s",
                "-t",
                &self.session_name,
                "-F",
                "#{window_index}\t#{pane_current_path}",
            ])
            .output()
            .await
            .context("Failed to list pane_current_path for session")?;

        if !output.status.success() {
            return Ok(HashMap::new());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        Ok(parse_pane_paths(&stdout))
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

    pub async fn init_session_metadata(&self, project_path: &str, num_experts: u32) -> Result<()> {
        self.set_env("MACOT_PROJECT_PATH", project_path).await?;
        self.set_env("MACOT_NUM_EXPERTS", &num_experts.to_string())
            .await?;
        self.set_env("MACOT_CREATED_AT", &Utc::now().to_rfc3339())
            .await?;
        Ok(())
    }

    /// Load session metadata from tmux environment variables.
    ///
    /// Fields like `project_path`, `num_experts`, and `created_at` are returned as `Option`
    /// so each caller can apply its own contextually-appropriate default.
    pub async fn load_session_metadata(&self) -> Result<SessionMetadata> {
        let project_path = self.get_env("MACOT_PROJECT_PATH").await?;

        let num_experts = self
            .get_env("MACOT_NUM_EXPERTS")
            .await?
            .and_then(|s| s.parse().ok());

        let created_at = self.get_env("MACOT_CREATED_AT").await?;

        let queue_path = self
            .get_env("MACOT_QUEUE_PATH")
            .await?
            .unwrap_or_else(|| "/tmp/macot".to_string());

        Ok(SessionMetadata {
            project_path,
            num_experts,
            created_at,
            queue_path,
        })
    }
}

/// Metadata stored as tmux environment variables for a running session.
///
/// `project_path`, `num_experts`, and `created_at` are `Option` because the tmux
/// env vars may not be set; each caller applies its own contextual default.
#[derive(Debug, Clone)]
pub struct SessionMetadata {
    pub project_path: Option<String>,
    pub num_experts: Option<u32>,
    pub created_at: Option<String>,
    pub queue_path: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::process::ExitStatusExt;
    use std::process::ExitStatus;

    fn make_output(code: i32, stdout: &str, stderr: &str) -> Output {
        Output {
            status: ExitStatus::from_raw(code << 8), // Unix: exit code is in bits 8-15
            stdout: stdout.as_bytes().to_vec(),
            stderr: stderr.as_bytes().to_vec(),
        }
    }

    #[test]
    fn check_tmux_output_success_returns_stdout() {
        let output = make_output(0, "pane content\n", "");
        let result = check_tmux_output(output, "test-cmd");
        assert_eq!(
            result.unwrap(),
            "pane content\n",
            "check_tmux_output: success should return stdout"
        );
    }

    #[test]
    fn check_tmux_output_failure_returns_error_with_stderr() {
        let output = make_output(1, "", "no such pane");
        let result = check_tmux_output(output, "capture-pane");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("capture-pane") && msg.contains("no such pane"),
            "check_tmux_output: error should contain context and stderr, got: {}",
            msg
        );
    }

    #[test]
    fn check_tmux_status_success_returns_ok() {
        let output = make_output(0, "", "");
        let result = check_tmux_status(output, "test-cmd");
        assert!(
            result.is_ok(),
            "check_tmux_status: success should return Ok"
        );
    }

    #[test]
    fn check_tmux_status_failure_returns_error() {
        let output = make_output(1, "", "session not found");
        let result = check_tmux_status(output, "send-keys");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("send-keys") && msg.contains("session not found"),
            "check_tmux_status: error should contain context and stderr, got: {}",
            msg
        );
    }

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

    #[tokio::test]
    async fn resize_pane_default_is_noop() {
        struct NoopSender;

        #[async_trait::async_trait]
        impl TmuxSender for NoopSender {
            async fn send_keys(&self, _window_id: u32, _keys: &str) -> Result<()> {
                Ok(())
            }
            async fn capture_pane(&self, _window_id: u32) -> Result<String> {
                Ok(String::new())
            }
        }

        let sender = NoopSender;
        let result = sender.resize_pane(0, 80, 24).await;
        assert!(
            result.is_ok(),
            "resize_pane: default trait impl should be a no-op that returns Ok"
        );
    }

    /// Mock TmuxSender that only implements the required methods (not `capture_pane_with_escapes`).
    /// Verifies the default trait implementation falls back to `capture_pane`.
    struct MockTmuxSender {
        capture_output: String,
    }

    #[async_trait::async_trait]
    impl TmuxSender for MockTmuxSender {
        async fn send_keys(&self, _window_id: u32, _keys: &str) -> Result<()> {
            Ok(())
        }

        async fn capture_pane(&self, _window_id: u32) -> Result<String> {
            Ok(self.capture_output.clone())
        }
    }

    #[tokio::test]
    async fn capture_full_history_default_falls_back_to_capture_pane_with_escapes() {
        let mock = MockTmuxSender {
            capture_output: "mock full history".to_string(),
        };

        let result = mock.capture_full_history(0).await.unwrap();
        assert_eq!(
            result, "mock full history",
            "capture_full_history: default impl should fall back to capture_pane_with_escapes → capture_pane"
        );
    }

    #[tokio::test]
    async fn capture_pane_with_escapes_default_falls_back() {
        let mock = MockTmuxSender {
            capture_output: "mock pane content".to_string(),
        };

        let result = mock.capture_pane_with_escapes(0).await.unwrap();
        assert_eq!(
            result, "mock pane content",
            "capture_pane_with_escapes: default impl should fall back to capture_pane"
        );
    }

    #[tokio::test]
    async fn send_text_default_falls_back_to_send_keys() {
        use std::sync::{Arc, Mutex};

        struct RecordingSender {
            sent: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl TmuxSender for RecordingSender {
            async fn send_keys(&self, _window_id: u32, keys: &str) -> Result<()> {
                self.sent.lock().unwrap().push(keys.to_string());
                Ok(())
            }
            async fn capture_pane(&self, _window_id: u32) -> Result<String> {
                Ok(String::new())
            }
        }

        let sent = Arc::new(Mutex::new(Vec::new()));
        let sender = RecordingSender { sent: sent.clone() };

        sender.send_text(0, "multiline\ntext").await.unwrap();

        let recorded = sent.lock().unwrap();
        assert_eq!(
            recorded.as_slice(),
            &["multiline\ntext"],
            "send_text: default impl should fall back to send_keys"
        );
    }

    #[tokio::test]
    async fn send_keys_with_enter_routes_text_through_send_text() {
        use std::sync::{Arc, Mutex};

        struct TextTracker {
            keys: Arc<Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl TmuxSender for TextTracker {
            async fn send_keys(&self, _window_id: u32, keys: &str) -> Result<()> {
                self.keys.lock().unwrap().push(format!("keys:{}", keys));
                Ok(())
            }
            async fn send_text(&self, _window_id: u32, text: &str) -> Result<()> {
                self.keys.lock().unwrap().push(format!("text:{}", text));
                Ok(())
            }
            async fn capture_pane(&self, _window_id: u32) -> Result<String> {
                Ok(String::new())
            }
        }

        let keys = Arc::new(Mutex::new(Vec::new()));
        let tracker = TextTracker { keys: keys.clone() };

        tracker
            .send_keys_with_enter(0, "hello\nworld")
            .await
            .unwrap();

        let recorded = keys.lock().unwrap();
        assert_eq!(
            recorded[0], "keys:C-l",
            "send_keys_with_enter: should send C-l via send_keys"
        );
        assert_eq!(
            recorded[1], "text:hello\nworld",
            "send_keys_with_enter: should route text through send_text"
        );
        assert_eq!(
            recorded[2], "keys:Enter",
            "send_keys_with_enter: should send Enter via send_keys"
        );
    }

    #[test]
    fn next_tmux_buffer_name_is_unique() {
        let a = next_tmux_buffer_name(0);
        let b = next_tmux_buffer_name(0);
        assert_ne!(
            a, b,
            "next_tmux_buffer_name: successive calls should be unique"
        );
        assert!(
            a.starts_with("macot-"),
            "next_tmux_buffer_name: should use macot- prefix"
        );
    }

    #[test]
    fn parse_pane_paths_multiple_windows() {
        let stdout = "0\t/home/user/project\n1\t/home/user/docs\n2\t/tmp\n";
        let paths = parse_pane_paths(stdout);
        assert_eq!(
            paths.len(),
            3,
            "parse_pane_paths: should parse all window entries"
        );
        assert_eq!(paths[&0], "/home/user/project");
        assert_eq!(paths[&1], "/home/user/docs");
        assert_eq!(paths[&2], "/tmp");
    }

    #[test]
    fn parse_pane_paths_single_window() {
        let stdout = "0\t/home/user/project\n";
        let paths = parse_pane_paths(stdout);
        assert_eq!(
            paths.len(),
            1,
            "parse_pane_paths: should parse single entry"
        );
        assert_eq!(paths[&0], "/home/user/project");
    }

    #[test]
    fn parse_pane_paths_empty_input() {
        let paths = parse_pane_paths("");
        assert!(
            paths.is_empty(),
            "parse_pane_paths: empty input should return empty map"
        );
    }

    #[test]
    fn parse_pane_paths_skips_malformed_lines() {
        let stdout = "0\t/valid/path\nnot_a_number\t/skip\n\n1\t/another/path\n";
        let paths = parse_pane_paths(stdout);
        assert_eq!(
            paths.len(),
            2,
            "parse_pane_paths: should skip malformed lines"
        );
        assert_eq!(paths[&0], "/valid/path");
        assert_eq!(paths[&1], "/another/path");
    }

    #[test]
    fn check_tmux_status_with_nonzero_exit_returns_error() {
        let output = make_output(2, "", "unknown command");
        let result = check_tmux_status(output, "setenv");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("setenv"),
            "check_tmux_status: error should contain context string, got: {}",
            msg
        );
    }

    #[test]
    fn check_tmux_status_with_empty_stderr_includes_context() {
        let output = make_output(127, "", "");
        let result = check_tmux_status(output, "kill-session");
        let err = result.unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("kill-session"),
            "check_tmux_status: error with empty stderr should still include context, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn get_pane_current_command_default_returns_none() {
        struct NoopSender;

        #[async_trait::async_trait]
        impl TmuxSender for NoopSender {
            async fn send_keys(&self, _window_id: u32, _keys: &str) -> Result<()> {
                Ok(())
            }
            async fn capture_pane(&self, _window_id: u32) -> Result<String> {
                Ok(String::new())
            }
        }

        let sender = NoopSender;
        let result = sender.get_pane_current_command(0).await.unwrap();
        assert!(
            result.is_none(),
            "get_pane_current_command: default trait impl should return None"
        );
    }
}
