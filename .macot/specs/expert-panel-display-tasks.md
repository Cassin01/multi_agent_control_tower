# Implementation Plan: Expert Panel Display

## Overview

This plan decomposes the Expert Panel Display feature into incremental TDD tasks. The build order is bottom-up: dependency (`ansi-to-tui`) → widget struct → tmux trait extension → key conversion utility → focus/layout integration → event handling → UI rendering → help/footer hints. Each implementation task is paired with a test task, and checkpoints validate accumulated progress.

## Requirements Traceability Key

The design document defines requirements implicitly through its correctness properties (P1–P11). All tasks reference these properties.

## Tasks

- [x] 1. Add `ansi-to-tui` dependency
  - Add `ansi-to-tui = "8"` to `[dependencies]` in `Cargo.toml`
  - Run `cargo check` to verify the dependency resolves and is compatible with `ratatui = "0.29"`
  - _Requirements: P10 (ANSI Fallback)_

- [x] 2. Create `ExpertPanelDisplay` widget with core state management
  - Create `src/tower/widgets/expert_panel_display.rs` (new file)
  - Implement struct fields: `expert_id`, `expert_name`, `content`, `raw_line_count`, `scroll_offset`, `visible`, `focused`, `auto_scroll`
  - Implement visibility methods: `toggle()`, `is_visible()`, `show()`, `hide()`
  - Implement focus methods: `set_focused(bool)`, `is_focused()`
  - Implement expert tracking: `set_expert(id, name)`, `expert_id()` — resets content/scroll on expert change
  - Implement scroll methods: `scroll_up()`, `scroll_down()`, `scroll_to_bottom()`
  - Implement content setter: `set_content(text, line_count)` with auto-scroll behavior
  - Implement `render(&mut self, frame, area)` — renders `Paragraph` with scroll, styled border (Cyan when focused)
  - _Requirements: P2, P4, P5, P8, P9_

  - [x] 2.1 Write unit tests for `ExpertPanelDisplay` state management
    - `panel_starts_hidden` — new panel has `is_visible() == false`
    - `toggle_makes_visible` / `toggle_twice_returns_to_hidden` — toggle idempotency
    - `starts_unfocused` / `set_focused_changes_state` — focus state transitions
    - `set_expert_tracks_id` — `expert_id()` returns set value
    - `set_expert_different_resets_scroll` — changing expert resets `scroll_offset` to 0 and clears content
    - `set_expert_same_preserves_scroll` — same expert ID preserves scroll position
    - `scroll_up_at_zero_stays_zero` — no underflow
    - `scroll_down_increments` — scroll offset increases
    - `scroll_to_bottom_enables_auto_scroll` / `scroll_up_disables_auto_scroll` — auto-scroll flag
    - **Property P9: Toggle Idempotency**
    - **Property P4: Scroll Bounds**
    - **Property P8: Content Reset on Expert Change**
    - **Property P2: Focus-Visibility Coherence (widget-level)**
    - **Validates: Requirements P2, P4, P5, P8, P9**

- [x] 3. Write ANSI parsing tests and integration
  - `ansi_parse_plain_text` — plain text converts to `Text` without error
  - `ansi_parse_colored_text` — ANSI color codes produce styled `Text`
  - `ansi_parse_malformed_does_not_panic` — malformed ANSI input falls back to raw text, no panic
  - **Property P10: ANSI Fallback**
  - **Validates: Requirements P10**

- [x] 4. Checkpoint — Widget and ANSI parsing
  - Run `make test` to ensure all `ExpertPanelDisplay` widget tests and ANSI parsing tests pass
  - Run `make` (clippy) to ensure no warnings
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Extend `TmuxSender` trait with `capture_pane_with_escapes`
  - Add `capture_pane_with_escapes(&self, pane_id: u32) -> Result<String>` to `TmuxSender` trait in `src/session/tmux.rs` with default implementation that falls back to `capture_pane`
  - Implement concrete `TmuxManager` version using `tmux capture-pane -e -p -t {session}:0.{pane_id}` (the `-e` flag preserves ANSI escape sequences)
  - _Requirements: P10_

  - [x] 5.1 Write test for `capture_pane_with_escapes` default fallback
    - `capture_pane_with_escapes_default_falls_back` — a mock `TmuxSender` that only implements `capture_pane` still works via default impl
    - **Property P10: ANSI Fallback**
    - **Validates: Requirements P10**

