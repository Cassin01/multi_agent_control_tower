use anyhow::{Context, Result};
use std::path::PathBuf;
use tokio::fs;

use crate::models::{Report, Task};

pub struct QueueManager {
    base_path: PathBuf,
}

impl QueueManager {
    pub fn new(queue_path: PathBuf) -> Self {
        Self {
            base_path: queue_path,
        }
    }

    fn tasks_path(&self) -> PathBuf {
        self.base_path.join("tasks")
    }

    fn reports_path(&self) -> PathBuf {
        self.base_path.join("reports")
    }

    fn task_file(&self, expert_id: u32) -> PathBuf {
        self.tasks_path().join(format!("expert{}.yaml", expert_id))
    }

    #[allow(dead_code)]
    fn report_file(&self, expert_id: u32) -> PathBuf {
        self.reports_path()
            .join(format!("expert{}_report.yaml", expert_id))
    }

    pub async fn init(&self) -> Result<()> {
        fs::create_dir_all(self.tasks_path()).await?;
        fs::create_dir_all(self.reports_path()).await?;
        Ok(())
    }

    pub async fn write_task(&self, task: &Task) -> Result<()> {
        let path = self.task_file(task.expert_id);
        let content = serde_yaml::to_string(task)?;
        fs::write(&path, content)
            .await
            .context("Failed to write task file")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn read_task(&self, expert_id: u32) -> Result<Option<Task>> {
        let path = self.task_file(expert_id);

        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .await
            .context("Failed to read task file")?;
        let task: Task = serde_yaml::from_str(&content)?;
        Ok(Some(task))
    }

    #[allow(dead_code)]
    pub async fn clear_task(&self, expert_id: u32) -> Result<()> {
        let path = self.task_file(expert_id);
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn write_report(&self, report: &Report) -> Result<()> {
        let path = self.report_file(report.expert_id);
        let content = serde_yaml::to_string(report)?;
        fs::write(&path, content)
            .await
            .context("Failed to write report file")?;
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn read_report(&self, expert_id: u32) -> Result<Option<Report>> {
        let path = self.report_file(expert_id);

        if !path.exists() {
            return Ok(None);
        }

        let content = fs::read_to_string(&path)
            .await
            .context("Failed to read report file")?;
        let report: Report = serde_yaml::from_str(&content)?;
        Ok(Some(report))
    }

    #[allow(dead_code)]
    pub async fn clear_report(&self, expert_id: u32) -> Result<()> {
        let path = self.report_file(expert_id);
        if path.exists() {
            fs::remove_file(&path).await?;
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub async fn list_pending_tasks(&self) -> Result<Vec<Task>> {
        let mut tasks = Vec::new();
        let tasks_path = self.tasks_path();

        if !tasks_path.exists() {
            return Ok(tasks);
        }

        let mut entries = fs::read_dir(&tasks_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml") {
                match fs::read_to_string(&path).await {
                    Ok(content) => match serde_yaml::from_str::<Task>(&content) {
                        Ok(task) => tasks.push(task),
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse task file {}: {}",
                                path.display(),
                                e
                            );
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to read task file {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        tasks.sort_by(|a, b| a.created_at.cmp(&b.created_at));
        Ok(tasks)
    }

    pub async fn list_reports(&self) -> Result<Vec<Report>> {
        let mut reports = Vec::new();
        let reports_path = self.reports_path();

        if !reports_path.exists() {
            return Ok(reports);
        }

        let mut entries = fs::read_dir(&reports_path).await?;
        while let Some(entry) = entries.next_entry().await? {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml") {
                match fs::read_to_string(&path).await {
                    Ok(content) => match serde_yaml::from_str::<Report>(&content) {
                        Ok(report) => {
                            if let Err(validation_errors) = report.validate() {
                                tracing::warn!(
                                    "Report {} has validation warnings: {:?}",
                                    path.display(),
                                    validation_errors
                                );
                            }
                            reports.push(report);
                        }
                        Err(e) => {
                            tracing::error!(
                                "Failed to parse report file {}: {}",
                                path.display(),
                                e
                            );
                        }
                    },
                    Err(e) => {
                        tracing::error!(
                            "Failed to read report file {}: {}",
                            path.display(),
                            e
                        );
                    }
                }
            }
        }

        reports.sort_by(|a, b| a.started_at.cmp(&b.started_at));
        Ok(reports)
    }

    #[allow(dead_code)]
    pub async fn cleanup(&self) -> Result<()> {
        if self.tasks_path().exists() {
            fs::remove_dir_all(self.tasks_path()).await?;
        }
        if self.reports_path().exists() {
            fs::remove_dir_all(self.reports_path()).await?;
        }
        self.init().await?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::TaskStatus;
    use tempfile::TempDir;

    async fn create_test_manager() -> (QueueManager, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let manager = QueueManager::new(temp_dir.path().to_path_buf());
        manager.init().await.unwrap();
        (manager, temp_dir)
    }

    #[tokio::test]
    async fn queue_manager_init_creates_directories() {
        let (manager, _temp) = create_test_manager().await;
        assert!(manager.tasks_path().exists());
        assert!(manager.reports_path().exists());
    }

    #[tokio::test]
    async fn queue_manager_write_and_read_task() {
        let (manager, _temp) = create_test_manager().await;

        let task = Task::new(0, "architect".to_string(), "Review code".to_string());
        manager.write_task(&task).await.unwrap();

        let loaded = manager.read_task(0).await.unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.expert_id, 0);
        assert_eq!(loaded.description, "Review code");
    }

    #[tokio::test]
    async fn queue_manager_read_task_returns_none_when_missing() {
        let (manager, _temp) = create_test_manager().await;
        let loaded = manager.read_task(99).await.unwrap();
        assert!(loaded.is_none());
    }

    #[tokio::test]
    async fn queue_manager_clear_task_removes_file() {
        let (manager, _temp) = create_test_manager().await;

        let task = Task::new(0, "architect".to_string(), "Test".to_string());
        manager.write_task(&task).await.unwrap();
        assert!(manager.read_task(0).await.unwrap().is_some());

        manager.clear_task(0).await.unwrap();
        assert!(manager.read_task(0).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn queue_manager_write_and_read_report() {
        let (manager, _temp) = create_test_manager().await;

        let report = Report::new("task-001".to_string(), 0, "architect".to_string())
            .complete("Done".to_string());
        manager.write_report(&report).await.unwrap();

        let loaded = manager.read_report(0).await.unwrap();
        assert!(loaded.is_some());

        let loaded = loaded.unwrap();
        assert_eq!(loaded.task_id, "task-001");
        assert_eq!(loaded.status, TaskStatus::Done);
    }

    #[tokio::test]
    async fn queue_manager_list_pending_tasks_returns_all() {
        let (manager, _temp) = create_test_manager().await;

        let task1 = Task::new(0, "architect".to_string(), "Task 1".to_string());
        let task2 = Task::new(1, "frontend".to_string(), "Task 2".to_string());

        manager.write_task(&task1).await.unwrap();
        manager.write_task(&task2).await.unwrap();

        let tasks = manager.list_pending_tasks().await.unwrap();
        assert_eq!(tasks.len(), 2);
    }

    #[tokio::test]
    async fn queue_manager_list_reports_returns_all() {
        let (manager, _temp) = create_test_manager().await;

        let report1 = Report::new("task-001".to_string(), 0, "architect".to_string());
        let report2 = Report::new("task-002".to_string(), 1, "frontend".to_string());

        manager.write_report(&report1).await.unwrap();
        manager.write_report(&report2).await.unwrap();

        let reports = manager.list_reports().await.unwrap();
        assert_eq!(reports.len(), 2);
    }

    #[tokio::test]
    async fn queue_manager_cleanup_removes_all() {
        let (manager, _temp) = create_test_manager().await;

        let task = Task::new(0, "architect".to_string(), "Test".to_string());
        manager.write_task(&task).await.unwrap();

        manager.cleanup().await.unwrap();

        let tasks = manager.list_pending_tasks().await.unwrap();
        assert!(tasks.is_empty());
    }
}
