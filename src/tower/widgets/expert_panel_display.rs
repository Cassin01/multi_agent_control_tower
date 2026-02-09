use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub struct ExpertPanelDisplay {
    expert_id: Option<u32>,
    expert_name: Option<String>,
    content: Text<'static>,
    raw_line_count: usize,
    scroll_offset: u16,
    visible: bool,
    focused: bool,
    auto_scroll: bool,
}

impl Default for ExpertPanelDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl ExpertPanelDisplay {
    pub fn new() -> Self {
        Self {
            expert_id: None,
            expert_name: None,
            content: Text::default(),
            raw_line_count: 0,
            scroll_offset: 0,
            visible: false,
            focused: false,
            auto_scroll: true,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn show(&mut self) {
        self.visible = true;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn expert_id(&self) -> Option<u32> {
        self.expert_id
    }

    pub fn set_expert(&mut self, id: u32, name: String) {
        if self.expert_id != Some(id) {
            self.scroll_offset = 0;
            self.content = Text::default();
            self.raw_line_count = 0;
            self.auto_scroll = true;
        }
        self.expert_id = Some(id);
        self.expert_name = Some(name);
    }

    pub fn set_content(&mut self, text: Text<'static>, line_count: usize) {
        self.content = text;
        self.raw_line_count = line_count;
        if self.auto_scroll && line_count > 0 {
            self.scroll_offset = line_count.saturating_sub(1) as u16;
        }
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_to_bottom(&mut self) {
        if self.raw_line_count > 0 {
            self.scroll_offset = self.raw_line_count.saturating_sub(1) as u16;
        }
        self.auto_scroll = true;
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = match (&self.expert_name, self.expert_id) {
            (Some(name), Some(id)) => format!(" Expert Panel: {} (ID: {}) ", name, id),
            _ => " Expert Panel (no expert selected) ".to_string(),
        };

        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let focus_indicator = if self.focused { " [FOCUSED] " } else { "" };

        let block = Block::default()
            .title(Span::styled(
                format!("{}{}", title, focus_indicator),
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let visible_height = area.height.saturating_sub(2) as usize;
        let max_scroll = self.raw_line_count.saturating_sub(visible_height) as u16;
        self.scroll_offset = self.scroll_offset.min(max_scroll);

        let paragraph = Paragraph::new(self.content.clone())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, area);
    }

    /// Parse raw ANSI-escaped string into styled `Text`.
    /// Falls back to plain `Text::raw()` on parse error (P10).
    pub fn parse_ansi(raw: &str) -> Text<'static> {
        raw.into_text().unwrap_or_else(|_| Text::raw(raw.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_starts_hidden() {
        let panel = ExpertPanelDisplay::new();
        assert!(!panel.is_visible(), "panel should start hidden");
    }

    #[test]
    fn toggle_makes_visible() {
        let mut panel = ExpertPanelDisplay::new();
        panel.toggle();
        assert!(panel.is_visible(), "toggle should make panel visible");
    }

    #[test]
    fn toggle_twice_returns_to_hidden() {
        let mut panel = ExpertPanelDisplay::new();
        panel.toggle();
        panel.toggle();
        assert!(!panel.is_visible(), "toggle twice should return to hidden");
    }

    #[test]
    fn starts_unfocused() {
        let panel = ExpertPanelDisplay::new();
        assert!(!panel.is_focused(), "panel should start unfocused");
    }

    #[test]
    fn set_focused_changes_state() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_focused(true);
        assert!(panel.is_focused(), "set_focused(true) should make focused");
        panel.set_focused(false);
        assert!(!panel.is_focused(), "set_focused(false) should unfocus");
    }

    #[test]
    fn set_expert_tracks_id() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(42, "Alice".to_string());
        assert_eq!(panel.expert_id(), Some(42), "expert_id should return set value");
    }

    #[test]
    fn set_expert_different_resets_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.scroll_down();
        panel.scroll_down();
        assert!(panel.scroll_offset > 0, "scroll should have advanced");

        panel.set_expert(2, "Bob".to_string());
        assert_eq!(panel.scroll_offset, 0, "changing expert should reset scroll to 0");
        assert_eq!(panel.raw_line_count, 0, "changing expert should clear content");
    }

    #[test]
    fn set_expert_same_preserves_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.scroll_down();
        panel.scroll_down();
        let offset = panel.scroll_offset;

        panel.set_expert(1, "Alice".to_string());
        assert_eq!(panel.scroll_offset, offset, "same expert should preserve scroll");
    }

    #[test]
    fn scroll_up_at_zero_stays_zero() {
        let mut panel = ExpertPanelDisplay::new();
        panel.scroll_up();
        assert_eq!(panel.scroll_offset, 0, "scroll_up at zero should stay zero");
    }

    #[test]
    fn scroll_down_increments() {
        let mut panel = ExpertPanelDisplay::new();
        panel.scroll_down();
        assert_eq!(panel.scroll_offset, 1, "scroll_down should increment offset");
    }

    #[test]
    fn scroll_to_bottom_enables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("line1\nline2\nline3"), 3);
        panel.scroll_up();
        assert!(!panel.auto_scroll, "scroll_up should disable auto_scroll");

        panel.scroll_to_bottom();
        assert!(panel.auto_scroll, "scroll_to_bottom should enable auto_scroll");
    }

    #[test]
    fn scroll_up_disables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        assert!(panel.auto_scroll, "auto_scroll should start enabled");

        panel.scroll_up();
        assert!(!panel.auto_scroll, "scroll_up should disable auto_scroll");
    }

