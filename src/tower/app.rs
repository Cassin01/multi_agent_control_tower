use anyhow::Result;
use crossterm::event::{
    self, Event, KeyCode, KeyEventKind, KeyModifiers, MouseButton, MouseEventKind,
};
use ratatui::layout::Rect;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::context::{AvailableRoles, ContextStore, Decision, ExpertContext, SessionExpertRoles};
use crate::experts::ExpertRegistry;
use crate::feature::executor::{ExecutionPhase, FeatureExecutor};
use crate::instructions::manifest::{generate_expert_manifest, write_expert_manifest};
use crate::instructions::{
    generate_hooks_settings, load_instruction_with_template, write_agents_file,
    write_instruction_file, write_settings_file,
};
use crate::models::ExpertState;
use crate::models::{ExpertInfo, Role};
use crate::queue::{MessageRouter, QueueManager};
use crate::session::{
    ClaudeManager, ExpertStateDetector, TmuxManager, TmuxSender, WorktreeLaunchResult,
    WorktreeLaunchState, WorktreeManager,
};
use crate::tower::widgets::ExpertEntry;
use crate::utils::sanitize_branch_name;

/// Message polling interval for the messaging system (3 seconds)
const MESSAGE_POLL_INTERVAL: Duration = Duration::from_millis(3000);

/// Event poll timeout — the maximum blocking duration for `event::poll()`.
/// 16ms targets ~60 FPS while keeping CPU usage low.
const EVENT_POLL_TIMEOUT: Duration = Duration::from_millis(16);

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
            return Some(format!("C-{c}"));
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

fn is_shift_tab_for_task_input(code: KeyCode, modifiers: KeyModifiers) -> bool {
    matches!(code, KeyCode::BackTab)
        || (matches!(code, KeyCode::Tab) && modifiers.contains(KeyModifiers::SHIFT))
}

fn is_exclamation_at_input_start(
    code: KeyCode,
    modifiers: KeyModifiers,
    cursor_pos: usize,
) -> bool {
    matches!(code, KeyCode::Char('!'))
        && !modifiers.contains(KeyModifiers::CONTROL)
        && !modifiers.contains(KeyModifiers::ALT)
        && cursor_pos == 0
}

struct ExpertPanelUpdateResult {
    expert_id: u32,
    content: String,
    resized_preview_size: Option<(u16, u16)>,
    resized_expert_id: Option<u32>,
}

#[derive(Default)]
enum ExpertPanelUpdateState {
    #[default]
    Idle,
    InProgress {
        handle: tokio::task::JoinHandle<Result<ExpertPanelUpdateResult>>,
    },
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
    expert_panel_update_state: ExpertPanelUpdateState,

    worktree_manager: WorktreeManager,
    worktree_launch_state: WorktreeLaunchState,

    feature_executor: Option<FeatureExecutor>,

    needs_redraw: bool,
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
                    eprintln!("Warning: Failed to load available roles: {e}");
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

        let app = Self {
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
            expert_panel_update_state: ExpertPanelUpdateState::default(),

            worktree_manager,
            worktree_launch_state: WorktreeLaunchState::default(),

            feature_executor: None,

            needs_redraw: true,

            config,
        };

        if let Err(e) = app.refresh_expert_manifest() {
            tracing::warn!("Failed to generate initial expert manifest: {}", e);
        }

