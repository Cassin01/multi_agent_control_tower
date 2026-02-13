use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{bail, Result};

use crate::config::FeatureExecutionConfig;
use crate::feature::task_parser::{self, TaskEntry};

pub enum ExecutionPhase {
    Idle,
    ExitingExpert {
        started_at: Instant,
    },
    RelaunchingExpert {
        started_at: Instant,
    },
    SendingBatch,
    WaitingPollDelay {
        started_at: Instant,
    },
    PollingStatus,
    Completed,
    Failed(String),
}

pub struct FeatureExecutor {
    feature_name: String,
    expert_id: u32,
    batch_size: usize,
    poll_delay: Duration,
    exit_wait: Duration,
    ready_timeout: Duration,

    phase: ExecutionPhase,
    current_batch: Vec<String>,
    batch_completion_wait_start: Option<Instant>,

    tasks_file: PathBuf,
    design_file: Option<PathBuf>,

    total_tasks: usize,
    completed_tasks: usize,

    instruction_file: Option<PathBuf>,
    working_dir: String,
}

impl FeatureExecutor {
    pub fn new(
        feature_name: String,
        expert_id: u32,
        config: &FeatureExecutionConfig,
        project_path: &PathBuf,
        instruction_file: Option<PathBuf>,
        working_dir: String,
    ) -> Self {
        let specs_dir = project_path.join(".macot").join("specs");
        Self {
            feature_name: feature_name.clone(),
            expert_id,
            batch_size: config.batch_size,
            poll_delay: Duration::from_secs(config.poll_delay_secs),
            exit_wait: Duration::from_secs(config.exit_wait_secs),
            ready_timeout: Duration::from_secs(config.ready_timeout_secs),
            phase: ExecutionPhase::Idle,
            current_batch: Vec::new(),
            batch_completion_wait_start: None,
            tasks_file: specs_dir.join(format!("{}-tasks.md", feature_name)),
            design_file: None,
            total_tasks: 0,
            completed_tasks: 0,
            instruction_file,
            working_dir,
        }
    }

    pub fn validate(&mut self) -> Result<()> {
        if !self.tasks_file.exists() {
            bail!(
                "Task file not found: {}",
                self.tasks_file.display()
            );
        }

        let design_path = self
            .tasks_file
            .parent()
            .unwrap()
            .join(format!("{}-design.md", self.feature_name));
        if design_path.exists() {
            self.design_file = Some(design_path);
        }

        Ok(())
    }

    pub fn parse_tasks(&mut self) -> Result<Vec<TaskEntry>> {
        let content = std::fs::read_to_string(&self.tasks_file)?;
        let tasks = task_parser::parse_tasks(&content);
        self.total_tasks = tasks.len();
        self.completed_tasks = tasks.iter().filter(|t| t.completed).count();
        Ok(tasks)
    }