    #[test]
    fn show_and_hide() {
        let mut panel = ExpertPanelDisplay::new();
        panel.show();
        assert!(panel.is_visible(), "show() should make visible");
        panel.hide();
        assert!(!panel.is_visible(), "hide() should make hidden");
    }

    #[test]
    fn set_content_auto_scrolls_when_enabled() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("a\nb\nc\nd\ne"), 5);
        assert_eq!(panel.scroll_offset, 4, "auto_scroll should go to last line");
    }

    #[test]
    fn set_content_does_not_auto_scroll_when_disabled() {
        let mut panel = ExpertPanelDisplay::new();
        panel.scroll_up();
        let offset = panel.scroll_offset;
        panel.set_content(Text::raw("a\nb\nc"), 3);
        assert_eq!(panel.scroll_offset, offset, "should not auto_scroll when disabled");
    }

    // ANSI parsing tests (P10)

    #[test]
    fn ansi_parse_plain_text() {
        let text = ExpertPanelDisplay::parse_ansi("hello world");
        assert_eq!(text.lines.len(), 1, "plain text should produce one line");
        let content: String = text.lines[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(content, "hello world", "plain text content should be preserved");
    }

    #[test]
    fn ansi_parse_colored_text() {
        // \x1b[31m = red foreground, \x1b[0m = reset
        let input = "\x1b[31mred text\x1b[0m normal";
        let text = ExpertPanelDisplay::parse_ansi(input);
        assert!(!text.lines.is_empty(), "colored text should produce lines");
        // Verify the text contains "red text" and "normal" somewhere in spans
        let full: String = text.lines.iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(full.contains("red text"), "should contain 'red text'");
        assert!(full.contains("normal"), "should contain 'normal'");
        // Verify that at least one span has a red style applied
        let has_red = text.lines.iter()
            .flat_map(|l| l.spans.iter())
            .any(|s| s.style.fg == Some(Color::Red));
        assert!(has_red, "should have a red-styled span");
    }

    #[test]
    fn ansi_parse_malformed_does_not_panic() {
        // Malformed ANSI sequences should not cause a panic — fallback to raw text
        let malformed_inputs = [
            "\x1b[",
            "\x1b[999m",
            "\x1b[38;5;",
            "\x1b[38;2;255;0;",
            "normal \x1b[ broken",
        ];
        for input in &malformed_inputs {
            let text = ExpertPanelDisplay::parse_ansi(input);
            assert!(!text.lines.is_empty(), "malformed input '{}' should still produce output", input);
        }
    }
}
