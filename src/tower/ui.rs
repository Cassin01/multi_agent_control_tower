use std::io::{self, Stdout};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
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

use super::app::{FocusArea, LayoutAreas, TowerApp};
use super::widgets::ViewMode;
use crate::utils::truncate_str_head;

pub struct UI;

impl UI {
    pub fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
        let backend = CrosstermBackend::new(stdout);
        Terminal::new(backend)
    }

    pub fn restore_terminal() -> io::Result<()> {
        disable_raw_mode()?;
        execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
        Ok(())
    }

    pub fn render(frame: &mut Frame, app: &mut TowerApp) {
        let badge = app.feature_executor().and_then(|e| e.execution_badge());
        app.status_display().set_execution_badge(badge);

        // Dynamic height: expert_count + 2 (borders), minimum 3
        let expert_height = (app.status_display().expert_count() + 2).max(3) as u16;
        let panel_visible = app.expert_panel_display().is_visible();

        if panel_visible {
            // 5 layout constraints when panel is visible
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),             // [0] Header
                    Constraint::Length(expert_height), // [1] Expert List
                    Constraint::Length(5),             // [2] Task Input (fixed compact)
                    Constraint::Min(10),               // [3] Expert Panel (takes remaining)
                    Constraint::Length(3),             // [4] Footer
                ])
                .split(frame.area());

            app.set_layout_areas(LayoutAreas {
                expert_list: chunks[1],
                task_input: chunks[2],
                expert_panel: chunks[3],
            });

            Self::render_header(frame, chunks[0], app);
            app.status_display().render(frame, chunks[1]);
            Self::render_task_input(frame, chunks[2], app);
            app.expert_panel_display().render(frame, chunks[3]);
            Self::render_footer(frame, chunks[4], app);
        } else {
            // 4 layout constraints when panel is hidden (default)
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([
                    Constraint::Length(3),             // [0] Header
                    Constraint::Length(expert_height), // [1] Expert List
                    Constraint::Min(8),                // [2] Task Input
                    Constraint::Length(3),             // [3] Footer
                ])
                .split(frame.area());

            app.set_layout_areas(LayoutAreas {
                expert_list: chunks[1],
                task_input: chunks[2],
                expert_panel: Rect::default(),
            });

            Self::render_header(frame, chunks[0], app);
            app.status_display().render(frame, chunks[1]);
            Self::render_task_input(frame, chunks[2], app);
            Self::render_footer(frame, chunks[3], app);
        }

        if app.report_display().view_mode() == ViewMode::Detail {
            let (percent_x, percent_y) = Self::responsive_modal_size(frame.area(), 80, 90);
            let modal_area = Self::centered_area(frame.area(), percent_x, percent_y);
            app.report_display().render_detail_modal(frame, modal_area);
        }

        if app.help_modal().is_visible() {
            let (percent_x, percent_y) = Self::responsive_modal_size(frame.area(), 60, 80);
            let modal_area = Self::centered_area(frame.area(), percent_x, percent_y);
            app.help_modal().render(frame, modal_area);
        }

        if app.role_selector().is_visible() {
            app.role_selector().render(frame, frame.area());
        }
    }

    fn responsive_modal_size(area: Rect, base_x: u16, base_y: u16) -> (u16, u16) {
        const NARROW_WIDTH_THRESHOLD: u16 = 80;
        const SHORT_HEIGHT_THRESHOLD: u16 = 30;
        const MAX_PERCENT: u16 = 98;

        let should_maximize =
            area.width < NARROW_WIDTH_THRESHOLD || area.height < SHORT_HEIGHT_THRESHOLD;

        if should_maximize {
            (MAX_PERCENT, MAX_PERCENT)
        } else {
            (base_x, base_y)
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

        let mut title = vec![Span::styled(
            " MACOT ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )];

        // Show subtitle only when width is sufficient
        let is_wide = area.width >= 100;
        if is_wide {
            title.push(Span::raw(" - Multi Agent Control Tower "));
        }
        title.push(Span::raw("| "));

        let trancate_len = if is_wide { 40 } else { 20 };
        let session_prefix = if is_wide { "Session: " } else { "" };
        title.extend([
            Span::styled(
                format!("{}{} ", session_prefix, app.config().session_name()),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("| "),
            Span::styled(
                format!(
                    "{} ",
                    truncate_str_head(
                        &app.config().project_path.display().to_string(),
                        trancate_len
                    )
                ),
                Style::default().fg(Color::DarkGray),
            ),
        ]);

        let right_spans = vec![
            Span::styled(
                format!("○ {} ", summary.idle),
                Style::default().fg(Color::Gray),
            ),
            Span::styled(
                format!("● {} ", summary.busy),
                Style::default().fg(Color::Green),
            ),
        ];

        let left_width: usize = title.iter().map(Span::width).sum();
        let right_width: usize = right_spans.iter().map(Span::width).sum();
        let available = (area.width as usize).saturating_sub(2);
        let padding = available.saturating_sub(left_width + right_width);

        title.push(Span::raw(" ".repeat(padding)));
        title.extend(right_spans);

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
            .map(|c| format!("Command for {} (Expert{})", c.expert_name, c.expert_id))
            .unwrap_or_else(|| "Command Input Center (no expert selected)".to_string());

        app.task_input().render(frame, area, &selected_expert);
    }

    fn render_footer(frame: &mut Frame, area: Rect, app: &mut TowerApp) {
        let message = app.message().unwrap_or("");
        let message = if message.is_empty() {
            String::new()
        } else {
            format!("{message} | ")
        };
        let message_style = if message.contains("Error") || message.contains("empty") {
            Style::default().fg(Color::Red)
        } else {
            Style::default().fg(Color::Green)
        };

        let mut help_text = vec![
            Span::styled(message, message_style),
            Span::styled("Ctrl+T", Style::default().fg(Color::Yellow)),
            Span::raw(": Switch focus "),
        ];

        if app.focus() == FocusArea::TaskInput {
            help_text.push(Span::styled("Ctrl+S", Style::default().fg(Color::Yellow)));
            help_text.push(Span::raw(": Assign task "));
        }

        if app.focus() == FocusArea::ExpertList {
            help_text.push(Span::styled("Ctrl+O", Style::default().fg(Color::Yellow)));
            help_text.push(Span::raw(": Role "));
            help_text.push(Span::styled("Ctrl+R", Style::default().fg(Color::Yellow)));
            help_text.push(Span::raw(": Reset "));
        }

        help_text.push(Span::styled("Ctrl+J", Style::default().fg(Color::Yellow)));
        help_text.push(Span::raw(": Panel "));
        help_text.push(Span::styled("Ctrl+I", Style::default().fg(Color::Yellow)));
        help_text.push(Span::raw(": Help "));
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
