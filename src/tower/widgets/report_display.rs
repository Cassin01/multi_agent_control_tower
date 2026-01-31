use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::models::{Report, TaskStatus};

pub struct ReportDisplay {
    reports: Vec<Report>,
    state: ListState,
    focused: bool,
}

impl ReportDisplay {
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
            state: ListState::default(),
            focused: false,
        }
    }

    pub fn set_reports(&mut self, reports: Vec<Report>) {
        self.reports = reports;
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    pub fn next(&mut self) {
        if self.reports.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.reports.len() - 1 {
                    0
                } else {
                    i + 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn prev(&mut self) {
        if self.reports.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.reports.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    fn status_symbol(status: &TaskStatus) -> (&'static str, Color) {
        match status {
            TaskStatus::Pending => ("○", Color::Gray),
            TaskStatus::InProgress => ("◐", Color::Yellow),
            TaskStatus::Done => ("✓", Color::Green),
            TaskStatus::Failed => ("✗", Color::Red),
        }
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .reports
            .iter()
            .map(|report| {
                let (symbol, color) = Self::status_symbol(&report.status);
                let status_style = Style::default().fg(color);

                let summary = if report.summary.len() > 40 {
                    format!("{}...", &report.summary[..37])
                } else if report.summary.is_empty() {
                    "In progress...".to_string()
                } else {
                    report.summary.clone()
                };

                let spans = vec![
                    Span::styled(
                        format!("[{}] ", report.expert_id),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(symbol, status_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:<12}", report.expert_name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" - "),
                    Span::styled(summary, Style::default()),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let border_style = if self.focused {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::Gray)
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title("Recent Reports"),
            )
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.state);
    }
}

impl Default for ReportDisplay {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_report(id: u32, name: &str, status: TaskStatus, summary: &str) -> Report {
        let mut report = Report::new(format!("task-{:03}", id), id, name.to_string());
        report.status = status;
        report.summary = summary.to_string();
        report
    }

    #[test]
    fn report_display_empty_by_default() {
        let display = ReportDisplay::new();
        assert!(display.reports.is_empty());
    }

    #[test]
    fn report_display_navigation() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![
            create_test_report(0, "architect", TaskStatus::Done, "Completed"),
            create_test_report(1, "frontend", TaskStatus::InProgress, "Working"),
            create_test_report(2, "backend", TaskStatus::Failed, "Error"),
        ]);

        display.next();
        assert_eq!(display.state.selected(), Some(0));

        display.next();
        assert_eq!(display.state.selected(), Some(1));

        display.next();
        assert_eq!(display.state.selected(), Some(2));

        display.next();
        assert_eq!(display.state.selected(), Some(0));
    }

    #[test]
    fn report_display_prev_navigation() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![
            create_test_report(0, "architect", TaskStatus::Done, "Completed"),
            create_test_report(1, "frontend", TaskStatus::InProgress, "Working"),
        ]);

        display.prev();
        assert_eq!(display.state.selected(), Some(0));

        display.prev();
        assert_eq!(display.state.selected(), Some(1));
    }

    #[test]
    fn report_display_focus_state() {
        let mut display = ReportDisplay::new();
        assert!(!display.focused);

        display.set_focused(true);
        assert!(display.focused);
    }
}
