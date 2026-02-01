use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind};
use ratatui::layout::Rect;
use std::time::Duration;

use crate::config::Config;
use crate::context::{ContextStore, Decision, ExpertContext};
use crate::instructions::load_instruction_with_template;
use crate::models::{EffortConfig, Task};
use crate::queue::QueueManager;
use crate::session::{CaptureManager, ClaudeManager, TmuxManager};

use super::ui::UI;
use super::widgets::{EffortSelector, HelpModal, ReportDisplay, StatusDisplay, TaskInput, ViewMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    ExpertList,
    TaskInput,
    EffortSelector,
    ReportList,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutAreas {
    pub expert_list: Rect,
    pub task_input: Rect,
    pub effort_selector: Rect,
    pub report_list: Rect,
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
    help_modal: HelpModal,

    focus: FocusArea,
    running: bool,
    message: Option<String>,
    poll_counter: u32,
    layout_areas: LayoutAreas,
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
            help_modal: HelpModal::new(),

            focus: FocusArea::ExpertList,
            running: true,
            message: None,
            poll_counter: 0,
            layout_areas: LayoutAreas::default(),
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

    pub fn help_modal(&mut self) -> &mut HelpModal {
        &mut self.help_modal
    }

    pub fn set_layout_areas(&mut self, areas: LayoutAreas) {
        self.layout_areas = areas;
    }

    pub fn set_focus(&mut self, area: FocusArea) {
        self.focus = area;
        self.update_focus();
    }

    fn handle_mouse_click(&mut self, column: u16, row: u16) {
        let pos = (column, row);

        if Self::point_in_rect(pos, self.layout_areas.expert_list) {
            self.set_focus(FocusArea::ExpertList);
        } else if Self::point_in_rect(pos, self.layout_areas.task_input) {
            self.set_focus(FocusArea::TaskInput);
        } else if Self::point_in_rect(pos, self.layout_areas.effort_selector) {
            self.set_focus(FocusArea::EffortSelector);
        } else if Self::point_in_rect(pos, self.layout_areas.report_list) {
            self.set_focus(FocusArea::ReportList);
        }
    }

    fn point_in_rect(pos: (u16, u16), rect: Rect) -> bool {
        pos.0 >= rect.x
            && pos.0 < rect.x + rect.width
            && pos.1 >= rect.y
            && pos.1 < rect.y + rect.height
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
        if !self.poll_counter.is_multiple_of(5) {
            return Ok(());
        }
        self.refresh_status().await
    }

    async fn poll_reports(&mut self) -> Result<()> {
        self.poll_counter += 1;
        if !self.poll_counter.is_multiple_of(10) {
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
            match event::read()? {
                Event::Mouse(mouse) => {
                    if mouse.kind == MouseEventKind::Down(MouseButton::Left) {
                        if !self.help_modal.is_visible()
                            && self.report_display.view_mode() != ViewMode::Detail
                        {
                            self.handle_mouse_click(mouse.column, mouse.row);
                        }
                    }
                    return Ok(());
                }
                Event::Key(key) => {
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
                        KeyCode::Char('h') => {
                            self.help_modal.toggle();
                            return Ok(());
                        }
                        _ => {}
                    }
                }

                if self.help_modal.is_visible() {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                            self.help_modal.hide();
                        }
                        _ => {}
                    }
                    return Ok(());
                }

                if self.report_display.view_mode() == ViewMode::Detail {
                    match key.code {
                        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                            self.report_display.close_detail();
                        }
                        KeyCode::Up | KeyCode::Char('k') => self.report_display.scroll_up(),
                        KeyCode::Down | KeyCode::Char('j') => self.report_display.scroll_down(),
                        _ => {}
                    }
                    return Ok(());
                }

                match self.focus {
                    FocusArea::ExpertList => self.handle_expert_list_keys(key.code),
                    FocusArea::TaskInput => self.handle_task_input_keys(key.code, key.modifiers),
                    FocusArea::EffortSelector => self.handle_effort_selector_keys(key.code),
                    FocusArea::ReportList => self.handle_report_list_keys(key.code, key.modifiers),
                }

                if key.code == KeyCode::Tab {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        self.prev_focus();
                    } else {
                        self.next_focus();
                    }
                }
                if key.code == KeyCode::BackTab {
                    self.prev_focus();
                }

                if key.code == KeyCode::Char('s')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.focus == FocusArea::TaskInput
                {
                    self.assign_task().await?;
                }

                if key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.focus == FocusArea::TaskInput
                {
                    match key.code {
                        KeyCode::Char('p') => self.status_display.prev(),
                        KeyCode::Char('n') => self.status_display.next(),
                        _ => {}
                    }
                }

                if key.code == KeyCode::Char('r')
                    && key.modifiers.contains(KeyModifiers::CONTROL)
                    && self.focus == FocusArea::ExpertList
                {
                    self.reset_expert().await?;
                }
                }
                _ => {}
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
                self.task_input.insert_newline();
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

    fn handle_report_list_keys(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match self.report_display.view_mode() {
            ViewMode::List => match code {
                KeyCode::Up | KeyCode::Char('k') => self.report_display.prev(),
                KeyCode::Down | KeyCode::Char('j') => self.report_display.next(),
                KeyCode::Enter => self.report_display.open_detail(),
                _ => {}
            },
            ViewMode::Detail => {
                match code {
                    KeyCode::Up | KeyCode::Char('k') => self.report_display.scroll_up(),
                    KeyCode::Down | KeyCode::Char('j') => self.report_display.scroll_down(),
                    KeyCode::Esc | KeyCode::Enter | KeyCode::Char('q') => {
                        self.report_display.close_detail()
                    }
                    _ => {}
                }
                if code == KeyCode::Char('q') && modifiers.contains(KeyModifiers::CONTROL) {
                    self.report_display.close_detail();
                }
            }
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

    pub async fn reset_expert(&mut self) -> Result<()> {
        let expert_id = match self.status_display.selected_expert_id() {
            Some(id) => id,
            None => {
                self.set_message("No expert selected".to_string());
                return Ok(());
            }
        };

        let expert_name = self.config.get_expert_name(expert_id);

        self.set_message(format!("Resetting {}...", expert_name));

        self.context_store
            .clear_expert_context(&self.config.session_hash(), expert_id)
            .await?;

        self.claude.send_clear(expert_id).await?;

        let instruction =
            load_instruction_with_template(&self.config.instructions_path, &expert_name)?;
        if !instruction.is_empty() {
            self.claude.send_instruction(expert_id, &instruction).await?;
        }

        self.set_message(format!("{} reset complete", expert_name));
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

    #[test]
    fn tower_app_set_focus_changes_focus() {
        let mut app = TowerApp::new(create_test_config());

        assert_eq!(app.focus(), FocusArea::ExpertList);

        app.set_focus(FocusArea::TaskInput);
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.set_focus(FocusArea::ReportList);
        assert_eq!(app.focus(), FocusArea::ReportList);
    }

    #[test]
    fn point_in_rect_detects_inside() {
        let rect = Rect::new(10, 20, 30, 40);

        assert!(TowerApp::point_in_rect((10, 20), rect));
        assert!(TowerApp::point_in_rect((25, 35), rect));
        assert!(TowerApp::point_in_rect((39, 59), rect));
    }

    #[test]
    fn point_in_rect_detects_outside() {
        let rect = Rect::new(10, 20, 30, 40);

        assert!(!TowerApp::point_in_rect((9, 20), rect));
        assert!(!TowerApp::point_in_rect((10, 19), rect));
        assert!(!TowerApp::point_in_rect((40, 20), rect));
        assert!(!TowerApp::point_in_rect((10, 60), rect));
    }

    #[test]
    fn handle_mouse_click_sets_focus_based_on_area() {
        let mut app = TowerApp::new(create_test_config());

        app.set_layout_areas(LayoutAreas {
            expert_list: Rect::new(0, 0, 100, 10),
            task_input: Rect::new(0, 10, 100, 10),
            effort_selector: Rect::new(0, 20, 100, 5),
            report_list: Rect::new(0, 25, 100, 10),
        });

        app.handle_mouse_click(50, 5);
        assert_eq!(app.focus(), FocusArea::ExpertList);

        app.handle_mouse_click(50, 15);
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.handle_mouse_click(50, 22);
        assert_eq!(app.focus(), FocusArea::EffortSelector);

        app.handle_mouse_click(50, 30);
        assert_eq!(app.focus(), FocusArea::ReportList);
    }
}
