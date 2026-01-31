use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::models::EffortLevel;

pub struct EffortSelector {
    selected: EffortLevel,
    focused: bool,
}

impl EffortSelector {
    pub fn new() -> Self {
        Self {
            selected: EffortLevel::default(),
            focused: false,
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    #[allow(dead_code)]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn next(&mut self) {
        self.selected = self.selected.next();
    }

    pub fn prev(&mut self) {
        self.selected = self.selected.prev();
    }

    pub fn selected(&self) -> EffortLevel {
        self.selected
    }

    #[allow(dead_code)]
    pub fn set_selected(&mut self, level: EffortLevel) {
        self.selected = level;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let levels = EffortLevel::all();

        let spans: Vec<Span> = levels
            .iter()
            .map(|level| {
                let marker = if *level == self.selected {
                    "●"
                } else {
                    "○"
                };
                let style = if *level == self.selected {
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else if self.focused {
                    Style::default().fg(Color::White)
                } else {
                    Style::default().fg(Color::Gray)
                };
                Span::styled(format!("[{}] {:?}  ", marker, level), style)
            })
            .collect();

        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        let paragraph = Paragraph::new(Line::from(spans)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(border_style)
                .title("Effort Level"),
        );

        frame.render_widget(paragraph, area);
    }
}

impl Default for EffortSelector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effort_selector_default_is_medium() {
        let selector = EffortSelector::new();
        assert_eq!(selector.selected(), EffortLevel::Medium);
    }

    #[test]
    fn effort_selector_next_cycles() {
        let mut selector = EffortSelector::new();
        assert_eq!(selector.selected(), EffortLevel::Medium);

        selector.next();
        assert_eq!(selector.selected(), EffortLevel::Complex);

        selector.next();
        assert_eq!(selector.selected(), EffortLevel::Critical);

        selector.next();
        assert_eq!(selector.selected(), EffortLevel::Simple);
    }

    #[test]
    fn effort_selector_prev_cycles() {
        let mut selector = EffortSelector::new();
        selector.prev();
        assert_eq!(selector.selected(), EffortLevel::Simple);

        selector.prev();
        assert_eq!(selector.selected(), EffortLevel::Critical);
    }

    #[test]
    fn effort_selector_focus_state() {
        let mut selector = EffortSelector::new();
        assert!(!selector.is_focused());

        selector.set_focused(true);
        assert!(selector.is_focused());
    }

    #[test]
    fn effort_selector_set_selected() {
        let mut selector = EffortSelector::new();
        selector.set_selected(EffortLevel::Critical);
        assert_eq!(selector.selected(), EffortLevel::Critical);
    }
}
