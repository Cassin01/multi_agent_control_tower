use ansi_to_tui::IntoText;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Span, Text},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use xxhash_rust::xxh3::xxh3_64;

/// Safety margin subtracted from inner width when setting tmux PTY size.
/// Prevents edge-case line wrapping at width boundaries.
const PREVIEW_WIDTH_MARGIN: u16 = 1;

/// Safety margin subtracted from inner height when setting tmux PTY size.
const PREVIEW_HEIGHT_MARGIN: u16 = 0;

pub struct ExpertPanelDisplay {
    expert_id: Option<u32>,
    expert_name: Option<String>,
    content: Text<'static>,
    raw_line_count: usize,
    scroll_offset: u16,
    visible: bool,
    focused: bool,
    auto_scroll: bool,
    is_scrolling: bool,
    last_render_size: (u16, u16),
    content_hash: u64,
    cached_visual_line_count: usize,
    cached_display_width: usize,
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
            visible: true,
            focused: false,
            auto_scroll: true,
            is_scrolling: false,
            last_render_size: (0, 0),
            content_hash: 0,
            cached_visual_line_count: 0,
            cached_display_width: 0,
        }
    }

    pub fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    #[allow(dead_code)]
    pub fn show(&mut self) {
        self.visible = true;
    }

    #[allow(dead_code)]
    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    #[allow(dead_code)]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn expert_id(&self) -> Option<u32> {
        self.expert_id
    }

    #[cfg(test)]
    pub fn last_render_size(&self) -> (u16, u16) {
        self.last_render_size
    }

    /// Returns the effective dimensions for tmux PTY synchronization.
    ///
    /// The preview size is smaller than the render inner size by
    /// PREVIEW_WIDTH_MARGIN columns and PREVIEW_HEIGHT_MARGIN rows.
    /// This ensures that tmux output (formatted at preview_width)
    /// fits within the display area without triggering ratatui's Wrap.
    ///
    /// Size chain:
    ///   Terminal → Layout margin(1) → Panel Rect → Borders::ALL
    ///   → inner size (last_render_size)
    ///   → preview size (inner - margins)
    ///   → tmux resize-pane
    pub fn preview_size(&self) -> (u16, u16) {
        let (w, h) = self.last_render_size;
        (
            w.saturating_sub(PREVIEW_WIDTH_MARGIN),
            h.saturating_sub(PREVIEW_HEIGHT_MARGIN),
        )
    }

    pub fn is_scrolling(&self) -> bool {
        self.is_scrolling
    }

    pub fn enter_scroll_mode(&mut self, raw: &str) {
        self.is_scrolling = true;
        self.auto_scroll = false;
        self.content_hash = 0;
        self.cached_visual_line_count = 0;
        self.cached_display_width = 0;
        let line_count = raw.lines().count();
        let text = Self::parse_ansi(raw);
        self.content = text;
        self.raw_line_count = line_count;
        self.scroll_offset = u16::MAX;
    }

    pub fn exit_scroll_mode(&mut self) {
        self.is_scrolling = false;
        self.content = Text::default();
        self.raw_line_count = 0;
        self.content_hash = 0;
        self.cached_visual_line_count = 0;
        self.cached_display_width = 0;
        self.auto_scroll = true;
    }

    pub fn set_expert(&mut self, id: u32, name: String) {
        if self.expert_id != Some(id) {
            if self.is_scrolling {
                self.exit_scroll_mode();
            }
            self.scroll_offset = 0;
            self.content = Text::default();
            self.raw_line_count = 0;
            self.auto_scroll = true;
            self.content_hash = 0;
            self.cached_visual_line_count = 0;
            self.cached_display_width = 0;
        }
        self.expert_id = Some(id);
        self.expert_name = Some(name);
    }

    pub fn set_content(&mut self, text: Text<'static>, line_count: usize) {
        self.content = text;
        self.raw_line_count = line_count;
        self.cached_visual_line_count = 0;
        self.cached_display_width = 0;
        if self.auto_scroll && line_count > 0 {
            self.scroll_offset = line_count.saturating_sub(1) as u16;
        }
    }

    /// Update content only if the raw pane capture has changed (xxh3 hash comparison).
    /// Returns `true` if content was updated, `false` if skipped (unchanged).
    pub fn try_set_content(&mut self, raw: &str) -> bool {
        if self.is_scrolling {
            return false;
        }
        let hash = xxh3_64(raw.as_bytes());
        if hash == self.content_hash {
            return false;
        }
        self.content_hash = hash;
        let line_count = raw.lines().count();
        let text = Self::parse_ansi(raw);
        self.set_content(text, line_count);
        true
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
        self.auto_scroll = false;
    }

    pub fn scroll_down(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_add(1);
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
        self.auto_scroll = false;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = u16::MAX;
        self.auto_scroll = true;
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let title = match (&self.expert_name, self.expert_id) {
            (Some(name), Some(id)) => format!("{} (Expert{})", name, id),
            _ => " Expert Panel (no expert selected) ".to_string(),
        };

        let border_color = if self.focused {
            Color::Cyan
        } else {
            Color::DarkGray
        };

        let inner_width = area.width.saturating_sub(2);
        let inner_height = area.height.saturating_sub(2);
        self.last_render_size = (inner_width, inner_height);

        let visible_height = inner_height as usize;
        let display_width = inner_width as usize;
        let visual_line_count =
            if display_width != self.cached_display_width || self.cached_display_width == 0 {
                let count = if display_width > 0 {
                    self.content
                        .lines
                        .iter()
                        .map(|line| {
                            let w = line.width();
                            if w == 0 {
                                1
                            } else {
                                w.div_ceil(display_width)
                            }
                        })
                        .sum()
                } else {
                    self.raw_line_count
                };
                self.cached_visual_line_count = count;
                self.cached_display_width = display_width;
                count
            } else {
                self.cached_visual_line_count
            };

        let max_scroll = visual_line_count.saturating_sub(visible_height) as u16;
        if self.auto_scroll {
            self.scroll_offset = max_scroll;
        } else {
            self.scroll_offset = self.scroll_offset.min(max_scroll);
        }
        if self.scroll_offset >= max_scroll && !self.is_scrolling {
            self.auto_scroll = true;
        }

        let history_indicator = if self.is_scrolling {
            " [SCROLL MODE]"
        } else {
            ""
        };
        let scroll_indicator = if !self.auto_scroll {
            format!(" [{}/{}]", self.scroll_offset + 1, visual_line_count)
        } else {
            String::new()
        };

        let block = Block::default()
            .title(Span::styled(
                format!("{}{}{} ", title, history_indicator, scroll_indicator),
                Style::default()
                    .fg(border_color)
                    .add_modifier(Modifier::BOLD),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color));

        let paragraph = Paragraph::new(self.content.clone())
            .block(block)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, area);
    }

    /// Parse raw ANSI-escaped string into styled `Text`.
    /// Falls back to plain `Text::raw()` on parse error (P10).
    pub fn parse_ansi(raw: &str) -> Text<'static> {
        raw.into_text()
            .unwrap_or_else(|_| Text::raw(raw.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_starts_visible() {
        let panel = ExpertPanelDisplay::new();
        assert!(panel.is_visible(), "panel should start visible");
    }

    #[test]
    fn toggle_hides_visible_panel() {
        let mut panel = ExpertPanelDisplay::new();
        panel.toggle();
        assert!(!panel.is_visible(), "toggle should hide visible panel");
    }

    #[test]
    fn toggle_twice_returns_to_visible() {
        let mut panel = ExpertPanelDisplay::new();
        panel.toggle();
        panel.toggle();
        assert!(panel.is_visible(), "toggle twice should return to visible");
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
        assert_eq!(
            panel.expert_id(),
            Some(42),
            "expert_id should return set value"
        );
    }

    #[test]
    fn set_expert_different_resets_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.scroll_down();
        panel.scroll_down();
        assert!(panel.scroll_offset > 0, "scroll should have advanced");

        panel.set_expert(2, "Bob".to_string());
        assert_eq!(
            panel.scroll_offset, 0,
            "changing expert should reset scroll to 0"
        );
        assert_eq!(
            panel.raw_line_count, 0,
            "changing expert should clear content"
        );
    }

    #[test]
    fn set_expert_same_preserves_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.scroll_down();
        panel.scroll_down();
        let offset = panel.scroll_offset;

        panel.set_expert(1, "Alice".to_string());
        assert_eq!(
            panel.scroll_offset, offset,
            "same expert should preserve scroll"
        );
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
        assert_eq!(
            panel.scroll_offset, 1,
            "scroll_down should increment offset"
        );
    }

    #[test]
    fn scroll_to_bottom_enables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("line1\nline2\nline3"), 3);
        panel.scroll_up();
        assert!(!panel.auto_scroll, "scroll_up should disable auto_scroll");

        panel.scroll_to_bottom();
        assert!(
            panel.auto_scroll,
            "scroll_to_bottom should enable auto_scroll"
        );
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
        assert_eq!(
            panel.scroll_offset, offset,
            "should not auto_scroll when disabled"
        );
    }

    #[test]
    fn scroll_down_to_bottom_re_enables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("a\nb\nc\nd\ne\nf\ng\nh\ni\nj"), 10);
        panel.scroll_up();
        assert!(!panel.auto_scroll, "scroll_up should disable auto_scroll");

        // visible_height = 7 - 2 = 5, max_scroll = 10 - 5 = 5
        // Scroll back to bottom
        for _ in 0..20 {
            panel.scroll_down();
        }
        // After render, if offset >= max_scroll, auto_scroll should re-enable
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                panel.render(frame, frame.area());
            })
            .unwrap();

        assert!(
            panel.auto_scroll,
            "scroll_down to bottom should re-enable auto_scroll after render"
        );
    }

    #[test]
    fn scroll_offset_above_bottom_does_not_re_enable_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        // 20 lines of content, visible_height=5 (area 7 - 2 borders), max_scroll=15
        let content = (0..20)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        panel.set_content(Text::raw(content), 20);
        // scroll_up multiple times to move well above bottom
        for _ in 0..5 {
            panel.scroll_up();
        }
        assert!(!panel.auto_scroll, "scroll_up should disable auto_scroll");

        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(40, 7);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                panel.render(frame, frame.area());
            })
            .unwrap();

        // offset = 19 - 5 = 14, max_scroll = 15, so offset < max_scroll
        assert!(
            !panel.auto_scroll,
            "scroll_offset above bottom should not re-enable auto_scroll"
        );
    }

    #[test]
    fn scroll_to_top_sets_offset_to_zero() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("a\nb\nc\nd\ne"), 5);
        assert!(panel.scroll_offset > 0, "auto_scroll should set offset > 0");

        panel.scroll_to_top();
        assert_eq!(
            panel.scroll_offset, 0,
            "scroll_to_top should set offset to 0"
        );
    }

    #[test]
    fn scroll_to_top_disables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        assert!(panel.auto_scroll, "auto_scroll should start enabled");

        panel.scroll_to_top();
        assert!(
            !panel.auto_scroll,
            "scroll_to_top should disable auto_scroll"
        );
    }

    #[test]
    fn last_render_size_starts_at_zero() {
        let panel = ExpertPanelDisplay::new();
        assert_eq!(
            panel.last_render_size(),
            (0, 0),
            "last_render_size should start at (0, 0)"
        );
    }

    #[test]
    fn render_updates_last_render_size() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("hello"), 1);

        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                panel.render(frame, frame.area());
            })
            .unwrap();

        // inner dimensions = (40 - 2, 10 - 2) = (38, 8)
        assert_eq!(
            panel.last_render_size(),
            (38, 8),
            "render should store inner dimensions (area - borders)"
        );
    }

    // Content hash detection tests (Phase 2, Issue 3.3)

    #[test]
    fn try_set_content_returns_true_on_new_content() {
        let mut panel = ExpertPanelDisplay::new();
        assert!(
            panel.try_set_content("hello"),
            "first set should return true"
        );
    }

    #[test]
    fn try_set_content_returns_false_on_same_content() {
        let mut panel = ExpertPanelDisplay::new();
        panel.try_set_content("hello");
        assert!(
            !panel.try_set_content("hello"),
            "same content should return false"
        );
    }

    #[test]
    fn try_set_content_returns_true_on_different_content() {
        let mut panel = ExpertPanelDisplay::new();
        panel.try_set_content("hello");
        assert!(
            panel.try_set_content("world"),
            "different content should return true"
        );
    }

    #[test]
    fn try_set_content_updates_line_count() {
        let mut panel = ExpertPanelDisplay::new();
        panel.try_set_content("a\nb\nc");
        assert_eq!(
            panel.raw_line_count, 3,
            "try_set_content should update line count"
        );
    }

    #[test]
    fn try_set_content_respects_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.try_set_content("a\nb\nc\nd\ne");
        assert_eq!(
            panel.scroll_offset, 4,
            "try_set_content should auto-scroll to bottom"
        );
    }

    #[test]
    fn set_expert_resets_content_hash() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.try_set_content("hello");
        assert!(
            !panel.try_set_content("hello"),
            "same content should be skipped"
        );

        panel.set_expert(2, "Bob".to_string());
        assert!(
            panel.try_set_content("hello"),
            "after expert change, same content should be accepted"
        );
    }

    #[test]
    fn try_set_content_skips_ansi_parsing_when_unchanged() {
        let mut panel = ExpertPanelDisplay::new();
        let ansi_content = "\x1b[31mred text\x1b[0m normal";
        assert!(
            panel.try_set_content(ansi_content),
            "first set should parse"
        );
        assert!(
            !panel.try_set_content(ansi_content),
            "second set should skip parsing"
        );
    }

    // Scroll indicator tests (Phase 3, Issue 3.10)

    fn render_to_string(panel: &mut ExpertPanelDisplay, width: u16, height: u16) -> String {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                panel.render(frame, frame.area());
            })
            .unwrap();

        let buffer = terminal.backend().buffer().clone();
        let mut result = String::new();
        for y in 0..buffer.area.height {
            for x in 0..buffer.area.width {
                let cell = &buffer[(x, y)];
                result.push_str(cell.symbol());
            }
            result.push('\n');
        }
        result
    }

    #[test]
    fn render_shows_scroll_indicator_when_not_auto_scrolling() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        let content = (0..30)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        panel.set_content(Text::raw(content), 30);
        // Scroll up enough to stay below max_scroll after render clamping.
        // visible_height=8, visual_line_count=30, max_scroll=22.
        // set_content auto-scrolls to offset 29; we need offset < 22.
        for _ in 0..10 {
            panel.scroll_up();
        }
        assert!(!panel.auto_scroll, "scroll_up should disable auto_scroll");

        let rendered = render_to_string(&mut panel, 60, 10);
        assert!(
            rendered.contains("/"),
            "render: should show scroll position indicator when auto_scroll is disabled, got title: {}",
            rendered.lines().next().unwrap_or("")
        );
    }

    #[test]
    fn render_no_scroll_indicator_when_auto_scrolling() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.set_content(Text::raw("line1\nline2"), 2);
        assert!(panel.auto_scroll, "auto_scroll should be enabled");

        let rendered = render_to_string(&mut panel, 60, 10);
        assert!(
            !rendered.contains("/"),
            "render: should NOT show scroll position indicator when auto_scroll is enabled"
        );
    }

    // Preview size tests (Preview Width Synchronization)

    #[test]
    fn preview_size_subtracts_margin_from_render_size() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("hello"), 1);

        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        // inner = (40-2, 10-2) = (38, 8)
        assert_eq!(
            panel.last_render_size(),
            (38, 8),
            "preview_size: last_render_size should be inner dimensions"
        );
        // preview = (38-1, 8-0) = (37, 8)
        assert_eq!(
            panel.preview_size(),
            (37, 8),
            "preview_size: should subtract margins from dimensions"
        );
    }

    #[test]
    fn preview_size_saturates_at_zero() {
        let panel = ExpertPanelDisplay::new();
        // last_render_size = (0, 0) by default
        assert_eq!(
            panel.preview_size(),
            (0, 0),
            "preview_size: should saturate at zero, not underflow"
        );
    }

    #[test]
    fn preview_size_with_narrow_terminal() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("x"), 1);

        use ratatui::{backend::TestBackend, Terminal};
        // Minimum viable: 3 wide (border + 1 content col + border)
        let backend = TestBackend::new(3, 5);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        // inner = (3-2, 5-2) = (1, 3)
        // preview = (1-1, 3-0) = (0, 3)
        assert_eq!(
            panel.preview_size(),
            (0, 3),
            "preview_size: narrow terminal should saturate at zero with width margin"
        );
    }

    // Scroll mode tests (full history scrollback)

    #[test]
    fn panel_starts_not_scrolling() {
        let panel = ExpertPanelDisplay::new();
        assert!(
            !panel.is_scrolling(),
            "panel should start not in scroll mode"
        );
    }

    #[test]
    fn enter_scroll_mode_sets_flag() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("line1\nline2");
        assert!(
            panel.is_scrolling(),
            "enter_scroll_mode: should set is_scrolling to true"
        );
    }

    #[test]
    fn enter_scroll_mode_loads_content() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("line1\nline2\nline3");
        assert_eq!(
            panel.raw_line_count, 3,
            "enter_scroll_mode: should load content with correct line count"
        );
    }

    #[test]
    fn enter_scroll_mode_positions_at_bottom() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("a\nb\nc\nd\ne");
        assert_eq!(
            panel.scroll_offset,
            u16::MAX,
            "enter_scroll_mode: should set scroll_offset to u16::MAX (sentinel for render clamping)"
        );
    }

    #[test]
    fn enter_scroll_mode_disables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        assert!(panel.auto_scroll, "auto_scroll should start enabled");
        panel.enter_scroll_mode("a\nb");
        assert!(
            !panel.auto_scroll,
            "enter_scroll_mode: should disable auto_scroll"
        );
    }

    #[test]
    fn exit_scroll_mode_clears_flag() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("a\nb");
        panel.exit_scroll_mode();
        assert!(
            !panel.is_scrolling(),
            "exit_scroll_mode: should clear is_scrolling flag"
        );
    }

    #[test]
    fn exit_scroll_mode_resets_hash() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("a\nb");
        panel.exit_scroll_mode();
        assert_eq!(
            panel.content_hash, 0,
            "exit_scroll_mode: should reset content hash so next poll refreshes"
        );
    }

    #[test]
    fn exit_scroll_mode_enables_auto_scroll() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("a\nb");
        assert!(!panel.auto_scroll, "should be disabled after enter");
        panel.exit_scroll_mode();
        assert!(
            panel.auto_scroll,
            "exit_scroll_mode: should re-enable auto_scroll"
        );
    }

    #[test]
    fn try_set_content_noop_when_scrolling() {
        let mut panel = ExpertPanelDisplay::new();
        panel.enter_scroll_mode("history content");
        let result = panel.try_set_content("new live content");
        assert!(
            !result,
            "try_set_content: should return false when in scroll mode"
        );
        assert!(
            panel.is_scrolling(),
            "try_set_content: should not exit scroll mode"
        );
    }

    #[test]
    fn set_expert_exits_scroll_mode() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.enter_scroll_mode("history");
        assert!(panel.is_scrolling(), "should be scrolling");

        panel.set_expert(2, "Bob".to_string());
        assert!(
            !panel.is_scrolling(),
            "set_expert: changing expert should exit scroll mode"
        );
    }

    #[test]
    fn set_expert_same_id_preserves_scroll_mode() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.enter_scroll_mode("history");

        panel.set_expert(1, "Alice".to_string());
        assert!(
            panel.is_scrolling(),
            "set_expert: same expert should preserve scroll mode"
        );
    }

    #[test]
    fn render_shows_history_indicator_when_scrolling() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.enter_scroll_mode("line1\nline2");

        let rendered = render_to_string(&mut panel, 80, 10);
        assert!(
            rendered.contains("SCROLL MODE"),
            "render: should show [SCROLL MODE] indicator when in scroll mode, got title: {}",
            rendered.lines().next().unwrap_or("")
        );
    }

    // ANSI parsing tests (P10)

    #[test]
    fn ansi_parse_plain_text() {
        let text = ExpertPanelDisplay::parse_ansi("hello world");
        assert_eq!(text.lines.len(), 1, "plain text should produce one line");
        let content: String = text.lines[0]
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(
            content, "hello world",
            "plain text content should be preserved"
        );
    }

    #[test]
    fn ansi_parse_colored_text() {
        // \x1b[31m = red foreground, \x1b[0m = reset
        let input = "\x1b[31mred text\x1b[0m normal";
        let text = ExpertPanelDisplay::parse_ansi(input);
        assert!(!text.lines.is_empty(), "colored text should produce lines");
        // Verify the text contains "red text" and "normal" somewhere in spans
        let full: String = text
            .lines
            .iter()
            .flat_map(|l| l.spans.iter())
            .map(|s| s.content.as_ref())
            .collect();
        assert!(full.contains("red text"), "should contain 'red text'");
        assert!(full.contains("normal"), "should contain 'normal'");
        // Verify that at least one span has a red style applied
        let has_red = text
            .lines
            .iter()
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
            assert!(
                !text.lines.is_empty(),
                "malformed input '{}' should still produce output",
                input
            );
        }
    }

    // Cache invalidation tests (Phase 1, Task 1.1)

    #[test]
    fn visual_line_count_cache_invalidated_on_content_change() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("line1\nline2\nline3"), 3);

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        let first_count = panel.cached_visual_line_count;
        assert!(first_count > 0, "cache should be populated after render");

        // Change content — cache should invalidate
        panel.set_content(Text::raw("a\nb\nc\nd\ne"), 5);
        assert_eq!(
            panel.cached_visual_line_count, 0,
            "cache should be invalidated after content change"
        );

        // Re-render to repopulate cache
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        assert_eq!(
            panel.cached_visual_line_count, 5,
            "cache should reflect new content after re-render"
        );
    }

    #[test]
    fn visual_line_count_cache_invalidated_on_width_change() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("line1\nline2\nline3"), 3);

        // Render at width 40
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        let cached_width = panel.cached_display_width;
        assert!(cached_width > 0, "cached width should be set after render");

        // Render at different width — cache should recompute
        let backend = TestBackend::new(80, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        assert_ne!(
            panel.cached_display_width, cached_width,
            "cached width should update after width change"
        );
    }

    #[test]
    fn visual_line_count_cache_reused_when_unchanged() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("line1\nline2\nline3"), 3);

        // Render twice at same width
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        let count_after_first = panel.cached_visual_line_count;
        let width_after_first = panel.cached_display_width;

        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();

        assert_eq!(
            panel.cached_visual_line_count, count_after_first,
            "cache should be reused when width is unchanged"
        );
        assert_eq!(
            panel.cached_display_width, width_after_first,
            "cached width should remain unchanged"
        );
    }

    #[test]
    fn scroll_state_preserved_after_cache_invalidation() {
        let mut panel = ExpertPanelDisplay::new();
        panel.set_content(Text::raw("a\nb\nc\nd\ne"), 5);
        panel.scroll_up();

        // Trigger cache invalidation via enter/exit scroll mode
        panel.enter_scroll_mode("x\ny\nz");
        assert_eq!(
            panel.cached_visual_line_count, 0,
            "cache should be invalidated on enter_scroll_mode"
        );
        assert!(panel.is_scrolling, "should be in scroll mode");

        panel.exit_scroll_mode();
        assert_eq!(
            panel.cached_visual_line_count, 0,
            "cache should be invalidated on exit_scroll_mode"
        );

        // Verify scroll state was reset as expected by exit_scroll_mode
        assert!(
            panel.auto_scroll,
            "auto_scroll should be re-enabled after exit_scroll_mode"
        );
        assert!(
            !panel.is_scrolling,
            "should not be in scroll mode after exit"
        );

        // set_expert with same id should preserve scroll state
        let mut panel2 = ExpertPanelDisplay::new();
        panel2.set_expert(1, "Alice".to_string());
        panel2.set_content(Text::raw("a\nb\nc\nd\ne"), 5);
        panel2.scroll_up();
        let offset = panel2.scroll_offset;
        let auto = panel2.auto_scroll;

        panel2.set_expert(1, "Alice".to_string());
        assert_eq!(
            panel2.scroll_offset, offset,
            "same expert should preserve scroll offset"
        );
        assert_eq!(
            panel2.auto_scroll, auto,
            "same expert should preserve auto_scroll"
        );
    }

    // Render idempotency tests (Phase 1, Task 2.1)

    #[test]
    fn render_idempotent_with_cache() {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;

        let mut panel = ExpertPanelDisplay::new();
        panel.set_expert(1, "Alice".to_string());
        panel.set_content(Text::raw("line1\nline2\nline3\nline4\nline5"), 5);

        // First render
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();
        let buffer1 = terminal.backend().buffer().clone();
        let cache_count_1 = panel.cached_visual_line_count;
        let cache_width_1 = panel.cached_display_width;

        // Second render with same state — should produce identical output
        let backend = TestBackend::new(40, 10);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();
        let buffer2 = terminal.backend().buffer().clone();
        let cache_count_2 = panel.cached_visual_line_count;
        let cache_width_2 = panel.cached_display_width;

        assert_eq!(
            cache_count_1, cache_count_2,
            "cached visual line count should be identical across renders"
        );
        assert_eq!(
            cache_width_1, cache_width_2,
            "cached display width should be identical across renders"
        );

        // Compare rendered buffers
        for y in 0..buffer1.area.height {
            for x in 0..buffer1.area.width {
                assert_eq!(
                    buffer1[(x, y)].symbol(),
                    buffer2[(x, y)].symbol(),
                    "render output should be identical at ({}, {})",
                    x,
                    y
                );
            }
        }
    }

    // Hash equivalence tests (Phase 3, Task 9.1)

    #[test]
    fn xxh3_identical_inputs_produce_identical_hashes() {
        let mut panel = ExpertPanelDisplay::new();
        panel.try_set_content("test content");
        let hash1 = panel.content_hash;

        // Reset and feed the same content
        panel.set_expert(1, "A".to_string());
        panel.try_set_content("test content");
        let hash2 = panel.content_hash;

        assert_eq!(
            hash1, hash2,
            "xxh3: identical inputs must produce identical hashes"
        );
    }

    #[test]
    fn xxh3_distinct_inputs_produce_distinct_hashes() {
        let mut panel = ExpertPanelDisplay::new();
        panel.try_set_content("content A");
        let hash_a = panel.content_hash;

        panel.set_expert(1, "X".to_string());
        panel.try_set_content("content B");
        let hash_b = panel.content_hash;

        assert_ne!(
            hash_a, hash_b,
            "xxh3: distinct inputs must produce distinct hashes"
        );
    }

    #[test]
    fn xxh3_change_detection_matches_sha256_behavior() {
        let mut panel = ExpertPanelDisplay::new();

        // First content: accepted
        assert!(
            panel.try_set_content("hello"),
            "xxh3: first content should be accepted"
        );
        // Same content: rejected
        assert!(
            !panel.try_set_content("hello"),
            "xxh3: same content should be rejected"
        );
        // Different content: accepted
        assert!(
            panel.try_set_content("world"),
            "xxh3: different content should be accepted"
        );
        // Same again: rejected
        assert!(
            !panel.try_set_content("world"),
            "xxh3: same content should be rejected again"
        );
    }

    #[test]
    fn xxh3_empty_content_handled_correctly() {
        let mut panel = ExpertPanelDisplay::new();
        assert!(
            panel.try_set_content(""),
            "xxh3: empty content should be accepted on first call"
        );
        assert!(
            !panel.try_set_content(""),
            "xxh3: empty content should be rejected on second call"
        );
        assert!(
            panel.try_set_content("non-empty"),
            "xxh3: non-empty should differ from empty"
        );
    }

    // Visual-bottom scroll tests (wrapping content fix)

    fn make_wrapping_content(lines: usize, chars_per_line: usize) -> String {
        (0..lines)
            .map(|i| {
                let prefix = format!("L{:03}:", i);
                let pad = chars_per_line.saturating_sub(prefix.len());
                format!("{}{}", prefix, "x".repeat(pad))
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn render_panel(panel: &mut ExpertPanelDisplay, width: u16, height: u16) {
        use ratatui::backend::TestBackend;
        use ratatui::Terminal;
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| panel.render(frame, frame.area()))
            .unwrap();
    }

    #[test]
    fn auto_scroll_positions_at_visual_bottom_with_wrapping() {
        let mut panel = ExpertPanelDisplay::new();
        let content = make_wrapping_content(5, 200);
        let line_count = content.lines().count();
        let text = ExpertPanelDisplay::parse_ansi(&content);
        panel.set_content(text, line_count);

        // Render at width 40 → inner_width=38, each 200-char line wraps to ceil(200/38)=6 visual lines
        // 5 lines × 6 = 30 visual lines, visible_height=8, max_scroll=22
        render_panel(&mut panel, 40, 10);

        let display_width = 38_usize;
        let expected_visual: usize = content
            .lines()
            .map(|l| {
                let w = l.len();
                if w == 0 {
                    1
                } else {
                    w.div_ceil(display_width)
                }
            })
            .sum();
        let visible_height = 8_usize;
        let expected_max_scroll = expected_visual.saturating_sub(visible_height) as u16;

        assert_eq!(
            panel.scroll_offset, expected_max_scroll,
            "auto_scroll should position at visual bottom (max_scroll), not raw line count"
        );
    }

    #[test]
    fn scroll_to_bottom_reaches_visual_bottom_after_render() {
        let mut panel = ExpertPanelDisplay::new();
        let content = make_wrapping_content(5, 200);
        panel.enter_scroll_mode(&content);
        panel.scroll_to_top();

        render_panel(&mut panel, 40, 10);
        panel.scroll_to_bottom();
        render_panel(&mut panel, 40, 10);

        let display_width = 38_usize;
        let expected_visual: usize = content
            .lines()
            .map(|l| {
                let w = l.len();
                if w == 0 {
                    1
                } else {
                    w.div_ceil(display_width)
                }
            })
            .sum();
        let visible_height = 8_usize;
        let expected_max_scroll = expected_visual.saturating_sub(visible_height) as u16;

        assert_eq!(
            panel.scroll_offset, expected_max_scroll,
            "scroll_to_bottom should reach visual bottom after render"
        );
    }

    #[test]
    fn enter_scroll_mode_reaches_visual_bottom_after_render() {
        let mut panel = ExpertPanelDisplay::new();
        let content = make_wrapping_content(5, 200);
        panel.enter_scroll_mode(&content);

        // Before render, offset is u16::MAX (sentinel)
        assert_eq!(
            panel.scroll_offset,
            u16::MAX,
            "enter_scroll_mode: should set sentinel before render"
        );

        render_panel(&mut panel, 40, 10);

        let display_width = 38_usize;
        let expected_visual: usize = content
            .lines()
            .map(|l| {
                let w = l.len();
                if w == 0 {
                    1
                } else {
                    w.div_ceil(display_width)
                }
            })
            .sum();
        let visible_height = 8_usize;
        let expected_max_scroll = expected_visual.saturating_sub(visible_height) as u16;

        assert_eq!(
            panel.scroll_offset, expected_max_scroll,
            "enter_scroll_mode: after render, offset should clamp to visual bottom"
        );
    }

    #[test]
    fn auto_scroll_not_re_enabled_during_scroll_mode() {
        let mut panel = ExpertPanelDisplay::new();
        let content = make_wrapping_content(5, 200);
        panel.enter_scroll_mode(&content);

        assert!(
            !panel.auto_scroll,
            "auto_scroll should be disabled in scroll mode"
        );

        // Render — offset is u16::MAX, will be clamped to max_scroll
        render_panel(&mut panel, 40, 10);

        assert!(
            !panel.auto_scroll,
            "auto_scroll should NOT be re-enabled during scroll mode even when at bottom"
        );
        assert!(panel.is_scrolling, "should remain in scroll mode");
    }
}
