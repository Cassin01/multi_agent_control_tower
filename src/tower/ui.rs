use std::io::{self, Stdout};

use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame, Terminal,
};

use super::app::{FocusArea, TowerApp};
use super::widgets::ViewMode;

pub struct UI;

impl UI {
    pub fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        let backend = CrosstermBackend::new(stdout);
        Terminal::new(backend)
    }

    pub fn restore_terminal() -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen)?;
        Ok(())
    }

    pub fn render(frame: &mut Frame, app: &mut TowerApp) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Min(8),
                Constraint::Length(8),
                Constraint::Length(3),
                Constraint::Length(6),
                Constraint::Length(3),
            ])
            .split(frame.area());

        Self::render_header(frame, chunks[0], app);
        app.status_display().render(frame, chunks[1]);
        Self::render_task_input(frame, chunks[2], app);
        app.effort_selector().render(frame, chunks[3]);
        app.report_display().render(frame, chunks[4]);
        Self::render_footer(frame, chunks[5], app);

        if app.report_display().view_mode() == ViewMode::Detail {
            let modal_area = Self::centered_area(frame.area(), 80, 90);
            app.report_display().render_detail_modal(frame, modal_area);
        }
    }

    fn centered_area(area: Rect, percent_x: u16, percent_y: u16) -> Rect {
        let popup_layout = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage((100 - percent_y) / 2),
                Constraint::Percentage(percent_y),
                Constraint::Percentage((100 - percent_y) / 2),
            ])
            .split(area);

        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage((100 - percent_x) / 2),
                Constraint::Percentage(percent_x),
                Constraint::Percentage((100 - percent_x) / 2),
            ])
            .split(popup_layout[1])[1]
    }

    fn render_header(frame: &mut Frame, area: Rect, app: &mut TowerApp) {
        let summary = app.status_display().get_status_summary();

        let mut title = vec![
            Span::styled(
                " MACOT ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        ];

        // Show subtitle only when width is sufficient
        if area.width >= 100 {
            title.push(Span::raw(" - Multi Agent Control Tower "));
        }
        title.push(Span::raw("| "));

        title.extend([
            Span::styled(
                format!("Session: {} ", app.config().session_name()),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("| "),
            Span::styled(
                format!("○ {} ", summary.idle),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("◐ {} ", summary.thinking),
                Style::default().fg(Color::Yellow),
            ),
            Span::styled(
                format!("● {} ", summary.executing),
                Style::default().fg(Color::Green),
            ),
            Span::styled(
                format!("✗ {}", summary.error),
                Style::default().fg(Color::Red),
            ),
        ]);

        let header = Paragraph::new(Line::from(title)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );

        frame.render_widget(header, area);
    }

    fn render_task_input(frame: &mut Frame, area: Rect, app: &mut TowerApp) {
        let selected_expert = app
            .status_display()
            .selected()
            .map(|c| format!("Task for {} (Expert {})", c.expert_name, c.expert_id))
            .unwrap_or_else(|| "Task Description (select an expert first)".to_string());

        app.task_input().render(frame, area, &selected_expert);
    }

    fn render_footer(frame: &mut Frame, area: Rect, app: &mut TowerApp) {
        let message = app.message().unwrap_or("");
        let message = if message.is_empty() {
            String::new()
        } else {
            format!("{} | ", message)
        };
        let message_style = if message.contains("Error") || message.contains("empty") {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };

        let mut help_text = vec![
            Span::styled(message, message_style),
            Span::styled("Tab", Style::default().fg(Color::Yellow)),
            Span::raw(": Switch focus "),
        ];

        if app.focus() == FocusArea::TaskInput {
            help_text.push(Span::styled("Ctrl+S", Style::default().fg(Color::Yellow)));
            help_text.push(Span::raw(": Assign task "));
        }

        if app.focus() == FocusArea::ExpertList {
            help_text.push(Span::styled("Ctrl+R", Style::default().fg(Color::Yellow)));
            help_text.push(Span::raw(": Reset "));
        }

        help_text.push(Span::styled("Ctrl+Q", Style::default().fg(Color::Yellow)));
        help_text.push(Span::raw(": Quit"));

        let footer = Paragraph::new(Line::from(help_text)).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)),
        );

        frame.render_widget(footer, area);
    }
}
