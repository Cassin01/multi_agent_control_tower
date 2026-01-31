use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::EffortConfig;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    #[default]
    Pending,
    InProgress,
    Done,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum TaskPriority {
    Low,
    #[default]
    Normal,
    High,
    Critical,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TaskContext {
    #[serde(default)]
    pub files: Vec<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Task {
    pub task_id: String,
    pub expert_id: u32,
    pub expert_name: String,
    pub status: TaskStatus,
    pub created_at: DateTime<Utc>,
    pub description: String,
    #[serde(default)]
    pub context: TaskContext,
    #[serde(default)]
    pub priority: TaskPriority,
    #[serde(default)]
    pub effort: Option<EffortConfig>,
}

impl Task {
    pub fn new(expert_id: u32, expert_name: String, description: String) -> Self {
        Self {
            task_id: format!("task-{}", Utc::now().format("%Y%m%d-%H%M%S")),
            expert_id,
            expert_name,
            status: TaskStatus::Pending,
            created_at: Utc::now(),
            description,
            context: TaskContext::default(),
            priority: TaskPriority::default(),
            effort: None,
        }
    }

    #[allow(dead_code)]
    pub fn with_context(mut self, context: TaskContext) -> Self {
        self.context = context;
        self
    }

    #[allow(dead_code)]
    pub fn with_priority(mut self, priority: TaskPriority) -> Self {
        self.priority = priority;
        self
    }

    pub fn with_effort(mut self, effort: EffortConfig) -> Self {
        self.effort = Some(effort);
        self
    }

    #[allow(dead_code)]
    pub fn set_status(&mut self, status: TaskStatus) {
        self.status = status;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_new_creates_with_defaults() {
        let task = Task::new(0, "architect".to_string(), "Review code".to_string());

        assert_eq!(task.expert_id, 0);
        assert_eq!(task.expert_name, "architect");
        assert_eq!(task.description, "Review code");
        assert_eq!(task.status, TaskStatus::Pending);
        assert_eq!(task.priority, TaskPriority::Normal);
        assert!(task.task_id.starts_with("task-"));
    }

    #[test]
    fn task_with_context_sets_context() {
        let context = TaskContext {
            files: vec!["src/main.rs".to_string()],
            notes: Some("Focus on error handling".to_string()),
        };

        let task = Task::new(0, "architect".to_string(), "Review".to_string())
            .with_context(context.clone());

        assert_eq!(task.context.files, vec!["src/main.rs".to_string()]);
        assert_eq!(
            task.context.notes,
            Some("Focus on error handling".to_string())
        );
    }

    #[test]
    fn task_status_transitions() {
        let mut task = Task::new(0, "architect".to_string(), "Test".to_string());
        assert_eq!(task.status, TaskStatus::Pending);

        task.set_status(TaskStatus::InProgress);
        assert_eq!(task.status, TaskStatus::InProgress);

        task.set_status(TaskStatus::Done);
        assert_eq!(task.status, TaskStatus::Done);
    }

    #[test]
    fn task_serializes_to_yaml() {
        let task = Task::new(0, "architect".to_string(), "Review authentication".to_string())
            .with_priority(TaskPriority::High);

        let yaml = serde_yaml::to_string(&task).unwrap();
        assert!(yaml.contains("expert_id: 0"));
        assert!(yaml.contains("expert_name: architect"));
        assert!(yaml.contains("priority: high"));
        assert!(yaml.contains("status: pending"));
    }

    #[test]
    fn task_deserializes_from_yaml() {
        let yaml = r#"
task_id: "task-20240115-001"
expert_id: 1
expert_name: "frontend"
status: in_progress
created_at: "2024-01-15T10:30:00Z"
description: "Build login form"
context:
  files:
    - "src/components/Login.tsx"
  notes: "Use React Hook Form"
priority: normal
"#;

        let task: Task = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(task.task_id, "task-20240115-001");
        assert_eq!(task.expert_id, 1);
        assert_eq!(task.status, TaskStatus::InProgress);
        assert_eq!(task.context.files, vec!["src/components/Login.tsx"]);
    }

    #[test]
    fn task_priority_default_is_normal() {
        assert_eq!(TaskPriority::default(), TaskPriority::Normal);
    }

    #[test]
    fn task_status_default_is_pending() {
        assert_eq!(TaskStatus::default(), TaskStatus::Pending);
    }
}