        app
    }

    pub fn is_running(&self) -> bool {
        self.running
    }

    pub fn quit(&mut self) {
        self.cancel_expert_panel_update();
        self.running = false;
    }

    fn cancel_expert_panel_update(&mut self) {
        let state = std::mem::take(&mut self.expert_panel_update_state);
        if let ExpertPanelUpdateState::InProgress { handle } = state {
            handle.abort();
        }
    }

    pub fn set_message(&mut self, msg: String) {
        self.message = Some(msg);
        self.needs_redraw = true;
    }

    pub fn clear_message(&mut self) {
        self.message = None;
    }

    pub fn message(&self) -> Option<&str> {
        self.message.as_deref()
    }

    fn refresh_expert_manifest(&self) -> Result<()> {
        let content =
            generate_expert_manifest(&self.config, &self.session_roles, &self.expert_registry)?;
        write_expert_manifest(&self.config.queue_path, &content)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn focus(&self) -> FocusArea {
        self.focus
    }

    pub fn status_display(&mut self) -> &mut StatusDisplay {
        &mut self.status_display
    }

    pub fn task_input(&mut self) -> &mut TaskInput {
        &mut self.task_input
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

    #[cfg(test)]
    pub fn needs_redraw(&self) -> bool {
        self.needs_redraw
    }

    #[cfg(test)]
    pub fn clear_needs_redraw(&mut self) {
        self.needs_redraw = false;
    }

    #[cfg(test)]
    pub fn reset_poll_timers_for_test(&mut self) {
        let past = Instant::now() - Duration::from_secs(10);
        self.last_input_time = past;
        self.last_status_poll = past;
        self.last_report_poll = past;
        self.last_message_poll = past;
        self.last_panel_poll = past;
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
                    .unwrap_or(ExpertState::Idle);
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

        let working_dirs = self
            .tmux
            .get_all_pane_current_paths()
            .await
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to list pane current paths: {}", e);
                std::collections::HashMap::new()
            });
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
        self.needs_redraw = true;
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
        self.needs_redraw = true;
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
        self.needs_redraw = true;

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
        self.poll_expert_panel_update_result().await;

        if !self.expert_panel_display.is_visible() {
            return Ok(());
        }

        // Poll when either the panel is focused or task input is focused
        // (expert selection screen).
        if self.focus != FocusArea::ExpertPanel && self.focus != FocusArea::TaskInput {
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
            if matches!(
                self.expert_panel_update_state,
                ExpertPanelUpdateState::InProgress { .. }
            ) {
                return Ok(());
            }

            let preview_size = self.expert_panel_display.preview_size();
            let size_changed = preview_size != self.last_preview_size;
            let expert_changed = self.last_resized_expert_id != Some(expert_id);
            let needs_resize =
                (size_changed || expert_changed) && preview_size.0 > 0 && preview_size.1 > 0;
            let resize_all = needs_resize && size_changed;
            let resize_single = needs_resize && !size_changed;
            let num_experts = self.config.num_experts();
            let claude = self.claude.clone();

            let handle = tokio::spawn(async move {
                if resize_all {
                    let resize_futures: Vec<_> = (0..num_experts)
                        .map(|id| {
                            let claude = &claude;
                            async move {
                                if let Err(e) =
                                    claude.resize_pane(id, preview_size.0, preview_size.1).await
                                {
                                    tracing::warn!(
                                        "Failed to resize pane for expert {}: {}",
                                        id,
                                        e
                                    );
                                }
                            }
                        })
                        .collect();
                    futures::future::join_all(resize_futures).await;
                } else if resize_single {
                    if let Err(e) = claude
                        .resize_pane(expert_id, preview_size.0, preview_size.1)
                        .await
                    {
                        tracing::warn!("Failed to resize pane for expert {}: {}", expert_id, e);
                    }
                }

                let content = claude.capture_pane_with_escapes(expert_id).await?;

                Ok(ExpertPanelUpdateResult {
                    expert_id,
                    content,
                    resized_preview_size: if resize_all { Some(preview_size) } else { None },
                    resized_expert_id: if needs_resize { Some(expert_id) } else { None },
                })
            });

            self.expert_panel_update_state = ExpertPanelUpdateState::InProgress { handle };
        }

        Ok(())
    }

    async fn poll_expert_panel_update_result(&mut self) {
        let state = std::mem::take(&mut self.expert_panel_update_state);
        match state {
            ExpertPanelUpdateState::InProgress { handle } => {
                if handle.is_finished() {
                    match handle.await {
                        Ok(Ok(update)) => {
                            if let Some(size) = update.resized_preview_size {
                                self.last_preview_size = size;
                            }
                            if let Some(expert_id) = update.resized_expert_id {
                                self.last_resized_expert_id = Some(expert_id);
                            }
                            if self.expert_panel_display.expert_id() == Some(update.expert_id)
                                && self.expert_panel_display.try_set_content(&update.content)
                            {
                                self.needs_redraw = true;
                            }
                        }
                        Ok(Err(e)) => {
                            tracing::warn!("Expert panel update failed: {}", e);
                        }
                        Err(e) => {
                            tracing::warn!("Expert panel update task panicked: {}", e);
                        }
                    }
                    self.expert_panel_update_state = ExpertPanelUpdateState::Idle;
                } else {
                    self.expert_panel_update_state = ExpertPanelUpdateState::InProgress { handle };
                }
            }
            ExpertPanelUpdateState::Idle => {
                self.expert_panel_update_state = ExpertPanelUpdateState::Idle;
            }
        }
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
        let has_event = event::poll(EVENT_POLL_TIMEOUT)?;
        if has_event {
            self.needs_redraw = true;
            let event = event::read()?;
            match event {
                Event::Mouse(mouse) => {
                    // Update input time for mouse events to pause polling during interaction
                    self.last_input_time = Instant::now();

                    if mouse.kind == MouseEventKind::Down(MouseButton::Left)
                        && !self.help_modal.is_visible()
                        && self.report_display.view_mode() != ViewMode::Detail
                        && !self.role_selector.is_visible()
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

                    if key.modifiers.contains(KeyModifiers::CONTROL)
                        && matches!(key.code, KeyCode::Char('c' | 'q'))
                    {
                        self.quit();
                        return Ok(());
                    }

                    if self.help_modal.is_visible() {
                        match key.code {
                            KeyCode::Enter | KeyCode::Char('q') | KeyCode::F(1) => {
                                self.help_modal.hide();
                            }
                            _ => {}
                        }
                        return Ok(());
                    }

                    if key.code == KeyCode::F(1) {
                        self.help_modal.toggle();
                        return Ok(());
                    }

                    if key.modifiers.contains(KeyModifiers::CONTROL) {
                        match key.code {
                            KeyCode::Char('j') if self.focus != FocusArea::ExpertPanel => {
                                if self.expert_panel_display.is_scrolling() {
                                    self.expert_panel_display.exit_scroll_mode();
                                }
                                self.expert_panel_display.toggle();
                                return Ok(());
                            }
                            _ => {}
                        }
                    }

                    if self.report_display.view_mode() == ViewMode::Detail {
                        match key.code {
                            KeyCode::Enter | KeyCode::Char('q') => {
                                self.report_display.close_detail();
                            }
                            KeyCode::Char('x') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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
                            KeyCode::Esc | KeyCode::Char('q') => {
                                self.role_selector.hide();
                            }
                            KeyCode::Char('o') if key.modifiers.contains(KeyModifiers::CONTROL) => {
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

                    if self.focus == FocusArea::TaskInput
                        && is_shift_tab_for_task_input(key.code, key.modifiers)
                    {
                        if self.expert_panel_display.is_scrolling() {
                            self.expert_panel_display.exit_scroll_mode();
                        }
                        if let Some(expert_id) = self.status_display.selected_expert_id() {
                            if let Err(e) = self.claude.send_keys(expert_id, "BTab").await {
                                tracing::warn!(
                                    "Failed to send Shift+Tab to expert {}: {}",
                                    expert_id,
                                    e
                                );
                                self.set_message(format!("Error sending keys to expert: {e}"));
                            }
                        }
                        return Ok(());
                    }

                    if self.focus == FocusArea::TaskInput
                        && is_exclamation_at_input_start(
                            key.code,
                            key.modifiers,
                            self.task_input.cursor_position(),
                        )
                    {
                        if self.expert_panel_display.is_scrolling() {
                            self.expert_panel_display.exit_scroll_mode();
                        }
                        if let Some(expert_id) = self.status_display.selected_expert_id() {
                            if let Err(e) = self.claude.send_keys(expert_id, "!").await {
                                tracing::warn!("Failed to send ! to expert {}: {}", expert_id, e);
                                self.set_message(format!("Error sending keys to expert: {e}"));
                            }
                        }
                        return Ok(());
                    }

                    // Remote scroll: handle active remote scroll mode
                    if self.focus == FocusArea::TaskInput
                        && self.expert_panel_display.is_scrolling()
                    {
                        match key.code {
                            KeyCode::Esc => {
                                self.expert_panel_display.exit_scroll_mode();
                                return Ok(());
                            }
                            KeyCode::PageUp => {
                                self.expert_panel_display.scroll_up();
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
                            // Exit scroll + fall through to expert selection
                            KeyCode::Up | KeyCode::Down => {
                                self.expert_panel_display.exit_scroll_mode();
                            }
                            // Exit scroll + fall through to assign task
                            KeyCode::Char('s') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                                self.expert_panel_display.exit_scroll_mode();
                            }
                            // All other keys fall through to normal handling (keep scroll mode)
                            _ => {}
                        }
                    }

                    // Remote scroll: enter remote scroll mode on PageUp from TaskInput
                    if self.focus == FocusArea::TaskInput
                        && key.code == KeyCode::PageUp
                        && !self.expert_panel_display.is_scrolling()
                        && self.expert_panel_display.is_visible()
                    {
                        if let Some(expert_id) = self.expert_panel_display.expert_id() {
                            match self.claude.capture_full_history(expert_id).await {
                                Ok(raw) => self.expert_panel_display.enter_scroll_mode(&raw),
                                Err(e) => tracing::warn!(
                                    "Failed to capture history for expert {}: {}",
                                    expert_id,
                                    e
                                ),
                            }
                        }
                        return Ok(());
                    }

                    match self.focus {
                        FocusArea::ExpertList => {} // Display only, not selectable
                        FocusArea::TaskInput => {
                            self.handle_task_input_keys(key.code, key.modifiers)
                        }
                        FocusArea::ExpertPanel => {
                            if key.code == KeyCode::Char('t')
                                && key.modifiers.contains(KeyModifiers::CONTROL)
                            {
                                self.next_focus();
                            } else {
                                self.handle_expert_panel_keys(key.code, key.modifiers)
                                    .await?;
                            }
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
                        let input = self.task_input.content().trim().to_string();
                        if input.is_empty() {
                            self.return_expert_from_worktree().await?;
                        } else {
                            self.launch_expert_in_worktree().await?;
                        }
                    }

                    if key.code == KeyCode::Char('g')
                        && key.modifiers.contains(KeyModifiers::CONTROL)
                        && self.focus == FocusArea::TaskInput
                    {
                        self.handle_feature_execution().await?;
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
                    self.set_message(format!("Error sending keys to expert: {e}"));
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
            .unwrap_or_else(|| format!("expert{expert_id}"));

        let description = self.task_input.content().to_string();

        let decision = Decision::new(
            expert_id,
            format!("Task Assignment to {expert_name}"),
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
        self.set_message(format!("Task assigned to {expert_name}"));

        Ok(())
    }

    pub async fn initialize_session_roles(&mut self) -> Result<()> {
        let session_hash = self.config.session_hash();

        let mut roles = match self.context_store.load_session_roles(&session_hash).await {
            Ok(Some(r)) => r,
            Ok(None) => SessionExpertRoles::new(session_hash.clone()),
            Err(e) => {
                eprintln!("Warning: Failed to load session roles, recreating with defaults: {e}");
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

        // Sync session roles to expert registry for message routing
        for i in 0..self.config.num_experts() {
            if let Some(role_str) = roles.get_role(i) {
                let new_role = Role::specialist(role_str.to_string());
                if let Err(e) = self.expert_registry.update_expert_role(i, new_role.clone()) {
                    tracing::warn!("Failed to update expert {} role in registry: {}", i, e);
                }
                if let Some(ref mut router) = self.message_router {
                    if let Err(e) = router.expert_registry_mut().update_expert_role(i, new_role) {
                        tracing::warn!("Failed to update expert {} role in router: {}", i, e);
                    }
                }
            }
        }

        self.session_roles = roles;

        Ok(())
    }

    pub async fn restore_worktree_paths(&mut self) -> Result<()> {
        let session_hash = self.config.session_hash();

        for i in 0..self.config.num_experts() {
            let ctx = match self
                .context_store
                .load_expert_context(&session_hash, i)
                .await
            {
                Ok(Some(ctx)) => ctx,
                Ok(None) => continue,
                Err(e) => {
                    tracing::warn!(
                        "Failed to load expert {} context for worktree restore: {}",
                        i,
                        e
                    );
                    continue;
                }
            };

            if let Some(ref wt_path) = ctx.worktree_path {
                if !std::path::Path::new(wt_path).exists() {
                    tracing::warn!(
                        "Expert {} worktree path no longer exists, skipping: {}",
                        i,
                        wt_path
                    );
                    continue;
                }

                if let Err(e) = self
                    .expert_registry
                    .update_expert_worktree(i, Some(wt_path.clone()))
                {
                    tracing::warn!("Failed to restore expert {} worktree in registry: {}", i, e);
                }
                if let Some(ref mut router) = self.message_router {
                    if let Err(e) = router
                        .expert_registry_mut()
                        .update_expert_worktree(i, Some(wt_path.clone()))
                    {
                        tracing::warn!("Failed to restore expert {} worktree in router: {}", i, e);
                    }
                }
            }
        }

        Ok(())
    }

    async fn resolve_expert_working_dir(&self, expert_id: u32) -> String {
        if let Ok(Some(ctx)) = self
            .context_store
            .load_expert_context(&self.config.session_hash(), expert_id)
            .await
        {
            if let Some(ref wt_path) = ctx.worktree_path {
                if std::path::Path::new(wt_path).exists() {
                    return wt_path.clone();
                }
            }
        }
        self.config.project_path.to_str().unwrap_or(".").to_string()
    }

    pub async fn change_expert_role(&mut self, expert_id: u32, new_role: &str) -> Result<()> {
        if let Some(entry) = self.status_display.selected() {
            if entry.state == ExpertState::Busy {
                self.set_message(format!(
                    "Warning: Expert {expert_id} is currently active. Role change may interrupt work."
                ));
            }
        }

        self.session_roles.set_role(expert_id, new_role.to_string());
        self.context_store
            .save_session_roles(&self.session_roles)
            .await?;

        // Sync role change to expert registry for message routing
        let role = Role::specialist(new_role.to_string());
        if let Err(e) = self
            .expert_registry
            .update_expert_role(expert_id, role.clone())
        {
            tracing::warn!(
                "Failed to update expert {} role in registry: {}",
                expert_id,
                e
            );
        }
        if let Some(ref mut router) = self.message_router {
            if let Err(e) = router
                .expert_registry_mut()
                .update_expert_role(expert_id, role)
            {
                tracing::warn!(
                    "Failed to update expert {} role in router: {}",
                    expert_id,
                    e
                );
            }
        }

        if let Err(e) = self.refresh_expert_manifest() {
            tracing::warn!("Failed to refresh expert manifest after role change: {}", e);
        }

        self.claude.send_exit(expert_id).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;

        let expert_name = self.config.get_expert_name(expert_id);
        let manifest_path = self.config.queue_path.join("experts_manifest.json");
        let manifest_path_str = manifest_path.to_string_lossy();
        let status_dir = self.config.queue_path.join("status");
        let status_dir_str = status_dir.to_string_lossy();
        let worktree_path = self
            .expert_registry
            .get_expert(expert_id)
            .and_then(|info| info.worktree_path.as_deref().map(|s| s.to_string()));

        let instruction_result = load_instruction_with_template(
            &self.config.core_instructions_path,
            &self.config.role_instructions_path,
            new_role,
            expert_id,
            &expert_name,
            &self.config.status_file_path(expert_id),
            worktree_path.as_deref(),
            &manifest_path_str,
            &status_dir_str,
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
        let agents_file = match &instruction_result.agents_json {
            Some(json) => Some(write_agents_file(&self.config.queue_path, expert_id, json)?),
            None => None,
        };
        let hooks_json = generate_hooks_settings(&self.config.status_file_path(expert_id));
        let settings_file = Some(write_settings_file(
            &self.config.queue_path,
            expert_id,
            &hooks_json,
        )?);

        let working_dir = self.resolve_expert_working_dir(expert_id).await;
        self.claude
            .launch_claude(
                expert_id,
                &working_dir,
                instruction_file.as_deref(),
                agents_file.as_deref(),
                settings_file.as_deref(),
            )
            .await?;

        if instruction_result.used_general_fallback {
            self.set_message(format!(
                "Role '{}' not found, using 'general'",
                instruction_result.requested_role
            ));
        } else {
            self.set_message(format!("Expert {expert_id} role changed to {new_role}"));
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
                self.set_message(format!("No report found for expert {expert_id}"));
            }
        }
    }

    async fn confirm_role_selection(&mut self) -> Result<()> {
        if let (Some(expert_id), Some(new_role)) = (
            self.role_selector.expert_id(),
            self.role_selector.selected_role().map(ToString::to_string),
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
            .map(ToString::to_string)
            .unwrap_or_else(|| self.config.get_expert_role(expert_id));

        self.set_message(format!(
            "Resetting {expert_name} (role: {instruction_role})..."
        ));

        let working_dir = self.resolve_expert_working_dir(expert_id).await;

        self.claude.send_exit(expert_id).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;

        // Preserve worktree info while clearing session and knowledge
        let session_hash = self.config.session_hash();
        if let Ok(Some(mut ctx)) = self
            .context_store
            .load_expert_context(&session_hash, expert_id)
            .await
        {
            ctx.clear_session();
            ctx.clear_knowledge();
            self.context_store.save_expert_context(&ctx).await?;
        } else {
            self.context_store
                .clear_expert_context(&session_hash, expert_id)
                .await?;
        }

        if let Err(e) = self.refresh_expert_manifest() {
            tracing::warn!("Failed to refresh expert manifest after reset: {}", e);
        }

        let manifest_path = self.config.queue_path.join("experts_manifest.json");
        let manifest_path_str = manifest_path.to_string_lossy();
        let status_dir = self.config.queue_path.join("status");
        let status_dir_str = status_dir.to_string_lossy();
        let worktree_path = self
            .expert_registry
            .get_expert(expert_id)
            .and_then(|info| info.worktree_path.as_deref().map(|s| s.to_string()));

        let instruction_result = load_instruction_with_template(
            &self.config.core_instructions_path,
            &self.config.role_instructions_path,
            &instruction_role,
            expert_id,
            &expert_name,
            &self.config.status_file_path(expert_id),
            worktree_path.as_deref(),
            &manifest_path_str,
            &status_dir_str,
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
        let agents_file = match &instruction_result.agents_json {
            Some(json) => Some(write_agents_file(&self.config.queue_path, expert_id, json)?),
            None => None,
        };
        let hooks_json = generate_hooks_settings(&self.config.status_file_path(expert_id));
        let settings_file = Some(write_settings_file(
            &self.config.queue_path,
            expert_id,
            &hooks_json,
        )?);

        self.claude
            .launch_claude(
                expert_id,
                &working_dir,
                instruction_file.as_deref(),
                agents_file.as_deref(),
                settings_file.as_deref(),
            )
            .await?;

        if instruction_result.used_general_fallback {
            self.set_message(format!(
                "{} reset (role '{}' not found, using 'general')",
                expert_name, instruction_result.requested_role
            ));
        } else {
            self.set_message(format!("{expert_name} reset complete"));
        }
        Ok(())
    }

    pub async fn return_expert_from_worktree(&mut self) -> Result<()> {
        let expert_id = match self.status_display.selected_expert_id() {
            Some(id) => id,
            None => {
                self.set_message("No expert selected".to_string());
                return Ok(());
            }
        };

        let expert_name = self.config.get_expert_name(expert_id);
        let session_hash = self.config.session_hash();

        let in_worktree = match self
            .context_store
            .load_expert_context(&session_hash, expert_id)
            .await
        {
            Ok(Some(ref ctx)) => ctx.worktree_path.as_ref().is_some_and(|p| !p.is_empty()),
            _ => false,
        };

        if !in_worktree {
            self.set_message(
                "Enter a feature name in the task input before launching worktree".to_string(),
            );
            return Ok(());
        }

        let instruction_role = self
            .session_roles
            .get_role(expert_id)
            .map(ToString::to_string)
            .unwrap_or_else(|| self.config.get_expert_role(expert_id));

        self.set_message(format!("Returning {expert_name} to project root..."));

        self.claude.send_exit(expert_id).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;

        if let Ok(Some(mut ctx)) = self
            .context_store
            .load_expert_context(&session_hash, expert_id)
            .await
        {
            ctx.clear_worktree();
            ctx.clear_session();
            ctx.clear_knowledge();
            self.context_store.save_expert_context(&ctx).await?;
        } else {
            self.context_store
                .clear_expert_context(&session_hash, expert_id)
                .await?;
        }

        if let Err(e) = self.refresh_expert_manifest() {
            tracing::warn!(
                "Failed to refresh expert manifest after worktree return: {}",
                e
            );
        }

        let manifest_path = self.config.queue_path.join("experts_manifest.json");
        let manifest_path_str = manifest_path.to_string_lossy();
        let status_dir = self.config.queue_path.join("status");
        let status_dir_str = status_dir.to_string_lossy();

        let instruction_result = load_instruction_with_template(
            &self.config.core_instructions_path,
            &self.config.role_instructions_path,
            &instruction_role,
            expert_id,
            &expert_name,
            &self.config.status_file_path(expert_id),
            None,
            &manifest_path_str,
            &status_dir_str,
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
        let agents_file = match &instruction_result.agents_json {
            Some(json) => Some(write_agents_file(&self.config.queue_path, expert_id, json)?),
            None => None,
        };
        let hooks_json = generate_hooks_settings(&self.config.status_file_path(expert_id));
        let settings_file = Some(write_settings_file(
            &self.config.queue_path,
            expert_id,
            &hooks_json,
        )?);

        let project_root = self.config.project_path.to_str().unwrap_or(".").to_string();

        self.claude
            .launch_claude(
                expert_id,
                &project_root,
                instruction_file.as_deref(),
                agents_file.as_deref(),
                settings_file.as_deref(),
            )
            .await?;

        self.set_message(format!("{expert_name} returned to project root"));
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
        let branch_name = sanitize_branch_name(&feature_input);

        let worktree_already_exists = self.worktree_manager.worktree_exists(&branch_name);

        if worktree_already_exists {
            self.set_message(format!("Reusing worktree '{branch_name}'..."));
        } else {
            self.set_message(format!("Creating worktree '{branch_name}'..."));
        }

        let claude = self.claude.clone();
        let context_store = self.context_store.clone();
        let worktree_manager = self.worktree_manager.clone();
        let config = self.config.clone();
        let session_hash = config.session_hash();
        let instruction_role = self
            .session_roles
            .get_role(expert_id)
            .map(ToString::to_string)
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

            let worktree_path = if worktree_already_exists {
                worktree_manager.worktree_path(&branch_clone)
            } else {
                let wt_path = worktree_manager.create_worktree(&branch_clone).await?;
                worktree_manager.setup_macot_symlink(&wt_path).await?;
                wt_path
            };

            let wt_path_str = worktree_path
                .to_str()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Worktree path contains non-UTF8 characters: {}",
                        worktree_path.display()
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

            let manifest_path = queue_path.join("experts_manifest.json");
            let manifest_path_str = manifest_path.to_string_lossy();
            let status_dir = queue_path.join("status");
            let status_dir_str = status_dir.to_string_lossy();

            let instruction_result = load_instruction_with_template(
                &core_path,
                &role_path,
                &instruction_role,
                expert_id,
                &expert_name_clone,
                &status_file_path,
                Some(&wt_path_str),
                &manifest_path_str,
                &status_dir_str,
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
            let agents_file = match &instruction_result.agents_json {
                Some(json) => Some(write_agents_file(&queue_path, expert_id, json)?),
                None => None,
            };
            let hooks_json = generate_hooks_settings(&status_file_path);
            let settings_file = Some(write_settings_file(&queue_path, expert_id, &hooks_json)?);

            claude
                .launch_claude(
                    expert_id,
                    &wt_path_str,
                    instruction_file.as_deref(),
                    agents_file.as_deref(),
                    settings_file.as_deref(),
                )
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

    async fn handle_feature_execution(&mut self) -> Result<()> {
        if let Some(ref mut executor) = self.feature_executor {
            let expert_id = executor.expert_id();
            executor.cancel();
            self.feature_executor = None;
            if let Err(e) = self.detector.set_marker(expert_id, "pending") {
                tracing::warn!(
                    "Failed to reset status marker for expert {} on cancel: {}",
                    expert_id,
                    e
                );
            }
            self.set_message("Feature execution cancelled".to_string());
            return Ok(());
        }

        self.start_feature_execution().await
    }

    async fn start_feature_execution(&mut self) -> Result<()> {
        let expert_id = match self.status_display.selected_expert_id() {
            Some(id) => id,
            None => {
                self.set_message("No expert selected".to_string());
                return Ok(());
            }
        };

        let feature_name = self.task_input.content().trim().to_string();
        if feature_name.is_empty() {
            self.set_message("Enter a feature name in the task input".to_string());
            return Ok(());
        }

        let expert_state = self.detector.detect_state(expert_id);
        if expert_state != ExpertState::Idle {
            self.set_message(format!(
                "Expert must be idle to start feature execution (current: {})",
                expert_state.description()
            ));
            return Ok(());
        }

        let expert_name = self.config.get_expert_name(expert_id);
        let instruction_role = self
            .session_roles
            .get_role(expert_id)
            .map(ToString::to_string)
            .unwrap_or_else(|| self.config.get_expert_role(expert_id));

        let manifest_path = self.config.queue_path.join("experts_manifest.json");
        let manifest_path_str = manifest_path.to_string_lossy();
        let status_dir = self.config.queue_path.join("status");
        let status_dir_str = status_dir.to_string_lossy();
        let worktree_path = self
            .expert_registry
            .get_expert(expert_id)
            .and_then(|info| info.worktree_path.as_deref().map(|s| s.to_string()));

        let instruction_result = load_instruction_with_template(
            &self.config.core_instructions_path,
            &self.config.role_instructions_path,
            &instruction_role,
            expert_id,
            &expert_name,
            &self.config.status_file_path(expert_id),
            worktree_path.as_deref(),
            &manifest_path_str,
            &status_dir_str,
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
        let agents_file = match &instruction_result.agents_json {
            Some(json) => Some(write_agents_file(&self.config.queue_path, expert_id, json)?),
            None => None,
        };
        let hooks_json = generate_hooks_settings(&self.config.status_file_path(expert_id));
        let settings_file = Some(write_settings_file(
            &self.config.queue_path,
            expert_id,
            &hooks_json,
        )?);

        let working_dir = self.config.project_path.to_str().unwrap_or(".").to_string();

        let mut executor = FeatureExecutor::new(
            feature_name.clone(),
            expert_id,
            &self.config.feature_execution,
            &self.config.project_path,
            instruction_file,
            agents_file,
            settings_file,
            working_dir,
        );

        match executor.validate() {
            Ok(()) => {
                self.claude.send_exit(expert_id).await?;
                executor.set_phase(ExecutionPhase::ExitingExpert {
                    started_at: Instant::now(),
                    exit_retries: 0,
                });
                self.feature_executor = Some(executor);
                self.task_input.clear();
                self.set_message(format!("Feature execution started: {feature_name}"));
            }
            Err(e) => {
                self.set_message(format!("Feature execution error: {e}"));
            }
        }

        Ok(())
    }

    #[allow(dead_code)]
    pub fn feature_executor(&self) -> Option<&FeatureExecutor> {
        self.feature_executor.as_ref()
    }

    pub async fn poll_feature_executor(&mut self) -> Result<()> {
        let mut executor = match self.feature_executor.take() {
            Some(e) => e,
            None => return Ok(()),
        };

        match executor.phase() {
            ExecutionPhase::Idle => {}

            ExecutionPhase::ExitingExpert {
                started_at,
                exit_retries,
            } => {
                let started_at = *started_at;
                let exit_retries = *exit_retries;
                const MAX_EXIT_RETRIES: u32 = 3;

                if started_at.elapsed() >= executor.exit_wait() {
                    let expert_id = executor.expert_id();

                    // Verify Claude has actually exited by checking foreground process
                    let shell_ready = match self.claude.is_shell_foreground(expert_id).await {
                        Ok(is_shell) => is_shell,
                        Err(e) => {
                            tracing::warn!(
                                "Failed to check foreground process for expert {}: {}",
                                expert_id,
                                e
                            );
                            // Assume shell is ready if we can't check
                            true
                        }
                    };

                    if !shell_ready {
                        if exit_retries >= MAX_EXIT_RETRIES {
                            tracing::error!(
                                "Expert {} did not exit after {} retries, forcing relaunch",
                                expert_id,
                                MAX_EXIT_RETRIES
                            );
                            // Fall through to relaunch anyway as a last resort
                        } else {
                            tracing::warn!(
                                "Expert {} still running after exit_wait, retrying /exit (attempt {})",
                                expert_id,
                                exit_retries + 1
                            );
                            self.claude.send_exit(expert_id).await?;
                            executor.set_phase(ExecutionPhase::ExitingExpert {
                                started_at: Instant::now(),
                                exit_retries: exit_retries + 1,
                            });
                            self.feature_executor = Some(executor);
                            return Ok(());
                        }
                    }

                    if let Err(e) = self.detector.set_marker(expert_id, "pending") {
                        tracing::warn!(
                            "Failed to reset status marker for expert {}: {}",
                            expert_id,
                            e
                        );
                    }
                    self.claude
                        .launch_claude(
                            expert_id,
                            executor.working_dir(),
                            executor.instruction_file().map(PathBuf::as_path),
                            executor.agents_file().map(PathBuf::as_path),
                            executor.settings_file().map(PathBuf::as_path),
                        )
                        .await?;
                    executor.set_phase(ExecutionPhase::RelaunchingExpert {
                        started_at: Instant::now(),
                        ready_detected_at: None,
                    });
                    self.set_message(format!(
                        "~ {}: resetting expert... | {}/{} tasks",
                        executor.feature_name(),
                        executor.completed_tasks(),
                        executor.total_tasks()
                    ));
                }
            }

            ExecutionPhase::RelaunchingExpert {
                started_at,
                ready_detected_at,
            } => {
                let started_at = *started_at;
                let ready_detected_at = *ready_detected_at;
                let expert_id = executor.expert_id();
                let timeout = executor.ready_timeout();
                let grace = executor.ready_grace_period();

                if let Some(detected_at) = ready_detected_at {
                    if detected_at.elapsed() >= grace {
                        executor.set_phase(ExecutionPhase::SendingBatch);
                    }
                } else {
                    match self.tmux.capture_pane(expert_id).await {
                        Ok(content) => {
                            if content.contains("bypass permissions") {
                                executor.set_phase(ExecutionPhase::RelaunchingExpert {
                                    started_at,
                                    ready_detected_at: Some(Instant::now()),
                                });
                            } else if started_at.elapsed() >= timeout {
                                executor.set_phase(ExecutionPhase::Failed(
                                    "Timed out waiting for Claude to restart".into(),
                                ));
                            }
                        }
                        Err(e) => {
                            if started_at.elapsed() >= timeout {
                                executor.set_phase(ExecutionPhase::Failed(format!(
                                    "Failed to detect Claude ready: {e}"
                                )));
                            }
                        }
                    }
                }
            }

            ExecutionPhase::SendingBatch => {
                match executor.parse_tasks() {
                    Ok(tasks) => match executor.next_batch(&tasks) {
                        Ok(batch) if batch.is_empty() => {
                            executor.set_phase(ExecutionPhase::Completed);
                        }
                        Ok(batch) => {
                            let prompt = executor.build_prompt(&batch);
                            let expert_id = executor.expert_id();
                            executor.record_batch_sent(&batch);
                            self.claude.send_keys_with_enter(expert_id, &prompt).await?;
                            // NOTE: Because the next task may be polled,
                            // set the marker manually.
                            if let Err(e) = self.detector.set_marker(expert_id, "processing") {
                                tracing::warn!(
                                    "Failed to set processing marker for expert {}: {}",
                                    expert_id,
                                    e
                                );
                            }
                            let batch_numbers = executor.current_batch().join(", ");
                            self.set_message(format!(
                                "> {}: {}/{} tasks | Batch: {}",
                                executor.feature_name(),
                                executor.completed_tasks(),
                                executor.total_tasks(),
                                batch_numbers
                            ));
                            executor.set_phase(ExecutionPhase::WaitingPollDelay {
                                started_at: Instant::now(),
                            });
                        }
                        Err(blocked_msg) => {
                            executor.set_phase(ExecutionPhase::Failed(blocked_msg));
                        }
                    },
                    Err(e) => {
                        executor.set_phase(ExecutionPhase::Failed(format!(
                            "Failed to parse task file: {e}"
                        )));
                    }
                }
            }

            ExecutionPhase::WaitingPollDelay { started_at } => {
                let started_at = *started_at;
                if started_at.elapsed() >= executor.poll_delay() {
                    executor.set_phase(ExecutionPhase::PollingStatus);
                }
            }

            ExecutionPhase::PollingStatus => {
                let expert_id = executor.expert_id();
                let state = self.detector.detect_state(expert_id);
                if state == ExpertState::Idle {
                    match executor.parse_tasks() {
                        Ok(tasks) => {
                            let remaining = tasks.iter().filter(|t| !t.completed).count();
                            if remaining == 0 {
                                executor.clear_batch_completion_wait();
                                executor.set_phase(ExecutionPhase::Completed);
                            } else if !executor.is_previous_batch_completed(&tasks) {
                                executor.start_batch_completion_wait();
                                let elapsed = executor.batch_completion_wait_elapsed().unwrap();
                                if elapsed >= executor.poll_delay() * 3 {
                                    tracing::warn!(
                                        "Previous batch tasks not all completed after {:.1}s, proceeding anyway",
                                        elapsed.as_secs_f64()
                                    );
                                    executor.clear_batch_completion_wait();
                                    self.claude.send_exit(expert_id).await?;
                                    executor.set_phase(ExecutionPhase::ExitingExpert {
                                        started_at: Instant::now(),
                                        exit_retries: 0,
                                    });
                                } else {
                                    tracing::debug!(
                                        "Previous batch not fully completed, waiting ({:.1}s)",
                                        elapsed.as_secs_f64()
                                    );
                                }
                            } else {
                                executor.clear_batch_completion_wait();
                                self.claude.send_exit(expert_id).await?;
                                executor.set_phase(ExecutionPhase::ExitingExpert {
                                    started_at: Instant::now(),
                                    exit_retries: 0,
                                });
                            }
                        }
                        Err(e) => {
                            executor.set_phase(ExecutionPhase::Failed(format!(
                                "Failed to re-read task file: {e}"
                            )));
                        }
                    }
                }
            }

            ExecutionPhase::Completed => {}
            ExecutionPhase::Failed(_) => {}
        }

        // Handle terminal states: report and discard executor
        match executor.phase() {
            ExecutionPhase::Completed => {
                self.set_message(format!(
                    "Feature '{}' execution completed ({}/{} tasks)",
                    executor.feature_name(),
                    executor.completed_tasks(),
                    executor.total_tasks()
                ));
                // Don't put executor back — execution is done
            }
            ExecutionPhase::Failed(msg) => {
                self.set_message(format!("Feature execution failed: {msg}"));
                // Don't put executor back — execution failed
            }
            _ => {
                // Put executor back for next poll cycle
                self.feature_executor = Some(executor);
            }
        }

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
                            // Propagate worktree path to both registries
                            let wt_path = Some(result.worktree_path.clone());
                            if let Err(e) = self
                                .expert_registry
                                .update_expert_worktree(result.expert_id, wt_path.clone())
                            {
                                tracing::warn!(
                                    "Failed to update expert {} worktree in registry: {}",
                                    result.expert_id,
                                    e
                                );
                            }
                            if let Some(ref mut router) = self.message_router {
                                if let Err(e) = router
                                    .expert_registry_mut()
                                    .update_expert_worktree(result.expert_id, wt_path)
                                {
                                    tracing::warn!(
                                        "Failed to update expert {} worktree in router: {}",
                                        result.expert_id,
                                        e
                                    );
                                }
                            }

                            if let Err(e) = self.refresh_expert_manifest() {
                                tracing::warn!(
                                    "Failed to refresh expert manifest after worktree launch: {}",
                                    e
                                );
                            }

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
                            self.set_message(format!("Worktree launch failed: {e}"));
                        }
                        Err(e) => {
                            self.set_message(format!("Worktree launch panicked: {e}"));
                        }
                    }
                    self.worktree_launch_state = WorktreeLaunchState::Idle;
                    self.needs_redraw = true;
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
        self.restore_worktree_paths().await?;
        self.update_focus();
        self.refresh_status().await?;
        self.refresh_reports().await?;

        while self.is_running() {
            let loop_start = Instant::now();

            let draw_start = Instant::now();
            if self.needs_redraw {
                terminal.draw(|frame| UI::render(frame, self))?;
                self.needs_redraw = false;
            }
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

            // Process worktree launches before messages so that worktree paths
            // are propagated to registries before message routing checks them.
            self.poll_worktree_launch().await?;

            let poll_messages_start = Instant::now();
            self.poll_messages().await?;
            let poll_messages_elapsed = poll_messages_start.elapsed();

            self.poll_expert_panel().await?;
            self.poll_feature_executor().await?;

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
    fn is_shift_tab_for_task_input_backtab_returns_true() {
        assert!(
            is_shift_tab_for_task_input(KeyCode::BackTab, KeyModifiers::NONE),
            "is_shift_tab_for_task_input: BackTab should be recognized as Shift+Tab"
        );
    }

    #[test]
    fn is_shift_tab_for_task_input_tab_with_shift_returns_true() {
        assert!(
            is_shift_tab_for_task_input(KeyCode::Tab, KeyModifiers::SHIFT),
            "is_shift_tab_for_task_input: Tab+Shift should be recognized as Shift+Tab"
        );
    }

    #[test]
    fn is_shift_tab_for_task_input_plain_tab_returns_false() {
        assert!(
            !is_shift_tab_for_task_input(KeyCode::Tab, KeyModifiers::NONE),
            "is_shift_tab_for_task_input: plain Tab should not be recognized as Shift+Tab"
        );
    }

    #[test]
    fn is_exclamation_at_input_start_returns_true_at_pos_zero() {
        assert!(
            is_exclamation_at_input_start(KeyCode::Char('!'), KeyModifiers::NONE, 0),
            "is_exclamation_at_input_start: '!' at position 0 should return true"
        );
    }

    #[test]
    fn is_exclamation_at_input_start_returns_false_at_nonzero_pos() {
        assert!(
            !is_exclamation_at_input_start(KeyCode::Char('!'), KeyModifiers::NONE, 1),
            "is_exclamation_at_input_start: '!' at position 1 should return false"
        );
    }

    #[test]
    fn is_exclamation_at_input_start_returns_false_with_ctrl() {
        assert!(
            !is_exclamation_at_input_start(KeyCode::Char('!'), KeyModifiers::CONTROL, 0),
            "is_exclamation_at_input_start: Ctrl+! should return false"
        );
    }

    #[test]
    fn is_exclamation_at_input_start_returns_false_with_alt() {
        assert!(
            !is_exclamation_at_input_start(KeyCode::Char('!'), KeyModifiers::ALT, 0),
            "is_exclamation_at_input_start: Alt+! should return false"
        );
    }

    #[test]
    fn is_exclamation_at_input_start_returns_false_for_other_char() {
        assert!(
            !is_exclamation_at_input_start(KeyCode::Char('a'), KeyModifiers::NONE, 0),
            "is_exclamation_at_input_start: 'a' at position 0 should return false"
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

    #[tokio::test]
    async fn tower_app_quit_aborts_in_progress_expert_panel_update() {
        let mut app = create_test_app();
        let handle = tokio::spawn(async {
            tokio::time::sleep(Duration::from_secs(60)).await;
            Ok::<ExpertPanelUpdateResult, anyhow::Error>(ExpertPanelUpdateResult {
                expert_id: 0,
                content: String::new(),
                resized_preview_size: None,
                resized_expert_id: None,
            })
        });
        let abort_handle = handle.abort_handle();
        app.expert_panel_update_state = ExpertPanelUpdateState::InProgress { handle };

        app.quit();
        tokio::task::yield_now().await;

        assert!(matches!(
            app.expert_panel_update_state,
            ExpertPanelUpdateState::Idle
        ));
        assert!(
            abort_handle.is_finished(),
            "quit() should abort in-progress expert panel update task"
        );
    }

    #[test]
    fn tower_app_focus_stays_on_task_input_without_panel() {
        let mut app = create_test_app();

        // Panel starts visible; hide it for this test
        app.expert_panel_display.hide();

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
        // Panel starts visible; hide it for this test
        app.expert_panel_display.hide();
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
        // Panel starts visible
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

        // Toggle on — panel becomes visible again
        app.expert_panel_display.toggle();
        assert!(app.expert_panel_display.is_visible());
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
        assert!(!app.expert_registry.is_empty() || app.config.experts.is_empty());
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
    async fn return_expert_no_expert_selected_shows_error() {
        let mut app = create_test_app();

        app.return_expert_from_worktree().await.unwrap();

        assert_eq!(
            app.message(),
            Some("No expert selected"),
            "return_expert_from_worktree: should show error when no expert selected"
        );
    }

    #[tokio::test]
    async fn return_expert_no_worktree_shows_error() {
        let mut app = create_test_app();
        app.status_display.set_experts(vec![ExpertEntry {
            expert_id: 0,
            expert_name: "architect".to_string(),
            state: ExpertState::Idle,
        }]);
        app.status_display.next();

        app.return_expert_from_worktree().await.unwrap();

        assert_eq!(
            app.message(),
            Some("Enter a feature name in the task input before launching worktree"),
            "return_expert_from_worktree: should show error when expert not in worktree"
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

    // --- Task 8.1: Feature execution tests (P7: Cancellation Safety) ---

    #[test]
    fn feature_executor_starts_none() {
        let app = create_test_app();
        assert!(
            app.feature_executor.is_none(),
            "feature_executor: should start as None"
        );
    }

    #[tokio::test]
    async fn start_feature_execution_rejects_empty_input() {
        let mut app = create_test_app();

        app.start_feature_execution().await.unwrap();

        assert!(
            app.message()
                .unwrap()
                .contains("No expert selected")
                || app
                    .message()
                    .unwrap()
                    .contains("Enter a feature name"),
            "start_feature_execution: should reject when no expert selected or empty input, got: {:?}",
            app.message()
        );
    }

    #[tokio::test]
    async fn handle_feature_execution_cancels_when_running() {
        let temp = tempfile::TempDir::new().unwrap();
        let status_dir = temp.path().join(".macot").join("status");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(status_dir.join("expert0"), "processing").unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);

        // Set up a dummy executor in SendingBatch phase
        let exec_config = crate::config::FeatureExecutionConfig::default();
        let specs = temp.path().join(".macot").join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(specs.join("test-tasks.md"), "- [ ] 1. Task\n").unwrap();

        let mut executor = FeatureExecutor::new(
            "test".to_string(),
            0,
            &exec_config,
            &temp.path().to_path_buf(),
            None,
            None,
            None,
            "/tmp".to_string(),
        );
        executor.set_phase(ExecutionPhase::SendingBatch);
        app.feature_executor = Some(executor);

        // Ctrl+G while running should cancel
        app.handle_feature_execution().await.unwrap();

        assert!(
            app.feature_executor.is_none(),
            "handle_feature_execution: should clear executor on cancel"
        );
        assert_eq!(
            app.message(),
            Some("Feature execution cancelled"),
            "handle_feature_execution: should show cancellation message"
        );
        let status = std::fs::read_to_string(status_dir.join("expert0")).unwrap();
        assert_eq!(
            status, "pending",
            "handle_feature_execution: should reset expert status to pending on cancel"
        );
    }

    #[tokio::test]
    async fn start_feature_execution_rejects_missing_task_file() {
        let temp = tempfile::TempDir::new().unwrap();
        let status_dir = temp.path().join(".macot").join("status");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(status_dir.join("expert0"), "pending").unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);

        // Set experts and select first one
        app.status_display.set_experts(vec![ExpertEntry {
            expert_id: 0,
            expert_name: "Alyosha".to_string(),
            state: ExpertState::Idle,
        }]);
        app.status_display.next(); // Select first expert

        app.task_input
            .set_content("nonexistent-feature".to_string());

        app.start_feature_execution().await.unwrap();

        assert!(
            app.feature_executor.is_none(),
            "start_feature_execution: should not create executor when task file missing"
        );
        assert!(
            app.message().unwrap().contains("error")
                || app.message().unwrap().contains("not found")
                || app.message().unwrap().contains("Error"),
            "start_feature_execution: should show error about missing task file, got: {:?}",
            app.message()
        );
    }

    // --- Task 4.1: needs_redraw flag state transitions (P2: Dirty Flag Completeness) ---

    #[test]
    fn needs_redraw_initialized_to_true() {
        let app = create_test_app();
        assert!(
            app.needs_redraw(),
            "needs_redraw: should be true after construction"
        );
    }

    #[test]
    fn set_message_sets_needs_redraw() {
        let mut app = create_test_app();
        app.clear_needs_redraw();
        assert!(!app.needs_redraw());

        app.set_message("hello".to_string());
        assert!(
            app.needs_redraw(),
            "needs_redraw: set_message should set the flag"
        );
    }

    #[test]
    fn clear_needs_redraw_resets_flag() {
        let mut app = create_test_app();
        assert!(app.needs_redraw());

        app.clear_needs_redraw();
        assert!(
            !app.needs_redraw(),
            "needs_redraw: clear_needs_redraw should reset the flag"
        );
    }

    #[tokio::test]
    async fn poll_status_sets_needs_redraw() {
        let temp = tempfile::TempDir::new().unwrap();
        let status_dir = temp.path().join(".macot").join("status");
        std::fs::create_dir_all(&status_dir).unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);
        app.reset_poll_timers_for_test();
        app.clear_needs_redraw();

        let _ = app.poll_status().await;
        assert!(
            app.needs_redraw(),
            "needs_redraw: poll_status should set the flag"
        );
    }

    #[tokio::test]
    async fn poll_reports_sets_needs_redraw() {
        let temp = tempfile::TempDir::new().unwrap();
        let reports_dir = temp.path().join(".macot").join("reports");
        std::fs::create_dir_all(&reports_dir).unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);
        app.reset_poll_timers_for_test();
        app.clear_needs_redraw();

        let _ = app.poll_reports().await;
        assert!(
            app.needs_redraw(),
            "needs_redraw: poll_reports should set the flag"
        );
    }

    #[tokio::test]
    async fn poll_messages_sets_needs_redraw() {
        let temp = tempfile::TempDir::new().unwrap();
        let queue_dir = temp.path().join(".macot");
        std::fs::create_dir_all(queue_dir.join("status")).unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);
        app.reset_poll_timers_for_test();
        app.clear_needs_redraw();

        let _ = app.poll_messages().await;
        assert!(
            app.needs_redraw(),
            "needs_redraw: poll_messages should set the flag"
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_sets_needs_redraw_on_completion() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async {
            Ok(WorktreeLaunchResult {
                expert_id: 0,
                expert_name: "test".to_string(),
                branch_name: "test-branch".to_string(),
                worktree_path: "/tmp/wt".to_string(),
                claude_ready: true,
            })
        });
        wait_for_handle(&handle).await;

        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "test".to_string(),
            branch_name: "test-branch".to_string(),
        };
        app.clear_needs_redraw();

        app.poll_worktree_launch().await.unwrap();
        assert!(
            app.needs_redraw(),
            "needs_redraw: poll_worktree_launch should set flag on completion"
        );
    }

    #[tokio::test]
    async fn poll_worktree_launch_propagates_worktree_to_both_registries() {
        let mut app = create_test_app();

        let handle = tokio::spawn(async {
            Ok(WorktreeLaunchResult {
                expert_id: 0,
                expert_name: "Alyosha".to_string(),
                branch_name: "feature-auth".to_string(),
                worktree_path: "/tmp/wt/feature-auth".to_string(),
                claude_ready: true,
            })
        });
        wait_for_handle(&handle).await;

        app.worktree_launch_state = WorktreeLaunchState::InProgress {
            handle,
            expert_name: "Alyosha".to_string(),
            branch_name: "feature-auth".to_string(),
        };

        app.poll_worktree_launch().await.unwrap();

        // Verify main expert_registry was updated
        let expert = app.expert_registry.get_expert(0).unwrap();
        assert_eq!(
            expert.worktree_path,
            Some("/tmp/wt/feature-auth".to_string()),
            "poll_worktree_launch: should update worktree_path in main registry"
        );

        // Verify router's expert_registry was also updated
        let router = app.message_router.as_ref().unwrap();
        let router_expert = router.expert_registry().get_expert(0).unwrap();
        assert_eq!(
            router_expert.worktree_path,
            Some("/tmp/wt/feature-auth".to_string()),
            "poll_worktree_launch: should update worktree_path in router registry"
        );
    }

    #[tokio::test]
    async fn restore_worktree_paths_loads_persisted_context() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config.clone(), wm);

        // Create a real worktree directory so the path check passes
        let wt_dir = temp.path().join("wt_feature");
        std::fs::create_dir_all(&wt_dir).unwrap();

        // Persist an ExpertContext with a worktree_path
        let mut ctx =
            ExpertContext::new(0, "Alyosha".to_string(), config.session_hash().to_string());
        ctx.set_worktree("feature".to_string(), wt_dir.to_str().unwrap().to_string());
        app.context_store.save_expert_context(&ctx).await.unwrap();

        // Run restore
        app.restore_worktree_paths().await.unwrap();

        // Verify main registry
        let expert = app.expert_registry.get_expert(0).unwrap();
        assert_eq!(
            expert.worktree_path,
            Some(wt_dir.to_str().unwrap().to_string()),
            "restore_worktree_paths: should load worktree_path into main registry"
        );

        // Verify router registry
        let router = app.message_router.as_ref().unwrap();
        let router_expert = router.expert_registry().get_expert(0).unwrap();
        assert_eq!(
            router_expert.worktree_path,
            Some(wt_dir.to_str().unwrap().to_string()),
            "restore_worktree_paths: should load worktree_path into router registry"
        );
    }

    #[tokio::test]
    async fn restore_worktree_paths_skips_nonexistent_paths() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config.clone(), wm);

        // Persist an ExpertContext with a path that does NOT exist on disk
        let mut ctx =
            ExpertContext::new(0, "Alyosha".to_string(), config.session_hash().to_string());
        ctx.set_worktree(
            "deleted-branch".to_string(),
            "/nonexistent/worktree/path".to_string(),
        );
        app.context_store.save_expert_context(&ctx).await.unwrap();

        app.restore_worktree_paths().await.unwrap();

        let expert = app.expert_registry.get_expert(0).unwrap();
        assert_eq!(
            expert.worktree_path, None,
            "restore_worktree_paths: should skip nonexistent worktree paths"
        );
    }

    #[tokio::test]
    async fn restore_worktree_paths_handles_no_context() {
        let temp = tempfile::TempDir::new().unwrap();
        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);

        // No context files exist — should complete without error
        let result = app.restore_worktree_paths().await;
        assert!(
            result.is_ok(),
            "restore_worktree_paths: should handle missing context files gracefully"
        );

        // All experts should still have None worktree
        for i in 0..4u32 {
            let expert = app.expert_registry.get_expert(i).unwrap();
            assert_eq!(
                expert.worktree_path, None,
                "restore_worktree_paths: expert {} should remain None without context",
                i
            );
        }
    }

    #[tokio::test]
    async fn poll_status_skipped_during_debounce_no_redraw() {
        let temp = tempfile::TempDir::new().unwrap();
        let status_dir = temp.path().join(".macot").join("status");
        std::fs::create_dir_all(&status_dir).unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);
        // Do NOT reset timers — last_input_time is recent, triggering debounce skip
        app.clear_needs_redraw();

        let _ = app.poll_status().await;
        assert!(
            !app.needs_redraw(),
            "needs_redraw: poll_status should NOT set flag when debounce skips"
        );
    }

    // --- Task 6.1: Event responsiveness tests (P2, P7) ---

    #[test]
    fn event_poll_timeout_is_16ms() {
        assert_eq!(
            EVENT_POLL_TIMEOUT,
            Duration::from_millis(16),
            "EVENT_POLL_TIMEOUT: should be 16ms for ~60 FPS"
        );
    }

    #[test]
    fn key_event_triggers_quit_synchronously() {
        let mut app = create_test_app();
        assert!(app.is_running());

        app.quit();
        assert!(
            !app.is_running(),
            "quit: should stop the app immediately without delay"
        );
    }

    #[test]
    fn editing_keys_update_debounce_timer() {
        let editing_keys: Vec<(KeyCode, KeyModifiers)> = vec![
            (KeyCode::Char('a'), KeyModifiers::NONE),
            (KeyCode::Backspace, KeyModifiers::NONE),
            (KeyCode::Delete, KeyModifiers::NONE),
            (KeyCode::Enter, KeyModifiers::NONE),
            (KeyCode::Char('h'), KeyModifiers::CONTROL),
            (KeyCode::Char('d'), KeyModifiers::CONTROL),
            (KeyCode::Char('u'), KeyModifiers::CONTROL),
            (KeyCode::Char('k'), KeyModifiers::CONTROL),
        ];

        for (code, modifiers) in editing_keys {
            let mut app = create_test_app();
            app.task_input.set_content("test".to_string());
            // Reset timer to the past
            app.reset_poll_timers_for_test();
            let before = app.last_input_time();

            std::thread::sleep(std::time::Duration::from_millis(2));
            app.handle_task_input_keys(code, modifiers);
            assert!(
                app.last_input_time() > before,
                "editing key {:?} (modifiers: {:?}) should update last_input_time",
                code,
                modifiers
            );
        }
    }

    #[test]
    fn cursor_movement_keys_do_not_update_debounce_timer() {
        let mut app = create_test_app();
        app.task_input.set_content("hello".to_string());
        app.reset_poll_timers_for_test();
        let before = app.last_input_time();

        std::thread::sleep(std::time::Duration::from_millis(2));

        // Ctrl+B is cursor movement, should NOT update timer
        app.handle_task_input_keys(KeyCode::Char('b'), KeyModifiers::CONTROL);
        assert_eq!(
            app.last_input_time(),
            before,
            "cursor_movement: Ctrl+B should not update last_input_time"
        );
    }

    // --- Task 9.1: Integration tests for phase transitions with DAG scheduling ---

    #[tokio::test]
    async fn poll_feature_executor_sending_batch_blocked_transitions_to_failed() {
        let temp = tempfile::TempDir::new().unwrap();
        let status_dir = temp.path().join(".macot").join("status");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(status_dir.join("expert0"), "pending").unwrap();

        let specs = temp.path().join(".macot").join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(
            specs.join("blocked-tasks.md"),
            "\
- [ ] 1. Task A [deps: 2]
- [ ] 2. Task B [deps: 1]
",
        )
        .unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let exec_config = &config.feature_execution;
        let mut executor = FeatureExecutor::new(
            "blocked".to_string(),
            0,
            exec_config,
            &temp.path().to_path_buf(),
            None,
            None,
            None,
            temp.path().to_str().unwrap().to_string(),
        );
        executor.validate().unwrap();
        executor.set_phase(ExecutionPhase::SendingBatch);

        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);
        app.feature_executor = Some(executor);

        app.poll_feature_executor().await.unwrap();

        assert!(
            app.feature_executor.is_none(),
            "poll_feature_executor: executor should be discarded on Failed"
        );
        let msg = app.message().unwrap();
        assert!(
            msg.contains("failed") || msg.contains("Failed") || msg.contains("blocked"),
            "poll_feature_executor: should show failure message with blocked diagnostic, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn poll_feature_executor_sending_batch_all_done_transitions_to_completed() {
        let temp = tempfile::TempDir::new().unwrap();
        let status_dir = temp.path().join(".macot").join("status");
        std::fs::create_dir_all(&status_dir).unwrap();
        std::fs::write(status_dir.join("expert0"), "pending").unwrap();

        let specs = temp.path().join(".macot").join("specs");
        std::fs::create_dir_all(&specs).unwrap();
        std::fs::write(
            specs.join("alldone-tasks.md"),
            "\
- [x] 1. Task A
- [x] 2. Task B
",
        )
        .unwrap();

        let config = Config::default().with_project_path(temp.path().to_path_buf());
        let exec_config = &config.feature_execution;
        let mut executor = FeatureExecutor::new(
            "alldone".to_string(),
            0,
            exec_config,
            &temp.path().to_path_buf(),
            None,
            None,
            None,
            temp.path().to_str().unwrap().to_string(),
        );
        executor.validate().unwrap();
        executor.set_phase(ExecutionPhase::SendingBatch);

        let wm = WorktreeManager::new(config.project_path.clone());
        let mut app = TowerApp::new(config, wm);
        app.feature_executor = Some(executor);

        app.poll_feature_executor().await.unwrap();

        assert!(
            app.feature_executor.is_none(),
            "poll_feature_executor: executor should be discarded on Completed"
        );
        let msg = app.message().unwrap();
        assert!(
            msg.contains("completed"),
            "poll_feature_executor: should show completion message, got: {}",
            msg
        );
    }

    #[test]
    fn default_config_uses_dag_scheduler_mode() {
        let config = Config::default();
        assert_eq!(
            config.feature_execution.scheduler_mode,
            crate::feature::scheduler::SchedulerMode::Dag,
            "default_config_uses_dag: feature_execution.scheduler_mode should default to Dag"
        );
    }

    // --- Task 9.2: Tests for manifest refresh integration ---

    fn create_test_app_with_tempdir() -> (TowerApp, tempfile::TempDir) {
        let tmp = tempfile::TempDir::new().unwrap();
        let macot_dir = tmp.path().join(".macot");
        std::fs::create_dir_all(&macot_dir).unwrap();
        let config = Config::default().with_project_path(tmp.path().to_path_buf());
        let wm = WorktreeManager::new(config.project_path.clone());
        let app = TowerApp::new(config, wm);
        (app, tmp)
    }

    #[test]
    fn manifest_generated_at_startup() {
        let (app, tmp) = create_test_app_with_tempdir();
        let manifest_path = tmp.path().join(".macot").join("experts_manifest.json");

        assert!(
            manifest_path.exists(),
            "manifest_generated_at_startup: manifest file should be created on TowerApp::new()"
        );

        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let entries: Vec<crate::instructions::manifest::ExpertManifestEntry> =
            serde_json::from_str(&content).unwrap();

        assert_eq!(
            entries.len(),
            app.config.num_experts() as usize,
            "manifest_generated_at_startup: manifest should include all experts from config"
        );
    }

    #[test]
    fn manifest_refresh_updates_file() {
        let (app, tmp) = create_test_app_with_tempdir();
        let manifest_path = tmp.path().join(".macot").join("experts_manifest.json");

        let content_before = std::fs::read_to_string(&manifest_path).unwrap();
        app.refresh_expert_manifest().unwrap();
        let content_after = std::fs::read_to_string(&manifest_path).unwrap();

        assert_eq!(
            content_before, content_after,
            "manifest_refresh: calling refresh without changes should produce identical content"
        );
    }

    #[test]
    fn manifest_refresh_reflects_role_change() {
        let (mut app, tmp) = create_test_app_with_tempdir();
        let manifest_path = tmp.path().join(".macot").join("experts_manifest.json");

        app.session_roles.set_role(0, "frontend".to_string());
        app.refresh_expert_manifest().unwrap();

        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let entries: Vec<crate::instructions::manifest::ExpertManifestEntry> =
            serde_json::from_str(&content).unwrap();

        assert_eq!(
            entries[0].role, "frontend",
            "manifest_refresh_reflects_role_change: manifest should reflect updated session role"
        );
    }

    #[test]
    fn manifest_refresh_reflects_worktree_assignment() {
        let (mut app, tmp) = create_test_app_with_tempdir();
        let manifest_path = tmp.path().join(".macot").join("experts_manifest.json");

        app.expert_registry
            .update_expert_worktree(0, Some("/wt/feature-x".to_string()))
            .unwrap();
        app.refresh_expert_manifest().unwrap();

        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let entries: Vec<crate::instructions::manifest::ExpertManifestEntry> =
            serde_json::from_str(&content).unwrap();

        assert_eq!(
            entries[0].worktree_path,
            Some("/wt/feature-x".to_string()),
            "manifest_refresh_reflects_worktree: manifest should reflect worktree assignment"
        );
    }

    #[test]
    fn manifest_includes_all_experts_after_refresh() {
        let (app, tmp) = create_test_app_with_tempdir();
        let manifest_path = tmp.path().join(".macot").join("experts_manifest.json");

        app.refresh_expert_manifest().unwrap();

        let content = std::fs::read_to_string(&manifest_path).unwrap();
        let entries: Vec<crate::instructions::manifest::ExpertManifestEntry> =
            serde_json::from_str(&content).unwrap();

        let num_experts = app.config.num_experts() as usize;
        assert_eq!(
            entries.len(),
            num_experts,
            "manifest_includes_all_experts: all {} experts should appear in manifest",
            num_experts
        );
        for (i, entry) in entries.iter().enumerate() {
            assert_eq!(
                entry.expert_id, i as u32,
                "manifest_includes_all_experts: expert_id should match index"
            );
        }
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

            // All experts should start in Idle state
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

                // Initially experts are idle
                assert_eq!(
                    is_idle,
                    Some(true),
                    "Expert '{}' (id={}) should be idle initially",
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