    pub fn next_batch<'a>(&self, tasks: &'a [TaskEntry]) -> Vec<&'a TaskEntry> {
        tasks
            .iter()
            .filter(|t| !t.completed)
            .take(self.batch_size)
            .collect()
    }

    pub fn build_prompt(&self, batch: &[&TaskEntry]) -> String {
        let task_numbers: Vec<&str> = batch.iter().map(|t| t.number.as_str()).collect();
        let numbers_str = task_numbers.join(", ");

        let mut prompt = String::new();

        if self.design_file.is_some() {
            let design_rel = format!(
                ".macot/specs/{}-design.md",
                self.feature_name
            );
            prompt.push_str(&format!(
                "Below are the design specifications and task list for {}.\n\n",
                self.feature_name
            ));
            prompt.push_str(&format!("@{}\n", design_rel));
        } else {
            prompt.push_str(&format!(
                "Below is the task list for {}.\n\n",
                self.feature_name
            ));
        }

        let tasks_rel = format!(
            ".macot/specs/{}-tasks.md",
            self.feature_name
        );
        prompt.push_str(&format!("@{}\n\n", tasks_rel));
        prompt.push_str("Implement the tasks in order.\n");
        prompt.push_str(&format!(
            "Execute Tasks {{{}}}. After completing each task, Mark them as finished in the task file.\n",
            numbers_str
        ));

        let status_path = format!(
            "{}/.macot/status/expert{}",
            self.working_dir, self.expert_id
        );
        prompt.push_str(&format!(
            "After completing all tasks, set your status to pending by running:\n\
             ```bash\n\
             bash -c 'echo -n \"pending\" > \"{}\"'\n\
             ```\n",
            status_path
        ));

        prompt
    }

    pub fn phase(&self) -> &ExecutionPhase {
        &self.phase
    }

    pub fn feature_name(&self) -> &str {
        &self.feature_name
    }

    pub fn expert_id(&self) -> u32 {
        self.expert_id
    }

    pub fn exit_wait(&self) -> Duration {
        self.exit_wait
    }

    pub fn ready_timeout(&self) -> Duration {
        self.ready_timeout
    }

    pub fn poll_delay(&self) -> Duration {
        self.poll_delay
    }

    pub fn completed_tasks(&self) -> usize {
        self.completed_tasks
    }

    pub fn total_tasks(&self) -> usize {
        self.total_tasks
    }

    pub fn working_dir(&self) -> &str {
        &self.working_dir
    }

    pub fn instruction_file(&self) -> Option<&PathBuf> {
        self.instruction_file.as_ref()
    }

    pub fn current_batch(&self) -> &[String] {
        &self.current_batch
    }

    pub fn design_file(&self) -> Option<&PathBuf> {
        self.design_file.as_ref()
    }

    pub fn execution_badge(&self) -> Option<String> {
        match &self.phase {
            ExecutionPhase::ExitingExpert { .. } | ExecutionPhase::RelaunchingExpert { .. } => {
                Some("~ resetting...".to_string())
            }
            ExecutionPhase::SendingBatch
            | ExecutionPhase::WaitingPollDelay { .. }
            | ExecutionPhase::PollingStatus => Some(format!("> {}", self.feature_name)),
            _ => None,
        }
    }

    pub fn set_phase(&mut self, phase: ExecutionPhase) {
        self.phase = phase;
    }

    pub fn record_batch_sent(&mut self, batch: &[&TaskEntry]) {
        self.current_batch = batch.iter().map(|t| t.number.clone()).collect();
    }

    pub fn cancel(&mut self) {
        self.phase = ExecutionPhase::Idle;
        self.current_batch.clear();
        self.batch_completion_wait_start = None;
    }

    pub fn is_previous_batch_completed(&self, tasks: &[TaskEntry]) -> bool {
        if self.current_batch.is_empty() {
            return true;
        }
        self.current_batch.iter().all(|num| {
            tasks.iter().any(|t| t.number == *num && t.completed)
        })
    }

    pub fn start_batch_completion_wait(&mut self) {
        if self.batch_completion_wait_start.is_none() {
            self.batch_completion_wait_start = Some(Instant::now());
        }
    }

    pub fn clear_batch_completion_wait(&mut self) {
        self.batch_completion_wait_start = None;
    }

    pub fn batch_completion_wait_elapsed(&self) -> Option<Duration> {
        self.batch_completion_wait_start.map(|s| s.elapsed())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_executor(temp: &TempDir) -> FeatureExecutor {
        let config = FeatureExecutionConfig::default();
        FeatureExecutor::new(
            "test-feature".to_string(),
            0,
            &config,
            &temp.path().to_path_buf(),
            None,
            "/tmp/project".to_string(),
        )
    }

    fn write_tasks_file(temp: &TempDir, content: &str) {
        let specs = temp.path().join(".macot").join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(specs.join("test-feature-tasks.md"), content).unwrap();
    }

    fn write_design_file(temp: &TempDir, content: &str) {
        let specs = temp.path().join(".macot").join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(specs.join("test-feature-design.md"), content).unwrap();
    }

    // --- Task 5.1: Validation tests ---

    #[test]
    fn validate_succeeds_when_tasks_file_exists() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n");
        let mut executor = make_executor(&temp);
        assert!(executor.validate().is_ok(), "validate: should succeed when tasks file exists");
    }

    #[test]
    fn validate_fails_when_tasks_file_missing() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        assert!(executor.validate().is_err(), "validate: should fail when tasks file is missing");
    }

    #[test]
    fn validate_sets_design_file_when_exists() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n");
        write_design_file(&temp, "# Design\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        assert!(executor.design_file().is_some(), "validate: should set design_file when it exists");
    }

    #[test]
    fn validate_leaves_design_file_none_when_absent() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        assert!(executor.design_file().is_none(), "validate: design_file should be None when absent");
    }

    // --- Task 5.2: Batch calculation tests ---

    #[test]
    fn next_batch_returns_first_batch_size_uncompleted() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [ ] 1. Task one
