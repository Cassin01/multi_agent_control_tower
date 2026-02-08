use std::collections::HashMap;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::models::ExpertState;

#[derive(Debug, Clone)]
pub struct ExpertEntry {
    pub expert_id: u32,
    pub expert_name: String,
    pub state: ExpertState,
}

pub struct StatusDisplay {
    experts: Vec<ExpertEntry>,
    state: ListState,
    focused: bool,
    expert_roles: HashMap<u32, String>,
}

impl StatusDisplay {
    pub fn new() -> Self {
        Self {
            experts: Vec::new(),
            state: ListState::default(),
            focused: false,
            expert_roles: HashMap::new(),
        }
    }

    pub fn set_experts(&mut self, experts: Vec<ExpertEntry>) {
        self.experts = experts;
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
        if self.experts.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.experts.len() - 1 {
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
        if self.experts.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.experts.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected(&self) -> Option<&ExpertEntry> {
        self.state.selected().and_then(|i| self.experts.get(i))
    }

    pub fn selected_expert_id(&self) -> Option<u32> {
        self.selected().map(|e| e.expert_id)
    }

    pub fn expert_count(&self) -> usize {
        self.experts.len()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .experts
            .iter()
            .map(|entry| {
                let status_style = Style::default().fg(entry.state.color());

                let role = self.expert_roles.get(&entry.expert_id);
                let role_display = match role {
                    Some(r) => format!(" ({})", r),
                    None => String::new(),
                };

                let spans = vec![
                    Span::styled(
                        format!("[{}] ", entry.expert_id),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(entry.state.symbol(), status_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:<12}", entry.expert_name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(role_display, Style::default().fg(Color::Cyan)),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

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

        for entry in &self.experts {
            match entry.state {
                ExpertState::Idle => summary.idle += 1,
                ExpertState::Busy => summary.busy += 1,
                ExpertState::Offline => summary.offline += 1,
            }
        }

        summary.total = self.experts.len();
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
    pub busy: usize,
    pub offline: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_entry(id: u32, name: &str, state: ExpertState) -> ExpertEntry {
        ExpertEntry {
            expert_id: id,
            expert_name: name.to_string(),
            state,
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
        display.set_experts(vec![
            create_test_entry(0, "architect", ExpertState::Idle),
            create_test_entry(1, "frontend", ExpertState::Busy),
            create_test_entry(2, "backend", ExpertState::Offline),
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
        display.set_experts(vec![
            create_test_entry(0, "architect", ExpertState::Idle),
            create_test_entry(1, "frontend", ExpertState::Busy),
        ]);

        display.prev();
        assert_eq!(display.selected_expert_id(), Some(0));

        display.prev();
        assert_eq!(display.selected_expert_id(), Some(1));
    }

    #[test]
    fn status_display_summary() {
        let mut display = StatusDisplay::new();
        display.set_experts(vec![
            create_test_entry(0, "architect", ExpertState::Idle),
            create_test_entry(1, "frontend", ExpertState::Idle),
            create_test_entry(2, "backend", ExpertState::Busy),
            create_test_entry(3, "tester", ExpertState::Offline),
        ]);

        let summary = display.get_status_summary();
        assert_eq!(summary.total, 4);
        assert_eq!(summary.idle, 2);
        assert_eq!(summary.busy, 1);
        assert_eq!(summary.offline, 1);
    }

    #[test]
    fn status_display_focus_state() {
        let mut display = StatusDisplay::new();
        assert!(!display.is_focused());

        display.set_focused(true);
        assert!(display.is_focused());
    }

    #[test]
    fn selected_returns_expert_entry() {
        let mut display = StatusDisplay::new();
        display.set_experts(vec![
            create_test_entry(5, "devops", ExpertState::Busy),
        ]);

        display.next();
        let selected = display.selected().unwrap();
        assert_eq!(selected.expert_id, 5);
        assert_eq!(selected.expert_name, "devops");
        assert_eq!(selected.state, ExpertState::Busy);
    }

    #[test]
    fn expert_count_returns_correct_count() {
        let mut display = StatusDisplay::new();
        assert_eq!(display.expert_count(), 0);

        display.set_experts(vec![
            create_test_entry(0, "a", ExpertState::Idle),
            create_test_entry(1, "b", ExpertState::Busy),
        ]);
        assert_eq!(display.expert_count(), 2);
    }
}
