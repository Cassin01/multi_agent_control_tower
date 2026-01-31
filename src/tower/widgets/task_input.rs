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
        self.cursor_position = content.len();
        self.content = content;
    }

    pub fn clear(&mut self) {
        self.content.clear();
        self.cursor_position = 0;
    }

    pub fn insert_char(&mut self, c: char) {
        self.content.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
            self.content.remove(self.cursor_position);
        }
    }

    pub fn delete_forward(&mut self) {
        if self.cursor_position < self.content.len() {
            self.content.remove(self.cursor_position);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.content.len() {
            self.cursor_position += 1;
        }
    }

    pub fn move_cursor_start(&mut self) {
        self.cursor_position = 0;
    }

    pub fn move_cursor_end(&mut self) {
        self.cursor_position = self.content.len();
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
            let before = &self.content[..self.cursor_position];
            let after = &self.content[self.cursor_position..];

            vec![Line::from(vec![
                Span::styled(before, text_style),
                Span::styled("│", Style::default().fg(Color::Yellow).add_modifier(Modifier::SLOW_BLINK)),
                Span::styled(after, text_style),
            ])]
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
}
