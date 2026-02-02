use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::context::RoleInfo;

pub struct RoleSelector {
    visible: bool,
    expert_id: Option<u32>,
    current_role: String,
    available_roles: Vec<RoleInfo>,
    state: ListState,
}

impl RoleSelector {
    pub fn new() -> Self {
        Self {
            visible: false,
            expert_id: None,
            current_role: String::new(),
            available_roles: Vec::new(),
            state: ListState::default(),
        }
    }

    pub fn show(&mut self, expert_id: u32, current_role: &str, roles: Vec<RoleInfo>) {
        self.visible = true;
        self.expert_id = Some(expert_id);
        self.current_role = current_role.to_string();
        self.available_roles = roles;

        let current_index = self
            .available_roles
            .iter()
            .position(|r| r.name == current_role)
            .unwrap_or(0);
        self.state.select(Some(current_index));
    }

    pub fn hide(&mut self) {
        self.visible = false;
        self.expert_id = None;
        self.current_role.clear();
        self.state.select(None);
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn expert_id(&self) -> Option<u32> {
        self.expert_id
    }

    pub fn selected_role(&self) -> Option<&str> {
        self.state
            .selected()
            .and_then(|i| self.available_roles.get(i))
            .map(|r| r.name.as_str())
    }

    pub fn next(&mut self) {
        if self.available_roles.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.available_roles.len() - 1 {
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
        if self.available_roles.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.available_roles.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let popup_width = 50.min(area.width.saturating_sub(4));
        let popup_height = (self.available_roles.len() as u16 + 6).min(area.height.saturating_sub(4));

        let popup_area = centered_rect(popup_width, popup_height, area);

        frame.render_widget(Clear, popup_area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(1),
                Constraint::Length(2),
            ])
            .split(popup_area);

        let title = format!(
            "Select Role for Expert {}",
            self.expert_id.unwrap_or(0)
        );
        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                format!("Current: {}", self.current_role),
                Style::default().fg(Color::Yellow),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(title),
        );
        frame.render_widget(header, chunks[0]);

        let items: Vec<ListItem> = self
            .available_roles
            .iter()
            .enumerate()
            .map(|(idx, role)| {
                let is_current = role.name == self.current_role;
                let marker = if is_current { "●" } else { "○" };

                let style = if is_current {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let spans = vec![
                    Span::styled(format!("[{}] ", idx + 1), Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("{} ", marker), style),
                    Span::styled(format!("{:<12}", role.display_name), style),
                    Span::styled(
                        format!(" - {}", truncate_str(&role.description, 25)),
                        Style::default().fg(Color::Gray),
                    ),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::LEFT | Borders::RIGHT))
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, chunks[1], &mut self.state);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(": Select  |  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(": Cancel  |  "),
            Span::styled("j/k", Style::default().fg(Color::Cyan)),
            Span::raw(": Navigate"),
        ]))
        .block(Block::default().borders(Borders::LEFT | Borders::RIGHT | Borders::BOTTOM));
        frame.render_widget(footer, chunks[2]);
    }
}

impl Default for RoleSelector {
    fn default() -> Self {
        Self::new()
    }
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let truncate_at = max_chars.saturating_sub(3);
        let byte_index = s
            .char_indices()
            .nth(truncate_at)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}...", &s[..byte_index])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_roles() -> Vec<RoleInfo> {
        vec![
            RoleInfo {
                name: "architect".to_string(),
                display_name: "Architect".to_string(),
                description: "System design".to_string(),
            },
            RoleInfo {
                name: "backend".to_string(),
                display_name: "Backend".to_string(),
                description: "Server logic".to_string(),
            },
            RoleInfo {
                name: "frontend".to_string(),
                display_name: "Frontend".to_string(),
                description: "UI development".to_string(),
            },
        ]
    }

    #[test]
    fn role_selector_initially_hidden() {
        let selector = RoleSelector::new();
        assert!(!selector.is_visible());
        assert!(selector.expert_id().is_none());
    }

    #[test]
    fn role_selector_show_makes_visible() {
        let mut selector = RoleSelector::new();
        selector.show(0, "architect", create_test_roles());

        assert!(selector.is_visible());
        assert_eq!(selector.expert_id(), Some(0));
        assert_eq!(selector.selected_role(), Some("architect"));
    }

    #[test]
    fn role_selector_hide_resets_state() {
        let mut selector = RoleSelector::new();
        selector.show(0, "architect", create_test_roles());
        selector.hide();

        assert!(!selector.is_visible());
        assert!(selector.expert_id().is_none());
    }

    #[test]
    fn role_selector_navigation() {
        let mut selector = RoleSelector::new();
        selector.show(0, "architect", create_test_roles());

        assert_eq!(selector.selected_role(), Some("architect"));

        selector.next();
        assert_eq!(selector.selected_role(), Some("backend"));

        selector.next();
        assert_eq!(selector.selected_role(), Some("frontend"));

        selector.next();
        assert_eq!(selector.selected_role(), Some("architect"));
    }

    #[test]
    fn role_selector_prev_navigation() {
        let mut selector = RoleSelector::new();
        selector.show(0, "architect", create_test_roles());

        selector.prev();
        assert_eq!(selector.selected_role(), Some("frontend"));

        selector.prev();
        assert_eq!(selector.selected_role(), Some("backend"));
    }

    #[test]
    fn truncate_str_short_string() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn truncate_str_long_string() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn truncate_str_utf8_safe() {
        // Japanese text: "こんにちは世界" (Hello World) = 7 characters
        let japanese = "こんにちは世界";
        assert_eq!(truncate_str(japanese, 10), japanese);
        assert_eq!(truncate_str(japanese, 5), "こん...");
    }
}
