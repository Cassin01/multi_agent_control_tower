use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::TaskStatus;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub description: String,
    pub severity: String,
    #[serde(default)]
    pub file: Option<String>,
    #[serde(default)]
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReportDetails {
    #[serde(default)]
    pub findings: Vec<Finding>,
    #[serde(default)]
    pub recommendations: Vec<String>,
    #[serde(default)]
    pub files_modified: Vec<String>,
    #[serde(default)]
    pub files_created: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Report {
    pub task_id: String,
    pub expert_id: u32,
    pub expert_name: String,
    pub status: TaskStatus,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
    pub summary: String,
    #[serde(default)]
    pub details: ReportDetails,
    #[serde(default)]
    pub errors: Vec<String>,
}

impl Report {
    #[allow(dead_code)]
    pub fn new(task_id: String, expert_id: u32, expert_name: String) -> Self {
        Self {
            task_id,
            expert_id,
            expert_name,
            status: TaskStatus::InProgress,
            started_at: Utc::now(),
            completed_at: None,
            summary: String::new(),
            details: ReportDetails::default(),
            errors: Vec::new(),
        }
    }

    #[allow(dead_code)]
    pub fn complete(mut self, summary: String) -> Self {
        self.status = TaskStatus::Done;
        self.completed_at = Some(Utc::now());
        self.summary = summary;
        self
    }

    #[allow(dead_code)]
    pub fn fail(mut self, error: String) -> Self {
        self.status = TaskStatus::Failed;
        self.completed_at = Some(Utc::now());
        self.errors.push(error);
        self
    }

    #[allow(dead_code)]
    pub fn with_details(mut self, details: ReportDetails) -> Self {
        self.details = details;
        self
    }

    #[allow(dead_code)]
    pub fn add_finding(&mut self, finding: Finding) {
        self.details.findings.push(finding);
    }

    #[allow(dead_code)]
    pub fn add_recommendation(&mut self, recommendation: String) {
        self.details.recommendations.push(recommendation);
    }

    #[allow(dead_code)]
    pub fn add_modified_file(&mut self, file: String) {
        self.details.files_modified.push(file);
    }

    #[allow(dead_code)]
    pub fn add_created_file(&mut self, file: String) {
        self.details.files_created.push(file);
    }

    #[allow(dead_code)]
    pub fn duration(&self) -> Option<chrono::Duration> {
        self.completed_at.map(|end| end - self.started_at)
    }

    /// Generate a sample YAML schema with example data for documentation.
    pub fn sample_yaml_schema() -> String {
        use chrono::TimeZone;

        let sample = Self {
            task_id: "task-YYYYMMDD-HHMMSS".to_string(),
            expert_id: 0,
            expert_name: "your_expert_name".to_string(),
            status: TaskStatus::Done,
            started_at: Utc.with_ymd_and_hms(2024, 1, 15, 10, 31, 0).unwrap(),
            completed_at: Some(Utc.with_ymd_and_hms(2024, 1, 15, 10, 45, 0).unwrap()),
            summary: "Brief description of work completed.".to_string(),
            details: ReportDetails {
                findings: vec![Finding {
                    description: "Issue description".to_string(),
                    severity: "high".to_string(),
                    file: Some("path/to/file.rs".to_string()),
                    line: Some(45),
                }],
                recommendations: vec!["Recommendation text".to_string()],
                files_modified: vec!["path/to/modified/file.rs".to_string()],
                files_created: vec!["path/to/new/file.rs".to_string()],
            },
            errors: vec![],
        };

        serde_yaml::to_string(&sample).unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_new_creates_in_progress() {
        let report = Report::new("task-001".to_string(), 0, "architect".to_string());

        assert_eq!(report.task_id, "task-001");
        assert_eq!(report.expert_id, 0);
        assert_eq!(report.status, TaskStatus::InProgress);
        assert!(report.completed_at.is_none());
    }

    #[test]
    fn report_complete_sets_done_status() {
        let report = Report::new("task-001".to_string(), 0, "architect".to_string())
            .complete("Task completed successfully".to_string());

        assert_eq!(report.status, TaskStatus::Done);
        assert!(report.completed_at.is_some());
        assert_eq!(report.summary, "Task completed successfully");
    }

    #[test]
    fn report_fail_sets_failed_status() {
        let report = Report::new("task-001".to_string(), 0, "architect".to_string())
            .fail("Connection timeout".to_string());

        assert_eq!(report.status, TaskStatus::Failed);
        assert!(report.completed_at.is_some());
        assert_eq!(report.errors, vec!["Connection timeout".to_string()]);
    }

    #[test]
    fn report_add_finding_appends_to_list() {
        let mut report = Report::new("task-001".to_string(), 0, "architect".to_string());

        report.add_finding(Finding {
            description: "Missing error handling".to_string(),
            severity: "high".to_string(),
            file: Some("src/main.rs".to_string()),
            line: Some(42),
        });

        assert_eq!(report.details.findings.len(), 1);
        assert_eq!(
            report.details.findings[0].description,
            "Missing error handling"
        );
    }

    #[test]
    fn report_serializes_to_yaml() {
        let mut report = Report::new("task-001".to_string(), 0, "architect".to_string());
        report.add_recommendation("Add input validation".to_string());
        report.add_modified_file("src/lib.rs".to_string());

        let yaml = serde_yaml::to_string(&report).unwrap();
        assert!(yaml.contains("task_id: task-001"));
        assert!(yaml.contains("status: in_progress"));
        assert!(yaml.contains("Add input validation"));
    }

    #[test]
    fn report_deserializes_from_yaml() {
        let yaml = r#"
task_id: "task-20240115-001"
expert_id: 0
expert_name: "architect"
status: done
started_at: "2024-01-15T10:31:00Z"
completed_at: "2024-01-15T10:45:00Z"
summary: "Reviewed authentication module"
details:
  findings:
    - description: "JWT expiration not validated"
      severity: "high"
      file: "internal/auth/middleware.go"
      line: 45
  recommendations:
    - "Add token expiration check"
  files_modified: []
  files_created: []
errors: []
"#;

        let report: Report = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(report.task_id, "task-20240115-001");
        assert_eq!(report.status, TaskStatus::Done);
        assert_eq!(report.details.findings.len(), 1);
        assert_eq!(report.details.findings[0].severity, "high");
    }

    #[test]
    fn report_duration_returns_none_when_not_completed() {
        let report = Report::new("task-001".to_string(), 0, "architect".to_string());
        assert!(report.duration().is_none());
    }

    #[test]
    fn report_duration_returns_some_when_completed() {
        let report = Report::new("task-001".to_string(), 0, "architect".to_string())
            .complete("Done".to_string());
        assert!(report.duration().is_some());
    }

    #[test]
    fn sample_yaml_schema_generates_valid_yaml() {
        let schema = Report::sample_yaml_schema();
        let parsed: Result<Report, _> = serde_yaml::from_str(&schema);
        assert!(parsed.is_ok(), "Generated schema should be valid YAML");
    }

    #[test]
    fn sample_yaml_schema_contains_required_fields() {
        let schema = Report::sample_yaml_schema();
        assert!(schema.contains("task_id:"));
        assert!(schema.contains("expert_id:"));
        assert!(schema.contains("expert_name:"));
        assert!(schema.contains("status:"));
        assert!(schema.contains("started_at:"));
        assert!(schema.contains("completed_at:"));
        assert!(schema.contains("summary:"));
        assert!(schema.contains("details:"));
        assert!(schema.contains("findings:"));
        assert!(schema.contains("recommendations:"));
        assert!(schema.contains("files_modified:"));
        assert!(schema.contains("files_created:"));
        assert!(schema.contains("errors:"));
    }
}
