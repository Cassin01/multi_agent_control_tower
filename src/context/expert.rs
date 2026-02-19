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
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
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
            worktree_branch: None,
            worktree_path: None,
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

    #[allow(dead_code)]
    pub fn clear_knowledge(&mut self) {
        self.knowledge = Knowledge::default();
        self.touch();
    }

    pub fn clear_session(&mut self) {
        self.claude_session = ClaudeSession::default();
        self.touch();
    }

    pub fn set_worktree(&mut self, branch: String, path: String) {
        self.worktree_branch = Some(branch);
        self.worktree_path = Some(path);
        self.touch();
    }

    pub fn clear_worktree(&mut self) {
        self.worktree_branch = None;
        self.worktree_path = None;
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
    }

    #[test]
    fn expert_context_set_session_id_updates_context() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        let initial_updated = ctx.updated_at;

        std::thread::sleep(std::time::Duration::from_millis(10));
        ctx.set_session_id("session-xyz".to_string());

        assert_eq!(
            ctx.claude_session.session_id,
            Some("session-xyz".to_string())
        );
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
    fn expert_context_set_worktree_stores_branch_and_path() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());

        ctx.set_worktree(
            "add-auth-20260207-120000".to_string(),
            "/tmp/worktrees/add-auth-20260207-120000".to_string(),
        );

        assert_eq!(
            ctx.worktree_branch,
            Some("add-auth-20260207-120000".to_string()),
            "set_worktree: should store branch name"
        );
        assert_eq!(
            ctx.worktree_path,
            Some("/tmp/worktrees/add-auth-20260207-120000".to_string()),
            "set_worktree: should store worktree path"
        );
    }

    #[test]
    fn expert_context_set_worktree_calls_touch() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        let initial_updated = ctx.updated_at;

        std::thread::sleep(std::time::Duration::from_millis(10));
        ctx.set_worktree("branch".to_string(), "/tmp/wt".to_string());

        assert!(
            ctx.updated_at > initial_updated,
            "set_worktree: should update updated_at via touch()"
        );
    }

    #[test]
    fn expert_context_serializes_worktree_fields_to_yaml() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        ctx.set_worktree(
            "feature-branch".to_string(),
            "/tmp/wt/feature-branch".to_string(),
        );

        let yaml = serde_yaml::to_string(&ctx).unwrap();
        assert!(
            yaml.contains("worktree_branch: feature-branch"),
            "serializes_worktree: YAML should contain worktree_branch"
        );
        assert!(
            yaml.contains("worktree_path: /tmp/wt/feature-branch"),
            "serializes_worktree: YAML should contain worktree_path"
        );
    }

    #[test]
    fn expert_context_deserializes_worktree_fields_from_yaml() {
        let yaml = r#"
expert_id: 2
expert_name: backend
session_hash: xyz789
created_at: "2024-01-15T10:00:00Z"
updated_at: "2024-01-15T11:00:00Z"
worktree_branch: feature-auth
worktree_path: /tmp/wt/feature-auth
"#;

        let ctx: ExpertContext = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            ctx.worktree_branch,
            Some("feature-auth".to_string()),
            "deserializes_worktree: should parse worktree_branch"
        );
        assert_eq!(
            ctx.worktree_path,
            Some("/tmp/wt/feature-auth".to_string()),
            "deserializes_worktree: should parse worktree_path"
        );
    }

    #[test]
    fn expert_context_backward_compatible_deserialization_without_worktree() {
        let yaml = r#"
expert_id: 1
expert_name: frontend
session_hash: def456
created_at: "2024-01-15T10:00:00Z"
updated_at: "2024-01-15T11:00:00Z"
"#;

        let ctx: ExpertContext = serde_yaml::from_str(yaml).unwrap();
        assert!(
            ctx.worktree_branch.is_none(),
            "backward_compat: worktree_branch should default to None"
        );
        assert!(
            ctx.worktree_path.is_none(),
            "backward_compat: worktree_path should default to None"
        );
    }

    #[test]
    fn expert_context_new_has_no_worktree() {
        let ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        assert!(
            ctx.worktree_branch.is_none(),
            "new: worktree_branch should be None by default"
        );
        assert!(
            ctx.worktree_path.is_none(),
            "new: worktree_path should be None by default"
        );
    }

    #[test]
    fn expert_context_clear_worktree_resets_fields_to_none() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        ctx.set_worktree(
            "feature-branch".to_string(),
            "/tmp/wt/feature-branch".to_string(),
        );

        ctx.clear_worktree();

        assert!(
            ctx.worktree_branch.is_none(),
            "clear_worktree: worktree_branch should be None after clear"
        );
        assert!(
            ctx.worktree_path.is_none(),
            "clear_worktree: worktree_path should be None after clear"
        );
    }

    #[test]
    fn expert_context_clear_worktree_calls_touch() {
        let mut ctx = ExpertContext::new(0, "architect".to_string(), "abc123".to_string());
        ctx.set_worktree("branch".to_string(), "/tmp/wt".to_string());
        let initial_updated = ctx.updated_at;

        std::thread::sleep(std::time::Duration::from_millis(10));
        ctx.clear_worktree();

        assert!(
            ctx.updated_at > initial_updated,
            "clear_worktree: should update updated_at via touch()"
        );
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
        assert_eq!(
            ctx.claude_session.session_id,
            Some("session-abc".to_string())
        );
        assert_eq!(ctx.knowledge.files_analyzed.len(), 1);
    }
}
