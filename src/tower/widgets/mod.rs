mod expert_panel_display;
mod help_modal;
mod messaging_display;
mod report_detail_modal;
mod report_display;
mod role_selector;
mod status_display;
mod task_input;

pub use expert_panel_display::ExpertPanelDisplay;
pub use help_modal::HelpModal;
#[allow(unused_imports)]
pub use messaging_display::{MessageFilter, MessagingDisplay};
pub use report_display::{ReportDisplay, ViewMode};
pub use role_selector::RoleSelector;
pub use status_display::{ExpertEntry, StatusDisplay};
pub use task_input::TaskInput;

use ratatui::widgets::ListState;

/// Advance selection to the next item in a circular list.
pub fn select_next(state: &mut ListState, item_count: usize) {
    if item_count == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) => {
            if i >= item_count - 1 {
                0
            } else {
                i + 1
            }
        }
        None => 0,
    };
    state.select(Some(i));
}

/// Move selection to the previous item in a circular list.
pub fn select_prev(state: &mut ListState, item_count: usize) {
    if item_count == 0 {
        return;
    }
    let i = match state.selected() {
        Some(i) => {
            if i == 0 {
                item_count - 1
            } else {
                i - 1
            }
        }
        None => 0,
    };
    state.select(Some(i));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn select_next_empty_is_noop() {
        let mut state = ListState::default();
        select_next(&mut state, 0);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn select_next_wraps_around() {
        let mut state = ListState::default();
        select_next(&mut state, 3);
        assert_eq!(state.selected(), Some(0));
        select_next(&mut state, 3);
        assert_eq!(state.selected(), Some(1));
        select_next(&mut state, 3);
        assert_eq!(state.selected(), Some(2));
        select_next(&mut state, 3);
        assert_eq!(state.selected(), Some(0));
    }

    #[test]
    fn select_prev_empty_is_noop() {
        let mut state = ListState::default();
        select_prev(&mut state, 0);
        assert_eq!(state.selected(), None);
    }

    #[test]
    fn select_prev_wraps_around() {
        let mut state = ListState::default();
        select_prev(&mut state, 3);
        assert_eq!(state.selected(), Some(0));
        select_prev(&mut state, 3);
        assert_eq!(state.selected(), Some(2));
        select_prev(&mut state, 3);
        assert_eq!(state.selected(), Some(1));
    }
}
