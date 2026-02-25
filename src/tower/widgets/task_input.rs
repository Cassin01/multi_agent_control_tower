use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};
use unicode_width::UnicodeWidthChar;

pub struct TaskInput {
    content: String,
    cursor_position: usize,
    focused: bool,
    scroll_offset: usize,
}

impl TaskInput {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor_position: 0,
            focused: false,
            scroll_offset: 0,
        }
    }

    /// Convert character-based cursor position to byte index
    fn cursor_byte_index(&self) -> usize {
        self.content
            .char_indices()
            .nth(self.cursor_position)
            .map(|(i, _)| i)
            .unwrap_or(self.content.len())
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    #[allow(dead_code)]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    #[allow(dead_code)]
    pub fn set_content(&mut self, content: String) {
        self.cursor_position = content.chars().count();
        self.content = content;
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor_position = 0;
        self.scroll_offset = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        let byte_idx = self.cursor_byte_index();
        self.content.insert(byte_idx, c);
        self.cursor_position += 1;
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            let byte_idx = self.cursor_byte_index();
            self.content.remove(byte_idx);
        }
    }

    pub fn delete_forward(&mut self) {
        let char_count = self.content.chars().count();
        if self.cursor_position < char_count {
            let byte_idx = self.cursor_byte_index();
            self.content.remove(byte_idx);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        let char_count = self.content.chars().count();
        if self.cursor_position < char_count {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.content.chars().count();
    }

    pub fn move_cursor_line_start(&mut self) {
        let chars: Vec<char> = self.content.chars().collect();
        let mut pos = self.cursor_position;
        while pos > 0 && chars[pos - 1] != '\n' {
            pos -= 1;
        }
        self.cursor_position = pos;
    }

    pub fn move_cursor_line_end(&mut self) {
        let chars: Vec<char> = self.content.chars().collect();
        let len = chars.len();
        let mut pos = self.cursor_position;
        while pos < len && chars[pos] != '\n' {
            pos += 1;
        }
        self.cursor_position = pos;
    }

    pub fn move_cursor_up(&mut self) {
        let chars: Vec<char> = self.content.chars().collect();
        // Find start of current line
        let mut line_start = self.cursor_position;
        while line_start > 0 && chars[line_start - 1] != '\n' {
            line_start -= 1;
        }
        let col = self.cursor_position - line_start;

        if line_start == 0 {
            // Already on the first line — go to position 0
            self.cursor_position = 0;
            return;
        }

        // Find start of previous line (line_start - 1 is the '\n' before current line)
        let prev_line_end = line_start - 1;
        let mut prev_line_start = prev_line_end;
        while prev_line_start > 0 && chars[prev_line_start - 1] != '\n' {
            prev_line_start -= 1;
        }
        let prev_line_len = prev_line_end - prev_line_start;

        self.cursor_position = prev_line_start + col.min(prev_line_len);
    }

    pub fn move_cursor_down(&mut self) {
        let chars: Vec<char> = self.content.chars().collect();
        let len = chars.len();
        // Find start of current line
        let mut line_start = self.cursor_position;
        while line_start > 0 && chars[line_start - 1] != '\n' {
            line_start -= 1;
        }
        let col = self.cursor_position - line_start;

        // Find end of current line
        let mut line_end = self.cursor_position;
        while line_end < len && chars[line_end] != '\n' {
            line_end += 1;
        }

        if line_end == len {
            // Already on the last line — go to end of content
            self.cursor_position = len;
            return;
        }

        // line_end is the '\n', next line starts at line_end + 1
        let next_line_start = line_end + 1;
        let mut next_line_end = next_line_start;
        while next_line_end < len && chars[next_line_end] != '\n' {
            next_line_end += 1;
        }
        let next_line_len = next_line_end - next_line_start;

        self.cursor_position = next_line_start + col.min(next_line_len);
    }

    pub fn kill_line(&mut self) {
        let chars: Vec<char> = self.content.chars().collect();
        let len = chars.len();
        let mut line_end = self.cursor_position;
        while line_end < len && chars[line_end] != '\n' {
            line_end += 1;
        }
        if line_end == self.cursor_position && line_end < len {
            let byte_idx = self.cursor_byte_index();
            self.content.remove(byte_idx);
        } else if line_end > self.cursor_position {
            let start_byte = self.cursor_byte_index();
            let end_byte: usize = chars[..line_end].iter().map(|c| c.len_utf8()).sum();
            self.content.replace_range(start_byte..end_byte, "");
        }
    }

    pub fn unix_line_discard(&mut self) {
        let chars: Vec<char> = self.content.chars().collect();
        let mut line_start = self.cursor_position;
        while line_start > 0 && chars[line_start - 1] != '\n' {
            line_start -= 1;
        }
        if line_start == self.cursor_position {
            return;
        }
        let start_byte: usize = chars[..line_start].iter().map(|c| c.len_utf8()).sum();
        let end_byte = self.cursor_byte_index();
        self.content.replace_range(start_byte..end_byte, "");
        self.cursor_position = line_start;
    }

    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    /// Returns (line_index, column) of the current cursor position.
    /// Both are 0-based. Operates on character indices for Unicode safety.
    #[allow(dead_code)]
    fn cursor_line_col(&self) -> (usize, usize) {
        let mut line = 0;
        let mut col = 0;
        for (i, ch) in self.content.chars().enumerate() {
            if i == self.cursor_position {
                return (line, col);
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }
        (line, col)
    }

    /// Returns the visual line number of the cursor, accounting for line wrapping
    /// at `wrap_width` display columns. Uses character-level wrapping to approximate
    /// the ratatui `Paragraph` word-wrapper behavior.
    fn visual_cursor_line(&self, wrap_width: usize) -> usize {
        if wrap_width == 0 {
            return 0;
        }
        let mut visual_line = 0usize;
        let mut col_width = 0usize;
        for (i, ch) in self.content.chars().enumerate() {
            if i == self.cursor_position {
                return visual_line;
            }
            if ch == '\n' {
                visual_line += 1;
                col_width = 0;
            } else {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if col_width + cw > wrap_width {
                    visual_line += 1;
                    col_width = cw;
                } else {
                    col_width += cw;
                }
            }
        }
        visual_line
    }

    /// Returns (total_visual_lines, width_of_last_visual_subline) accounting for
    /// character-level line wrapping at `wrap_width` display columns.
    fn visual_line_metrics(&self, wrap_width: usize) -> (usize, usize) {
        if self.content.is_empty() {
            return (1, 0);
        }
        if wrap_width == 0 {
            return (1, 0);
        }
        let mut lines = 1usize;
        let mut col_width = 0usize;
        for ch in self.content.chars() {
            if ch == '\n' {
                lines += 1;
                col_width = 0;
            } else {
                let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
                if col_width + cw > wrap_width {
                    lines += 1;
                    col_width = cw;
                } else {
                    col_width += cw;
                }
            }
        }
        (lines, col_width)
    }

    /// Adjusts scroll_offset so the cursor's visual line is within
    /// [scroll_offset, scroll_offset + visible_height).
    /// Uses visual (wrapped) line counts so that long lines wrapping at the right
    /// edge are accounted for in scroll calculations.
    fn ensure_cursor_visible(&mut self, visible_height: usize, wrap_width: usize) {
        if visible_height == 0 {
            return;
        }
        let (text_visual_lines, last_subline_width) = self.visual_line_metrics(wrap_width);
        let eob_width = 5; // "[EOB]"
        let eob_extra = if (self.content.is_empty() && !self.focused)
            || (wrap_width > 0 && last_subline_width + eob_width > wrap_width)
        {
            1
        } else {
            0
        };
        let total_visual_lines = text_visual_lines + eob_extra;
        let max_offset = total_visual_lines.saturating_sub(visible_height);
        self.scroll_offset = self.scroll_offset.min(max_offset);

        let cursor_visual_line = self.visual_cursor_line(wrap_width);
        if cursor_visual_line < self.scroll_offset {
            self.scroll_offset = cursor_visual_line;
        } else if cursor_visual_line >= self.scroll_offset + visible_height {
            self.scroll_offset = cursor_visual_line - visible_height + 1;
        }
    }

    /// Returns the total number of logical lines in the buffer.
    /// An empty buffer has 1 line. A trailing newline adds an extra empty line.
    fn line_count(&self) -> usize {
        if self.content.is_empty() {
            return 1;
        }
        self.content.chars().filter(|&c| c == '\n').count() + 1
    }

    /// Returns the gutter width: max(2, digits in line_count) + 1 for trailing space.
    fn gutter_width(&self) -> usize {
        let count = self.line_count();
        let digits = if count == 0 {
            1
        } else {
            ((count as f64).log10().floor() as usize) + 1
        };
        digits.max(2) + 1
    }

    /// Counts how many visual lines a single logical line occupies at a given wrap width.
    fn visual_line_count_for(line: &str, wrap_width: usize) -> usize {
        if wrap_width == 0 {
            return 1;
        }
        let mut visual_lines = 1usize;
        let mut col_width = 0usize;
        for ch in line.chars() {
            let cw = UnicodeWidthChar::width(ch).unwrap_or(0);
            if col_width + cw > wrap_width {
                visual_lines += 1;
                col_width = cw;
            } else {
                col_width += cw;
            }
        }
        visual_lines
    }

    /// Builds gutter lines aligned with the visual content lines.
    fn build_gutter_lines(&self, visible_height: usize, wrap_width: usize) -> Vec<Line<'_>> {
        let gw = self.gutter_width();
        let gutter_style = Style::default().fg(Color::DarkGray);
        let mut gutter_lines: Vec<Line<'_>> = Vec::new();

        // Split content into logical lines; empty content = single empty line
        let logical_lines: Vec<&str> = if self.content.is_empty() {
            vec![""]
        } else {
            self.content.split('\n').collect()
        };

        for (i, logical_line) in logical_lines.iter().enumerate() {
            let line_num = i + 1;
            let visual_count = Self::visual_line_count_for(logical_line, wrap_width);
            // First visual line: right-aligned line number + space
            let num_str = format!("{:>width$} ", line_num, width = gw - 1);
            gutter_lines.push(Line::from(Span::styled(num_str, gutter_style)));
            // Continuation lines: blank
            for _ in 1..visual_count {
                gutter_lines.push(Line::from(Span::raw(" ".repeat(gw))));
            }
        }

        // Account for EOB possibly wrapping to a new visual line
        let (_, last_subline_width) = self.visual_line_metrics(wrap_width);
        let eob_width = 5; // "[EOB]"
        if (self.content.is_empty() && !self.focused)
            || (wrap_width > 0 && last_subline_width + eob_width > wrap_width)
        {
            // EOB wraps to its own line: blank gutter for it
            gutter_lines.push(Line::from(Span::raw(" ".repeat(gw))));
        }

        // Tilde lines for beyond-content area
        for _ in 0..visible_height {
            let tilde_str = format!("{:>width$} ", "~", width = gw - 1);
            gutter_lines.push(Line::from(Span::styled(tilde_str, gutter_style)));
        }

        gutter_lines
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, title: &str) {
        let visible_height = area.height.saturating_sub(2) as usize;
        let gutter_w = self.gutter_width() as u16;
        let content_wrap_width = area.width.saturating_sub(2 + gutter_w) as usize;

        // Minimum width guard: fall back to no-gutter if content area is too narrow
        let use_gutter = content_wrap_width > 0;
        let wrap_width = if use_gutter {
            content_wrap_width
        } else {
            area.width.saturating_sub(2) as usize
        };
        self.ensure_cursor_visible(visible_height, wrap_width);

        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        let text_style = if self.focused {
            Style::default().fg(Color::White)
        } else {
            Style::default().fg(Color::Gray)
        };

        let mut display_text = if self.content.is_empty() && !self.focused {
            vec![Line::from(Span::styled(
                "Enter task description...",
                Style::default().fg(Color::DarkGray),
            ))]
        } else if self.focused {
            let byte_idx = self.cursor_byte_index();
            let before = &self.content[..byte_idx];
            let after = &self.content[byte_idx..];

            let before_lines: Vec<&str> = before.split('\n').collect();
            let after_parts: Vec<&str> = after.split('\n').collect();

            let mut lines = Vec::new();

            // Lines before cursor line
            for line in &before_lines[..before_lines.len().saturating_sub(1)] {
                lines.push(Line::from(Span::styled(*line, text_style)));
            }

            // Cursor line: last part of before + cursor + first part of after
            let cursor_line_before = before_lines.last().unwrap_or(&"");
            let cursor_line_after = after_parts.first().unwrap_or(&"");
            lines.push(Line::from(vec![
                Span::styled(*cursor_line_before, text_style),
                Span::styled(
                    "│",
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::SLOW_BLINK),
                ),
                Span::styled(*cursor_line_after, text_style),
            ]));

            // Lines after cursor line
            for line in after_parts.iter().skip(1) {
                lines.push(Line::from(Span::styled(*line, text_style)));
            }

            lines
        } else {
            self.content
                .lines()
                .map(|line| Line::from(Span::styled(line, text_style)))
                .collect()
        };

        // Append EOB indicator: always inline with the last display line,
        // except when placeholder is shown (empty + unfocused) where it's separate.
        let eob_span = Span::styled("[EOB]", Style::default().fg(Color::DarkGray));
        if self.content.is_empty() && !self.focused {
            display_text.push(Line::from(eob_span));
        } else if let Some(last_line) = display_text.last_mut() {
            let mut spans: Vec<Span<'_>> = last_line.spans.drain(..).collect();
            spans.push(eob_span);
            *last_line = Line::from(spans);
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(title);

        if !use_gutter {
            // Fallback: single paragraph without gutter
            let paragraph = Paragraph::new(display_text)
                .block(block)
                .wrap(Wrap { trim: false })
                .scroll((self.scroll_offset as u16, 0));
            frame.render_widget(paragraph, area);
            return;
        }

        // Two-column layout: gutter | content
        let inner = block.inner(area);
        frame.render_widget(block, area);

        let gutter_area = Rect {
            x: inner.x,
            y: inner.y,
            width: gutter_w,
            height: inner.height,
        };
        let content_area = Rect {
            x: inner.x + gutter_w,
            y: inner.y,
            width: inner.width.saturating_sub(gutter_w),
            height: inner.height,
        };

        let gutter_lines = self.build_gutter_lines(visible_height, wrap_width);
        let gutter_paragraph = Paragraph::new(gutter_lines).scroll((self.scroll_offset as u16, 0));
        frame.render_widget(gutter_paragraph, gutter_area);

        let content_paragraph = Paragraph::new(display_text)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset as u16, 0));
        frame.render_widget(content_paragraph, content_area);
    }
}

