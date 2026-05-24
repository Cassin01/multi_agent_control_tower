//! Add-Expert modal for the tower TUI.
//!
//! Backs the F2 keybinding documented in dynamic-expert-add-design.md
//! §3.7. The modal collects:
//!
//! - Role (select among architect / planner / general / <custom>)
//! - Name (optional text input — auto-picked from the literary pool when
//!   empty)
//! - Worktree (checkbox flag passed through to the add service)
//!
//! Keybindings:
//! - `Tab` / `Shift+Tab`: cycle focus between fields.
//! - `Up` / `Down` (Role focus): change role selection.
//! - typed chars / `Backspace` (Name focus): edit name.
//! - `Space` (Worktree focus): toggle worktree.
//! - `Enter`: submit. The host (`TowerApp`) reads the form state via
//!   [`AddExpertModal::form`] and dispatches the add.
//! - `Esc`: cancel.
//!
//! Submission and the actual `add_expert` call live in `TowerApp` —
//! the widget is a pure state container so it stays unit-testable.
//!
//! `Ctrl+A` (move-to-line-start) and `Ctrl+N` (next-line) were
//! considered for the open binding but rejected: both collide with
//! the task input's emacs-style cursor controls (`handle_task_input_keys`
//! in `tower::app`). `F2` has no existing assignment and is reachable
//! on tenkey-less keyboards.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

/// The role choices the modal shows. Maps 1:1 to
/// [`crate::expert::role::RoleSpec`] when the host dispatches the add.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalRole {
    Architect,
    Planner,
    General,
    Custom,
}

impl ModalRole {
    pub const ALL: [ModalRole; 4] = [
        ModalRole::Architect,
        ModalRole::Planner,
        ModalRole::General,
        ModalRole::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ModalRole::Architect => "architect",
            ModalRole::Planner => "planner",
            ModalRole::General => "general",
            ModalRole::Custom => "<custom>",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModalField {
    Role,
    Name,
    CustomRole,
    Worktree,
}

/// Snapshot of the form state — used by the host to dispatch the add.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddExpertForm {
    pub role: ModalRole,
    /// Custom role name when `role == ModalRole::Custom`.
    pub custom_role: String,
    pub name: String,
    pub worktree: bool,
}

pub struct AddExpertModal {
    visible: bool,
    role_index: usize,
    custom_role: String,
    name: String,
    worktree: bool,
    focus: ModalField,
}

impl Default for AddExpertModal {
    fn default() -> Self {
        Self::new()
    }
}

impl AddExpertModal {
    pub fn new() -> Self {
        Self {
            visible: false,
            role_index: 2, // default to "general" per design
            custom_role: String::new(),
            name: String::new(),
            worktree: false,
            focus: ModalField::Role,
        }
    }

    pub fn show(&mut self) {
        self.visible = true;
        self.role_index = 2;
        self.custom_role.clear();
        self.name.clear();
        self.worktree = false;
        self.focus = ModalField::Role;
    }

    pub fn hide(&mut self) {
        self.visible = false;
    }

    pub fn is_visible(&self) -> bool {
        self.visible
    }

    pub fn focus(&self) -> ModalField {
        self.focus
    }

    pub fn role(&self) -> ModalRole {
        ModalRole::ALL[self.role_index]
    }

    #[allow(dead_code)]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[allow(dead_code)]
    pub fn custom_role(&self) -> &str {
        &self.custom_role
    }

    #[allow(dead_code)]
    pub fn worktree(&self) -> bool {
        self.worktree
    }

    /// Snapshot the current state. The host dispatches the add based on
    /// this value.
    pub fn form(&self) -> AddExpertForm {
        AddExpertForm {
            role: self.role(),
            custom_role: self.custom_role.clone(),
            name: self.name.clone(),
            worktree: self.worktree,
        }
    }

    /// Cycle focus forward. Skips `CustomRole` unless the role is
    /// `Custom`.
    pub fn next_focus(&mut self) {
        self.focus = match self.focus {
            ModalField::Role => {
                if self.role() == ModalRole::Custom {
                    ModalField::CustomRole
                } else {
                    ModalField::Name
                }
            }
            ModalField::CustomRole => ModalField::Name,
            ModalField::Name => ModalField::Worktree,
            ModalField::Worktree => ModalField::Role,
        };
    }

