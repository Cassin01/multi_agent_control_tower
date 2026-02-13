use std::collections::{HashMap, HashSet};
use std::path::Path;

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

use crate::models::ExpertState;
use crate::utils::truncate_str_head;

#[derive(Debug, Clone)]
pub struct ExpertEntry {
    pub expert_id: u32,
    pub expert_name: String,
    pub state: ExpertState,
}

const WORKING_DIR_MAX_CHARS: usize = 25;

pub struct StatusDisplay {
    experts: Vec<ExpertEntry>,
    state: ListState,
    focused: bool,
    expert_roles: HashMap<u32, String>,
    expert_reports: HashSet<u32>,
    expert_working_dirs: HashMap<u32, String>,
    project_path: String,
    execution_badge: Option<String>,
}

impl StatusDisplay {
    pub fn new() -> Self {
        Self {
            experts: Vec::new(),
            state: ListState::default(),
            focused: false,
            expert_roles: HashMap::new(),
            expert_reports: HashSet::new(),
            expert_working_dirs: HashMap::new(),
            project_path: String::new(),
            execution_badge: None,
        }
    }

    pub fn set_experts(&mut self, experts: Vec<ExpertEntry>) {
        self.experts = experts;
    }

    #[allow(dead_code)]
    pub fn set_expert_role(&mut self, expert_id: u32, role: String) {
        self.expert_roles.insert(expert_id, role);
    }

    pub fn set_expert_roles(&mut self, roles: HashMap<u32, String>) {
        self.expert_roles = roles;
    }

    pub fn set_expert_reports(&mut self, ids: HashSet<u32>) {
        self.expert_reports = ids;
    }

    pub fn set_expert_working_dirs(&mut self, dirs: HashMap<u32, String>) {
        self.expert_working_dirs = dirs;
    }

    pub fn set_project_path(&mut self, path: String) {
        self.project_path = path;
    }

    pub fn set_execution_badge(&mut self, badge: Option<String>) {
        self.execution_badge = badge;
    }

    #[allow(dead_code)]
    pub fn execution_badge(&self) -> Option<&str> {
        self.execution_badge.as_deref()
    }

    fn format_relative_path(pane_path: &str, project_path: &str) -> String {
        if project_path.is_empty() || pane_path.is_empty() {
            return String::new();
        }
        let pane = Path::new(pane_path);
        let project = Path::new(project_path);
        match pane.strip_prefix(project) {
            Ok(rel) => {
                let rel_str = rel.to_string_lossy();
                let display = if rel_str.is_empty() {
                    "./".to_string()
                } else {
                    format!("./{}", rel_str)
                };
                truncate_str_head(&display, WORKING_DIR_MAX_CHARS)
            }
            Err(_) => truncate_str_head(pane_path, WORKING_DIR_MAX_CHARS),
        }
    }

    #[allow(dead_code)]
    pub fn has_report(&self, expert_id: u32) -> bool {
        self.expert_reports.contains(&expert_id)
    }

    fn report_symbol(has_report: bool) -> (&'static str, Color) {
        if has_report {
            ("󰧮", Color::Yellow)
        } else {
            (" ", Color::Reset)
        }
    }

    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }

    #[allow(dead_code)]
    pub fn is_focused(&self) -> bool {
        self.focused
    }

    pub fn next(&mut self) {
        if self.experts.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i >= self.experts.len() - 1 {
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
        if self.experts.is_empty() {
            return;
        }
        let i = match self.state.selected() {
            Some(i) => {
                if i == 0 {
                    self.experts.len() - 1
                } else {
                    i - 1
                }
            }
            None => 0,
        };
        self.state.select(Some(i));
    }

    pub fn selected(&self) -> Option<&ExpertEntry> {
        self.state.selected().and_then(|i| self.experts.get(i))
    }

    pub fn selected_expert_id(&self) -> Option<u32> {
        self.selected().map(|e| e.expert_id)
    }

    pub fn expert_count(&self) -> usize {
        self.experts.len()
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .experts
            .iter()
            .map(|entry| {
                let status_style = Style::default().fg(entry.state.color());

                let role = self.expert_roles.get(&entry.expert_id);
                let role_display = match role {
                    Some(r) => format!("{:<11}", format!("({})", r)),
                    None => format!("{:<11}", ""),
                };

                let (report_sym, report_color) =
                    Self::report_symbol(self.expert_reports.contains(&entry.expert_id));

                let working_dir_display =
                    match self.expert_working_dirs.get(&entry.expert_id) {
                        Some(dir) => Self::format_relative_path(dir, &self.project_path),
                        None => String::new(),
                    };

                let spans = vec![
                    Span::styled(
                        format!("[{}] ", entry.expert_id),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(entry.state.symbol(), status_style),
                    Span::raw(" "),
                    Span::styled(
                        format!("{:<8}", entry.expert_name),
                        Style::default().add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(role_display, Style::default().fg(Color::Cyan)),
                    Span::raw(" "),
                    Span::styled(report_sym, Style::default().fg(report_color)),
                    Span::raw(" "),
                    Span::styled(working_dir_display, Style::default().fg(Color::DarkGray)),
                ];

                ListItem::new(Line::from(spans))
            })
            .collect();

        let border_style = Style::default().fg(ratatui::style::Color::DarkGray);

        let title = match &self.execution_badge {
            Some(badge) => format!("Experts [{}]", badge),
            None => "Experts".to_string(),
        };

        let list = List::new(items)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(border_style)
                    .title(title),
            )
            .highlight_style(
                Style::default()
                    .add_modifier(Modifier::REVERSED)
                    .add_modifier(Modifier::BOLD),
            )
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.state);
    }

    pub fn get_status_summary(&self) -> StatusSummary {
        let mut summary = StatusSummary::default();

        for entry in &self.experts {
            match entry.state {
                ExpertState::Idle => summary.idle += 1,
                ExpertState::Busy => summary.busy += 1,
                ExpertState::Offline => summary.offline += 1,
            }
        }

        summary.total = self.experts.len();
        summary
    }
}