impl Default for TaskInput {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_input_empty_by_default() {
        let input = TaskInput::new();
        assert!(input.is_empty());
        assert_eq!(input.content(), "");
    }

    // --- scroll_offset tests ---

    #[test]
    fn scroll_offset_zero_after_new() {
        let input = TaskInput::new();
        assert_eq!(
            input.scroll_offset, 0,
            "scroll_offset: should be 0 after new()"
        );
    }

    #[test]
    fn scroll_offset_zero_after_clear() {
        let mut input = TaskInput::new();
        input.set_content("line1\nline2\nline3\nline4\nline5".to_string());
        input.scroll_offset = 3;
        input.clear();
        assert_eq!(
            input.scroll_offset, 0,
            "scroll_offset: should reset to 0 after clear()"
        );
    }

    // --- cursor_line_col tests ---

    #[test]
    fn cursor_line_col_empty_buffer() {
        let input = TaskInput::new();
        assert_eq!(
            input.cursor_line_col(),
            (0, 0),
            "cursor_line_col: empty buffer should return (0, 0)"
        );
    }

    #[test]
    fn cursor_line_col_single_line_end() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        assert_eq!(
            input.cursor_line_col(),
            (0, 5),
            "cursor_line_col: cursor at end of 'hello' should return (0, 5)"
        );
    }

    #[test]
    fn cursor_line_col_multiline_end() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        assert_eq!(
            input.cursor_line_col(),
            (1, 3),
            "cursor_line_col: cursor at end of 'abc\\ndef' should return (1, 3)"
        );
    }

    #[test]
    fn cursor_line_col_start_of_second_line() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        input.move_cursor_start();
        // Move to position 4 (start of "def")
        for _ in 0..4 {
            input.move_cursor_right();
        }
        assert_eq!(
            input.cursor_line_col(),
            (1, 0),
            "cursor_line_col: cursor at start of second line should return (1, 0)"
        );
    }

    #[test]
    fn cursor_line_col_japanese_multiline() {
        let mut input = TaskInput::new();
        input.set_content("あいう\nえお".to_string());
        assert_eq!(
            input.cursor_line_col(),
            (1, 2),
            "cursor_line_col: Japanese multiline at end should return (1, 2)"
        );
    }

    #[test]
    fn cursor_line_col_trailing_newline() {
        let mut input = TaskInput::new();
        input.set_content("abc\n".to_string());
        assert_eq!(
            input.cursor_line_col(),
            (1, 0),
            "cursor_line_col: trailing newline cursor at end should return (1, 0)"
        );
    }

    // --- line_count tests ---

    #[test]
    fn line_count_empty_buffer() {
        let input = TaskInput::new();
        assert_eq!(
            input.line_count(),
            1,
            "line_count: empty buffer should return 1"
        );
    }

    #[test]
    fn line_count_single_line() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        assert_eq!(input.line_count(), 1, "line_count: 'hello' should return 1");
    }

    #[test]
    fn line_count_two_lines() {
        let mut input = TaskInput::new();
        input.set_content("a\nb".to_string());
        assert_eq!(input.line_count(), 2, "line_count: 'a\\nb' should return 2");
    }

    #[test]
    fn line_count_trailing_newline() {
        let mut input = TaskInput::new();
        input.set_content("a\nb\n".to_string());
        assert_eq!(
            input.line_count(),
            3,
            "line_count: 'a\\nb\\n' (trailing newline) should return 3"
        );
    }

    #[test]
    fn line_count_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう\nえお".to_string());
        assert_eq!(
            input.line_count(),
            2,
            "line_count: Japanese two-line content should return 2"
        );
    }

    #[test]
    fn task_input_insert_char() {
        let mut input = TaskInput::new();
        input.insert_char('H');
        input.insert_char('i');
        assert_eq!(input.content(), "Hi");
    }

    #[test]
    fn task_input_delete_char() {
        let mut input = TaskInput::new();
        input.set_content("Hello".to_string());

        input.delete_char();
        assert_eq!(input.content(), "Hell");

        input.delete_char();
        assert_eq!(input.content(), "Hel");
    }

    #[test]
    fn task_input_delete_at_start_does_nothing() {
        let mut input = TaskInput::new();
        input.set_content("Hello".to_string());
        input.move_cursor_start();

        input.delete_char();
        assert_eq!(input.content(), "Hello");
    }

    #[test]
    fn task_input_cursor_movement() {
        let mut input = TaskInput::new();
        input.set_content("Hello".to_string());

        assert_eq!(input.cursor_position, 5);

        input.move_cursor_left();
        assert_eq!(input.cursor_position, 4);

        input.move_cursor_start();
        assert_eq!(input.cursor_position, 0);

        input.move_cursor_right();
        assert_eq!(input.cursor_position, 1);

        input.move_cursor_end();
        assert_eq!(input.cursor_position, 5);
    }

    #[test]
    fn task_input_insert_in_middle() {
        let mut input = TaskInput::new();
        input.set_content("Hllo".to_string());
        input.move_cursor_start();
        input.move_cursor_right();

        input.insert_char('e');
        assert_eq!(input.content(), "Hello");
    }

    #[test]
    fn task_input_clear() {
        let mut input = TaskInput::new();
        input.set_content("Hello".to_string());

        input.clear();
        assert!(input.is_empty());
        assert_eq!(input.cursor_position, 0);
    }

    #[test]
    fn task_input_newline() {
        let mut input = TaskInput::new();
        input.set_content("Line 1".to_string());
        input.insert_newline();
        input.insert_char('L');
        input.insert_char('i');
        input.insert_char('n');
        input.insert_char('e');
        input.insert_char(' ');
        input.insert_char('2');

        assert!(input.content().contains('\n'));
        assert!(input.content().contains("Line 2"));
    }

    #[test]
    fn task_input_focus_state() {
        let mut input = TaskInput::new();
        assert!(!input.is_focused());

        input.set_focused(true);
        assert!(input.is_focused());
    }

    #[test]
    fn task_input_is_empty_with_whitespace() {
        let mut input = TaskInput::new();
        input.set_content("   \n  \t  ".to_string());
        assert!(input.is_empty());
    }

    #[test]
    fn task_input_japanese_insert() {
        let mut input = TaskInput::new();
        input.insert_char('あ');
        input.insert_char('い');
        input.insert_char('う');
        assert_eq!(input.content(), "あいう");
        assert_eq!(input.cursor_position, 3);
    }

    #[test]
    fn task_input_japanese_delete() {
        let mut input = TaskInput::new();
        input.set_content("あいう".to_string());

        input.delete_char();
        assert_eq!(input.content(), "あい");

        input.delete_char();
        assert_eq!(input.content(), "あ");
    }

    #[test]
    fn task_input_japanese_cursor_movement() {
        let mut input = TaskInput::new();
        input.set_content("あいう".to_string());

        assert_eq!(input.cursor_position, 3);

        input.move_cursor_left();
        assert_eq!(input.cursor_position, 2);

        input.move_cursor_start();
        assert_eq!(input.cursor_position, 0);

        input.move_cursor_right();
        assert_eq!(input.cursor_position, 1);

        input.move_cursor_end();
        assert_eq!(input.cursor_position, 3);
    }

    #[test]
    fn task_input_japanese_insert_in_middle() {
        let mut input = TaskInput::new();
        input.set_content("あう".to_string());
        input.move_cursor_start();
        input.move_cursor_right();

        input.insert_char('い');
        assert_eq!(input.content(), "あいう");
    }

    #[test]
    fn task_input_mixed_ascii_japanese() {
        let mut input = TaskInput::new();
        input.set_content("Hello世界".to_string());

        assert_eq!(input.cursor_position, 7);

        input.delete_char();
        assert_eq!(input.content(), "Hello世");

        input.move_cursor_start();
        for _ in 0..5 {
            input.move_cursor_right();
        }
        input.insert_char('!');
        assert_eq!(input.content(), "Hello!世");
    }

    #[test]
    fn task_input_delete_forward_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう".to_string());
        input.move_cursor_start();

        input.delete_forward();
        assert_eq!(input.content(), "いう");

        input.delete_forward();
        assert_eq!(input.content(), "う");
    }

    // --- move_cursor_line_start tests ---

    #[test]
    fn move_cursor_line_start_single_line() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        // cursor at end (pos 5)
        input.move_cursor_line_start();
        assert_eq!(
            input.cursor_position(),
            0,
            "move_cursor_line_start: single line should go to 0"
        );
    }

    #[test]
    fn move_cursor_line_start_multiline_second_line() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // cursor at end (pos 7), on second line
        input.move_cursor_line_start();
        assert_eq!(
            input.cursor_position(),
            4,
            "move_cursor_line_start: should go to start of second line (after newline)"
        );
    }

    #[test]
    fn move_cursor_line_start_middle_of_line() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // put cursor at 'e' (pos 5)
        input.move_cursor_start();
        for _ in 0..5 {
            input.move_cursor_right();
        }
        input.move_cursor_line_start();
        assert_eq!(
            input.cursor_position(),
            4,
            "move_cursor_line_start: from middle of second line should go to start of that line"
        );
    }

    #[test]
    fn move_cursor_line_start_empty() {
        let mut input = TaskInput::new();
        input.move_cursor_line_start();
        assert_eq!(
            input.cursor_position(),
            0,
            "move_cursor_line_start: empty content should stay at 0"
        );
    }

    #[test]
    fn move_cursor_line_start_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう\nえお".to_string());
        // cursor at end (pos 6)
        input.move_cursor_line_start();
        assert_eq!(
            input.cursor_position(),
            4,
            "move_cursor_line_start: Japanese multiline should go to start of second line"
        );
    }

    // --- move_cursor_line_end tests ---

    #[test]
    fn move_cursor_line_end_single_line() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        input.move_cursor_start();
        input.move_cursor_line_end();
        assert_eq!(
            input.cursor_position(),
            5,
            "move_cursor_line_end: single line should go to end"
        );
    }

    #[test]
    fn move_cursor_line_end_first_line_of_multiline() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        input.move_cursor_start();
        input.move_cursor_line_end();
        assert_eq!(
            input.cursor_position(),
            3,
            "move_cursor_line_end: first line should stop before newline"
        );
    }

    #[test]
    fn move_cursor_line_end_second_line_of_multiline() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // put cursor at start of second line (pos 4)
        input.move_cursor_start();
        for _ in 0..4 {
            input.move_cursor_right();
        }
        input.move_cursor_line_end();
        assert_eq!(
            input.cursor_position(),
            7,
            "move_cursor_line_end: second line should go to end of content"
        );
    }

    #[test]
    fn move_cursor_line_end_empty() {
        let mut input = TaskInput::new();
        input.move_cursor_line_end();
        assert_eq!(
            input.cursor_position(),
            0,
            "move_cursor_line_end: empty content should stay at 0"
        );
    }

    #[test]
    fn move_cursor_line_end_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう\nえお".to_string());
        input.move_cursor_start();
        input.move_cursor_line_end();
        assert_eq!(
            input.cursor_position(),
            3,
            "move_cursor_line_end: Japanese first line should stop before newline"
        );
    }

    // --- move_cursor_up tests ---

    #[test]
    fn move_cursor_up_single_line_goes_to_start() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        input.move_cursor_up();
        assert_eq!(
            input.cursor_position(),
            0,
            "move_cursor_up: on first line should go to position 0"
        );
    }

    #[test]
    fn move_cursor_up_from_second_line() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // cursor at end of second line (pos 7, col 3)
        input.move_cursor_up();
        assert_eq!(
            input.cursor_position(),
            3,
            "move_cursor_up: should go to same column on previous line"
        );
    }

    #[test]
    fn move_cursor_up_column_clamp() {
        let mut input = TaskInput::new();
        input.set_content("ab\ndefgh".to_string());
        // cursor at end of second line (pos 8, col 5)
        input.move_cursor_up();
        assert_eq!(
            input.cursor_position(),
            2,
            "move_cursor_up: should clamp to end of shorter previous line"
        );
    }

    #[test]
    fn move_cursor_up_empty() {
        let mut input = TaskInput::new();
        input.move_cursor_up();
        assert_eq!(
            input.cursor_position(),
            0,
            "move_cursor_up: empty content should stay at 0"
        );
    }

    #[test]
    fn move_cursor_up_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あい\nうえお".to_string());
        // cursor at end (pos 6, second line col 3)
        input.move_cursor_up();
        assert_eq!(
            input.cursor_position(),
            2,
            "move_cursor_up: should clamp to end of shorter Japanese prev line"
        );
    }

    #[test]
    fn move_cursor_up_three_lines() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef\nghi".to_string());
        // cursor at end (pos 11, third line col 3)
        input.move_cursor_up();
        assert_eq!(
            input.cursor_position(),
            7,
            "move_cursor_up: from third line should go to second line same col"
        );
    }

    // --- move_cursor_down tests ---

    #[test]
    fn move_cursor_down_single_line_goes_to_end() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        input.move_cursor_start();
        input.move_cursor_down();
        assert_eq!(
            input.cursor_position(),
            5,
            "move_cursor_down: on last line should go to end of content"
        );
    }

    #[test]
    fn move_cursor_down_from_first_line() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // put cursor at pos 2 (col 2 on first line)
        input.move_cursor_start();
        input.move_cursor_right();
        input.move_cursor_right();
        input.move_cursor_down();
        assert_eq!(
            input.cursor_position(),
            6,
            "move_cursor_down: should go to same column on next line"
        );
    }

    #[test]
    fn move_cursor_down_column_clamp() {
        let mut input = TaskInput::new();
        input.set_content("abcde\nfg".to_string());
        // cursor at pos 4 (col 4 on first line)
        input.move_cursor_start();
        for _ in 0..4 {
            input.move_cursor_right();
        }
        input.move_cursor_down();
        assert_eq!(
            input.cursor_position(),
            8,
            "move_cursor_down: should clamp to end of shorter next line"
        );
    }

    #[test]
    fn move_cursor_down_empty() {
        let mut input = TaskInput::new();
        input.move_cursor_down();
        assert_eq!(
            input.cursor_position(),
            0,
            "move_cursor_down: empty content should stay at 0"
        );
    }

    #[test]
    fn move_cursor_down_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう\nえ".to_string());
        // cursor at pos 2 (col 2 on first line)
        input.move_cursor_start();
        input.move_cursor_right();
        input.move_cursor_right();
        input.move_cursor_down();
        assert_eq!(
            input.cursor_position(),
            5,
            "move_cursor_down: should clamp to end of shorter Japanese next line"
        );
    }

    #[test]
    fn move_cursor_down_three_lines() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef\nghi".to_string());
        // cursor at pos 1 (col 1 on first line)
        input.move_cursor_start();
        input.move_cursor_right();
        input.move_cursor_down();
        assert_eq!(
            input.cursor_position(),
            5,
            "move_cursor_down: from first line should go to second line same col"
        );
    }

    // --- kill_line tests ---

    #[test]
    fn kill_line_deletes_to_end_of_line() {
        let mut input = TaskInput::new();
        input.set_content("hello world".to_string());
        input.move_cursor_start();
        for _ in 0..5 {
            input.move_cursor_right();
        }
        input.kill_line();
        assert_eq!(
            input.content(),
            "hello",
            "kill_line: should delete from cursor to end of line"
        );
        assert_eq!(input.cursor_position(), 5);
    }

    #[test]
    fn kill_line_at_end_of_line_deletes_newline() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // move to end of first line (pos 3)
        input.move_cursor_start();
        input.move_cursor_line_end();
        input.kill_line();
        assert_eq!(
            input.content(),
            "abcdef",
            "kill_line: at end of line should delete newline to join lines"
        );
    }

    #[test]
    fn kill_line_at_end_of_content_does_nothing() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        // cursor at end
        input.kill_line();
        assert_eq!(
            input.content(),
            "hello",
            "kill_line: at end of content should do nothing"
        );
    }

    #[test]
    fn kill_line_from_start_clears_line() {
        let mut input = TaskInput::new();
        input.set_content("hello\nworld".to_string());
        input.move_cursor_start();
        input.kill_line();
        assert_eq!(
            input.content(),
            "\nworld",
            "kill_line: from start should clear entire first line content"
        );
    }

    #[test]
    fn kill_line_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう\nえお".to_string());
        input.move_cursor_start();
        input.move_cursor_right();
        input.kill_line();
        assert_eq!(
            input.content(),
            "あ\nえお",
            "kill_line: should work with Japanese characters"
        );
    }

    #[test]
    fn kill_line_empty() {
        let mut input = TaskInput::new();
        input.kill_line();
        assert_eq!(
            input.content(),
            "",
            "kill_line: empty content should stay empty"
        );
    }

    // --- unix_line_discard tests ---

    #[test]
    fn unix_line_discard_deletes_to_start_of_line() {
        let mut input = TaskInput::new();
        input.set_content("hello world".to_string());
        input.move_cursor_start();
        for _ in 0..5 {
            input.move_cursor_right();
        }
        input.unix_line_discard();
        assert_eq!(
            input.content(),
            " world",
            "unix_line_discard: should delete from start of line to cursor"
        );
        assert_eq!(input.cursor_position(), 0);
    }

    #[test]
    fn unix_line_discard_at_start_does_nothing() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        input.move_cursor_start();
        input.unix_line_discard();
        assert_eq!(
            input.content(),
            "hello",
            "unix_line_discard: at start of line should do nothing"
        );
    }

    #[test]
    fn unix_line_discard_second_line() {
        let mut input = TaskInput::new();
        input.set_content("abc\ndef".to_string());
        // cursor at end (pos 7, second line)
        input.unix_line_discard();
        assert_eq!(
            input.content(),
            "abc\n",
            "unix_line_discard: should delete second line content before cursor"
        );
        assert_eq!(input.cursor_position(), 4);
    }

    #[test]
    fn unix_line_discard_japanese() {
        let mut input = TaskInput::new();
        input.set_content("あいう".to_string());
        // cursor at end (pos 3)
        input.move_cursor_left();
        input.unix_line_discard();
        assert_eq!(
            input.content(),
            "う",
            "unix_line_discard: should work with Japanese characters"
        );
        assert_eq!(input.cursor_position(), 0);
    }

    #[test]
    fn unix_line_discard_empty() {
        let mut input = TaskInput::new();
        input.unix_line_discard();
        assert_eq!(
            input.content(),
            "",
            "unix_line_discard: empty content should stay empty"
        );
    }

    // --- ensure_cursor_visible tests ---

    #[test]
    fn ensure_cursor_visible_scroll_follows_cursor_down() {
        let mut input = TaskInput::new();
        // 10 lines of content, visible_height = 3
        input.set_content("0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string());
        // cursor at end = line 9
        input.ensure_cursor_visible(3, 80);
        let (cursor_line, _) = input.cursor_line_col();
        assert!(
            input.scroll_offset <= cursor_line && cursor_line < input.scroll_offset + 3,
            "ensure_cursor_visible: cursor line {} should be within viewport [{}, {})",
            cursor_line,
            input.scroll_offset,
            input.scroll_offset + 3
        );
        assert_eq!(
            input.scroll_offset, 7,
            "ensure_cursor_visible: scroll_offset should be cursor_line - visible_height + 1"
        );
    }

    #[test]
    fn ensure_cursor_visible_scroll_follows_cursor_up() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string());
        input.scroll_offset = 7;
        input.move_cursor_start(); // cursor to line 0
        input.ensure_cursor_visible(3, 80);
        assert_eq!(
            input.scroll_offset, 0,
            "ensure_cursor_visible: scroll should follow cursor up to 0"
        );
    }

    #[test]
    fn ensure_cursor_visible_no_change_when_cursor_in_viewport() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4".to_string());
        input.scroll_offset = 1;
        // Move cursor to line 2 (within viewport [1, 4))
        input.move_cursor_start();
        for _ in 0..4 {
            input.move_cursor_right(); // "0\n1\n" -> pos 4, line 2 col 0
        }
        let original_offset = input.scroll_offset;
        input.ensure_cursor_visible(3, 80);
        assert_eq!(
            input.scroll_offset, original_offset,
            "ensure_cursor_visible: scroll_offset should not change when cursor is within viewport"
        );
    }

    #[test]
    fn ensure_cursor_visible_clear_resets() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4".to_string());
        input.ensure_cursor_visible(3, 80);
        assert!(input.scroll_offset > 0);
        input.clear();
        input.ensure_cursor_visible(3, 80);
        assert_eq!(
            input.scroll_offset, 0,
            "ensure_cursor_visible: after clear(), scroll_offset should be 0"
        );
    }

    #[test]
    fn ensure_cursor_visible_compact_layout() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string());
        input.ensure_cursor_visible(3, 80);
        let (cursor_line, _) = input.cursor_line_col();
        assert!(
            input.scroll_offset <= cursor_line && cursor_line < input.scroll_offset + 3,
            "ensure_cursor_visible: compact layout (visible_height=3) cursor should be visible"
        );
    }

    #[test]
    fn ensure_cursor_visible_expanded_layout() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string());
        input.ensure_cursor_visible(6, 80);
        let (cursor_line, _) = input.cursor_line_col();
        assert!(
            input.scroll_offset <= cursor_line && cursor_line < input.scroll_offset + 6,
            "ensure_cursor_visible: expanded layout (visible_height=6) cursor should be visible"
        );
    }

    #[test]
    fn ensure_cursor_visible_zero_height_noop() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2".to_string());
        input.scroll_offset = 2;
        input.ensure_cursor_visible(0, 80);
        assert_eq!(
            input.scroll_offset, 2,
            "ensure_cursor_visible: visible_height=0 should be a no-op"
        );
    }

    #[test]
    fn ensure_cursor_visible_clamps_overflow_after_deletion() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string());
        input.scroll_offset = 8;
        // Simulate content deletion: replace with short content
        input.clear();
        input.set_content("a\nb".to_string());
        input.ensure_cursor_visible(3, 80);
        // "a\nb" is non-empty, so EOB is inline: total_lines = 2 + 0 = 2, max_offset = 0
        assert_eq!(
            input.scroll_offset, 0,
            "ensure_cursor_visible: scroll_offset should be clamped after content deletion"
        );
    }

    // --- EOB indicator and render integration tests ---

    fn render_to_lines(input: &mut TaskInput, width: u16, height: u16) -> Vec<String> {
        use ratatui::{backend::TestBackend, Terminal};
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| {
                let area = Rect::new(0, 0, width, height);
                input.render(frame, area, "Test");
            })
            .unwrap();
        let buffer = terminal.backend().buffer().clone();
        let mut lines = Vec::new();
        for y in 0..height {
            let mut line = String::new();
            for x in 0..width {
                let cell = &buffer[(x, y)];
                line.push_str(cell.symbol());
            }
            lines.push(line.trim_end().to_string());
        }
        lines
    }

    #[test]
    fn render_eob_present_in_output() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        input.set_focused(true);
        // height=5 gives inner_height=3, enough for "hello", cursor, "[EOB]"
        let lines = render_to_lines(&mut input, 30, 5);
        let has_eob = lines.iter().any(|l| l.contains("[EOB]"));
        assert!(has_eob, "render: output should contain [EOB] indicator");
    }

    #[test]
    fn render_eob_inline_with_last_content_line() {
        let mut input = TaskInput::new();
        input.set_content("line1\nline2".to_string());
        input.set_focused(true);
        // No trailing newline: [EOB] should be inline with "line2"
        let lines = render_to_lines(&mut input, 30, 6);
        let eob_idx = lines.iter().position(|l| l.contains("[EOB]"));
        let line2_idx = lines.iter().position(|l| l.contains("line2"));
        assert!(
            eob_idx.is_some() && line2_idx.is_some(),
            "render: both line2 and [EOB] should be present"
        );
        assert_eq!(
            eob_idx.unwrap(),
            line2_idx.unwrap(),
            "render: [EOB] should be on the same line as last content line (no trailing newline)"
        );
    }

    #[test]
    fn render_eob_scrollable_with_cursor_at_last_line() {
        let mut input = TaskInput::new();
        // 2 content lines, no trailing newline: [EOB] is inline with line2
        input.set_content("line1\nline2".to_string());
        input.set_focused(true);
        // height=5 gives inner_height=3: line1, line2+cursor+[EOB] (inline)
        let lines = render_to_lines(&mut input, 30, 5);
        let has_eob = lines.iter().any(|l| l.contains("[EOB]"));
        assert!(
            has_eob,
            "render: [EOB] should be visible when cursor is on last content line and viewport has room"
        );
    }

    #[test]
    fn render_eob_inline_with_trailing_newline() {
        let mut input = TaskInput::new();
        input.set_content("line1\nline2\n".to_string());
        input.set_focused(true);
        // Trailing newline: cursor is on empty line 3, [EOB] inline with cursor
        let lines = render_to_lines(&mut input, 30, 7);
        let eob_idx = lines.iter().position(|l| l.contains("[EOB]"));
        let line2_idx = lines.iter().position(|l| l.contains("line2"));
        assert!(
            eob_idx.is_some() && line2_idx.is_some(),
            "render: both line2 and [EOB] should be present"
        );
        assert!(
            eob_idx.unwrap() > line2_idx.unwrap(),
            "render: [EOB] should be on the line after line2 (inline with cursor on empty line)"
        );
        // [EOB] is inline with cursor, not on a 4th line
        let eob_line = &lines[eob_idx.unwrap()];
        assert!(
            eob_line.contains("│") && eob_line.contains("[EOB]"),
            "render: cursor and [EOB] should be on the same line"
        );
    }

    #[test]
    fn unicode_scroll_integration_10_line_japanese() {
        let mut input = TaskInput::new();
        // 10 lines of Japanese content
        let lines_content = [
            "あいうえお",
            "かきくけこ",
            "さしすせそ",
            "たちつてと",
            "なにぬねの",
            "はひふへほ",
            "まみむめも",
            "やゆよ",
            "らりるれろ",
            "わをん",
        ];
        let content = lines_content.join("\n");
        input.set_content(content);
        input.move_cursor_start();

        let visible_height: usize = 3;

        // Walk cursor down through every line and verify invariants
        for expected_line in 0..10 {
            let (cursor_line, _col) = input.cursor_line_col();
            assert_eq!(
                cursor_line, expected_line,
                "unicode_scroll: cursor should be on line {} but is on line {}",
                expected_line, cursor_line
            );

            input.ensure_cursor_visible(visible_height, 80);

            assert!(
                input.scroll_offset <= cursor_line
                    && cursor_line < input.scroll_offset + visible_height,
                "unicode_scroll: line {} — cursor_line {} not in viewport [{}, {})",
                expected_line,
                cursor_line,
                input.scroll_offset,
                input.scroll_offset + visible_height,
            );

            // Move to the next line (move to end of current line, then one right to cross newline)
            if expected_line < 9 {
                input.move_cursor_down();
                input.move_cursor_line_start();
            }
        }

        // Now walk cursor back up to line 0
        for expected_line in (0..10).rev() {
            let (cursor_line, _col) = input.cursor_line_col();
            assert_eq!(
                cursor_line, expected_line,
                "unicode_scroll (up): cursor should be on line {} but is on line {}",
                expected_line, cursor_line
            );

            input.ensure_cursor_visible(visible_height, 80);

            assert!(
                input.scroll_offset <= cursor_line
                    && cursor_line < input.scroll_offset + visible_height,
                "unicode_scroll (up): line {} — cursor_line {} not in viewport [{}, {})",
                expected_line,
                cursor_line,
                input.scroll_offset,
                input.scroll_offset + visible_height,
            );

            if expected_line > 0 {
                input.move_cursor_up();
                input.move_cursor_line_start();
            }
        }

        assert_eq!(
            input.scroll_offset, 0,
            "unicode_scroll: after returning to line 0, scroll_offset should be 0"
        );
    }

    #[test]
    fn render_scroll_offset_adjusted_after_render() {
        let mut input = TaskInput::new();
        input.set_content("0\n1\n2\n3\n4\n5\n6\n7\n8\n9".to_string());
        input.set_focused(true);
        assert_eq!(input.scroll_offset, 0, "scroll_offset should start at 0");
        // Render with small height; cursor is at line 9
        let _ = render_to_lines(&mut input, 30, 5);
        assert!(
            input.scroll_offset > 0,
            "render: scroll_offset should be adjusted after render() when cursor is below viewport"
        );
    }

    // --- visual_line_metrics tests ---

    #[test]
    fn visual_line_metrics_empty() {
        let input = TaskInput::new();
        assert_eq!(
            input.visual_line_metrics(20),
            (1, 0),
            "visual_line_metrics: empty buffer should return (1, 0)"
        );
    }

    #[test]
    fn visual_line_metrics_short_line_no_wrap() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        assert_eq!(
            input.visual_line_metrics(20),
            (1, 5),
            "visual_line_metrics: short line should not wrap"
        );
    }

    #[test]
    fn visual_line_metrics_exact_width() {
        let mut input = TaskInput::new();
        input.set_content("abcde".to_string());
        assert_eq!(
            input.visual_line_metrics(5),
            (1, 5),
            "visual_line_metrics: line exactly filling width should be 1 visual line"
        );
    }

    #[test]
    fn visual_line_metrics_wrap_once() {
        let mut input = TaskInput::new();
        input.set_content("abcdef".to_string()); // 6 chars, width 5
        assert_eq!(
            input.visual_line_metrics(5),
            (2, 1),
            "visual_line_metrics: 6 chars in width 5 should wrap to 2 visual lines"
        );
    }

    #[test]
    fn visual_line_metrics_wrap_twice() {
        let mut input = TaskInput::new();
        input.set_content("abcdefghijk".to_string()); // 11 chars, width 5
        assert_eq!(
            input.visual_line_metrics(5),
            (3, 1),
            "visual_line_metrics: 11 chars in width 5 should wrap to 3 visual lines"
        );
    }

    #[test]
    fn visual_line_metrics_cjk_wrapping() {
        let mut input = TaskInput::new();
        // Each CJK char is 2 display columns. 5 chars = 10 columns.
        // With wrap_width=6: "あいう" = 6 cols (1 line), "えお" = 4 cols (1 line) -> 2 visual lines
        input.set_content("あいうえお".to_string());
        assert_eq!(
            input.visual_line_metrics(6),
            (2, 4),
            "visual_line_metrics: CJK chars should wrap based on display width"
        );
    }

    #[test]
    fn visual_line_metrics_cjk_wide_char_at_boundary() {
        let mut input = TaskInput::new();
        // "abc" = 3 cols, "あ" = 2 cols -> total 5 cols. wrap_width = 4.
        // "abc" = 3 cols, then "あ" needs 2 but only 1 remaining -> wraps.
        // Line 1: "abc" (3 cols), Line 2: "あ" (2 cols) -> 2 visual lines
        input.set_content("abcあ".to_string());
        assert_eq!(
            input.visual_line_metrics(4),
            (2, 2),
            "visual_line_metrics: wide char at boundary should force wrap"
        );
    }

    #[test]
    fn visual_line_metrics_multiline_with_wrapping() {
        let mut input = TaskInput::new();
        // Line 1: "abcdef" (6 chars) with wrap_width 4 -> wraps to 2 visual lines
        // Line 2: "gh" (2 chars) -> 1 visual line
        // Total: 3 visual lines
        input.set_content("abcdef\ngh".to_string());
        assert_eq!(
            input.visual_line_metrics(4),
            (3, 2),
            "visual_line_metrics: multiline content with wrapping"
        );
    }

    // --- visual_cursor_line tests ---

    #[test]
    fn visual_cursor_line_empty() {
        let input = TaskInput::new();
        assert_eq!(
            input.visual_cursor_line(20),
            0,
            "visual_cursor_line: empty buffer cursor should be on line 0"
        );
    }

    #[test]
    fn visual_cursor_line_no_wrap() {
        let mut input = TaskInput::new();
        input.set_content("hello".to_string());
        assert_eq!(
            input.visual_cursor_line(20),
            0,
            "visual_cursor_line: short line cursor should be on line 0"
        );
    }

    #[test]
    fn visual_cursor_line_after_wrap() {
        let mut input = TaskInput::new();
        // "abcdef" with wrap_width 4: wraps to "abcd" + "ef"
        // cursor at end (pos 6) is on visual line 1
        input.set_content("abcdef".to_string());
        assert_eq!(
            input.visual_cursor_line(4),
            1,
            "visual_cursor_line: cursor after wrap should be on visual line 1"
        );
    }

    #[test]
    fn visual_cursor_line_before_wrap() {
        let mut input = TaskInput::new();
        // "abcdef" with wrap_width 4, cursor at position 2 ("ab|cdef")
        input.set_content("abcdef".to_string());
        input.move_cursor_start();
        input.move_cursor_right();
        input.move_cursor_right();
        assert_eq!(
            input.visual_cursor_line(4),
            0,
            "visual_cursor_line: cursor before wrap point should be on visual line 0"
        );
    }

    #[test]
    fn visual_cursor_line_cjk_wrap() {
        let mut input = TaskInput::new();
        // "あいうえお" (5 CJK chars, 10 cols), wrap_width=6
        // Visual: "あいう" (6 cols) + "えお" (4 cols)
        // cursor at end (pos 5) -> visual line 1
        input.set_content("あいうえお".to_string());
        assert_eq!(
            input.visual_cursor_line(6),
            1,
            "visual_cursor_line: CJK cursor after wrap should be on visual line 1"
        );
    }

    #[test]
    fn visual_cursor_line_multiline_with_wrap() {
        let mut input = TaskInput::new();
        // "abcdef\ngh" with wrap_width 4
        // "abcdef" wraps to 2 visual lines (abcd + ef)
        // "gh" is on visual line 2
        // cursor at end (pos 9) -> visual line 2
        input.set_content("abcdef\ngh".to_string());
        assert_eq!(
            input.visual_cursor_line(4),
            2,
            "visual_cursor_line: cursor on second logical line after wrapping"
        );
    }

    // --- ensure_cursor_visible with wrapping tests ---

    #[test]
    fn ensure_cursor_visible_wrapping_scrolls_correctly() {
        let mut input = TaskInput::new();
        // 20 chars on one logical line, wrap_width=5 -> 4 visual lines
        // visible_height=2 -> should scroll to show cursor
        input.set_content("abcdefghijklmnopqrst".to_string()); // 20 chars
        input.ensure_cursor_visible(2, 5);
        // cursor at end = visual line 3 (0-indexed), visible_height=2
        // scroll_offset should be 2 (shows visual lines 2, 3)
        assert_eq!(
            input.scroll_offset, 2,
            "ensure_cursor_visible: wrapping should scroll to show cursor on wrapped lines"
        );
    }

    #[test]
    fn ensure_cursor_visible_no_wrap_unchanged() {
        let mut input = TaskInput::new();
        input.set_content("short".to_string());
        input.ensure_cursor_visible(3, 80);
        assert_eq!(
            input.scroll_offset, 0,
            "ensure_cursor_visible: short content should not scroll"
        );
    }

    // --- EOB overflow regression test ---

    #[test]
    fn render_eob_within_frame_with_wrapping() {
        let mut input = TaskInput::new();
        // Long text that wraps: 20 chars, width=15 (inner=13, gutter=3, content=10), height=5 (inner=3)
        // This previously caused EOB to overflow below the border.
        input.set_content("abcdefghijklmnopqrst".to_string());
        input.set_focused(true);
        let lines = render_to_lines(&mut input, 15, 5);
        // EOB should be visible within the frame, not below it
        let has_eob = lines.iter().any(|l| l.contains("[EOB]"));
        assert!(
            has_eob,
            "render: [EOB] should be visible within the frame even with long wrapping text"
        );
        // The last line (border) should be a border character, not content
        let last_line = lines.last().unwrap();
        assert!(
            last_line.contains('└') || last_line.contains('─'),
            "render: bottom border should be intact, not overflowed by content"
        );
    }

    #[test]
    fn render_eob_within_frame_cjk_wrapping() {
        let mut input = TaskInput::new();
        // Long CJK text that wraps heavily
        input.set_content("あいうえおかきくけこさしすせそ".to_string());
        input.set_focused(true);
        // width=15 (inner=13, gutter=3, content=10), height=7 (inner=5)
        let lines = render_to_lines(&mut input, 15, 7);
        let has_eob = lines.iter().any(|l| l.contains("[EOB]"));
        assert!(
            has_eob,
            "render: [EOB] should be visible with CJK wrapping text"
        );
        let last_line = lines.last().unwrap();
        assert!(
            last_line.contains('└') || last_line.contains('─'),
            "render: bottom border should be intact with CJK wrapping"
        );
    }

    // --- gutter_width tests ---

    #[test]
    fn gutter_width_min_3_for_single_line() {
        let input = TaskInput::new();
        assert_eq!(
            input.gutter_width(),
            3,
            "gutter_width: empty buffer (1 line) should return 3 (2 digits min + 1 space)"
        );
    }

    #[test]
    fn gutter_width_min_3_for_99_lines() {
        let mut input = TaskInput::new();
        let content = (1..=99)
            .map(|i| format!("line{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        input.set_content(content);
        assert_eq!(
            input.gutter_width(),
            3,
            "gutter_width: 99 lines should return 3 (2 digits + 1 space)"
        );
    }

    #[test]
    fn gutter_width_4_for_100_lines() {
        let mut input = TaskInput::new();
        let content = (1..=100)
            .map(|i| format!("l{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        input.set_content(content);
        assert_eq!(
            input.gutter_width(),
            4,
            "gutter_width: 100 lines should return 4 (3 digits + 1 space)"
        );
    }

    #[test]
    fn gutter_width_5_for_1000_lines() {
        let mut input = TaskInput::new();
        let content = (1..=1000)
            .map(|i| format!("l{}", i))
            .collect::<Vec<_>>()
            .join("\n");
        input.set_content(content);
        assert_eq!(
            input.gutter_width(),
            5,
            "gutter_width: 1000 lines should return 5 (4 digits + 1 space)"
        );
    }

    // --- visual_line_count_for tests ---

    #[test]
    fn visual_line_count_for_short_line() {
        assert_eq!(
            TaskInput::visual_line_count_for("hello", 10),
            1,
            "visual_line_count_for: short line should be 1"
        );
    }

    #[test]
    fn visual_line_count_for_wrapping_line() {
        assert_eq!(
            TaskInput::visual_line_count_for("abcdefghijk", 5),
            3,
            "visual_line_count_for: 11 chars at width 5 should wrap to 3 visual lines"
        );
    }

    #[test]
    fn visual_line_count_for_cjk() {
        assert_eq!(
            TaskInput::visual_line_count_for("あいうえお", 6),
            2,
            "visual_line_count_for: 5 CJK chars (10 cols) at width 6 should wrap to 2"
        );
    }

    // --- gutter rendering tests ---

    #[test]
    fn gutter_line_numbers_displayed() {
        let mut input = TaskInput::new();
        input.set_content("aaa\nbbb\nccc".to_string());
        input.set_focused(true);
        // width=33 (border=2, gutter=3, content=28), height=7 (inner=5)
        let lines = render_to_lines(&mut input, 33, 7);
        // Lines 1-3 should show line numbers in the gutter area (inside the border)
        assert!(
            lines[1].contains(" 1 "),
            "gutter: line 1 should show number 1, got: {:?}",
            lines[1]
        );
        assert!(
            lines[2].contains(" 2 "),
            "gutter: line 2 should show number 2, got: {:?}",
            lines[2]
        );
        assert!(
            lines[3].contains(" 3 "),
            "gutter: line 3 should show number 3, got: {:?}",
            lines[3]
        );
    }

    #[test]
    fn gutter_tilde_beyond_content() {
        let mut input = TaskInput::new();
        input.set_content("one".to_string());
        input.set_focused(true);
        // width=33 (border=2, gutter=3, content=28), height=6 (inner=4)
        // Content: line 1 "one│[EOB]" = 1 visual line
        // Lines 2-4 of inner should show tildes
        let lines = render_to_lines(&mut input, 33, 6);
        // lines[0] = top border, lines[1] = content, lines[2..5] = inner rows, lines[5] = bottom border
        // Beyond content rows should have tildes
        let tilde_count = lines[2..5].iter().filter(|l| l.contains('~')).count();
        assert!(
            tilde_count >= 1,
            "gutter: tilde lines should appear beyond content, got lines: {:?}",
            &lines[2..5]
        );
    }

    #[test]
    fn gutter_no_blank_row_when_cursor_at_start() {
        let mut input = TaskInput::new();
        // Empty buffer, focused: EOB is inline with the cursor on line 1
        input.set_focused(true);
        // width=33 (border=2, gutter=3, content=28), height=6 (inner=4)
        // Expected: line 1 has "│[EOB]", rows 2-4 should all be tildes
        let lines = render_to_lines(&mut input, 33, 6);
        // lines[0] = top border, lines[1] = line 1 content, lines[2..5] = inner rows
        // The row immediately after line 1 (lines[2]) should contain a tilde, not be blank
        assert!(
            lines[2].contains('~'),
            "gutter: row after line 1 should be a tilde when empty+focused, got: {:?}",
            lines[2]
        );
        // All remaining inner rows should also be tildes
        for (i, line) in lines[2..5].iter().enumerate() {
            assert!(
                line.contains('~'),
                "gutter: inner row {} should contain tilde, got: {:?}",
                i + 2,
                line
            );
        }
    }

    #[test]
    fn gutter_continuation_blank() {
        let mut input = TaskInput::new();
        // "abcdefghij" is 10 chars. With width=15 (border=2, gutter=3, content=10),
        // it wraps to 2 visual lines: "abcdefghij" fits exactly in 10. No wrapping.
        // Use longer text: "abcdefghijklmno" = 15 chars, wraps to 2 lines at width 10.
        input.set_content("abcdefghijklmno".to_string());
        input.set_focused(true);
        // width=15, border=2, gutter=3, content=10. height=6 (inner=4)
        let lines = render_to_lines(&mut input, 15, 6);
        // lines[0] = top border
        // lines[1] = first visual line: gutter " 1 " + content "abcdefghij"
        // lines[2] = continuation: blank gutter + "klmno│[EOB]"
        // The continuation line should have spaces in gutter, not a number
        assert!(
            lines[1].contains(" 1 "),
            "gutter: first visual line should have line number, got: {:?}",
            lines[1]
        );
        // Continuation line should NOT have " 2 " since it's still logical line 1
        assert!(
            !lines[2].contains(" 2 "),
            "gutter: continuation line should not have line number 2, got: {:?}",
            lines[2]
        );
    }
}