- [ ] 2. Task two
- [ ] 3. Task three
- [ ] 4. Task four
- [ ] 5. Task five
- [ ] 6. Task six
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        assert_eq!(batch.len(), 4, "next_batch: should return batch_size (4) tasks");
        assert_eq!(batch[0].number, "1");
        assert_eq!(batch[3].number, "4");
    }

    #[test]
    fn next_batch_returns_fewer_when_fewer_remain() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Done
- [ ] 3. Task three
- [ ] 4. Task four
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        assert_eq!(batch.len(), 2, "next_batch: should return fewer than batch_size when fewer remain");
        assert_eq!(batch[0].number, "3");
        assert_eq!(batch[1].number, "4");
    }

    #[test]
    fn next_batch_returns_empty_when_all_completed() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Done
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        assert!(batch.is_empty(), "next_batch: should return empty vec when all tasks completed");
    }

    #[test]
    fn next_batch_skips_completed_tasks() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [ ] 2. Not done
- [x] 3. Done
- [ ] 4. Not done
- [ ] 5. Not done
- [ ] 6. Not done
- [ ] 7. Not done
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        assert_eq!(batch.len(), 4, "next_batch: should return 4 uncompleted tasks");
        assert_eq!(batch[0].number, "2");
        assert_eq!(batch[1].number, "4");
        assert_eq!(batch[2].number, "5");
        assert_eq!(batch[3].number, "6");
    }

    // --- Task 5.3: Prompt building tests ---

    #[test]
    fn build_prompt_includes_design_file_when_present() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n- [ ] 2. Task two\n");
        write_design_file(&temp, "# Design\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        let prompt = executor.build_prompt(&batch);

        assert!(
            prompt.contains("Below are the design specifications and task list for"),
            "build_prompt: should mention design specifications when design file exists"
        );
        assert!(
            prompt.contains("@.macot/specs/test-feature-design.md"),
            "build_prompt: should reference design file"
        );
        assert!(
            prompt.contains("@.macot/specs/test-feature-tasks.md"),
            "build_prompt: should reference tasks file"
        );
    }

    #[test]
    fn build_prompt_omits_design_file_when_absent() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n- [ ] 2. Task two\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        let prompt = executor.build_prompt(&batch);

        assert!(
            prompt.contains("Below is the task list for"),
            "build_prompt: should use simpler intro when no design file"
        );
        assert!(
            !prompt.contains("design.md"),
            "build_prompt: should not reference design file"
        );
        assert!(
            prompt.contains("@.macot/specs/test-feature-tasks.md"),
            "build_prompt: should reference tasks file"
        );
    }

    #[test]
    fn build_prompt_includes_status_pending_instruction() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n- [ ] 2. Task two\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        let prompt = executor.build_prompt(&batch);

        assert!(
            prompt.contains(r#"echo -n "pending" > "/tmp/project/.macot/status/expert0""#),
            "build_prompt: should include status pending instruction with correct path, got: {}",
            prompt
        );
    }

    #[test]
    fn build_prompt_includes_comma_separated_task_numbers() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Done
- [ ] 3. Task three
- [ ] 4. Task four
- [ ] 5. Task five
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        let prompt = executor.build_prompt(&batch);

        assert!(
            prompt.contains("Execute Tasks {3, 4, 5}"),
            "build_prompt: should contain comma-separated task numbers, got: {}",
            prompt
        );
    }

    // --- Progress tracking tests ---

    #[test]
    fn parse_tasks_updates_progress_tracking() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Done