impl Default for StatusDisplay {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Default)]
pub struct StatusSummary {
    pub total: usize,
    pub idle: usize,
    pub busy: usize,
    pub offline: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_entry(id: u32, name: &str, state: ExpertState) -> ExpertEntry {
        ExpertEntry {
            expert_id: id,
            expert_name: name.to_string(),
            state,
        }
    }

    #[test]
    fn status_display_empty_by_default() {
        let display = StatusDisplay::new();
        assert!(display.selected().is_none());
    }

    #[test]
    fn status_display_navigation() {
        let mut display = StatusDisplay::new();
        display.set_experts(vec![
            create_test_entry(0, "architect", ExpertState::Idle),
            create_test_entry(1, "frontend", ExpertState::Busy),
            create_test_entry(2, "backend", ExpertState::Offline),
        ]);

        display.next();
        assert_eq!(display.selected_expert_id(), Some(0));

        display.next();
        assert_eq!(display.selected_expert_id(), Some(1));

        display.next();
        assert_eq!(display.selected_expert_id(), Some(2));

        display.next();
        assert_eq!(display.selected_expert_id(), Some(0));
    }

    #[test]
    fn status_display_prev_navigation() {
        let mut display = StatusDisplay::new();
        display.set_experts(vec![
            create_test_entry(0, "architect", ExpertState::Idle),
            create_test_entry(1, "frontend", ExpertState::Busy),
        ]);

        display.prev();
        assert_eq!(display.selected_expert_id(), Some(0));

        display.prev();
        assert_eq!(display.selected_expert_id(), Some(1));
    }

    #[test]
    fn status_display_summary() {
        let mut display = StatusDisplay::new();
        display.set_experts(vec![
            create_test_entry(0, "architect", ExpertState::Idle),
            create_test_entry(1, "frontend", ExpertState::Idle),
            create_test_entry(2, "backend", ExpertState::Busy),
            create_test_entry(3, "tester", ExpertState::Offline),
        ]);

        let summary = display.get_status_summary();
        assert_eq!(summary.total, 4);
        assert_eq!(summary.idle, 2);
        assert_eq!(summary.busy, 1);
        assert_eq!(summary.offline, 1);
    }

    #[test]
    fn status_display_focus_state() {
        let mut display = StatusDisplay::new();
        assert!(!display.is_focused());

        display.set_focused(true);
        assert!(display.is_focused());
    }

    #[test]
    fn selected_returns_expert_entry() {
        let mut display = StatusDisplay::new();
        display.set_experts(vec![create_test_entry(5, "devops", ExpertState::Busy)]);

        display.next();
        let selected = display.selected().unwrap();
        assert_eq!(selected.expert_id, 5);
        assert_eq!(selected.expert_name, "devops");
        assert_eq!(selected.state, ExpertState::Busy);
    }

    #[test]
    fn expert_count_returns_correct_count() {
        let mut display = StatusDisplay::new();
        assert_eq!(display.expert_count(), 0);

        display.set_experts(vec![
            create_test_entry(0, "a", ExpertState::Idle),
            create_test_entry(1, "b", ExpertState::Busy),
        ]);
        assert_eq!(display.expert_count(), 2);
    }

