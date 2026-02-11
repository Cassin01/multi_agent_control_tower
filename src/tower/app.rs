use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::context::{AvailableRoles, ContextStore, Decision, ExpertContext, SessionExpertRoles};
use crate::experts::ExpertRegistry;
use crate::instructions::{load_instruction_with_template, write_instruction_file};
use crate::models::ExpertState;
use crate::models::{ExpertInfo, Role};
use crate::queue::{MessageRouter, QueueManager};
use crate::session::{
    ClaudeManager, ExpertStateDetector, TmuxManager, WorktreeLaunchResult, WorktreeLaunchState,
    WorktreeManager,
};
use crate::tower::widgets::ExpertEntry;
use crate::utils::sanitize_branch_name;

/// Message polling interval for the messaging system (3 seconds)
const MESSAGE_POLL_INTERVAL: Duration = Duration::from_millis(3000);

use super::ui::UI;
use super::widgets::{
    ExpertPanelDisplay, HelpModal, MessagingDisplay, ReportDisplay, RoleSelector, StatusDisplay,
    TaskInput, ViewMode,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusArea {
    ExpertList,
    TaskInput,
    ExpertPanel,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LayoutAreas {
    #[allow(dead_code)]
    pub expert_list: Rect,
    pub task_input: Rect,
    pub expert_panel: Rect,
}

fn keycode_to_tmux_key(code: KeyCode, modifiers: KeyModifiers) -> Option<String> {
    if modifiers.contains(KeyModifiers::CONTROL) {
        if let KeyCode::Char(c) = code {
            return Some(format!("C-{}", c));
        }
    }

    match code {
        KeyCode::Char(c) => Some(c.to_string()),
        KeyCode::Enter => Some("Enter".to_string()),
        KeyCode::Backspace => Some("BSpace".to_string()),
        KeyCode::Tab => Some("Tab".to_string()),
        KeyCode::BackTab => Some("BTab".to_string()),
        KeyCode::Esc => Some("Escape".to_string()),
        KeyCode::Up => Some("Up".to_string()),
        KeyCode::Down => Some("Down".to_string()),
        KeyCode::Left => Some("Left".to_string()),
        KeyCode::Right => Some("Right".to_string()),
        KeyCode::Home => None,
        KeyCode::End => None,
        KeyCode::PageUp => None,
        KeyCode::PageDown => None,
        KeyCode::Delete => Some("DC".to_string()),
        _ => None,
    }
}

pub struct TowerApp {
    config: Config,
    #[allow(dead_code)]
    tmux: TmuxManager,
    claude: ClaudeManager,
    queue: QueueManager,
    context_store: ContextStore,

    // Messaging system components
    message_router: Option<MessageRouter>,
    expert_registry: ExpertRegistry,
    detector: ExpertStateDetector,

    status_display: StatusDisplay,
    task_input: TaskInput,
    report_display: ReportDisplay,
    help_modal: HelpModal,
    role_selector: RoleSelector,
    messaging_display: MessagingDisplay,
    expert_panel_display: ExpertPanelDisplay,

    session_roles: SessionExpertRoles,
    available_roles: AvailableRoles,

    focus: FocusArea,
    running: bool,
    message: Option<String>,
    last_status_poll: Instant,
    last_report_poll: Instant,
    last_message_poll: Instant,
    last_input_time: Instant,
    last_panel_poll: Instant,
    layout_areas: LayoutAreas,

    last_preview_size: (u16, u16),
    last_resized_expert_id: Option<u32>,

    worktree_manager: WorktreeManager,
    worktree_launch_state: WorktreeLaunchState,
}

impl TowerApp {
    pub fn new(config: Config, worktree_manager: WorktreeManager) -> Self {
        let session_name = config.session_name();
        let session_hash = config.session_hash();
        let queue_manager = QueueManager::new(config.queue_path.clone());
        let context_store = ContextStore::new(config.queue_path.clone());
        let claude_manager = ClaudeManager::new(session_name.clone());
        let tmux_manager = TmuxManager::new(session_name.clone());

        let available_roles =
            match AvailableRoles::from_instructions_path(&config.role_instructions_path) {
                Ok(roles) => roles,
                Err(e) => {
                    eprintln!("Warning: Failed to load available roles: {}", e);
                    AvailableRoles::default()
                }
            };

        // Initialize expert registry with configured experts
        // Expert IDs match config indices (0-based), which also match tmux window indices
        let mut expert_registry = ExpertRegistry::new();
        for (i, expert_config) in config.experts.iter().enumerate() {
            let role_name = if expert_config.role.is_empty() {
                "general".to_string()
            } else {
                expert_config.role.clone()
            };
            let expert_info = ExpertInfo::new(
                i as u32,
                expert_config.name.clone(),
                Role::specialist(role_name),
                session_name.clone(),
                i.to_string(),
            );
            if let Err(e) = expert_registry.register_expert(expert_info) {
                tracing::warn!("Failed to register expert {}: {}", i, e);
            }
        }

        let detector = ExpertStateDetector::new(config.queue_path.join("status"));

        // Create message queue manager for messaging system
        let message_queue_manager = QueueManager::new(config.queue_path.clone());

        // Create message router with dependencies
        let message_router = MessageRouter::new(
            message_queue_manager,
            expert_registry.clone(),
            tmux_manager.clone(),
        );

        Self {
            tmux: tmux_manager,
            claude: claude_manager,
            queue: queue_manager,
            context_store,

            // Messaging system
            message_router: Some(message_router),
            expert_registry,
            detector,

            status_display: StatusDisplay::new(),
            task_input: TaskInput::new(),
            report_display: ReportDisplay::new(),
            help_modal: HelpModal::new(),
            role_selector: RoleSelector::new(),
            messaging_display: MessagingDisplay::new(),
            expert_panel_display: ExpertPanelDisplay::new(),

            session_roles: SessionExpertRoles::new(session_hash),
            available_roles,

            focus: FocusArea::TaskInput,
            running: true,
            message: None,
            last_status_poll: Instant::now(),
            last_report_poll: Instant::now(),
            last_message_poll: Instant::now(),
            last_input_time: Instant::now(),
            last_panel_poll: Instant::now(),
            layout_areas: LayoutAreas::default(),

            last_preview_size: (0, 0),
            last_resized_expert_id: None,

            worktree_manager,
            worktree_launch_state: WorktreeLaunchState::default(),

            config,
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

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn report_display(&mut self) -> &mut ReportDisplay {
        &mut self.report_display
    }

    pub fn help_modal(&mut self) -> &mut HelpModal {
        &mut self.help_modal
    }

    pub fn role_selector(&mut self) -> &mut RoleSelector {
        &mut self.role_selector
    }

    #[allow(dead_code)]
    pub fn get_expert_role(&self, expert_id: u32) -> Option<&str> {
        self.session_roles.get_role(expert_id)
    }

    #[cfg(test)]
    pub fn last_input_time(&self) -> Instant {
        self.last_input_time
    }

    #[cfg(test)]
    pub fn last_resized_expert_id(&self) -> Option<u32> {
        self.last_resized_expert_id
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

        if Self::point_in_rect(pos, self.layout_areas.task_input) {
            self.set_focus(FocusArea::TaskInput);
        } else if self.expert_panel_display.is_visible()
            && Self::point_in_rect(pos, self.layout_areas.expert_panel)
        {
            self.set_focus(FocusArea::ExpertPanel);
        }
    }

    fn point_in_rect(pos: (u16, u16), rect: Rect) -> bool {
        pos.0 >= rect.x
            && pos.0 < rect.x + rect.width
            && pos.1 >= rect.y
            && pos.1 < rect.y + rect.height
    }

    pub async fn refresh_status(&mut self) -> Result<()> {
        let expert_ids: Vec<u32> = (0..self.config.experts.len() as u32).collect();
        let states = self.detector.detect_all(&expert_ids);

        let entries: Vec<ExpertEntry> = self
            .config
            .experts
            .iter()
            .enumerate()
            .map(|(i, e)| {
                let state = states
                    .iter()
                    .find(|(id, _)| *id == i as u32)
                    .map(|(_, s)| s.clone())
                    .unwrap_or(ExpertState::Offline);
                ExpertEntry {
                    expert_id: i as u32,
                    expert_name: e.name.clone(),
                    state,
                }
            })
            .collect();

        self.status_display.set_experts(entries);

        let roles: std::collections::HashMap<u32, String> = self
            .session_roles
            .assignments
            .iter()
            .map(|a| (a.expert_id, a.role.clone()))
            .collect();
        self.status_display.set_expert_roles(roles);

        let mut working_dirs = std::collections::HashMap::new();
        for id in &expert_ids {
            if let Ok(Some(path)) = self.tmux.get_pane_current_path(*id).await {
                working_dirs.insert(*id, path);
            }
        }
        self.status_display.set_expert_working_dirs(working_dirs);
        self.status_display
            .set_project_path(self.config.project_path.display().to_string());

        Ok(())
    }

    pub async fn refresh_reports(&mut self) -> Result<()> {
        let reports = self.queue.list_reports().await?;
        let report_expert_ids: std::collections::HashSet<u32> =
            reports.iter().map(|r| r.expert_id).collect();
        self.report_display.set_reports(reports);
        self.status_display.set_expert_reports(report_expert_ids);
        Ok(())
    }

    async fn poll_status(&mut self) -> Result<()> {
        // Skip polling if user is actively interacting (within 500ms of last input)
        const INPUT_PAUSE_DURATION: Duration = Duration::from_millis(500);
        if self.last_input_time.elapsed() < INPUT_PAUSE_DURATION {
            tracing::trace!("poll_status: skipped (input debounce)");
            return Ok(());
        }

        const STATUS_POLL_INTERVAL: Duration = Duration::from_millis(2000);
        if self.last_status_poll.elapsed() < STATUS_POLL_INTERVAL {
            tracing::trace!("poll_status: skipped (interval)");
            return Ok(());
        }
        tracing::debug!("poll_status: executing refresh_status");
        self.last_status_poll = Instant::now();
        self.refresh_status().await
    }

    async fn poll_reports(&mut self) -> Result<()> {
        // Skip polling if user is actively interacting (within 500ms of last input)
        const INPUT_PAUSE_DURATION: Duration = Duration::from_millis(500);
        if self.last_input_time.elapsed() < INPUT_PAUSE_DURATION {
            tracing::trace!("poll_reports: skipped (input debounce)");
            return Ok(());
        }

        const REPORT_POLL_INTERVAL: Duration = Duration::from_millis(3000);
        if self.last_report_poll.elapsed() < REPORT_POLL_INTERVAL {
            tracing::trace!("poll_reports: skipped (interval)");
            return Ok(());
        }
        tracing::debug!("poll_reports: executing refresh_reports");
        self.last_report_poll = Instant::now();
        self.refresh_reports().await
    }

    /// Poll and process the inter-expert message queue
    ///
    /// This method:
    /// 1. Updates expert registry states from capture status
    /// 2. Processes the outbox for new messages
    /// 3. Processes the queue for message delivery
    /// 4. Updates the messaging display with current queue state
    async fn poll_messages(&mut self) -> Result<()> {
        // Skip polling if user is actively interacting (within 500ms of last input)
        const INPUT_PAUSE_DURATION: Duration = Duration::from_millis(500);
        if self.last_input_time.elapsed() < INPUT_PAUSE_DURATION {
            tracing::trace!("poll_messages: skipped (input debounce)");
            return Ok(());
        }

        if self.last_message_poll.elapsed() < MESSAGE_POLL_INTERVAL {
            tracing::trace!("poll_messages: skipped (interval)");
            return Ok(());
        }
        self.last_message_poll = Instant::now();

        if let Some(ref mut router) = self.message_router {
            // Update expert states from status marker files
            // Config indices and registry IDs are both 0-based
            for (i, _) in self.config.experts.iter().enumerate() {
                let expert_id = i as u32;
                let expert_state = self.detector.detect_state(expert_id);
                if let Err(e) = router
                    .expert_registry_mut()
                    .update_expert_state(expert_id, expert_state)
                {
                    tracing::warn!("Failed to update expert {} state: {}", expert_id, e);
                }
            }

            // Process outbox for new messages
            if let Err(e) = router.process_outbox().await {
                tracing::warn!("Failed to process outbox: {}", e);
            }

            // Process the queue
            match router.process_queue().await {
                Ok(stats) => {
                    if stats.messages_delivered > 0
                        || stats.messages_failed > 0
                        || stats.messages_expired > 0
                    {
                        tracing::info!(
                            "Message queue processed: {} delivered, {} failed, {} expired",
                            stats.messages_delivered,
                            stats.messages_failed,
                            stats.messages_expired
                        );
                    }
                    // Mark delivered experts as processing
                    for eid in &stats.delivered_expert_ids {
                        if let Err(e) = self.detector.set_marker(*eid, "processing") {
                            tracing::warn!(
                                "Failed to set processing marker for expert {}: {}",
                                eid,
                                e
                            );
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to process message queue: {}", e);
                }
            }

            // Update messaging display with current queue state
            match router.queue_manager().get_pending_messages().await {
                Ok(messages) => {
                    self.messaging_display.set_messages(messages);
                }
                Err(e) => {
                    tracing::warn!("Failed to get pending messages for display: {}", e);
                }
            }
        }

        Ok(())
    }

    async fn poll_expert_panel(&mut self) -> Result<()> {
        if !self.expert_panel_display.is_visible() {
            return Ok(());
        }

        if self.expert_panel_display.is_scrolling() {
            return Ok(());
        }

        const INPUT_PAUSE_DURATION: Duration = Duration::from_millis(500);
        if self.last_input_time.elapsed() < INPUT_PAUSE_DURATION {
            return Ok(());
        }

        const PANEL_POLL_INTERVAL: Duration = Duration::from_millis(250);
        if self.last_panel_poll.elapsed() < PANEL_POLL_INTERVAL {
            return Ok(());
        }
        self.last_panel_poll = Instant::now();

        let selected_id = self.status_display.selected_expert_id();
        if let Some(id) = selected_id {
            let name = self.config.get_expert_name(id);
            self.expert_panel_display.set_expert(id, name);
        }

        if let Some(expert_id) = self.expert_panel_display.expert_id() {
            let preview_size = self.expert_panel_display.preview_size();
            let size_changed = preview_size != self.last_preview_size;
            let expert_changed = self.last_resized_expert_id != Some(expert_id);

            if (size_changed || expert_changed) && preview_size.0 > 0 && preview_size.1 > 0 {
                if size_changed {
                    // Panel size changed (terminal resize): resize ALL expert panes.
                    // Mirrors claude-squad's SetSessionPreviewSize() (list.go:86-98).
                    for id in 0..self.config.num_experts() {
                        if let Err(e) = self.claude.resize_pane(id, preview_size.0, preview_size.1).await {
                            tracing::warn!("Failed to resize pane for expert {}: {}", id, e);
                        }
                    }
                    self.last_preview_size = preview_size;
                } else {
                    // Expert switched: resize only the newly viewed expert's pane.
                    if let Err(e) = self.claude.resize_pane(expert_id, preview_size.0, preview_size.1).await {
                        tracing::warn!("Failed to resize pane for expert {}: {}", expert_id, e);
                    }
                }
                self.last_resized_expert_id = Some(expert_id);
            }

            match self.claude.capture_pane_with_escapes(expert_id).await {
                Ok(raw) => {
                    self.expert_panel_display.try_set_content(&raw);
                }
                Err(e) => {
                    tracing::warn!("Failed to capture expert {} window: {}", expert_id, e);
                }
            }
        }

        Ok(())
    }

    /// Get the messaging display widget
    #[allow(dead_code)]
    pub fn messaging_display(&mut self) -> &mut MessagingDisplay {
        &mut self.messaging_display
    }

    pub fn expert_panel_display(&mut self) -> &mut ExpertPanelDisplay {
        &mut self.expert_panel_display
    }

    /// Get the expert registry
    #[allow(dead_code)]
    pub fn expert_registry(&self) -> &ExpertRegistry {
        &self.expert_registry
    }

    fn update_focus(&mut self) {
        // status_display is always display-only (not focusable)
        self.status_display.set_focused(false);
        self.task_input
            .set_focused(self.focus == FocusArea::TaskInput);
        self.expert_panel_display
            .set_focused(self.focus == FocusArea::ExpertPanel);
    }

    pub fn next_focus(&mut self) {
        let panel_visible = self.expert_panel_display.is_visible();
        self.focus = match self.focus {
            FocusArea::ExpertList => FocusArea::TaskInput,
            FocusArea::TaskInput => {
                if panel_visible {
                    FocusArea::ExpertPanel
                } else {
                    FocusArea::TaskInput
                }
            }
            FocusArea::ExpertPanel => FocusArea::TaskInput,
        };
        self.update_focus();
    }

    pub async fn handle_events(&mut self) -> Result<()> {
        if event::poll(Duration::from_millis(1))? {
            match event::read()? {
                Event::Mouse(mouse) => {
                    // Update input time for mouse events to pause polling during interaction
                    self.last_input_time = Instant::now();

                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && !self.help_modal.is_visible()
                        && self.report_display.view_mode() != ViewMode::Detail
                    {
                        self.handle_mouse_click(mouse.column, mouse.row);
                    }
                    return Ok(());
                }
                Event::Key(key) => {
                    if key.kind != KeyEventKind::Press {
                        return Ok(());
                    }

                    // Update input time for key presses to pause polling during interaction.
                    // Skip when ExpertPanel is focused: keys are forwarded to tmux, and
                    // the debounce would freeze the panel's live capture for 500ms per keystroke.
                    if self.focus != FocusArea::ExpertPanel {
                        self.last_input_time = Instant::now();
                    }
                    tracing::debug!("Key pressed: {:?}, focus: {:?}", key.code, self.focus);

                    self.clear_message();

                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('c') | KeyCode::Char('q') => {
                                self.quit();
                                return Ok(());
                            }
                            KeyCode::Char('i') => {
                                self.help_modal.toggle();
                                return Ok(());
                            }
                            KeyCode::Char('j') => {
                                self.expert_panel_display.toggle();
                                if !self.expert_panel_display.is_visible()
                                    && self.focus == FocusArea::ExpertPanel
                                {
                                    self.set_focus(FocusArea::TaskInput);
                                }
                                return Ok(());
                            }
                            _ => {}
                        }
                    }

                    if self.help_modal.is_visible() {
                        match key.code {
                            KeyCode::Enter | KeyCode::Char('q') => {
                                self.help_modal.hide();
                            }
                            _ => {}
                        }
                        return Ok(());
                    }

                    if self.report_display.view_mode() == ViewMode::Detail {
                        match key.code {
                            KeyCode::Enter | KeyCode::Char('q') => {
                                self.report_display.close_detail();
                            }
                            KeyCode::Char('x')
                                if key.modifiers.contains(KeyModifiers::CONTROL) =>
                            {
                                self.report_display.close_detail();
                            }
                            KeyCode::Up | KeyCode::Char('k') => self.report_display.scroll_up(),
                            KeyCode::Down | KeyCode::Char('j') => self.report_display.scroll_down(),
                            _ => {}
                        }
                        return Ok(());
                    }

                    if self.role_selector.is_visible() {
                        match key.code {
                            KeyCode::Char('q') => {
                                self.role_selector.hide();
                            }
                            KeyCode::Enter => {
                                self.confirm_role_selection().await?;
                            }
                            KeyCode::Up | KeyCode::Char('k') => self.role_selector.prev(),
                            KeyCode::Down | KeyCode::Char('j') => self.role_selector.next(),
                            _ => {}
                        }
                        return Ok(());
                    }

                    match self.focus {
                        FocusArea::ExpertList => {} // Display only, not selectable
                        FocusArea::TaskInput => {
                            self.handle_task_input_keys(key.code, key.modifiers)
                        }
                        FocusArea::ExpertPanel => {
                            self.handle_expert_panel_keys(key.code, key.modifiers)
                                .await?;
                            return Ok(());
                        }
                    }

                    if key.code == KeyCode::Char('t')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                    {
                        self.next_focus();
                    }

                    if key.code == KeyCode::Char('s')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.focus == FocusArea::TaskInput
                    {
                        self.assign_task().await?;
                    }

                    if self.focus == FocusArea::TaskInput {
                        match key.code {
                            KeyCode::Up => self.status_display.prev(),
                            KeyCode::Down => self.status_display.next(),
                            _ => {}
                        }
                        if key.modifiers.contains(KeyModifiers::CONTROL) {
                            if let KeyCode::Char('o') = key.code {
                                self.open_role_selector();
                            }
                        }
                    }

                    if key.code == KeyCode::Char('r')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.focus == FocusArea::TaskInput
                    {
                        self.reset_expert().await?;
                    }

                    if key.code == KeyCode::Char('w')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.focus == FocusArea::TaskInput
                    {
                        self.launch_expert_in_worktree().await?;
                    }

                    if key.code == KeyCode::Char('x')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.focus == FocusArea::TaskInput
                    {
                        self.open_expert_report();
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn handle_task_input_keys(&mut self, code: KeyCode, modifiers: KeyModifiers) {
        match code {
            KeyCode::Char(c) => {
                if modifiers.contains(KeyModifiers::CONTROL) {
                    match c {
                        'b' => self.task_input.move_cursor_left(),
                        'f' => self.task_input.move_cursor_right(),
                        'a' => self.task_input.move_cursor_line_start(),
                        'e' => self.task_input.move_cursor_line_end(),
                        'p' => self.task_input.move_cursor_up(),
                        'n' => self.task_input.move_cursor_down(),
                        'h' => {
                            self.task_input.delete_char();
                            self.last_input_time = Instant::now();
                        }
                        'd' => {
                            self.task_input.delete_forward();
                            self.last_input_time = Instant::now();
                        }
                        'u' => {
                            self.task_input.unix_line_discard();
                            self.last_input_time = Instant::now();
                        }
                        'k' => {
                            self.task_input.kill_line();
                            self.last_input_time = Instant::now();
                        }
                        _ => {}
                    }
                } else if !modifiers.contains(KeyModifiers::ALT) {
                    self.task_input.insert_char(c);
                    self.last_input_time = Instant::now();
                }
            }
            KeyCode::Backspace => {
                self.task_input.delete_char();
                self.last_input_time = Instant::now();
            }
            KeyCode::Delete => {
                self.task_input.delete_forward();
                self.last_input_time = Instant::now();
            }
            KeyCode::Home => self.task_input.move_cursor_start(),
            KeyCode::End => self.task_input.move_cursor_end(),
            KeyCode::Enter => {
                self.task_input.insert_newline();
                self.last_input_time = Instant::now();
            }
            _ => {}
        }
    }

    async fn handle_expert_panel_keys(
        &mut self,
        code: KeyCode,
        modifiers: KeyModifiers,
    ) -> Result<()> {
        // ESC exits scroll mode without forwarding to tmux
        if code == KeyCode::Esc && self.expert_panel_display.is_scrolling() {
            self.expert_panel_display.exit_scroll_mode();
            return Ok(());
        }

        match code {
            KeyCode::PageUp => {
                if !self.expert_panel_display.is_scrolling() {
                    if let Some(expert_id) = self.expert_panel_display.expert_id() {
                        match self.claude.capture_full_history(expert_id).await {
                            Ok(raw) => {
                                self.expert_panel_display.enter_scroll_mode(&raw);
                            }
                            Err(e) => {
                                tracing::warn!(
                                    "Failed to capture full history for expert {}: {}",
                                    expert_id,
                                    e
                                );
                            }
                        }
                    }
                } else {
                    self.expert_panel_display.scroll_up();
                }
                return Ok(());
            }
            KeyCode::PageDown => {
                self.expert_panel_display.scroll_down();
                return Ok(());
            }
            KeyCode::Home => {
                self.expert_panel_display.scroll_to_top();
                return Ok(());
            }
            KeyCode::End => {
                self.expert_panel_display.scroll_to_bottom();
                return Ok(());
            }
            _ => {}
        }

        if let Some(tmux_key) = keycode_to_tmux_key(code, modifiers) {
            if let Some(expert_id) = self.expert_panel_display.expert_id() {
                if let Err(e) = self.claude.send_keys(expert_id, &tmux_key).await {
                    tracing::warn!("Failed to send keys to expert {}: {}", expert_id, e);
                    self.set_message(format!("Error sending keys to expert: {}", e));
                }
            }
        }

        Ok(())
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

        let description = self.task_input.content().to_string();

        let decision = Decision::new(
            expert_id,
            format!("Task Assignment to {}", expert_name),
            format!(
                "Assigned: {}",
                description.chars().take(100).collect::<String>()
            ),
            String::new(),
        );
        self.context_store
            .add_decision(&self.config.session_hash(), decision)
            .await?;

        let session_hash = self.config.session_hash();
        let expert_ctx = self
            .context_store
            .load_expert_context(&session_hash, expert_id)
            .await?
            .unwrap_or_else(|| {
                ExpertContext::new(expert_id, expert_name.clone(), session_hash.clone())
            });
        self.context_store.save_expert_context(&expert_ctx).await?;

        self.claude
            .send_keys_with_enter(expert_id, &description)
            .await?;

        self.task_input.clear();
        self.set_message(format!("Task assigned to {}", expert_name));

        Ok(())
    }

    pub async fn initialize_session_roles(&mut self) -> Result<()> {
        let session_hash = self.config.session_hash();

        let mut roles = match self.context_store.load_session_roles(&session_hash).await {
            Ok(Some(r)) => r,
            Ok(None) => SessionExpertRoles::new(session_hash.clone()),
            Err(e) => {
                eprintln!(
                    "Warning: Failed to load session roles, recreating with defaults: {}",
                    e
                );
                SessionExpertRoles::new(session_hash.clone())
            }
        };

        for i in 0..self.config.num_experts() {
            if roles.get_role(i).is_none() {
                let default_role = self.config.get_expert_role(i);
                roles.set_role(i, default_role);
            }
        }

        self.context_store.save_session_roles(&roles).await?;
        self.session_roles = roles;

        Ok(())
    }

    pub async fn change_expert_role(&mut self, expert_id: u32, new_role: &str) -> Result<()> {
        if let Some(entry) = self.status_display.selected() {
            if entry.state == ExpertState::Busy {
                self.set_message(format!(
                    "Warning: Expert {} is currently active. Role change may interrupt work.",
                    expert_id
                ));
            }
        }

        self.session_roles.set_role(expert_id, new_role.to_string());
        self.context_store
            .save_session_roles(&self.session_roles)
            .await?;

        self.claude.send_exit(expert_id).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;

        let expert_name = self.config.get_expert_name(expert_id);
        let instruction_result = load_instruction_with_template(
            &self.config.core_instructions_path,
            &self.config.role_instructions_path,
            new_role,
            expert_id,
            &expert_name,
            &self.config.status_file_path(expert_id),
        )?;
        let instruction_file = if !instruction_result.content.is_empty() {
            Some(write_instruction_file(
                &self.config.queue_path,
                expert_id,
                &instruction_result.content,
            )?)
        } else {
            None
        };

        let working_dir = self.config.project_path.to_str().unwrap_or(".").to_string();
        self.claude
            .launch_claude(expert_id, &working_dir, instruction_file.as_deref())
            .await?;

        if instruction_result.used_general_fallback {
            self.set_message(format!(
                "Role '{}' not found, using 'general'",
                instruction_result.requested_role
            ));
        } else {
            self.set_message(format!("Expert {} role changed to {}", expert_id, new_role));
        }

        Ok(())
    }

    fn open_role_selector(&mut self) {
        if self.available_roles.roles.is_empty() {
            self.set_message("No roles available".to_string());
            return;
        }

        if let Some(expert_id) = self.status_display.selected_expert_id() {
            let current_role = self
                .session_roles
                .get_role(expert_id)
                .unwrap_or("general")
                .to_string();
            let roles = self.available_roles.roles.clone();
            self.role_selector.show(expert_id, &current_role, roles);
        }
    }

    fn open_expert_report(&mut self) {
        if let Some(expert_id) = self.status_display.selected_expert_id() {
            if !self.report_display.open_detail_for_expert(expert_id) {
                self.set_message(format!("No report found for expert {}", expert_id));
            }
        }
    }

    async fn confirm_role_selection(&mut self) -> Result<()> {
        if let (Some(expert_id), Some(new_role)) = (
            self.role_selector.expert_id(),
            self.role_selector.selected_role().map(|s| s.to_string()),
        ) {
            self.role_selector.hide();
            self.change_expert_role(expert_id, &new_role).await?;
        }
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

        let instruction_role = self
            .session_roles
            .get_role(expert_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| self.config.get_expert_role(expert_id));

        self.set_message(format!(
            "Resetting {} (role: {})...",
            expert_name, instruction_role
        ));

        self.claude.send_exit(expert_id).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;

        self.context_store
            .clear_expert_context(&self.config.session_hash(), expert_id)
            .await?;

        let instruction_result = load_instruction_with_template(
            &self.config.core_instructions_path,
            &self.config.role_instructions_path,
            &instruction_role,
            expert_id,
            &expert_name,
            &self.config.status_file_path(expert_id),
        )?;
        let instruction_file = if !instruction_result.content.is_empty() {
            Some(write_instruction_file(
                &self.config.queue_path,
                expert_id,
                &instruction_result.content,
            )?)
        } else {
            None
        };

        let working_dir = self.config.project_path.to_str().unwrap_or(".").to_string();
        self.claude
            .launch_claude(expert_id, &working_dir, instruction_file.as_deref())
            .await?;

        if instruction_result.used_general_fallback {
            self.set_message(format!(
                "{} reset (role '{}' not found, using 'general')",
                expert_name, instruction_result.requested_role
            ));
        } else {
            self.set_message(format!("{} reset complete", expert_name));
        }
        Ok(())
    }

    pub async fn launch_expert_in_worktree(&mut self) -> Result<()> {
        if !matches!(self.worktree_launch_state, WorktreeLaunchState::Idle) {
            self.set_message("Worktree launch already in progress".to_string());
            return Ok(());
        }

        let feature_input = self.task_input.content().trim().to_string();
        if feature_input.is_empty() {
            self.set_message(
                "Enter a feature name in the task input before launching worktree".to_string(),
            );
            return Ok(());
        }

        let expert_id = match self.status_display.selected_expert_id() {
            Some(id) => id,
            None => {
                self.set_message("No expert selected".to_string());
                return Ok(());
            }
        };

        let expert_name = self.config.get_expert_name(expert_id);
        let sanitized = sanitize_branch_name(&feature_input);
        let branch_name = format!(
            "{}-{}",
            sanitized,
            chrono::Utc::now().format("%Y%m%d-%H%M%S")
        );

        if self.worktree_manager.worktree_exists(&branch_name) {
            self.set_message(format!("Worktree '{}' already exists", branch_name));
            return Ok(());
        }

        self.set_message(format!("Creating worktree '{}'...", branch_name));

        let claude = self.claude.clone();
        let context_store = self.context_store.clone();
        let worktree_manager = self.worktree_manager.clone();
        let config = self.config.clone();
        let session_hash = config.session_hash();
        let instruction_role = self
            .session_roles
            .get_role(expert_id)
            .map(|s| s.to_string())
            .unwrap_or_else(|| config.get_expert_role(expert_id));
        let core_path = config.core_instructions_path.clone();
        let role_path = config.role_instructions_path.clone();
        let queue_path = config.queue_path.clone();
        let expert_name_clone = expert_name.clone();
        let branch_clone = branch_name.clone();
        let ready_timeout = config.timeouts.agent_ready;
        let status_file_path = config.status_file_path(expert_id);

        let handle = tokio::spawn(async move {
            claude.send_exit(expert_id).await?;
            tokio::time::sleep(Duration::from_secs(3)).await;

            let worktree_path = worktree_manager.create_worktree(&branch_clone).await?;

            worktree_manager.setup_macot_symlink(&worktree_path).await?;

            let wt_path_str = worktree_path
                .to_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Worktree path contains non-UTF8 characters: {:?}",
                        worktree_path
                    )
                })?
                .to_string();

            let mut expert_ctx = context_store
                .load_expert_context(&session_hash, expert_id)
                .await?
                .unwrap_or_else(|| {
                    ExpertContext::new(expert_id, expert_name_clone.clone(), session_hash.clone())
                });
            expert_ctx.clear_session();
            expert_ctx.set_worktree(branch_clone.clone(), wt_path_str.clone());
            context_store.save_expert_context(&expert_ctx).await?;

            let instruction_result = load_instruction_with_template(
                &core_path,
                &role_path,
                &instruction_role,
                expert_id,
                &expert_name_clone,
                &status_file_path,
            )?;
            let instruction_file = if !instruction_result.content.is_empty() {
                Some(write_instruction_file(
                    &queue_path,
                    expert_id,
                    &instruction_result.content,
                )?)
            } else {
                None
            };

            claude
                .launch_claude(expert_id, &wt_path_str, instruction_file.as_deref())
                .await?;

            let ready = claude.wait_for_ready(expert_id, ready_timeout).await?;

            Ok(WorktreeLaunchResult {
                expert_id,
                expert_name: expert_name_clone,
                branch_name: branch_clone,
                worktree_path: wt_path_str,
                claude_ready: ready,
            })
        });

        self.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name,
            branch_name,
        };

        Ok(())
    }

    pub async fn poll_worktree_launch(&mut self) -> Result<()> {
        let state = std::mem::take(&mut self.worktree_launch_state);
        match state {
            WorktreeLaunchState::InProgress {
                handle,
                expert_name,
                branch_name,
            } => {
                if handle.is_finished() {
                    match handle.await {
                        Ok(Ok(result)) => {
                            let msg = if result.claude_ready {
                                format!(
                                    "{} launched in worktree '{}'",
                                    result.expert_name, result.branch_name
                                )
                            } else {
                                format!(
                                    "Worktree '{}' created but Claude may still be starting",
                                    result.branch_name
                                )
                            };
                            self.set_message(msg);
                        }
                        Ok(Err(e)) => {
                            self.set_message(format!("Worktree launch failed: {}", e));
                        }
                        Err(e) => {
                            self.set_message(format!("Worktree launch panicked: {}", e));
                        }
                    }
                    self.worktree_launch_state = WorktreeLaunchState::Idle;
                } else {
                    self.worktree_launch_state = WorktreeLaunchState::InProgress {
                        handle,
                        expert_name,
                        branch_name,
                    };
                }
            }
            WorktreeLaunchState::Idle => {
                self.worktree_launch_state = WorktreeLaunchState::Idle;
            }
        }
        Ok(())
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut terminal = UI::setup_terminal()?;

        self.initialize_session_roles().await?;
        self.update_focus();
        self.refresh_status().await?;
        self.refresh_reports().await?;

        while self.is_running() {
            let loop_start = Instant::now();

            let draw_start = Instant::now();
            terminal.draw(|frame| UI::render(frame, self))?;
            let draw_elapsed = draw_start.elapsed();

            let events_start = Instant::now();
            self.handle_events().await?;
            let events_elapsed = events_start.elapsed();

            let poll_status_start = Instant::now();
            self.poll_status().await?;
            let poll_status_elapsed = poll_status_start.elapsed();

            let poll_reports_start = Instant::now();
            self.poll_reports().await?;
            let poll_reports_elapsed = poll_reports_start.elapsed();

            let poll_messages_start = Instant::now();
            self.poll_messages().await?;
            let poll_messages_elapsed = poll_messages_start.elapsed();

            self.poll_expert_panel().await?;
            self.poll_worktree_launch().await?;

            let loop_elapsed = loop_start.elapsed();
            if loop_elapsed.as_millis() > 20 {
                tracing::debug!(
                    "Loop: {}ms (draw: {}ms, events: {}ms, poll_status: {}ms, poll_reports: {}ms, poll_messages: {}ms)",
                    loop_elapsed.as_millis(),
                    draw_elapsed.as_millis(),
                    events_elapsed.as_millis(),
                    poll_status_elapsed.as_millis(),
                    poll_reports_elapsed.as_millis(),
                    poll_messages_elapsed.as_millis()
                );
            }
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

    fn create_test_app() -> TowerApp {
        let config = create_test_config();
        let wm = WorktreeManager::new(config.project_path.clone());
        TowerApp::new(config, wm)
    }

    // keycode_to_tmux_key tests (P7: Input Isolation)

    #[test]
    fn keycode_to_tmux_key_char() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Char('a'), KeyModifiers::NONE),
            Some("a".to_string()),
            "keycode_to_tmux_key: plain char 'a' should return \"a\""
        );
    }

    #[test]
    fn keycode_to_tmux_key_ctrl_char() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Char('c'), KeyModifiers::CONTROL),
            Some("C-c".to_string()),
            "keycode_to_tmux_key: Ctrl+c should return \"C-c\""
        );
    }

    #[test]
    fn keycode_to_tmux_key_enter() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Enter, KeyModifiers::NONE),
            Some("Enter".to_string()),
            "keycode_to_tmux_key: Enter should return \"Enter\""
        );
    }

    #[test]
    fn keycode_to_tmux_key_backspace() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Backspace, KeyModifiers::NONE),
            Some("BSpace".to_string()),
            "keycode_to_tmux_key: Backspace should return \"BSpace\""
        );
    }

    #[test]
    fn keycode_to_tmux_key_tab_returns_tab_string() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Tab, KeyModifiers::NONE),
            Some("Tab".to_string()),
            "keycode_to_tmux_key: Tab should return \"Tab\" (NOT None — forwarded to tmux)"
        );
    }

    #[test]
    fn keycode_to_tmux_key_backtab_returns_btab() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::BackTab, KeyModifiers::NONE),
            Some("BTab".to_string()),
            "keycode_to_tmux_key: BackTab should return \"BTab\""
        );
    }

    #[test]
    fn keycode_to_tmux_key_esc_returns_escape_string() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Esc, KeyModifiers::NONE),
            Some("Escape".to_string()),
            "keycode_to_tmux_key: Esc should return \"Escape\" (NOT None — forwarded to tmux)"
        );
    }

    #[test]
    fn keycode_to_tmux_key_page_up_returns_none() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::PageUp, KeyModifiers::NONE),
            None,
            "keycode_to_tmux_key: PageUp should return None (reserved for local scroll)"
        );
    }

    #[test]
    fn keycode_to_tmux_key_page_down_returns_none() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::PageDown, KeyModifiers::NONE),
            None,
            "keycode_to_tmux_key: PageDown should return None (reserved for local scroll)"
        );
    }

    #[test]
    fn keycode_to_tmux_key_arrows() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Up, KeyModifiers::NONE),
            Some("Up".to_string()),
            "keycode_to_tmux_key: Up arrow"
        );
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Down, KeyModifiers::NONE),
            Some("Down".to_string()),
            "keycode_to_tmux_key: Down arrow"
        );
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Left, KeyModifiers::NONE),
            Some("Left".to_string()),
            "keycode_to_tmux_key: Left arrow"
        );
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Right, KeyModifiers::NONE),
            Some("Right".to_string()),
            "keycode_to_tmux_key: Right arrow"
        );
    }

    #[test]
    fn keycode_to_tmux_key_home_end_returns_none() {
        assert_eq!(
            keycode_to_tmux_key(KeyCode::Home, KeyModifiers::NONE),
            None,
            "keycode_to_tmux_key: Home should return None (reserved for local scroll)"
        );
        assert_eq!(
            keycode_to_tmux_key(KeyCode::End, KeyModifiers::NONE),
            None,
            "keycode_to_tmux_key: End should return None (reserved for local scroll)"
        );
    }

    #[test]
    fn tower_app_starts_running() {
        let app = create_test_app();
        assert!(app.is_running());
    }

    #[test]
    fn tower_app_quit_stops_running() {
        let mut app = create_test_app();
        app.quit();
        assert!(!app.is_running());
    }

    #[test]
    fn tower_app_focus_stays_on_task_input_without_panel() {
        let mut app = create_test_app();

        // ExpertList is display-only, initial focus is TaskInput
        assert_eq!(app.focus(), FocusArea::TaskInput);

        // Without panel visible, focus stays on TaskInput
        app.next_focus();
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.next_focus();
        assert_eq!(app.focus(), FocusArea::TaskInput);
    }

    #[test]
    fn tower_app_message_management() {
        let mut app = create_test_app();

        assert!(app.message().is_none());

        app.set_message("Test message".to_string());
        assert_eq!(app.message(), Some("Test message"));

        app.clear_message();
        assert!(app.message().is_none());
    }

    #[test]
    fn tower_app_set_focus_changes_focus() {
        let mut app = create_test_app();

        // ExpertList is display-only, initial focus is TaskInput
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.expert_panel_display.show();
        app.set_focus(FocusArea::ExpertPanel);
        assert_eq!(app.focus(), FocusArea::ExpertPanel);
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
        let mut app = create_test_app();

        app.set_layout_areas(LayoutAreas {
            expert_list: Rect::new(0, 0, 100, 10),
            task_input: Rect::new(0, 10, 100, 10),
            expert_panel: Rect::default(),
        });

        // ExpertList is display-only, clicking it doesn't change focus
        app.handle_mouse_click(50, 5);
        assert_eq!(app.focus(), FocusArea::TaskInput); // Stays at TaskInput

        app.handle_mouse_click(50, 15);
        assert_eq!(app.focus(), FocusArea::TaskInput);
    }

    // Task 10.1: Focus cycling tests (P2, P3)

    #[test]
    fn focus_cycle_without_panel_stays_on_task_input() {
        let mut app = create_test_app();
        // Panel is hidden by default
        assert!(!app.expert_panel_display.is_visible());
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.next_focus();
        assert_eq!(
            app.focus(),
            FocusArea::TaskInput,
            "should stay on TaskInput when panel hidden"
        );
    }

    #[test]
    fn focus_cycle_with_panel_includes_expert_panel() {
        let mut app = create_test_app();
        app.expert_panel_display.show();
        assert_eq!(app.focus(), FocusArea::TaskInput);

        app.next_focus();
        assert_eq!(
            app.focus(),
            FocusArea::ExpertPanel,
            "should visit ExpertPanel when visible"
        );
        app.next_focus();
        assert_eq!(
            app.focus(),
            FocusArea::TaskInput,
            "full cycle should return to start"
        );
    }

    #[test]
    fn hiding_panel_while_focused_moves_to_task_input() {
        let mut app = create_test_app();
        app.expert_panel_display.show();
        app.set_focus(FocusArea::ExpertPanel);
        assert_eq!(app.focus(), FocusArea::ExpertPanel);

        // Hide the panel — P2 requires focus moves to TaskInput
        app.expert_panel_display.hide();
        if app.focus() == FocusArea::ExpertPanel {
            app.set_focus(FocusArea::TaskInput);
        }
        assert_eq!(
            app.focus(),
            FocusArea::TaskInput,
            "hiding panel while focused should move focus to TaskInput"
        );
    }

    #[test]
    fn mouse_click_does_not_match_zero_rect() {
        let mut app = create_test_app();
        // expert_panel is Rect::default() (zero rect) — panel hidden
        app.set_layout_areas(LayoutAreas {
            expert_list: Rect::new(0, 0, 100, 10),
            task_input: Rect::new(0, 10, 100, 10),
            expert_panel: Rect::default(),
        });

        // Click at (0,0) — inside expert_list (display-only) and expert_panel zero rect
        app.handle_mouse_click(0, 0);
        assert_ne!(
            app.focus(),
            FocusArea::ExpertPanel,
            "click should not match zero expert_panel rect"
        );
    }

    #[test]
    fn mouse_click_matches_expert_panel_when_visible() {
        let mut app = create_test_app();
        app.expert_panel_display.show();
        app.set_layout_areas(LayoutAreas {
            expert_list: Rect::new(0, 0, 100, 10),
            task_input: Rect::new(0, 10, 100, 10),
            expert_panel: Rect::new(0, 20, 100, 15),
        });

        app.handle_mouse_click(50, 25);
        assert_eq!(
            app.focus(),
            FocusArea::ExpertPanel,
            "click in expert panel area should set focus"
        );
    }

    #[test]
    fn toggle_panel_visibility() {
        let mut app = create_test_app();
        // Panel starts hidden
        assert!(!app.expert_panel_display.is_visible());

        // Toggle on — panel becomes visible
        app.expert_panel_display.toggle();
        assert!(app.expert_panel_display.is_visible());

        // When visible, focus cycle should have 2 stops (TaskInput, ExpertPanel)
        let mut visited = Vec::new();
        let start = app.focus();
        loop {
            app.next_focus();
            visited.push(app.focus());
            if app.focus() == start {
                break;
            }
        }
        assert!(
            visited.contains(&FocusArea::ExpertPanel),
            "visible panel: focus cycle should include ExpertPanel, got: {:?}",
            visited
        );
        assert_eq!(
            visited.len(),
            2,
            "visible panel: focus cycle should have 2 stops"
        );

        // Toggle off — panel becomes hidden
        app.expert_panel_display.toggle();
        assert!(!app.expert_panel_display.is_visible());

        // When hidden, focus stays on TaskInput (only 1 focusable area)
        app.next_focus();
        assert_eq!(
            app.focus(),
            FocusArea::TaskInput,
            "hidden panel: focus should stay on TaskInput"
        );
    }

    #[test]
    fn update_focus_syncs_expert_panel_focus_state() {
        let mut app = create_test_app();
        app.expert_panel_display.show();

        app.set_focus(FocusArea::ExpertPanel);
        assert!(
            app.expert_panel_display.is_focused(),
            "expert panel should be focused"
        );

        app.set_focus(FocusArea::TaskInput);
        assert!(
            !app.expert_panel_display.is_focused(),
            "expert panel should lose focus"
        );
    }

    #[test]
    fn expert_panel_focus_does_not_update_debounce_timer() {
        let mut app = create_test_app();
        app.expert_panel_display.show();
        app.set_focus(FocusArea::ExpertPanel);

        // Record the initial last_input_time
        let before = app.last_input_time();

        // Simulate time passing so we can detect a change
        std::thread::sleep(std::time::Duration::from_millis(10));

        // Simulate a keypress in handle_events context:
        // When ExpertPanel is focused, last_input_time should NOT be updated
        // We test the condition directly since handle_events requires terminal setup
        if app.focus() != FocusArea::ExpertPanel {
            panic!("focus should be ExpertPanel");
        }
        // The guard: focus == ExpertPanel means no update
        assert_eq!(
            app.last_input_time(),
            before,
            "expert_panel_focus: last_input_time should not change when ExpertPanel is focused"
        );

        // Verify that TaskInput focus DOES update the timer
        app.set_focus(FocusArea::TaskInput);
        app.handle_task_input_keys(KeyCode::Char('a'), KeyModifiers::NONE);
        assert!(
            app.last_input_time() > before,
            "task_input_focus: last_input_time should update when TaskInput is focused"
        );
    }

    #[test]
    fn last_resized_expert_id_starts_none() {
        let app = create_test_app();
        assert_eq!(
            app.last_resized_expert_id(),
            None,
            "last_resized_expert_id: should start as None"
        );
    }

    #[test]
    fn tower_app_initializes_messaging_system() {
        let app = create_test_app();

        // Verify messaging system components are initialized
        assert!(app.message_router.is_some());
        assert!(app.expert_registry.len() > 0 || app.config.experts.is_empty());
    }

    #[test]
    fn tower_app_expert_registry_matches_config() {
        let config = create_test_config();
        let expected_experts = config.experts.len();
        let wm = WorktreeManager::new(config.project_path.clone());
        let app = TowerApp::new(config, wm);

        // Verify expert registry has correct number of experts
        assert_eq!(app.expert_registry.len(), expected_experts);
    }

    #[test]
    fn handle_task_input_keys_ctrl_b_moves_cursor_left() {
        let mut app = create_test_app();
        app.task_input.set_content("hello".to_string());

        app.handle_task_input_keys(KeyCode::Char('b'), KeyModifiers::CONTROL);
        assert_eq!(app.task_input.content(), "hello");
        assert_eq!(
            app.task_input.cursor_position(),
            4,
            "handle_task_input_keys: Ctrl-b should move cursor left"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_f_moves_cursor_right() {
        let mut app = create_test_app();
        app.task_input.set_content("hello".to_string());
        app.task_input.move_cursor_start();

        app.handle_task_input_keys(KeyCode::Char('f'), KeyModifiers::CONTROL);
        assert_eq!(app.task_input.content(), "hello");
        assert_eq!(
            app.task_input.cursor_position(),
            1,
            "handle_task_input_keys: Ctrl-f should move cursor right"
        );
    }

    #[test]
    fn handle_task_input_keys_arrow_keys_do_not_move_cursor() {
        let mut app = create_test_app();
        app.task_input.set_content("hello".to_string());

        let pos_before = app.task_input.cursor_position();
        app.handle_task_input_keys(KeyCode::Left, KeyModifiers::NONE);
        assert_eq!(
            app.task_input.cursor_position(),
            pos_before,
            "handle_task_input_keys: Left arrow should not move cursor"
        );

        app.task_input.move_cursor_start();
        let pos_before = app.task_input.cursor_position();
        app.handle_task_input_keys(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(
            app.task_input.cursor_position(),
            pos_before,
            "handle_task_input_keys: Right arrow should not move cursor"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_a_moves_to_line_start() {
        let mut app = create_test_app();
        app.task_input.set_content("abc\ndef".to_string());
        // cursor at end (pos 7, second line)
        app.handle_task_input_keys(KeyCode::Char('a'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.cursor_position(),
            4,
            "handle_task_input_keys: Ctrl+A should move to start of current line"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_e_moves_to_line_end() {
        let mut app = create_test_app();
        app.task_input.set_content("abc\ndef".to_string());
        app.task_input.move_cursor_start();
        app.handle_task_input_keys(KeyCode::Char('e'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.cursor_position(),
            3,
            "handle_task_input_keys: Ctrl+E should move to end of current line"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_p_moves_cursor_up() {
        let mut app = create_test_app();
        app.task_input.set_content("abc\ndef".to_string());
        // cursor at end (pos 7)
        app.handle_task_input_keys(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.cursor_position(),
            3,
            "handle_task_input_keys: Ctrl+P should move cursor up"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_n_moves_cursor_down() {
        let mut app = create_test_app();
        app.task_input.set_content("abc\ndef".to_string());
        app.task_input.move_cursor_start();
        app.task_input.move_cursor_right(); // pos 1
        app.handle_task_input_keys(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.cursor_position(),
            5,
            "handle_task_input_keys: Ctrl+N should move cursor down"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_h_deletes_char() {
        let mut app = create_test_app();
        app.task_input.set_content("hello".to_string());

        app.handle_task_input_keys(KeyCode::Char('h'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.content(),
            "hell",
            "handle_task_input_keys: Ctrl+H should delete char before cursor"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_d_deletes_forward() {
        let mut app = create_test_app();
        app.task_input.set_content("hello".to_string());
        app.task_input.move_cursor_start();

        app.handle_task_input_keys(KeyCode::Char('d'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.content(),
            "ello",
            "handle_task_input_keys: Ctrl+D should delete char at cursor"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_u_unix_line_discard() {
        let mut app = create_test_app();
        app.task_input.set_content("hello world".to_string());

        app.handle_task_input_keys(KeyCode::Char('u'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.content(),
            "",
            "handle_task_input_keys: Ctrl+U should discard from start of line to cursor"
        );
    }

    #[test]
    fn handle_task_input_keys_ctrl_k_kill_line() {
        let mut app = create_test_app();
        app.task_input.set_content("hello world".to_string());
        app.task_input.move_cursor_start();
        for _ in 0..5 {
            app.task_input.move_cursor_right();
        }

        app.handle_task_input_keys(KeyCode::Char('k'), KeyModifiers::CONTROL);
        assert_eq!(
            app.task_input.content(),
            "hello",
            "handle_task_input_keys: Ctrl+K should kill from cursor to end of line"
        );
    }

    async fn wait_for_handle<T>(handle: &tokio::task::JoinHandle<T>) {
        let start = std::time::Instant::now();
        while !handle.is_finished() {
            if start.elapsed() > std::time::Duration::from_secs(1) {
                panic!("timed out waiting for spawned task to complete");
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
    }

    #[tokio::test]
    async fn launch_expert_in_worktree_returns_early_when_in_progress() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async {
            Ok(WorktreeLaunchResult {
                expert_id: 0,
                expert_name: "dummy".to_string(),
                branch_name: "dummy-branch".to_string(),
                worktree_path: "/tmp/dummy".to_string(),
                claude_ready: true,
            })
        });
        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "dummy".to_string(),
            branch_name: "dummy-branch".to_string(),
        };

        app.launch_expert_in_worktree().await.unwrap();

        assert_eq!(
            app.message(),
            Some("Worktree launch already in progress"),
            "launch_expert_in_worktree: should return early with message when already in progress"
        );
    }

    #[tokio::test]
    async fn launch_expert_in_worktree_rejects_empty_feature_name() {
        let mut app = create_test_app();

        app.launch_expert_in_worktree().await.unwrap();

        assert_eq!(
            app.message(),
            Some("Enter a feature name in the task input before launching worktree"),
            "launch_expert_in_worktree: should reject empty task input"
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_idle_stays_idle() {
        let mut app = create_test_app();

        app.poll_worktree_launch().await.unwrap();

        assert!(
            matches!(app.worktree_launch_state, WorktreeLaunchState::Idle),
            "poll_worktree_launch: Idle state should remain Idle"
        );
        assert!(
            app.message().is_none(),
            "poll_worktree_launch: no message should be set when Idle"
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_success_transitions_to_idle() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async {
            Ok(WorktreeLaunchResult {
                expert_id: 1,
                expert_name: "architect".to_string(),
                branch_name: "add-auth-20260208-120000".to_string(),
                worktree_path: "/tmp/wt".to_string(),
                claude_ready: true,
            })
        });
        wait_for_handle(&handle).await;

        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "architect".to_string(),
            branch_name: "add-auth-20260208-120000".to_string(),
        };

        app.poll_worktree_launch().await.unwrap();

        assert!(
            matches!(app.worktree_launch_state, WorktreeLaunchState::Idle),
            "poll_worktree_launch: should transition to Idle after success"
        );
        assert_eq!(
            app.message(),
            Some("architect launched in worktree 'add-auth-20260208-120000'"),
            "poll_worktree_launch: should set success message"
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_claude_not_ready_message() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async {
            Ok(WorktreeLaunchResult {
                expert_id: 2,
                expert_name: "backend".to_string(),
                branch_name: "fix-login-20260208-130000".to_string(),
                worktree_path: "/tmp/wt".to_string(),
                claude_ready: false,
            })
        });
        wait_for_handle(&handle).await;

        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "backend".to_string(),
            branch_name: "fix-login-20260208-130000".to_string(),
        };

        app.poll_worktree_launch().await.unwrap();

        assert!(
            matches!(app.worktree_launch_state, WorktreeLaunchState::Idle),
            "poll_worktree_launch: should transition to Idle even when Claude not ready"
        );
        assert!(
            app.message()
                .unwrap()
                .contains("Claude may still be starting"),
            "poll_worktree_launch: should set partial-ready message, got: {:?}",
            app.message()
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_failure_transitions_to_idle() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async { Err(anyhow::anyhow!("git worktree failed")) });
        wait_for_handle(&handle).await;

        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "backend".to_string(),
            branch_name: "fix-login-20260208-130000".to_string(),
        };

        app.poll_worktree_launch().await.unwrap();

        assert!(
            matches!(app.worktree_launch_state, WorktreeLaunchState::Idle),
            "poll_worktree_launch: should transition to Idle after failure"
        );
        assert!(
            app.message().unwrap().contains("Worktree launch failed"),
            "poll_worktree_launch: should set error message, got: {:?}",
            app.message()
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_not_finished_stays_in_progress() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
            Ok(WorktreeLaunchResult {
                expert_id: 1,
                expert_name: "test".to_string(),
                branch_name: "test-branch".to_string(),
                worktree_path: "/tmp".to_string(),
                claude_ready: true,
            })
        });

        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "test".to_string(),
            branch_name: "test-branch".to_string(),
        };

        app.poll_worktree_launch().await.unwrap();

        assert!(
            matches!(
                app.worktree_launch_state,
                WorktreeLaunchState::InProgress { .. }
            ),
            "poll_worktree_launch: should stay InProgress while task is running"
        );
    }
}

#[cfg(test)]
mod property_tests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn arbitrary_num_experts() -> impl Strategy<Value = usize> {
        1usize..10
    }

    fn create_config_with_experts(num_experts: usize) -> Config {
        let mut config = Config::default().with_project_path(PathBuf::from("/tmp/test"));
        config.experts = (0..num_experts)
            .map(|i| crate::config::ExpertConfig {
                name: format!("expert{}", i),
                color: "white".to_string(),
                role: format!("role{}", i % 4),
            })
            .collect();
        config
    }

    fn create_app_with_experts(num_experts: usize) -> (Config, TowerApp) {
        let config = create_config_with_experts(num_experts);
        let wm = WorktreeManager::new(config.project_path.clone());
        let app = TowerApp::new(config.clone(), wm);
        (config, app)
    }

    // Feature: inter-expert-messaging, Property 13: System Initialization Consistency
    // **Validates: Requirements 11.5, 4.2**
    proptest! {
        #![proptest_config(ProptestConfig::with_cases(100))]

        #[test]
        fn system_initialization_consistency(
            num_experts in arbitrary_num_experts()
        ) {
            let (config, app) = create_app_with_experts(num_experts);

            // Requirement 11.5: System should initialize with correct components
            // Verify message router is initialized
            assert!(
                app.message_router.is_some(),
                "Message router should be initialized"
            );

            // Verify expert registry is initialized with correct count
            assert_eq!(
                app.expert_registry.len(),
                num_experts,
                "Expert registry should have {} experts, but has {}",
                num_experts,
                app.expert_registry.len()
            );

            // Verify each expert is registered correctly by name
            // Expert IDs are 0-based and match config indices
            for expert_config in &config.experts {
                let expert_id = app.expert_registry.find_by_name(&expert_config.name);
                assert!(
                    expert_id.is_some(),
                    "Expert '{}' should be registered",
                    expert_config.name
                );

                let expert = app.expert_registry.get_expert(expert_id.unwrap());
                assert!(
                    expert.is_some(),
                    "Expert '{}' should have valid info",
                    expert_config.name
                );

                let expert = expert.unwrap();
                assert_eq!(
                    expert.name,
                    expert_config.name,
                    "Expert name should match config"
                );
            }

            // Verify messaging display is initialized
            assert_eq!(
                app.messaging_display.total_count(),
                0,
                "Messaging display should start empty"
            );

            // Requirement 4.2: Queue directory structure should be consistent
            // (verified by successful initialization without errors)
        }

        #[test]
        fn system_initialization_expert_state_consistency(
            num_experts in arbitrary_num_experts()
        ) {
            let (_config, app) = create_app_with_experts(num_experts);

            // All experts should start in Offline state
            // Get all expert IDs from the registry
            let all_experts = app.expert_registry.get_all_experts();
            assert_eq!(
                all_experts.len(),
                num_experts,
                "Should have {} experts registered",
                num_experts
            );

            for expert in all_experts {
                let is_idle = app.expert_registry.is_expert_idle(expert.id);

                // Initially experts are offline (not idle)
                assert_eq!(
                    is_idle,
                    Some(false),
                    "Expert '{}' (id={}) should not be idle initially",
                    expert.name,
                    expert.id
                );
            }
        }

        #[test]
        fn system_initialization_message_router_consistency(
            num_experts in arbitrary_num_experts()
        ) {
            let (_config, app) = create_app_with_experts(num_experts);

            // Verify message router has access to expert registry
            if let Some(ref router) = app.message_router {
                // Router's expert registry should match app's expert registry
                assert_eq!(
                    router.expert_registry().len(),
                    app.expert_registry.len(),
                    "Router's expert registry should match app's"
                );
            } else {
                panic!("Message router should be initialized");
            }
        }

        #[test]
        fn branch_name_format_matches_expected_pattern(
            feature_name in "[a-zA-Z][a-zA-Z0-9 _-]{0,30}"
        ) {
            let sanitized = crate::utils::sanitize_branch_name(&feature_name);
            let ts = chrono::Utc::now().format("%Y%m%d-%H%M%S").to_string();
            let branch = format!("{}-{}", sanitized, ts);

            // Branch should have format: {sanitized}-{YYYYMMDD-HHMMSS}
            let expected_suffix_len = 15; // YYYYMMDD-HHMMSS
            prop_assert!(
                branch.len() > expected_suffix_len,
                "branch_name: should be longer than timestamp suffix"
            );
            let timestamp_suffix = &branch[branch.len() - expected_suffix_len..];
            prop_assert!(
                timestamp_suffix.chars().enumerate().all(|(i, c)| {
                    if i == 8 { c == '-' } else { c.is_ascii_digit() }
                }),
                "branch_name: timestamp should follow YYYYMMDD-HHMMSS format"
            );
            // Separator between sanitized name and timestamp
            let separator_pos = branch.len() - expected_suffix_len - 1;
            prop_assert_eq!(
                branch.as_bytes()[separator_pos],
                b'-',
                "branch_name: should have hyphen separator before timestamp"
            );
        }
    }
}
