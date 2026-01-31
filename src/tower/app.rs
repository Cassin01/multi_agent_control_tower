use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::time::Duration;

use crate::config::Config;
use crate::context::ContextStore;
use crate::models::{EffortConfig, Task};
use crate::queue::QueueManager;
use crate::session::{CaptureManager, ClaudeManager, TmuxManager};

use super::ui::UI;
use super::widgets::{EffortSelector, StatusDisplay, TaskInput};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    ExpertList,
    TaskInput,
    EffortSelector,
}

pub struct TowerApp {
    config: Config,
    tmux: TmuxManager,
    capture: CaptureManager,
    claude: ClaudeManager,
    queue: QueueManager,
    context_store: ContextStore,

    status_display: StatusDisplay,
    task_input: TaskInput,
    effort_selector: EffortSelector,

    focus: FocusArea,
    running: bool,
    message: Option<String>,
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

            focus: FocusArea::ExpertList,
            running: true,
            message: None,
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

    fn update_focus(&mut self) {
        self.status_display.set_focused(self.focus == FocusArea::ExpertList);
        self.task_input.set_focused(self.focus == FocusArea::TaskInput);
        self.effort_selector.set_focused(self.focus == FocusArea::EffortSelector);
    }

    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            FocusArea::ExpertList => FocusArea::TaskInput,
            FocusArea::TaskInput => FocusArea::EffortSelector,
            FocusArea::EffortSelector => FocusArea::ExpertList,
        };
        self.update_focus();
    }

    pub fn prev_focus(&mut self) {
        self.focus = match self.focus {
            FocusArea::ExpertList => FocusArea::EffortSelector,
            FocusArea::TaskInput => FocusArea::ExpertList,
            FocusArea::EffortSelector => FocusArea::TaskInput,
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
                        KeyCode::Char('r') => {
                            self.refresh_status().await?;
                            self.set_message("Status refreshed".to_string());
                            return Ok(());
                        }
                        _ => {}
                    }
                }

                match self.focus {
                    FocusArea::ExpertList => self.handle_expert_list_keys(key.code),
                    FocusArea::TaskInput => self.handle_task_input_keys(key.code, key.modifiers),
                    FocusArea::EffortSelector => self.handle_effort_selector_keys(key.code),
                }

                if key.code == KeyCode::Tab {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        self.prev_focus();
                    } else {
                        self.next_focus();
                    }
                }

                if key.code == KeyCode::Enter && self.focus != FocusArea::TaskInput {
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
            KeyCode::Char(c) => self.task_input.insert_char(c),
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

        let task = Task::new(expert_id, expert_name.clone(), self.task_input.content().to_string())
            .with_effort(EffortConfig::from_level(self.effort_selector.selected()));

        self.queue.write_task(&task).await?;

        let task_prompt = format!(
            "New task assigned:\n{}\n\nEffort level: {:?}\nPlease read the task file at queue/tasks/expert{}.yaml",
            task.description,
            self.effort_selector.selected(),
            expert_id
        );
        self.claude.send_keys_with_enter(expert_id, &task_prompt).await?;

        self.task_input.clear();
        self.set_message(format!("Task assigned to {}", expert_name));

        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = UI::setup_terminal()?;

        self.update_focus();
        self.refresh_status().await?;

        while self.is_running() {
            terminal.draw(|frame| UI::render(frame, self))?;
            self.handle_events().await?;
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
        assert_eq!(app.focus(), FocusArea::ExpertList);
    }

    #[test]
    fn tower_app_focus_cycles_backwards() {
        let mut app = TowerApp::new(create_test_config());

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
