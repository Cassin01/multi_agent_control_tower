use anyhow::Result;
use chrono::{DateTime, Utc};
use ratatui::style::Color;

use super::TmuxManager;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AgentStatus {
    #[default]
    Idle,
    Thinking,
    Executing,
    Error,
    Unknown,
}

impl AgentStatus {
    pub fn symbol(&self) -> &'static str {
        match self {
            AgentStatus::Idle => "○",
            AgentStatus::Thinking => "◐",
            AgentStatus::Executing => "●",
            AgentStatus::Error => "✗",
            AgentStatus::Unknown => "?",
        }
    }

    pub fn color(&self) -> Color {
        match self {
            AgentStatus::Idle => Color::Gray,
            AgentStatus::Thinking => Color::Yellow,
            AgentStatus::Executing => Color::Green,
            AgentStatus::Error => Color::Red,
            AgentStatus::Unknown => Color::DarkGray,
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            AgentStatus::Idle => "Waiting for input",
            AgentStatus::Thinking => "Processing",
            AgentStatus::Executing => "Running tools",
            AgentStatus::Error => "Error detected",
            AgentStatus::Unknown => "Unknown state",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PaneCapture {
    pub expert_id: u32,
    pub expert_name: String,
    #[allow(dead_code)]
    pub lines: Vec<String>,
    #[allow(dead_code)]
    pub captured_at: DateTime<Utc>,
    pub status: AgentStatus,
    pub last_activity: String,
}

pub struct CaptureManager {
    tmux: TmuxManager,
}

impl CaptureManager {
    pub fn new(session_name: String) -> Self {
        Self {
            tmux: TmuxManager::new(session_name),
        }
    }

    pub async fn capture_pane(&self, expert_id: u32, expert_name: &str) -> Result<PaneCapture> {
        let content = self.tmux.capture_pane(expert_id).await?;
        let lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
        let status = Self::analyze_status(&lines);
        let last_activity = Self::extract_last_activity(&lines);

        Ok(PaneCapture {
            expert_id,
            expert_name: expert_name.to_string(),
            lines,
            captured_at: Utc::now(),
            status,
            last_activity,
        })
    }

    pub async fn capture_all(&self, experts: &[(u32, String)]) -> Vec<PaneCapture> {
        let mut captures = Vec::new();

        for (id, name) in experts {
            match self.capture_pane(*id, name).await {
                Ok(capture) => captures.push(capture),
                Err(e) => {
                    tracing::warn!("Failed to capture pane {}: {}", id, e);
                }
            }
        }

        captures
    }

    fn analyze_status(lines: &[String]) -> AgentStatus {
        // Check for errors first (highest priority)
        if lines.iter().any(|line| {
            line.contains("Error:")
                || line.contains("error:")
                || line.contains("FAILED")
                || line.contains("panic")
        }) {
            return AgentStatus::Error;
        }

        // Claude Code tool execution indicator (⏺)
        // Search more lines since UI elements appear at the bottom
        if lines.iter().rev().take(15).any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('⏺') || trimmed.contains("Running")
        }) {
            return AgentStatus::Executing;
        }

        // Check for executing keywords in recent lines
        if lines.iter().rev().take(15).any(|line| {
            line.contains("Reading")
                || line.contains("Writing")
                || line.contains("Executing")
                || line.contains("Searching")
        }) {
            return AgentStatus::Executing;
        }

        // Claude Code thinking indicators
        let thinking_indicators = [
            '✻', // Claude Code thinking asterisk (six teardrop)
            '✳', // Claude Code thinking asterisk (eight spoked)
            '⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏', // Braille spinners
            '◐', '◓', '◑', '◒', // Circle spinners
        ];
        // Check recent lines for thinking indicators
        if lines.iter().rev().take(10).any(|line| {
            thinking_indicators.iter().any(|c| line.contains(*c))
        }) {
            return AgentStatus::Thinking;
        }

        // Check for thinking status messages in recent lines
        if lines.iter().rev().take(10).any(|line| {
            line.contains("Thinking")
                || line.contains("thought for")
                || line.contains("Churned")
                || line.contains("gibbeting")
                || line.contains("Cogitating")
        }) {
            return AgentStatus::Thinking;
        }

        // Claude Code idle prompt (❯) and standard prompt (>)
        // Check recent lines since UI elements may appear after the prompt
        if lines.iter().rev().take(10).any(|line| {
            let trimmed = line.trim();
            trimmed.starts_with('❯')
                || trimmed.starts_with('>')
                || trimmed.ends_with('>')
        }) {
            return AgentStatus::Idle;
        }

        AgentStatus::Unknown
    }

    fn extract_last_activity(lines: &[String]) -> String {
        lines
            .iter()
            .rev()
            .find(|line| !line.trim().is_empty())
            .map(|s| {
                let trimmed = s.trim();
                if trimmed.len() > 60 {
                    format!("{}...", &trimmed[..57])
                } else {
                    trimmed.to_string()
                }
            })
            .unwrap_or_else(|| "(no activity)".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_status_symbols_are_unique() {
        let statuses = [
            AgentStatus::Idle,
            AgentStatus::Thinking,
            AgentStatus::Executing,
            AgentStatus::Error,
            AgentStatus::Unknown,
        ];

        let symbols: Vec<_> = statuses.iter().map(|s| s.symbol()).collect();
        let unique: std::collections::HashSet<_> = symbols.iter().collect();
        assert_eq!(symbols.len(), unique.len());
    }

    #[test]
    fn agent_status_default_is_idle() {
        assert_eq!(AgentStatus::default(), AgentStatus::Idle);
    }

    #[test]
    fn analyze_status_detects_idle_with_prompt() {
        let lines = vec![
            "Some output".to_string(),
            "> ".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Idle);
    }

    #[test]
    fn analyze_status_detects_idle_with_claude_prompt() {
        let lines = vec![
            "Some output".to_string(),
            "❯ ".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Idle);
    }

    #[test]
    fn analyze_status_detects_thinking_with_spinner() {
        let lines = vec![
            "Some output".to_string(),
            "⠋ Processing...".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Thinking);
    }

    #[test]
    fn analyze_status_detects_thinking_with_claude_asterisk() {
        let lines = vec![
            "Some output".to_string(),
            "✻ Churned for 59s".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Thinking);
    }

    #[test]
    fn analyze_status_detects_executing_with_claude_tool() {
        let lines = vec![
            "Some output".to_string(),
            "⏺ Bash(cargo build)".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Executing);
    }

    #[test]
    fn analyze_status_detects_executing_with_keywords() {
        let lines = vec![
            "Some output".to_string(),
            "Reading file: src/main.rs".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Executing);

        let lines2 = vec![
            "Some output".to_string(),
            "Writing to output.txt".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines2), AgentStatus::Executing);
    }

    #[test]
    fn analyze_status_detects_error() {
        let lines = vec![
            "Some output".to_string(),
            "Error: something went wrong".to_string(),
            "".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Error);
    }

    #[test]
    fn analyze_status_returns_unknown_for_ambiguous() {
        let lines = vec![
            "Some random output".to_string(),
            "Nothing special here".to_string(),
        ];
        assert_eq!(CaptureManager::analyze_status(&lines), AgentStatus::Unknown);
    }

    #[test]
    fn extract_last_activity_gets_last_non_empty() {
        let lines = vec![
            "First line".to_string(),
            "Second line".to_string(),
            "   ".to_string(),
            "".to_string(),
        ];
        assert_eq!(CaptureManager::extract_last_activity(&lines), "Second line");
    }

    #[test]
    fn extract_last_activity_truncates_long_lines() {
        let long_line = "A".repeat(100);
        let lines = vec![long_line];
        let result = CaptureManager::extract_last_activity(&lines);
        assert!(result.len() <= 60);
        assert!(result.ends_with("..."));
    }

    #[test]
    fn extract_last_activity_returns_default_for_empty() {
        let lines: Vec<String> = vec![];
        assert_eq!(CaptureManager::extract_last_activity(&lines), "(no activity)");
    }

    #[test]
    fn pane_capture_contains_all_fields() {
        let capture = PaneCapture {
            expert_id: 0,
            expert_name: "architect".to_string(),
            lines: vec!["test".to_string()],
            captured_at: Utc::now(),
            status: AgentStatus::Idle,
            last_activity: "test".to_string(),
        };

        assert_eq!(capture.expert_id, 0);
        assert_eq!(capture.expert_name, "architect");
        assert_eq!(capture.status, AgentStatus::Idle);
    }
}