    /// Cycle focus backward.
    pub fn prev_focus(&mut self) {
        self.focus = match self.focus {
            ModalField::Role => ModalField::Worktree,
            ModalField::CustomRole => ModalField::Role,
            ModalField::Name => {
                if self.role() == ModalRole::Custom {
                    ModalField::CustomRole
                } else {
                    ModalField::Role
                }
            }
            ModalField::Worktree => ModalField::Name,
        };
    }

    pub fn next_role(&mut self) {
        self.role_index = (self.role_index + 1) % ModalRole::ALL.len();
    }

    pub fn prev_role(&mut self) {
        self.role_index = if self.role_index == 0 {
            ModalRole::ALL.len() - 1
        } else {
            self.role_index - 1
        };
    }

    pub fn type_char(&mut self, c: char) {
        match self.focus {
            ModalField::Name => self.name.push(c),
            ModalField::CustomRole => self.custom_role.push(c),
            _ => {}
        }
    }

    pub fn backspace(&mut self) {
        match self.focus {
            ModalField::Name => {
                self.name.pop();
            }
            ModalField::CustomRole => {
                self.custom_role.pop();
            }
            _ => {}
        }
    }

    pub fn toggle_worktree(&mut self) {
        if self.focus == ModalField::Worktree {
            self.worktree = !self.worktree;
        }
    }

    pub fn render(&self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }

        let popup_width = 60.min(area.width.saturating_sub(4));
        let popup_height = 14.min(area.height.saturating_sub(4));
        let popup = centered_rect(popup_width, popup_height, area);

