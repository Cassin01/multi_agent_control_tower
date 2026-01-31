use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::time::Duration;

use crate::config::Config;
use crate::context::{ContextStore, Decision, ExpertContext};
use crate::models::{EffortConfig, Task};
use crate::queue::QueueManager;
use crate::session::{CaptureManager, ClaudeManager, TmuxManager};

use super::ui::UI;
use super::widgets::{EffortSelector, ReportDisplay, StatusDisplay, TaskInput, ViewMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    ExpertList,
    TaskInput,
    EffortSelector,
    ReportList,
}

pub struct TowerApp {
    config: Config,
    #[allow(dead_code)]
    tmux: TmuxManager,
    capture: CaptureManager,
    claude: ClaudeManager,
    queue: QueueManager,
    context_store: ContextStore,

    status_display: StatusDisplay,
    task_input: TaskInput,
    effort_selector: EffortSelector,
    report_display: ReportDisplay,

    focus: FocusArea,
    running: bool,
    message: Option<String>,
    poll_counter: u32,
}

impl TowerApp {
    pub fn new(config: Config) -> Self {
        let session_name = config.session_name();
        let queue_manager = QueueManager::new(config.queue_path.clone());
        let context_store = ContextStore::new(config.queue_path.clone());
        let claude_manager = ClaudeManager::new(session_name.clone(), context_store.clone());

        Self {
            tmux: TmuxManager::new(session_name.clone()),
            capture: CaptureManager::new(session_name),
            claude: claude_manager,
            queue: queue_manager,
            context_store,
            config,

            status_display: StatusDisplay::new(),
            task_input: TaskInput::new(),
            effort_selector: EffortSelector::new(),
            report_display: ReportDisplay::new(),

            focus: FocusArea::ExpertList,
            running: true,
            message: None,
            poll_counter: 0,
        }
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn set_message(&mut self, msg: String) {
        self.message = Some(msg);
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    #[allow(dead_code)]
    pub fn focus(&self) -> FocusArea {
        self.focus
    }

    pub fn status_display(&mut self) -> &mut StatusDisplay {
        &mut self.status_display
    }

    pub fn task_input(&self) -> &TaskInput {
        &self.task_input
    }

    pub fn effort_selector(&self) -> &EffortSelector {
        &self.effort_selector
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn report_display(&mut self) -> &mut ReportDisplay {
        &mut self.report_display
    }

    pub async fn refresh_status(&mut self) -> Result<()> {
        let experts: Vec<(u32, String)> = self
            .config
            .experts
            .iter()
            .enumerate()
            .map(|(i, e)| (i as u32, e.name.clone()))
            .collect();

        let captures = self.capture.capture_all(&experts).await;
        self.status_display.set_captures(captures);
        Ok(())
    }

    pub async fn refresh_reports(&mut self) -> Result<()> {
        let reports = self.queue.list_reports().await?;
        self.report_display.set_reports(reports);
        Ok(())
    }

    async fn poll_status(&mut self) -> Result<()> {
        if self.poll_counter % 5 != 0 {
            return Ok(());
        }
        self.refresh_status().await
    }

    async fn poll_reports(&mut self) -> Result<()> {
        self.poll_counter += 1;
        if self.poll_counter % 10 != 0 {
            return Ok(());
        }
        self.refresh_reports().await
    }

    fn update_focus(&mut self) {
        self.status_display
            .set_focused(self.focus == FocusArea::ExpertList);
        self.task_input
            .set_focused(self.focus == FocusArea::TaskInput);
        self.effort_selector
            .set_focused(self.focus == FocusArea::EffortSelector);
        self.report_display
            .set_focused(self.focus == FocusArea::ReportList);
    }

    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            FocusArea::ExpertList => FocusArea::TaskInput,
            FocusArea::TaskInput => FocusArea::EffortSelector,
            FocusArea::EffortSelector => FocusArea::ReportList,
            FocusArea::ReportList => FocusArea::ExpertList,
        };
        self.update_focus();
    }

    pub fn prev_focus(&mut self) {
        self.focus = match self.focus {
            FocusArea::ExpertList => FocusArea::ReportList,
            FocusArea::TaskInput => FocusArea::ExpertList,
            FocusArea::EffortSelector => FocusArea::TaskInput,
            FocusArea::ReportList => FocusArea::EffortSelector,
        };
        self.update_focus();
    }

    pub async fn handle_events(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    return Ok(());
                }

                self.clear_message();

                if key.modifiers.contains(KeyModifiers::CONTROL) {
                    match key.code {
                        KeyCode::Char('c') | KeyCode::Char('q') => {
                            self.quit();
                            return Ok(());
                        }
                        _ => {}
                    }
                }

                match self.focus {
                    FocusArea::ExpertList => self.handle_expert_list_keys(key.code),
                    FocusArea::TaskInput => self.handle_task_input_keys(key.code, key.modifiers),
                    FocusArea::EffortSelector => self.handle_effort_selector_keys(key.code),
                    FocusArea::ReportList => self.handle_report_list_keys(key.code),
                }

                if key.code == KeyCode::Tab {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        self.prev_focus();
                    } else {
                        self.next_focus();
                    }
                }

