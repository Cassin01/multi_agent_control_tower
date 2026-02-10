use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::models::{Report, TaskStatus};
use crate::utils::truncate_str;

use super::report_detail_modal::ReportDetailModal;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    List,
    Detail,
}

pub struct ReportDisplay {
    reports: Vec<Report>,
    state: ListState,
    focused: bool,
    view_mode: ViewMode,
    detail_modal: ReportDetailModal,
}

impl ReportDisplay {
    pub fn new() -> Self {
        Self {
            reports: Vec::new(),
            state: ListState::default(),
            focused: false,
            view_mode: ViewMode::List,
            detail_modal: ReportDetailModal::new(),
        }
    }

    pub fn view_mode(&self) -> ViewMode {
        self.view_mode
    }

    pub fn selected_report(&self) -> Option<&Report> {
        self.state.selected().and_then(|i| self.reports.get(i))
    }

    pub fn open_detail(&mut self) {
        if let Some(report) = self.selected_report().cloned() {
            self.detail_modal.show(report);
            self.view_mode = ViewMode::Detail;
        }
    }

    pub fn open_detail_for_expert(&mut self, expert_id: u32) -> bool {
        if let Some(report) = self.reports.iter().find(|r| r.expert_id == expert_id).cloned() {
            self.detail_modal.show(report);
            self.view_mode = ViewMode::Detail;
            true
        } else {
            false
        }
    }

    pub fn close_detail(&mut self) {
        self.detail_modal.hide();
        self.view_mode = ViewMode::List;
    }

    pub fn scroll_up(&mut self) {
        self.detail_modal.scroll_up();
    }

    pub fn scroll_down(&mut self) {
        self.detail_modal.scroll_down(100);
    }

    pub fn render_detail_modal(&self, frame: &mut Frame, area: Rect) {
        self.detail_modal.render(frame, area);
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

                let summary = if report.summary.is_empty() {
                    "In progress...".to_string()
                } else {
                    truncate_str(&report.summary, 40)
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

    #[test]
    fn report_display_starts_in_list_mode() {
        let display = ReportDisplay::new();
        assert_eq!(display.view_mode(), ViewMode::List);
    }

    #[test]
    fn report_display_open_detail_switches_to_detail_mode() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![create_test_report(
            0,
            "architect",
            TaskStatus::Done,
            "Completed",
        )]);
        display.next();
        display.open_detail();
        assert_eq!(display.view_mode(), ViewMode::Detail);
    }

    #[test]
    fn report_display_close_detail_switches_to_list_mode() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![create_test_report(
            0,
            "architect",
            TaskStatus::Done,
            "Completed",
        )]);
        display.next();
        display.open_detail();
        display.close_detail();
        assert_eq!(display.view_mode(), ViewMode::List);
    }

    #[test]
    fn report_display_selected_report_returns_current() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![
            create_test_report(0, "architect", TaskStatus::Done, "First"),
            create_test_report(1, "frontend", TaskStatus::InProgress, "Second"),
        ]);
        display.next();
        let selected = display.selected_report();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().expert_name, "architect");

        display.next();
        let selected = display.selected_report();
        assert!(selected.is_some());
        assert_eq!(selected.unwrap().expert_name, "frontend");
    }

    #[test]
    fn report_display_open_detail_without_selection_stays_in_list() {
        let mut display = ReportDisplay::new();
        display.open_detail();
        assert_eq!(display.view_mode(), ViewMode::List);
    }

    #[test]
    fn open_detail_for_expert_opens_matching_report() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![
            create_test_report(0, "architect", TaskStatus::Done, "First"),
            create_test_report(1, "frontend", TaskStatus::InProgress, "Second"),
        ]);

        let result = display.open_detail_for_expert(1);
        assert!(
            result,
            "open_detail_for_expert: should return true when report exists"
        );
        assert_eq!(
            display.view_mode(),
            ViewMode::Detail,
            "open_detail_for_expert: should switch to Detail mode"
        );
    }

    #[test]
    fn open_detail_for_expert_returns_false_when_no_report() {
        let mut display = ReportDisplay::new();
        display.set_reports(vec![create_test_report(
            0,
            "architect",
            TaskStatus::Done,
            "First",
        )]);

        let result = display.open_detail_for_expert(99);
        assert!(
            !result,
            "open_detail_for_expert: should return false when no report exists"
        );
        assert_eq!(
            display.view_mode(),
            ViewMode::List,
            "open_detail_for_expert: should remain in List mode"
        );
    }
}
