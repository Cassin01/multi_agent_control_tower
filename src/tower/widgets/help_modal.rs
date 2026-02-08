use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

pub struct HelpModal {
    visible: bool,
}

impl HelpModal {
    pub fn new() -> Self {
        Self { visible: false }
    }

    #[allow(dead_code)]
    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        frame.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Help ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let lines = self.build_help_lines();

        let content_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .split(inner_area)[0];

        let paragraph = Paragraph::new(lines);
        frame.render_widget(paragraph, content_area);
    }

    fn build_help_lines(&self) -> Vec<Line<'static>> {
        vec![
            Self::section_title("Keyboard Shortcuts", Color::Cyan),
            Line::from(""),
            Self::subsection_title("Global"),
            Self::key_line("Tab / Shift+Tab", "Switch focus between panels"),
            Self::key_line("Mouse Click", "Focus clicked panel"),
            Self::key_line("Ctrl+C / Ctrl+Q", "Quit application"),
            Self::key_line("Ctrl+H", "Toggle this help"),
            Line::from(""),
            Self::subsection_title("Task Input"),
            Self::nested_subsection_title("Expert Operations"),
            Self::key_line("Ctrl+P", "Select previous expert"),
            Self::key_line("Ctrl+N", "Select next expert"),
            Self::key_line("Ctrl+O", "Change expert role"),
            Self::key_line("Ctrl+R", "Reset selected expert"),
            Self::key_line("Ctrl+W", "Launch expert in worktree (uses task input as branch name)"),
            Self::nested_subsection_title("Submit / Cancel"),
            Self::key_line("Ctrl+S", "Assign task to selected expert"),
            Self::key_line("Enter", "Insert newline"),
            Self::key_line("Esc", "Clear input"),
            Line::from(""),
            Self::subsection_title("Effort Selector"),
            Self::key_line("h / ←", "Previous effort level"),
            Self::key_line("l / →", "Next effort level"),
            Line::from(""),
            Self::subsection_title("Report List"),
            Self::key_line("j / ↓", "Select next report"),
            Self::key_line("k / ↑", "Select previous report"),
            Self::key_line("Enter", "Open report detail"),
            Line::from(""),
            Self::subsection_title("Report Detail"),
            Self::key_line("j / ↓", "Scroll down"),
            Self::key_line("k / ↑", "Scroll up"),
            Self::key_line("Esc / Enter / q", "Close detail"),
            Line::from(""),
            Line::from(Span::styled(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(vec![
                Span::styled("Esc / Enter / q", Style::default().fg(Color::Yellow)),
                Span::raw(": Close this help"),
            ]),
        ]
    }

    fn section_title(title: &'static str, color: Color) -> Line<'static> {
        Line::from(Span::styled(
            title,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        ))
    }

    fn subsection_title(title: &'static str) -> Line<'static> {
        Line::from(Span::styled(
            format!("━━━ {} ━━━", title),
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ))
    }

    fn nested_subsection_title(title: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("── {} ──", title),
                Style::default().fg(Color::DarkGray),
            ),
        ])
    }

    fn key_line(key: &'static str, description: &'static str) -> Line<'static> {
        Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{:20}", key),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw(description),
        ])
    }
}

impl Default for HelpModal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_starts_hidden() {
        let modal = HelpModal::new();
        assert!(!modal.is_visible());
    }

    #[test]
    fn modal_becomes_visible_after_show() {
        let mut modal = HelpModal::new();
        modal.show();
        assert!(modal.is_visible());
    }

    #[test]
    fn modal_becomes_hidden_after_hide() {
        let mut modal = HelpModal::new();
        modal.show();
        modal.hide();
        assert!(!modal.is_visible());
    }

    #[test]
    fn toggle_switches_visibility() {
        let mut modal = HelpModal::new();
        assert!(!modal.is_visible());

        modal.toggle();
        assert!(modal.is_visible());

        modal.toggle();
        assert!(!modal.is_visible());
    }

    #[test]
    fn help_text_includes_worktree_shortcut() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Ctrl+W"),
            "build_help_lines: should contain Ctrl+W shortcut"
        );
        assert!(
            text.contains("worktree"),
            "build_help_lines: should describe worktree functionality"
        );
    }
}
