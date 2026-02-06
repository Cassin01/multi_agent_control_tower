use std::collections::HashMap;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::session::{AgentStatus, PaneCapture};

pub struct StatusDisplay {
    captures: Vec<PaneCapture>,
    state: ListState,
    focused: bool,
    expert_roles: HashMap<u32, String>,
}

impl StatusDisplay {
    pub fn new() -> Self {
        Self {
            captures: Vec::new(),
            state: ListState::default(),
            focused: false,
            expert_roles: HashMap::new(),
        }
    }

    pub fn set_captures(&mut self, captures: Vec<PaneCapture>) {
        self.captures = captures;
    }

    #[allow(dead_code)]
    pub fn set_expert_role(&mut self, expert_id: u32, role: String) {
        self.expert_roles.insert(expert_id, role);
    }

    pub fn set_expert_roles(&mut self, roles: HashMap<u32, String>) {
        self.expert_roles = roles;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    #[allow(dead_code)]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn next(&mut self) {
        if self.captures.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.captures.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.captures.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.captures.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected(&self) -> Option<&PaneCapture> {
        self.state.selected().and_then(|i| self.captures.get(i))
    }

    pub fn selected_expert_id(&self) -> Option<u32> {
        self.selected().map(|c| c.expert_id)
    }

    pub fn expert_count(&self) -> usize {
        self.captures.len()
    }

    /// Get a capture by expert ID
    pub fn get_capture(&self, expert_id: u32) -> Option<&PaneCapture> {
        self.captures.iter().find(|c| c.expert_id == expert_id)
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .captures
            .iter()
            .map(|capture| {
                let status_style = Style::default().fg(capture.status.color());

                let role = self.expert_roles.get(&capture.expert_id);
                let role_display = match role {
                    Some(r) => format!(" ({})", r),
                    None => String::new(),
                };

                let spans = vec![
                    Span::styled(
                        format!("[{}] ", capture.expert_id),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(capture.status.symbol(), status_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:<12}", capture.expert_name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(role_display, Style::default().fg(Color::Cyan)),
                    Span::raw(" - "),
                    Span::styled(&capture.last_activity, Style::default()),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        // Use DarkGray consistently for display-only panel (non-interactive)
        let border_style = Style::default().fg(ratatui::style::Color::DarkGray);

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Experts"),
            )
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.state);
    }

    pub fn get_status_summary(&self) -> StatusSummary {
        let mut summary = StatusSummary::default();

        for capture in &self.captures {
            match capture.status {
                AgentStatus::Idle => summary.idle += 1,
                AgentStatus::Thinking => summary.thinking += 1,
                AgentStatus::Executing => summary.executing += 1,
                AgentStatus::Error => summary.error += 1,
                AgentStatus::Unknown => summary.unknown += 1,
            }
        }

        summary.total = self.captures.len();
        summary
    }
}

impl Default for StatusDisplay {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct StatusSummary {
    pub total: usize,
    pub idle: usize,
    pub thinking: usize,
    pub executing: usize,
    pub error: usize,
    pub unknown: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn create_test_capture(id: u32, name: &str, status: AgentStatus) -> PaneCapture {
        PaneCapture {
            expert_id: id,
            expert_name: name.to_string(),
            lines: vec![],
            captured_at: Utc::now(),
            status,
            last_activity: "test".to_string(),
        }
    }

    #[test]
    fn status_display_empty_by_default() {
        let display = StatusDisplay::new();
        assert!(display.selected().is_none());
    }

    #[test]
    fn status_display_navigation() {
        let mut display = StatusDisplay::new();
        display.set_captures(vec![
            create_test_capture(0, "architect", AgentStatus::Idle),
            create_test_capture(1, "frontend", AgentStatus::Thinking),
            create_test_capture(2, "backend", AgentStatus::Executing),
        ]);

        display.next();
        assert_eq!(display.selected_expert_id(), Some(0));

        display.next();
        assert_eq!(display.selected_expert_id(), Some(1));

        display.next();
        assert_eq!(display.selected_expert_id(), Some(2));

        display.next();
        assert_eq!(display.selected_expert_id(), Some(0));
    }

    #[test]
    fn status_display_prev_navigation() {
        let mut display = StatusDisplay::new();
        display.set_captures(vec![
            create_test_capture(0, "architect", AgentStatus::Idle),
            create_test_capture(1, "frontend", AgentStatus::Thinking),
        ]);

        display.prev();
        assert_eq!(display.selected_expert_id(), Some(0));

        display.prev();
        assert_eq!(display.selected_expert_id(), Some(1));
    }

    #[test]
    fn status_display_summary() {
        let mut display = StatusDisplay::new();
        display.set_captures(vec![
            create_test_capture(0, "architect", AgentStatus::Idle),
            create_test_capture(1, "frontend", AgentStatus::Idle),
            create_test_capture(2, "backend", AgentStatus::Executing),
            create_test_capture(3, "tester", AgentStatus::Error),
        ]);

        let summary = display.get_status_summary();
        assert_eq!(summary.total, 4);
        assert_eq!(summary.idle, 2);
        assert_eq!(summary.executing, 1);
        assert_eq!(summary.error, 1);
    }

    #[test]
    fn status_display_focus_state() {
        let mut display = StatusDisplay::new();
        assert!(!display.is_focused());

        display.set_focused(true);
        assert!(display.is_focused());
    }
}
