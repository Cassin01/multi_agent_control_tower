use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ClaudeSession {
    pub session_id: Option<String>,
    pub last_conversation_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAnalysis {
    pub path: String,
    pub summary: String,
    pub last_read: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Pattern {
    pub pattern_type: String,
    pub pattern: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dependency {
    pub from: String,
    pub to: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Knowledge {
    #[serde(default)]
    pub files_analyzed: Vec<FileAnalysis>,
    #[serde(default)]
    pub patterns_discovered: Vec<Pattern>,
    #[serde(default)]
    pub dependencies_mapped: Vec<Dependency>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskHistoryEntry {
    pub task_id: String,
    pub status: String,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertContext {
    pub expert_id: u32,
    pub expert_name: String,
    pub session_hash: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub claude_session: ClaudeSession,
    #[serde(default)]
    pub knowledge: Knowledge,
    #[serde(default)]
    pub task_history: Vec<TaskHistoryEntry>,
}

impl ExpertContext {
    pub fn new(expert_id: u32, expert_name: String, session_hash: String) -> Self {
        let now = Utc::now();
        Self {
            expert_id,
            expert_name,
            session_hash,
            created_at: now,
            updated_at: now,
            claude_session: ClaudeSession::default(),
            knowledge: Knowledge::default(),
            task_history: Vec::new(),
        }
    }

    pub fn touch(&mut self) {
        self.updated_at = Utc::now();
    }

    #[allow(dead_code)]
    pub fn set_session_id(&mut self, session_id: String) {
        self.claude_session.session_id = Some(session_id);
        self.touch();
    }

    #[allow(dead_code)]
    pub fn add_file_analysis(&mut self, path: String, summary: String) {
        self.knowledge.files_analyzed.push(FileAnalysis {
            path,
            summary,
            last_read: Utc::now(),
        });
        self.touch();
    }

    pub fn add_task_history(&mut self, task_id: String, status: String, summary: String) {
        self.task_history.push(TaskHistoryEntry {
            task_id,
            status,
            summary,
        });
        self.touch();
    }

    #[allow(dead_code)]
    pub fn clear_knowledge(&mut self) {
        self.knowledge = Knowledge::default();
        self.touch();
    }

    #[allow(dead_code)]
    pub fn clear_session(&mut self) {
        self.claude_session = ClaudeSession::default();
        self.touch();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expert_context_new_creates_with_defaults() {
        let ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());

        assert_eq!(ctx.expert_id, 0);
        assert_eq!(ctx.expert_name, "architect");
        assert_eq!(ctx.session_hash, "abc123");
        assert!(ctx.claude_session.session_id.is_none());
        assert!(ctx.knowledge.files_analyzed.is_empty());
        assert!(ctx.task_history.is_empty());
    }

    #[test]
    fn expert_context_set_session_id_updates_context() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        let initial_updated = ctx.updated_at;

        std::thread::sleep(std::time::Duration::from_millis(10));
        ctx.set_session_id("session-xyz".to_string());

        assert_eq!(ctx.claude_session.session_id, Some("session-xyz".to_string()));
        assert!(ctx.updated_at > initial_updated);
    }

    #[test]
    fn expert_context_add_file_analysis_appends() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());

        ctx.add_file_analysis("src/main.rs".to_string(), "Main entry point".to_string());
        ctx.add_file_analysis("src/lib.rs".to_string(), "Library root".to_string());

        assert_eq!(ctx.knowledge.files_analyzed.len(), 2);
        assert_eq!(ctx.knowledge.files_analyzed[0].path, "src/main.rs");
        assert_eq!(ctx.knowledge.files_analyzed[1].path, "src/lib.rs");
    }

    #[test]
    fn expert_context_add_task_history_appends() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());

        ctx.add_task_history("task-001".to_string(), "done".to_string(), "Completed review".to_string());

        assert_eq!(ctx.task_history.len(), 1);
        assert_eq!(ctx.task_history[0].task_id, "task-001");
        assert_eq!(ctx.task_history[0].status, "done");
    }

    #[test]
    fn expert_context_clear_knowledge_resets() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        ctx.add_file_analysis("src/main.rs".to_string(), "Test".to_string());

        ctx.clear_knowledge();

        assert!(ctx.knowledge.files_analyzed.is_empty());
    }

    #[test]
    fn expert_context_serializes_to_yaml() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        ctx.set_session_id("session-xyz".to_string());

        let yaml = serde_yaml::to_string(&ctx).unwrap();
        assert!(yaml.contains("expert_id: 0"));
        assert!(yaml.contains("expert_name: architect"));
        assert!(yaml.contains("session_id: session-xyz"));
    }

    #[test]
    fn expert_context_deserializes_from_yaml() {
        let yaml = r#"
expert_id: 1
expert_name: frontend
session_hash: def456
created_at: "2024-01-15T10:00:00Z"
updated_at: "2024-01-15T11:00:00Z"
claude_session:
  session_id: session-abc
knowledge:
  files_analyzed:
    - path: "src/App.tsx"
      summary: "Main React component"
      last_read: "2024-01-15T10:30:00Z"
task_history:
  - task_id: task-001
    status: done
    summary: Created login form
"#;

        let ctx: ExpertContext = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(ctx.expert_id, 1);
        assert_eq!(ctx.expert_name, "frontend");
        assert_eq!(ctx.claude_session.session_id, Some("session-abc".to_string()));
        assert_eq!(ctx.knowledge.files_analyzed.len(), 1);
        assert_eq!(ctx.task_history.len(), 1);
    }
}