                if key.code == KeyCode::Char('s') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.assign_task().await?;
                }
            }
        }
        Ok(())
    }

    fn handle_expert_list_keys(&mut self, code: KeyCode) {
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.status_display.prev(),
            KeyCode::Down | KeyCode::Char('j') => self.status_display.next(),
            _ => {}
        }
    }

    fn handle_task_input_keys(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Char(c) => {
                if !modifiers.contains(KeyModifiers::CONTROL)
                    && !modifiers.contains(KeyModifiers::ALT)
                {
                    self.task_input.insert_char(c);
                }
            }
            KeyCode::Backspace => self.task_input.delete_char(),
            KeyCode::Delete => self.task_input.delete_forward(),
            KeyCode::Left => self.task_input.move_cursor_left(),
            KeyCode::Right => self.task_input.move_cursor_right(),
            KeyCode::Home => self.task_input.move_cursor_start(),
            KeyCode::End => self.task_input.move_cursor_end(),
            KeyCode::Enter => {
                if modifiers.contains(KeyModifiers::SHIFT) {
                    self.task_input.insert_newline();
                }
            }
            KeyCode::Esc => {
                self.task_input.clear();
            }
            _ => {}
        }
    }

    fn handle_effort_selector_keys(&mut self, code: KeyCode) {
        match code {
            KeyCode::Left | KeyCode::Char('h') => self.effort_selector.prev(),
            KeyCode::Right | KeyCode::Char('l') => self.effort_selector.next(),
            _ => {}
        }
    }

    fn handle_report_list_keys(&mut self, code: KeyCode) {
        match self.report_display.view_mode() {
            ViewMode::List => match code {
                KeyCode::Up | KeyCode::Char('k') => self.report_display.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.report_display.next(),
                KeyCode::Enter => self.report_display.open_detail(),
                _ => {}
            },
            ViewMode::Detail => match code {
                KeyCode::Up | KeyCode::Char('k') => self.report_display.scroll_up(),
                KeyCode::Down | KeyCode::Char('j') => self.report_display.scroll_down(),
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') | KeyCode::Tab => {
                    self.report_display.close_detail()
                }
                _ => {}
            },
        }
    }

    pub async fn assign_task(&mut self) -> Result<()> {
        let expert_id = match self.status_display.selected_expert_id() {
            Some(id) => id,
            None => {
                self.set_message("No expert selected".to_string());
                return Ok(());
            }
        };

        if self.task_input.is_empty() {
            self.set_message("Task description is empty".to_string());
            return Ok(());
        }

        let expert_name = self
            .config
            .get_expert(expert_id)
            .map(|e| e.name.clone())
            .unwrap_or_else(|| format!("expert{}", expert_id));

        let task = Task::new(
            expert_id,
            expert_name.clone(),
            self.task_input.content().to_string(),
        )
        .with_effort(EffortConfig::from_level(self.effort_selector.selected()));

        self.queue.write_task(&task).await?;

        let decision = Decision::new(
            expert_id,
            format!("Task Assignment to {}", expert_name),
            format!(
                "Assigned: {}",
                task.description.chars().take(100).collect::<String>()
            ),
            format!("Effort: {:?}", self.effort_selector.selected()),
        );
        self.context_store
            .add_decision(&self.config.session_hash(), decision)
            .await?;

        let session_hash = self.config.session_hash();
        let mut expert_ctx = self
            .context_store
            .load_expert_context(&session_hash, expert_id)
            .await?
            .unwrap_or_else(|| {
                ExpertContext::new(expert_id, expert_name.clone(), session_hash.clone())
            });
        expert_ctx.add_task_history(
            task.task_id.clone(),
            "assigned".to_string(),
            task.description.chars().take(100).collect(),
        );
        self.context_store.save_expert_context(&expert_ctx).await?;

        let task_prompt = format!(
            "New task assigned:\n{}\n\nEffort level: {:?}\nPlease read the task file at queue/tasks/expert{}.yaml",
            task.description,
            self.effort_selector.selected(),
            expert_id
        );
        self.claude
            .send_keys_with_enter(expert_id, &task_prompt)
            .await?;

        self.task_input.clear();
        self.set_message(format!("Task assigned to {}", expert_name));

        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = UI::setup_terminal()?;

        self.update_focus();
        self.refresh_status().await?;
        self.refresh_reports().await?;

        while self.is_running() {
            terminal.draw(|frame| UI::render(frame, self))?;
            self.handle_events().await?;
            self.poll_status().await?;
            self.poll_reports().await?;
        }

        UI::restore_terminal()?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn create_test_config() -> Config {
        Config::default().with_project_path(PathBuf::from("/tmp/test"))
    }

    #[test]
    fn tower_app_starts_running() {
        let app = TowerApp::new(create_test_config());
        assert!(app.is_running());
    }

    #[test]
    fn tower_app_quit_stops_running() {
        let mut app = TowerApp::new(create_test_config());
        app.quit();
        assert!(!app.is_running());
    }

    #[test]
    fn tower_app_focus_cycles() {
        let mut app = TowerApp::new(create_test_config());

        assert_eq!(app.focus(), FocusArea::ExpertList);

        app.next_focus();
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.next_focus();
        assert_eq!(app.focus(), FocusArea::EffortSelector);

        app.next_focus();
        assert_eq!(app.focus(), FocusArea::ReportList);

        app.next_focus();
        assert_eq!(app.focus(), FocusArea::ExpertList);
    }

    #[test]
    fn tower_app_focus_cycles_backwards() {
        let mut app = TowerApp::new(create_test_config());

        app.prev_focus();
        assert_eq!(app.focus(), FocusArea::ReportList);

        app.prev_focus();
        assert_eq!(app.focus(), FocusArea::EffortSelector);

        app.prev_focus();
        assert_eq!(app.focus(), FocusArea::TaskInput);
    }

    #[test]
    fn tower_app_message_management() {
        let mut app = TowerApp::new(create_test_config());

        assert!(app.message().is_none());

        app.set_message("Test message".to_string());
        assert_eq!(app.message(), Some("Test message"));

        app.clear_message();
        assert!(app.message().is_none());
    }
}