    #[test]
    fn set_expert_reports_stores_ids() {
        let mut display = StatusDisplay::new();
        let ids: HashSet<u32> = [1, 3].into_iter().collect();
        display.set_expert_reports(ids.clone());
        assert!(
            display.has_report(1),
            "has_report: should return true for stored expert id 1"
        );
        assert!(
            display.has_report(3),
            "has_report: should return true for stored expert id 3"
        );
        assert!(
            !display.has_report(0),
            "has_report: should return false for unstored expert id 0"
        );
        assert!(
            !display.has_report(2),
            "has_report: should return false for unstored expert id 2"
        );
    }

    #[test]
    fn set_expert_reports_empty_set() {
        let mut display = StatusDisplay::new();
        display.set_expert_reports(HashSet::new());
        assert!(
            !display.has_report(0),
            "has_report: should return false when reports set is empty"
        );
    }

    #[test]
    fn has_report_returns_correct_value() {
        let mut display = StatusDisplay::new();
        assert!(
            !display.has_report(5),
            "has_report: should return false before any reports are set"
        );

        let ids: HashSet<u32> = [5].into_iter().collect();
        display.set_expert_reports(ids);
        assert!(
            display.has_report(5),
            "has_report: should return true after setting report for id 5"
        );
        assert!(
            !display.has_report(99),
            "has_report: should return false for id not in the set"
        );
    }

    #[test]
    fn format_relative_path_same_dir() {
        let result = StatusDisplay::format_relative_path("/home/user/project", "/home/user/project");
        assert_eq!(
            result, "./",
            "format_relative_path: same directory should return ./"
        );
    }

    #[test]
    fn format_relative_path_subdirectory() {
        let result = StatusDisplay::format_relative_path(
            "/home/user/project/src/main",
            "/home/user/project",
        );
        assert_eq!(
            result, "./src/main",
            "format_relative_path: subdirectory should return relative path"
        );
    }

    #[test]
    fn format_relative_path_unrelated() {
        let result = StatusDisplay::format_relative_path("/other/path", "/home/user/project");
        assert_eq!(
            result, "/other/path",
            "format_relative_path: unrelated path should return absolute path"
        );
    }

    #[test]
    fn format_relative_path_truncates_long_path() {
        let result = StatusDisplay::format_relative_path(
            "/home/user/project/.macot/worktrees/very-long-branch-name",
            "/home/user/project",
        );
        assert!(
            result.chars().count() <= WORKING_DIR_MAX_CHARS,
            "format_relative_path: should truncate to max {} chars, got {}",
            WORKING_DIR_MAX_CHARS,
            result.chars().count()
        );
        assert!(
            result.starts_with("..."),
            "format_relative_path: truncated path should start with ..."
        );
    }

    #[test]
    fn format_relative_path_empty_inputs() {
        assert_eq!(
            StatusDisplay::format_relative_path("", "/home/user/project"),
            "",
            "format_relative_path: empty pane path should return empty string"
        );
        assert_eq!(
            StatusDisplay::format_relative_path("/home/user/project", ""),
            "",
            "format_relative_path: empty project path should return empty string"
        );
    }

    // --- Task 11.1: Execution badge rendering tests ---

    #[test]
    fn execution_badge_shows_feature_name_when_set() {
        let mut display = StatusDisplay::new();
        display.set_execution_badge(Some("> my-feature".to_string()));
        assert_eq!(
            display.execution_badge(),
            Some("> my-feature"),
            "execution_badge: should return set badge value"
        );
    }

    #[test]
    fn execution_badge_shows_resetting_when_set() {
        let mut display = StatusDisplay::new();
        display.set_execution_badge(Some("~ resetting...".to_string()));
        assert_eq!(
            display.execution_badge(),
            Some("~ resetting..."),
            "execution_badge: should return resetting badge"
        );
    }

    #[test]
    fn execution_badge_absent_when_none() {
        let display = StatusDisplay::new();
        assert_eq!(
            display.execution_badge(),
            None,
            "execution_badge: should be None by default"
        );
    }

    #[test]
    fn execution_badge_cleared_when_set_to_none() {
        let mut display = StatusDisplay::new();
        display.set_execution_badge(Some("> feature".to_string()));
        display.set_execution_badge(None);
        assert_eq!(
            display.execution_badge(),
            None,
            "execution_badge: should be None after clearing"
        );
    }
}