        frame.render_widget(Clear, popup);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(3), // header
                Constraint::Length(2), // role row
                Constraint::Length(2), // custom role row
                Constraint::Length(2), // name row
                Constraint::Length(2), // worktree row
                Constraint::Min(1),    // footer
            ])
            .split(popup);

        let header = Paragraph::new(Line::from(vec![Span::styled(
            "Add Expert",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(header, chunks[0]);

        let role_marker = if self.focus == ModalField::Role {
            "▶"
        } else {
            " "
        };
        let role_line = Line::from(vec![
            Span::styled(
                format!("{role_marker} Role: "),
                focus_style(self.focus == ModalField::Role),
            ),
            Span::styled(self.role().label(), Style::default().fg(Color::Yellow)),
            Span::raw("   (Up/Down to change)"),
        ]);
        frame.render_widget(Paragraph::new(role_line), chunks[1]);

        let custom_marker = if self.focus == ModalField::CustomRole {
            "▶"
        } else {
            " "
        };
        let custom_line = Line::from(vec![
            Span::styled(
                format!("{custom_marker} Custom : "),
                focus_style(self.focus == ModalField::CustomRole),
            ),
            Span::styled(
                if self.custom_role.is_empty() {
                    "<role-template-name>"
                } else {
                    &self.custom_role
                },
                if self.custom_role.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]);
        frame.render_widget(Paragraph::new(custom_line), chunks[2]);

        let name_marker = if self.focus == ModalField::Name {
            "▶"
        } else {
            " "
        };
        let name_line = Line::from(vec![
            Span::styled(
                format!("{name_marker} Name : "),
                focus_style(self.focus == ModalField::Name),
            ),
            Span::styled(
                if self.name.is_empty() {
                    "<auto-pick>"
                } else {
                    &self.name
                },
                if self.name.is_empty() {
                    Style::default().fg(Color::DarkGray)
                } else {
                    Style::default().fg(Color::White)
                },
            ),
        ]);
        frame.render_widget(Paragraph::new(name_line), chunks[3]);

        let wt_marker = if self.focus == ModalField::Worktree {
            "▶"
        } else {
            " "
        };
        let wt_state = if self.worktree { "[x]" } else { "[ ]" };
        let wt_line = Line::from(vec![
            Span::styled(
                format!("{wt_marker} Worktree : "),
                focus_style(self.focus == ModalField::Worktree),
            ),
            Span::styled(
                wt_state,
                Style::default().fg(if self.worktree {
                    Color::Green
                } else {
                    Color::Gray
                }),
            ),
            Span::raw("   (Space to toggle)"),
        ]);
        frame.render_widget(Paragraph::new(wt_line), chunks[4]);

        let footer = Paragraph::new(Line::from(vec![
            Span::styled("Tab", Style::default().fg(Color::Cyan)),
            Span::raw(": next field  "),
            Span::styled("Enter", Style::default().fg(Color::Cyan)),
            Span::raw(": confirm  "),
            Span::styled("Esc", Style::default().fg(Color::Cyan)),
            Span::raw(": cancel"),
        ]))
        .block(Block::default().borders(Borders::TOP));
        frame.render_widget(footer, chunks[5]);
    }
}

fn focus_style(focused: bool) -> Style {
    if focused {
        Style::default()
            .fg(Color::Black)
            .bg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    }
}

fn centered_rect(width: u16, height: u16, r: Rect) -> Rect {
    let x = r.x + (r.width.saturating_sub(width)) / 2;
    let y = r.y + (r.height.saturating_sub(height)) / 2;
    Rect::new(x, y, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modal_starts_hidden() {
        let m = AddExpertModal::new();
        assert!(!m.is_visible());
    }

    #[test]
    fn show_resets_state_and_defaults_to_general() {
        let mut m = AddExpertModal::new();
        m.show();
        assert!(m.is_visible());
        assert_eq!(m.role(), ModalRole::General);
        assert_eq!(m.focus(), ModalField::Role);
        assert!(!m.worktree());
        assert!(m.name().is_empty());
    }

    #[test]
    fn next_focus_skips_custom_unless_role_is_custom() {
        let mut m = AddExpertModal::new();
        m.show();
        // Role -> Name (skips CustomRole because role is General)
        m.next_focus();
        assert_eq!(m.focus(), ModalField::Name);
        m.next_focus();
        assert_eq!(m.focus(), ModalField::Worktree);
        m.next_focus();
        assert_eq!(m.focus(), ModalField::Role);
    }

    #[test]
    fn next_focus_includes_custom_when_role_is_custom() {
        let mut m = AddExpertModal::new();
        m.show();
        m.next_role(); // Architect -> Planner... wait, default is General (idx 2)
                       // 2 -> 3 (Custom)
        assert_eq!(m.role(), ModalRole::Custom);
        m.next_focus();
        assert_eq!(m.focus(), ModalField::CustomRole);
        m.next_focus();
        assert_eq!(m.focus(), ModalField::Name);
    }

    #[test]
    fn role_navigation_wraps() {
        let mut m = AddExpertModal::new();
        m.show();
        // start at General (idx 2)
        m.next_role();
        assert_eq!(m.role(), ModalRole::Custom);
        m.next_role();
        assert_eq!(m.role(), ModalRole::Architect);
        m.prev_role();
        assert_eq!(m.role(), ModalRole::Custom);
    }

    #[test]
    fn type_char_writes_to_focused_field_only() {
        let mut m = AddExpertModal::new();
        m.show();
        // Default focus is Role — typing should be ignored.
        m.type_char('X');
        assert!(m.name().is_empty());

        m.next_focus();
        assert_eq!(m.focus(), ModalField::Name);
        m.type_char('S');
        m.type_char('m');
        assert_eq!(m.name(), "Sm");
        m.backspace();
        assert_eq!(m.name(), "S");
    }

    #[test]
    fn type_char_writes_to_custom_role_when_focused() {
        let mut m = AddExpertModal::new();
        m.show();
        m.next_role(); // -> Custom
        m.next_focus(); // -> CustomRole
        assert_eq!(m.focus(), ModalField::CustomRole);
        m.type_char('q');
        m.type_char('a');
        assert_eq!(m.custom_role(), "qa");
    }

    #[test]
    fn toggle_worktree_only_when_focused() {
        let mut m = AddExpertModal::new();
        m.show();
        m.toggle_worktree();
        assert!(
            !m.worktree(),
            "toggle should be a no-op when not focused on Worktree"
        );

        m.next_focus(); // Name
        m.next_focus(); // Worktree
        m.toggle_worktree();
        assert!(m.worktree());
        m.toggle_worktree();
        assert!(!m.worktree());
    }

    #[test]
    fn form_snapshot_reflects_state() {
        let mut m = AddExpertModal::new();
        m.show();
        m.next_focus(); // -> Name
        m.type_char('A');
        m.type_char('l');
        let f = m.form();
        assert_eq!(f.role, ModalRole::General);
        assert_eq!(f.name, "Al");
        assert!(!f.worktree);
        assert!(f.custom_role.is_empty());
    }

    #[test]
    fn hide_does_not_clear_form_state() {
        // hide() preserves state so accidental Esc + reopen via show()
        // still resets cleanly via show() — only `show()` clears.
        let mut m = AddExpertModal::new();
        m.show();
        m.next_focus();
        m.type_char('Z');
        m.hide();
        assert!(!m.is_visible());
        // re-show() clears
        m.show();
        assert!(m.name().is_empty());
    }
}