- [ ] 3. Not done
- [ ] 4. Not done
- [ ] 5. Not done
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        executor.parse_tasks().unwrap();
        assert_eq!(executor.total_tasks(), 5, "parse_tasks: total_tasks should be 5");
        assert_eq!(executor.completed_tasks(), 2, "parse_tasks: completed_tasks should be 2");
    }

    #[test]
    fn record_batch_sent_stores_task_numbers() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [ ] 1. Task one\n- [ ] 2. Task two\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        executor.record_batch_sent(&batch);
        assert_eq!(executor.current_batch(), &["1", "2"]);
    }

    #[test]
    fn cancel_resets_to_idle() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::SendingBatch);
        executor.cancel();
        assert!(matches!(executor.phase(), ExecutionPhase::Idle), "cancel: should reset to Idle");
        assert!(executor.current_batch().is_empty(), "cancel: should clear current batch");
    }

    // --- Task 11.1: Execution badge tests ---

    #[test]
    fn execution_badge_none_when_idle() {
        let temp = TempDir::new().unwrap();
        let executor = make_executor(&temp);
        assert!(
            executor.execution_badge().is_none(),
            "execution_badge: should be None in Idle phase"
        );
    }

    #[test]
    fn execution_badge_resetting_during_exiting_expert() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::ExitingExpert {
            started_at: Instant::now(),
        });
        assert_eq!(
            executor.execution_badge().as_deref(),
            Some("~ resetting..."),
            "execution_badge: should show resetting during ExitingExpert"
        );
    }

    #[test]
    fn execution_badge_resetting_during_relaunching_expert() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::RelaunchingExpert {
            started_at: Instant::now(),
        });
        assert_eq!(
            executor.execution_badge().as_deref(),
            Some("~ resetting..."),
            "execution_badge: should show resetting during RelaunchingExpert"
        );
    }

    #[test]
    fn execution_badge_shows_feature_name_during_sending_batch() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::SendingBatch);
        assert_eq!(
            executor.execution_badge().as_deref(),
            Some("> test-feature"),
            "execution_badge: should show feature name during SendingBatch"
        );
    }

    #[test]
    fn execution_badge_shows_feature_name_during_waiting_poll_delay() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::WaitingPollDelay {
            started_at: Instant::now(),
        });
        assert_eq!(
            executor.execution_badge().as_deref(),
            Some("> test-feature"),
            "execution_badge: should show feature name during WaitingPollDelay"
        );
    }

    #[test]
    fn execution_badge_shows_feature_name_during_polling_status() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::PollingStatus);
        assert_eq!(
            executor.execution_badge().as_deref(),
            Some("> test-feature"),
            "execution_badge: should show feature name during PollingStatus"
        );
    }

    #[test]
    fn execution_badge_none_when_completed() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::Completed);
        assert!(
            executor.execution_badge().is_none(),
            "execution_badge: should be None when Completed"
        );
    }

    #[test]
    fn execution_badge_none_when_failed() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.set_phase(ExecutionPhase::Failed("error".into()));
        assert!(
            executor.execution_badge().is_none(),
            "execution_badge: should be None when Failed"
        );
    }

    // --- Task 12.1: Progress display tests ---

    #[test]
    fn progress_message_format_during_execution() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Done
- [ ] 3. Task three
- [ ] 4. Task four
- [ ] 5. Task five
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch = executor.next_batch(&tasks);
        executor.record_batch_sent(&batch);

        let batch_numbers = executor.current_batch().join(", ");
        let msg = format!(
            "> {}: {}/{} tasks | Batch: {}",
            executor.feature_name(),
            executor.completed_tasks(),
            executor.total_tasks(),
            batch_numbers
        );
        assert_eq!(
            msg, "> test-feature: 2/5 tasks | Batch: 3, 4, 5",
            "progress: execution message should match expected format"
        );
    }

    #[test]
    fn progress_message_format_during_reset() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Done
- [x] 3. Done
- [ ] 4. Task four
- [ ] 5. Task five
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        executor.parse_tasks().unwrap();

        let msg = format!(
            "~ {}: resetting expert... | {}/{} tasks",
            executor.feature_name(),
            executor.completed_tasks(),
            executor.total_tasks()
        );
        assert_eq!(
            msg, "~ test-feature: resetting expert... | 3/5 tasks",
            "progress: reset message should match expected format"
        );
    }

    #[test]
    fn progress_counts_reflect_task_file_state() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [ ] 2. Not done
