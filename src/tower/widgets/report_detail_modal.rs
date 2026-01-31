use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

use crate::models::{Report, TaskStatus};

pub struct ReportDetailModal {
    report: Option<Report>,
    scroll_offset: u16,
}

impl ReportDetailModal {
    pub fn new() -> Self {
        Self {
            report: None,
            scroll_offset: 0,
        }
    }

    pub fn show(&mut self, report: Report) {
        self.report = Some(report);
        self.scroll_offset = 0;
    }

    pub fn hide(&mut self) {
        self.report = None;
        self.scroll_offset = 0;
    }

    #[allow(dead_code)]
    pub fn is_visible(&self) -> bool {
        self.report.is_some()
    }

    pub fn scroll_up(&mut self) {
        self.scroll_offset = self.scroll_offset.saturating_sub(1);
    }

    pub fn scroll_down(&mut self, max_lines: u16) {
        if self.scroll_offset < max_lines {
            self.scroll_offset += 1;
        }
    }

    fn status_style(status: &TaskStatus) -> (String, Style) {
        match status {
            TaskStatus::Pending => ("○ Pending".to_string(), Style::default().fg(Color::Gray)),
            TaskStatus::InProgress => (
                "◐ In Progress".to_string(),
                Style::default().fg(Color::Yellow),
            ),
            TaskStatus::Done => ("✓ Done".to_string(), Style::default().fg(Color::Green)),
            TaskStatus::Failed => ("✗ Failed".to_string(), Style::default().fg(Color::Red)),
        }
    }

    fn severity_style(severity: &str) -> Style {
        match severity.to_lowercase().as_str() {
            "critical" | "high" => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            "medium" => Style::default().fg(Color::Yellow),
            "low" => Style::default().fg(Color::Gray),
            _ => Style::default(),
        }
    }

    fn format_duration(report: &Report) -> String {
        match report.duration() {
            Some(duration) => {
                let secs = duration.num_seconds();
                if secs < 60 {
                    format!("{}s", secs)
                } else if secs < 3600 {
                    format!("{}m {}s", secs / 60, secs % 60)
                } else {
                    format!("{}h {}m", secs / 3600, (secs % 3600) / 60)
                }
            }
            None => "In progress...".to_string(),
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        let report = match &self.report {
            Some(r) => r,
            None => return,
        };

        frame.render_widget(Clear, area);

        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(Span::styled(
                " Report Details ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));

        let inner_area = block.inner(area);
        frame.render_widget(block, area);

        let mut lines: Vec<Line> = Vec::new();

        let (status_text, status_style) = Self::status_style(&report.status);
        lines.push(Line::from(vec![
            Span::styled("Task ID: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&report.task_id),
            Span::raw("  |  "),
            Span::styled("Expert: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(
                format!("[{}] {}", report.expert_id, &report.expert_name),
                Style::default().fg(Color::Yellow),
            ),
        ]));

        lines.push(Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(status_text, status_style),
            Span::raw("  |  "),
            Span::styled("Duration: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(Self::format_duration(report)),
        ]));

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "━━━ Summary ━━━",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )));

        if report.summary.is_empty() {
            lines.push(Line::from(Span::styled(
                "  (No summary yet)",
                Style::default().fg(Color::Gray),
            )));
        } else {
            for line in report.summary.lines() {
                lines.push(Line::from(format!("  {}", line)));
            }
        }

        if !report.details.findings.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "━━━ Findings ━━━",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            for (i, finding) in report.details.findings.iter().enumerate() {
                let severity_style = Self::severity_style(&finding.severity);
                let location = match (&finding.file, finding.line) {
                    (Some(file), Some(line)) => format!(" ({}:{})", file, line),
                    (Some(file), None) => format!(" ({})", file),
                    _ => String::new(),
                };

                lines.push(Line::from(vec![
                    Span::raw(format!("  {}. ", i + 1)),
                    Span::styled(
                        format!("[{}]", finding.severity.to_uppercase()),
                        severity_style,
                    ),
                    Span::raw(format!(" {}{}", finding.description, location)),
                ]));
            }
        }

        if !report.details.recommendations.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "━━━ Recommendations ━━━",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            for (i, rec) in report.details.recommendations.iter().enumerate() {
                lines.push(Line::from(format!("  {}. {}", i + 1, rec)));
            }
        }

        if !report.details.files_modified.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "━━━ Files Modified ━━━",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            for file in &report.details.files_modified {
                lines.push(Line::from(Span::styled(
                    format!("  📝 {}", file),
                    Style::default().fg(Color::Yellow),
                )));
            }
        }

        if !report.details.files_created.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "━━━ Files Created ━━━",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )));

            for file in &report.details.files_created {
                lines.push(Line::from(Span::styled(
                    format!("  ✨ {}", file),
                    Style::default().fg(Color::Green),
                )));
            }
        }

        if !report.errors.is_empty() {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "━━━ Errors ━━━",
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            )));

            for error in &report.errors {
                lines.push(Line::from(Span::styled(
                    format!("  ✗ {}", error),
                    Style::default().fg(Color::Red),
                )));
            }
        }

        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(vec![
            Span::styled("j/↓", Style::default().fg(Color::Yellow)),
            Span::raw(": Scroll down  "),
            Span::styled("k/↑", Style::default().fg(Color::Yellow)),
            Span::raw(": Scroll up  "),
            Span::styled("Esc/Enter/Tab/q", Style::default().fg(Color::Yellow)),
            Span::raw(": Close"),
        ]));

        let content_area = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(1)])
            .split(inner_area)[0];

        let paragraph = Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((self.scroll_offset, 0));

        frame.render_widget(paragraph, content_area);
    }
}

impl Default for ReportDetailModal {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_report() -> Report {
        let mut report = Report::new("task-001".to_string(), 0, "architect".to_string());
        report.summary = "Test summary".to_string();
        report.status = TaskStatus::Done;
        report
    }

    #[test]
    fn modal_starts_hidden() {
        let modal = ReportDetailModal::new();
        assert!(!modal.is_visible());
    }

    #[test]
    fn modal_becomes_visible_after_show() {
        let mut modal = ReportDetailModal::new();
        modal.show(create_test_report());
        assert!(modal.is_visible());
    }

    #[test]
    fn modal_becomes_hidden_after_hide() {
        let mut modal = ReportDetailModal::new();
        modal.show(create_test_report());
        modal.hide();
        assert!(!modal.is_visible());
    }

    #[test]
    fn scroll_offset_resets_on_show() {
        let mut modal = ReportDetailModal::new();
        modal.show(create_test_report());
        modal.scroll_down(100);
        modal.scroll_down(100);
        assert!(modal.scroll_offset > 0);

        modal.show(create_test_report());
        assert_eq!(modal.scroll_offset, 0);
    }

    #[test]
    fn scroll_up_does_not_go_negative() {
        let mut modal = ReportDetailModal::new();
        modal.show(create_test_report());
        modal.scroll_up();
        modal.scroll_up();
        assert_eq!(modal.scroll_offset, 0);
    }

    #[test]
    fn scroll_down_respects_max_lines() {
        let mut modal = ReportDetailModal::new();
        modal.show(create_test_report());
        modal.scroll_down(5);
        modal.scroll_down(5);
        modal.scroll_down(5);
        modal.scroll_down(5);
        modal.scroll_down(5);
        modal.scroll_down(5);
        assert!(modal.scroll_offset <= 5);
    }
}
