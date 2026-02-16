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
            Self::key_line("Ctrl+T", "Switch focus between panels"),
            Self::key_line("Mouse Click", "Focus clicked panel"),
            Self::key_line("Ctrl+C / Ctrl+Q", "Quit application"),
            Self::key_line("Ctrl+I", "Toggle this help"),
            Self::key_line("Ctrl+J", "Toggle expert panel"),
            Line::from(""),
            Self::subsection_title("Task Input"),
            Self::nested_subsection_title("Expert Operations"),
            Self::key_line("\u{2191} / \u{2193}", "Select previous / next expert"),
            Self::key_line("Ctrl+O", "Change expert role"),
            Self::key_line("Ctrl+R", "Reset selected expert"),
            Self::key_line(
                "Ctrl+W",
                "Launch expert in worktree (uses task input as branch name)",
            ),
            Self::key_line("Ctrl+G", "Implement tasks / Cancel implementation"),
            Self::key_line("Ctrl+X", "View report for selected expert"),
            Self::nested_subsection_title("Cursor Movement"),
            Self::key_line("Ctrl+B / Ctrl+F", "Move cursor left / right"),
            Self::key_line("Ctrl+A / Ctrl+E", "Move to line start / end"),
            Self::key_line("Ctrl+P / Ctrl+N", "Move to previous / next line"),
            Self::nested_subsection_title("Editing"),
            Self::key_line("Ctrl+H", "Delete character before cursor (backspace)"),
            Self::key_line("Ctrl+D", "Delete character at cursor (delete)"),
            Self::key_line(
                "Ctrl+U",
                "Delete from line start to cursor (unix-line-discard)",
            ),
            Self::key_line("Ctrl+K", "Delete from cursor to line end (kill-line)"),
            Self::nested_subsection_title("Submit"),
            Self::key_line("Ctrl+S", "Assign task to selected expert"),
            Self::key_line("Enter", "Insert newline"),
            Line::from(""),
            Self::subsection_title("Expert Panel"),
            Self::key_line("PageUp", "Enter scroll mode / Scroll up"),
            Self::key_line("PageDown", "Scroll down"),
            Self::key_line("Home / End", "Scroll to top / bottom"),
            Self::key_line("Esc", "Exit scroll mode"),
            Line::from(""),
            Self::subsection_title("Report Detail"),
            Self::key_line("j / \u{2193}", "Scroll down"),
            Self::key_line("k / \u{2191}", "Scroll up"),
            Self::key_line("Enter / q / Ctrl+X", "Close detail"),
            Line::from(""),
            Line::from(Span::styled(
                "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(vec![
                Span::styled("Enter / q", Style::default().fg(Color::Yellow)),
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
            Span::styled(format!("{:20}", key), Style::default().fg(Color::Yellow)),
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

    #[test]
    fn help_text_shows_ctrl_i_for_help() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Ctrl+I"),
            "build_help_lines: should show Ctrl+I for help toggle"
        );
    }

    #[test]
    fn help_text_shows_ctrl_t_for_panel_switch() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Ctrl+T"),
            "build_help_lines: should show Ctrl+T for panel switching"
        );
    }

    #[test]
    fn help_text_shows_ctrl_j_for_expert_panel() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Ctrl+J"),
            "build_help_lines: should show Ctrl+J for expert panel toggle"
        );
        assert!(
            text.contains("expert panel"),
            "build_help_lines: should describe expert panel functionality"
        );
    }

    #[test]
    fn help_text_shows_expert_panel_keybindings() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Expert Panel"),
            "build_help_lines: should have Expert Panel subsection"
        );
        assert!(
            text.contains("PageUp") && text.contains("scroll mode"),
            "build_help_lines: should show PageUp for scroll mode"
        );
        assert!(
            text.contains("Esc") && text.contains("Exit scroll mode"),
            "build_help_lines: should show Esc for exiting scroll mode"
        );
    }

    #[test]
    fn help_text_shows_ctrl_g_for_feature_execution() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Ctrl+G"),
            "build_help_lines: should show Ctrl+G for feature execution"
        );
        assert!(
            text.contains("Execute feature"),
            "build_help_lines: should describe feature execution functionality"
        );
    }

    #[test]
    fn help_text_shows_editing_keybindings() {
        let modal = HelpModal::new();
        let lines = modal.build_help_lines();
        let text: String = lines
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.as_ref()))
            .collect();
        assert!(
            text.contains("Ctrl+H") && text.contains("backspace"),
            "build_help_lines: should show Ctrl+H for backspace"
        );
        assert!(
            text.contains("Ctrl+D") && text.contains("delete"),
            "build_help_lines: should show Ctrl+D for delete"
        );
        assert!(
            text.contains("Ctrl+U") && text.contains("unix-line-discard"),
            "build_help_lines: should show Ctrl+U for unix-line-discard"
        );
        assert!(
            text.contains("Ctrl+K") && text.contains("kill-line"),
            "build_help_lines: should show Ctrl+K for kill-line"
        );
    }
}