- [x] 3. Done
- [ ] 4. Not done
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        executor.parse_tasks().unwrap();
        assert_eq!(
            executor.completed_tasks(),
            2,
            "progress: completed_tasks should reflect actual completed count"
        );
        assert_eq!(
            executor.total_tasks(),
            4,
            "progress: total_tasks should reflect actual total count"
        );
    }

    // --- Batch completion check tests ---

    #[test]
    fn is_previous_batch_completed_true_when_empty_batch() {
        let temp = TempDir::new().unwrap();
        let executor = make_executor(&temp);
        let tasks: Vec<TaskEntry> = vec![];
        assert!(
            executor.is_previous_batch_completed(&tasks),
            "is_previous_batch_completed: should return true when current_batch is empty"
        );
    }

    #[test]
    fn is_previous_batch_completed_true_when_all_done() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [x] 1. Done\n- [x] 2. Done\n- [ ] 3. Not done\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch: Vec<&TaskEntry> = tasks.iter().filter(|t| t.number == "1" || t.number == "2").collect();
        executor.record_batch_sent(&batch);
        assert!(
            executor.is_previous_batch_completed(&tasks),
            "is_previous_batch_completed: should return true when all batch tasks are completed"
        );
    }

    #[test]
    fn is_previous_batch_completed_false_when_some_incomplete() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(&temp, "- [x] 1. Done\n- [ ] 2. Not done\n- [ ] 3. Not done\n");
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        let tasks = executor.parse_tasks().unwrap();
        let batch: Vec<&TaskEntry> = tasks.iter().filter(|t| t.number == "1" || t.number == "2").collect();
        executor.record_batch_sent(&batch);
        assert!(
            !executor.is_previous_batch_completed(&tasks),
            "is_previous_batch_completed: should return false when some batch tasks are incomplete"
        );
    }

    #[test]
    fn batch_completion_wait_tracking() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);

        assert!(
            executor.batch_completion_wait_elapsed().is_none(),
            "batch_completion_wait_elapsed: should be None initially"
        );

        executor.start_batch_completion_wait();
        std::thread::sleep(Duration::from_millis(10));
        let elapsed = executor.batch_completion_wait_elapsed();
        assert!(
            elapsed.is_some(),
            "batch_completion_wait_elapsed: should be Some after starting"
        );
        assert!(
            elapsed.unwrap() >= Duration::from_millis(10),
            "batch_completion_wait_elapsed: should track elapsed time"
        );

        // start_batch_completion_wait is idempotent — does not reset
        let first = executor.batch_completion_wait_elapsed().unwrap();
        executor.start_batch_completion_wait();
        let second = executor.batch_completion_wait_elapsed().unwrap();
        assert!(
            second >= first,
            "start_batch_completion_wait: should be idempotent (not reset timer)"
        );

        executor.clear_batch_completion_wait();
        assert!(
            executor.batch_completion_wait_elapsed().is_none(),
            "clear_batch_completion_wait: should reset to None"
        );
    }

    #[test]
    fn cancel_clears_batch_completion_wait() {
        let temp = TempDir::new().unwrap();
        let mut executor = make_executor(&temp);
        executor.start_batch_completion_wait();
        assert!(executor.batch_completion_wait_elapsed().is_some());
        executor.cancel();
        assert!(
            executor.batch_completion_wait_elapsed().is_none(),
            "cancel: should clear batch_completion_wait_start"
        );
    }

    #[test]
    fn progress_counts_update_on_reparse() {
        let temp = TempDir::new().unwrap();
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [ ] 2. Not done
- [ ] 3. Not done
",
        );
        let mut executor = make_executor(&temp);
        executor.validate().unwrap();
        executor.parse_tasks().unwrap();
        assert_eq!(executor.completed_tasks(), 1);
        assert_eq!(executor.total_tasks(), 3);

        // Simulate expert completing tasks by rewriting the file
        write_tasks_file(
            &temp,
            "\
- [x] 1. Done
- [x] 2. Now done
- [x] 3. Now done
",
        );
        executor.parse_tasks().unwrap();
        assert_eq!(
            executor.completed_tasks(),
            3,
            "progress: completed_tasks should update after reparse"
        );
        assert_eq!(
            executor.total_tasks(),
            3,
            "progress: total_tasks should remain consistent after reparse"
        );
    }
}