- [x] 6. Implement `keycode_to_tmux_key` conversion utility
  - Add private function `keycode_to_tmux_key(code: KeyCode, modifiers: KeyModifiers) -> Option<String>` in `src/tower/app.rs`
  - Implement full conversion table per design §3.3: plain chars, Shift chars, Ctrl prefix, Enter, Backspace, Tab, BackTab, Esc, arrow keys, Home, End
  - Return `None` only for `PageUp`/`PageDown` (reserved for local panel scroll)
  - _Requirements: P7_

  - [x] 6.1 Write unit tests for `keycode_to_tmux_key`
    - `keycode_to_tmux_key_char` — `Char('a')` → `"a"`
    - `keycode_to_tmux_key_ctrl_char` — `Ctrl+Char('c')` → `"C-c"`
    - `keycode_to_tmux_key_enter` — `Enter` → `"Enter"`
    - `keycode_to_tmux_key_backspace` — `Backspace` → `"BSpace"`
    - `keycode_to_tmux_key_tab_returns_tab_string` — `Tab` → `"Tab"` (NOT None)
    - `keycode_to_tmux_key_backtab_returns_btab` — `BackTab` → `"BTab"`
    - `keycode_to_tmux_key_esc_returns_escape_string` — `Esc` → `"Escape"` (NOT None)
    - `keycode_to_tmux_key_page_up_returns_none` — `PageUp` → `None`
    - `keycode_to_tmux_key_page_down_returns_none` — `PageDown` → `None`
    - `keycode_to_tmux_key_arrows` — Up/Down/Left/Right → corresponding strings
    - **Property P7: Input Isolation**
    - **Validates: Requirements P7**

- [x] 7. Checkpoint — Trait extension and key conversion
  - Run `make test` to ensure all tests pass including new tmux and key conversion tests
  - Ensure all tests pass, ask the user if questions arise.

- [x] 8. Extend `FocusArea` and `LayoutAreas` for `ExpertPanel`
  - Add `ExpertPanel` variant to `FocusArea` enum in `src/tower/app.rs:29`
  - Add `expert_panel: Rect` field to `LayoutAreas` struct in `src/tower/app.rs:37`
  - _Requirements: P1, P2, P3, P11_

- [x] 9. Add `ExpertPanelDisplay` field and accessors to `TowerApp`
  - Add `expert_panel_display: ExpertPanelDisplay` and `last_panel_poll: Instant` fields to `TowerApp` struct
  - Initialize both in `TowerApp::new()`
  - Add accessor: `expert_panel_display(&mut self) -> &mut ExpertPanelDisplay`
  - _Requirements: P1, P5_

- [x] 10. Update focus cycling to conditionally include `ExpertPanel`
  - Update `next_focus()`: when panel is visible, cycle `TaskInput → ExpertPanel → EffortSelector → ReportList → TaskInput`; when hidden, skip `ExpertPanel`
  - Update `prev_focus()`: reverse cycle with same conditional logic
  - Update `update_focus()`: add `self.expert_panel_display.set_focused(self.focus == FocusArea::ExpertPanel)`
  - _Requirements: P2, P3_

  - [x] 10.1 Write focus cycling tests
    - `focus_cycle_without_panel_skips_expert_panel` — when panel hidden, N `next_focus()` calls return to start without visiting `ExpertPanel`
    - `focus_cycle_with_panel_includes_expert_panel` — when panel visible, cycle includes `ExpertPanel` between `TaskInput` and `EffortSelector`
    - `hiding_panel_while_focused_moves_to_task_input` — hiding while `focus == ExpertPanel` sets focus to `TaskInput`
    - Property test: focus cycle roundtrip for arbitrary panel visibility
    - **Property P3: Focus Cycle Completeness**
    - **Property P2: Focus-Visibility Coherence**
    - **Validates: Requirements P2, P3**

- [x] 11. Update mouse click handling
  - Update `handle_mouse_click()` in `src/tower/app.rs` to check `expert_panel` rect when panel is visible
  - Ensure `Rect::default()` (zero rect) does not match clicks when panel is hidden
  - _Requirements: P11_

  - [x] 11.1 Write mouse click test
    - `mouse_click_does_not_match_zero_rect` — clicking at (0,0) with zero `expert_panel` rect does not set focus to `ExpertPanel`
    - `toggle_panel_visibility` — toggling changes layout area count (6 vs 7 constraints verified indirectly through LayoutAreas content)
    - **Property P11: LayoutAreas Zero Rect**
    - **Property P1: Visibility-Layout**
    - **Validates: Requirements P1, P11**

- [x] 12. Checkpoint — Focus, layout, mouse integration
  - Run `make test` to ensure all focus cycling, mouse click, and layout tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [x] 13. Implement `handle_expert_panel_keys` method
  - Add async method `handle_expert_panel_keys(&mut self, code: KeyCode, modifiers: KeyModifiers) -> Result<()>` to `TowerApp`
  - Intercept `PageUp`/`PageDown` for local scroll (NOT forwarded to tmux)
  - Forward all other keys via `keycode_to_tmux_key` → `self.claude.send_keys(expert_id, &tmux_key).await`
  - Log warning and show status message on `send_keys` failure
  - _Requirements: P7_

