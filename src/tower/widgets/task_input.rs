use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub struct TaskInput {
    content: String,
    cursor_position: usize,
    focused: bool,
}

impl TaskInput {
    pub fn new() -> Self {
        Self {
            content: String::new(),
            cursor_position: 0,
            focused: false,
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

    pub fn cursor_position(&self) -> usize {
        self.cursor_position
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }

    pub fn render(&self, frame: &mut Frame, area: Rect, title: &str) {
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

        let display_text = if self.content.is_empty() && !self.focused {
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

        let paragraph = Paragraph::new(display_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(title),
            )
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
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
}