- [x] 14. Implement `poll_expert_panel` method
  - Add async method `poll_expert_panel()` to `TowerApp`
  - Poll gating: skip when (a) panel hidden, (b) input within 500ms, (c) <1000ms since last poll
  - Call `capture_pane_with_escapes` on selected expert's pane
  - Parse ANSI via `ansi_to_tui::IntoText`, fall back to `Text::raw()` on parse error
  - Call `expert_panel_display.set_content(text, line_count)`
  - Sync `expert_panel_display.expert_id` with `status_display.selected_expert_id()` — reset on change
  - _Requirements: P5, P6, P10_

- [x] 15. Integrate `Ctrl+J` toggle and `ExpertPanel` arm in `handle_events`
  - Add `Ctrl+J` to global Ctrl handler block (before focus dispatch): toggles panel visibility
  - On hide: if focus was `ExpertPanel`, move to `TaskInput` (P2)
  - Add `ExpertPanel` match arm in focus-specific dispatch with `.await` and early `return Ok(())`
  - The early return ensures Tab, Esc, and all other keys are forwarded, not leaked to downstream handlers
  - _Requirements: P2, P7_

- [x] 16. Add `poll_expert_panel().await` call to `run()` main loop
  - Insert `self.poll_expert_panel().await;` in the main event loop in `run()`
  - _Requirements: P6_

- [x] 17. Checkpoint — Async event handling and polling
  - Run `make test` to ensure all tests still pass after async integration
  - Run `make` (clippy) to verify no warnings
  - Ensure all tests pass, ask the user if questions arise.

- [x] 18. Implement conditional layout in `UI::render`
  - When panel hidden: 6 layout constraints (unchanged from current behavior)
  - When panel visible: 7 layout constraints — insert `ExpertPanel` (Percentage 40) after `TaskInput` (Min 5, shrunk from 8), shrink `ReportDisplay` (Length 4 from 6)
  - Set `expert_panel: Rect` in `LayoutAreas` (or `Rect::default()` when hidden)
  - Call `expert_panel_display.render(frame, chunks[3])` when visible
  - _Requirements: P1, P11_

- [x] 19. Register module in `widgets/mod.rs`
  - Add `mod expert_panel_display;` and `pub use expert_panel_display::ExpertPanelDisplay;`
  - _Requirements: (structural)_

- [x] 20. Update footer with `Ctrl+J: Panel` hint
  - Add `Ctrl+J` hint to `render_footer` in `src/tower/ui.rs`
  - _Requirements: (UX)_

- [x] 21. Update help modal with `Ctrl+J: Toggle expert panel`
  - Add `Ctrl+J: Toggle expert panel` line to `build_help_lines()` in `src/tower/widgets/help_modal.rs`
  - _Requirements: (UX)_

  - [x] 21.1 Write test for help modal update
    - Verify `build_help_lines()` output contains `Ctrl+J`
    - **Validates: (UX completeness)**

- [x] 22. Final checkpoint — Full integration validation
  - Run `make test` — all existing + new tests pass
  - Run `make` — compiles without warnings (clippy)
  - Manual verification checklist:
    - Run `macot tower`, press `Ctrl+J` → panel appears below Task Input
    - Select an expert, verify pane content appears with colors
    - Focus panel (Tab from TaskInput when panel visible), type characters → forwarded to expert pane
    - `Tab` and `Esc` in panel are forwarded to tmux (not intercepted)
    - `PageUp`/`PageDown` scrolls panel content locally
    - `Ctrl+J` again → panel hides, focus returns to TaskInput
    - Resize terminal → layout degrades gracefully
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- **TDD discipline**: Tasks 2.1, 3, 5.1, 6.1, 10.1, 11.1, 21.1 are RED-phase test tasks that must be written before their corresponding implementation code turns them GREEN.
- **Async consideration**: `handle_expert_panel_keys` uses `.await` inside the already-async `handle_events()`. The `send_keys` call is lightweight (~1ms tmux command) and safe in the event loop.
- **ansi-to-tui v8**: Verified compatible with ratatui 0.29 per design document.
- **Focus isolation**: The `ExpertPanel` arm in `handle_events` uses early `return Ok(())` to prevent key leakage to Tab cycling and TaskInput-specific handlers.
- **Files affected**: `Cargo.toml`, `src/tower/widgets/expert_panel_display.rs` (NEW), `src/tower/widgets/mod.rs`, `src/tower/app.rs`, `src/tower/ui.rs`, `src/tower/widgets/help_modal.rs`, `src/session/tmux.rs` — 7 files total.
