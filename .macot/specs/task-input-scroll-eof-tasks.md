# Implementation Plan: TaskInput Vim-like Scrolling and EOF Indicator

## Overview

This plan decomposes the scrolling and EOF indicator feature into incremental TDD steps. Each phase adds a single testable capability: first the data model helpers (`cursor_line_col`, `line_count`), then the viewport logic (`ensure_cursor_visible`, `scroll_offset`), then the EOF indicator, and finally the render signature change and caller integration. Tests precede implementation per the project's Red-Green-Refactor cycle.

## Tasks

- [x] 1. Add `scroll_offset` field to `TaskInput`
  - Add `scroll_offset: usize` field to the `TaskInput` struct in `src/tower/widgets/task_input.rs`
  - Initialize to `0` in `TaskInput::new()`
  - Reset to `0` in `TaskInput::clear()`
  - _Requirements: R5 (Scroll Reset on Clear)_

- [x] 1.1 Write test for `scroll_offset` initialization and clear reset
  - **Property 5: Scroll Reset on Clear**
  - Verify `scroll_offset` is `0` after `new()` and after `clear()` on scrolled state
  - **Validates: Requirements R5**

- [x] 2. Add `cursor_line_col()` helper method
  - Implement `fn cursor_line_col(&self) -> (usize, usize)` in `src/tower/widgets/task_input.rs`
  - Count newlines before `cursor_position` to derive line index (0-based)
  - Compute column as offset from the last newline (or content start)
  - Must operate on character indices, not byte indices
  - _Requirements: R1, R7 (Unicode Safety)_

- [x] 2.1 Write tests for `cursor_line_col()`
  - **Property 7: Unicode Safety**
  - Test: empty buffer returns `(0, 0)`
  - Test: single-line content "hello" with cursor at end returns `(0, 5)`
  - Test: multi-line "abc\ndef" with cursor at end returns `(1, 3)`
  - Test: cursor at start of second line returns `(1, 0)`
  - Test: Japanese multi-line content returns correct line/col
  - Test: trailing newline "abc\n" with cursor at end returns `(1, 0)`
  - **Validates: Requirements R1, R7**

- [x] 3. Add `line_count()` helper method
  - Implement `fn line_count(&self) -> usize` in `src/tower/widgets/task_input.rs`
  - Empty buffer returns `1`
  - Trailing newline adds an extra empty line
  - _Requirements: R3, R4_

- [x] 3.1 Write tests for `line_count()`
  - **Property 3: EOF Always Present** (line_count drives EOF positioning)
  - Test: empty buffer returns `1`
  - Test: "hello" returns `1`
  - Test: "a\nb" returns `2`
  - Test: "a\nb\n" (trailing newline) returns `3`
  - Test: Japanese content returns correct count
  - **Validates: Requirements R3, R4**

- [x] 4. Checkpoint - Verify helper methods
  - Run `make test` to ensure all new unit tests pass
  - Verify no regressions in existing tests

- [x] 5. Add `ensure_cursor_visible()` method
  - Implement `fn ensure_cursor_visible(&mut self, visible_height: usize)` in `src/tower/widgets/task_input.rs`
  - Clamp `scroll_offset` to valid range: `scroll_offset = scroll_offset.min(total_lines.saturating_sub(visible_height))`
  - If `cursor_line < scroll_offset`: set `scroll_offset = cursor_line`
  - If `cursor_line >= scroll_offset + visible_height`: set `scroll_offset = cursor_line - visible_height + 1`
  - No-op when `visible_height == 0`
  - Total lines = `line_count() + 1` (content lines + EOF line)
  - _Requirements: R1, R2, R6_

- [x] 5.1 Write tests for `ensure_cursor_visible()`
  - **Property 1: Cursor Always Visible**
  - Test: scroll follows cursor down (N lines > visible_height, cursor at last line)
  - Test: scroll follows cursor up (scroll_offset at bottom, cursor moved to first line)
  - **Property 2: Scroll Minimum Movement**
  - Test: cursor in middle of viewport, move one line down — `scroll_offset` unchanged
  - **Property 5: Scroll Reset on Clear**
  - Test: after `clear()`, `scroll_offset` resets to `0`
  - **Property 6: Layout Independence**
  - Test: same content renders correctly with `visible_height = 3` (compact) and `visible_height = 6` (expanded)
  - Test: `visible_height = 0` is a no-op
  - Test: `scroll_offset` overflow after content deletion is clamped
  - **Validates: Requirements R1, R2, R5, R6**

- [x] 6. Checkpoint - Verify scroll logic
  - Run `make test` to ensure all scroll-related tests pass
  - Verify no regressions in existing tests

- [x] 7. Change `render()` signature to `&mut self` and add scroll + EOF rendering
  - Change `pub fn render(&self, ...)` to `pub fn render(&mut self, ...)` in `src/tower/widgets/task_input.rs`
  - Compute `visible_height = area.height.saturating_sub(2)` (minus borders)
  - Call `self.ensure_cursor_visible(visible_height as usize)` at the start of `render()`
  - Append `[EOF]` line (styled `Color::DarkGray`) after content lines in display text
  - Use `Paragraph::scroll((self.scroll_offset as u16, 0))` instead of the default (no scroll)
  - _Requirements: R1, R3, R4, R8_

- [x] 7.1 Update `TowerApp::task_input()` to return `&mut TaskInput`
  - Change `pub fn task_input(&self) -> &TaskInput` to `pub fn task_input(&mut self) -> &mut TaskInput` in `src/tower/app.rs`
  - Verify all call sites compile with mutable borrow (inspect `ui.rs:220` which already has `app: &mut TowerApp`)
  - Check for any other call sites that use `task_input()` from an immutable context and fix as needed
  - _Requirements: R8_

- [x] 7.2 Write tests for EOF indicator and render integration
  - **Property 3: EOF Always Present**
  - Test: rendered line list always ends with `[EOF]` styled `DarkGray`
  - **Property 4: EOF Scrollable**
  - Test: with cursor on last content line, `[EOF]` is within the visible viewport
  - **Property 8: Render Signature Change**
  - Test: `scroll_offset` is adjusted after `render()` call (verified via `scroll_offset` field value)
  - **Validates: Requirements R3, R4, R8**

- [x] 8. Checkpoint - Verify full integration
  - Run `make test` to ensure all tests pass
  - Run `make lint` and `make fmt-check` to verify code quality
  - Run `make ci` for full validation

- [x] 9. Unicode scroll integration test
  - Test: multi-line Japanese content with scrolling works correctly end-to-end
  - Move cursor through all lines of a 10-line Japanese buffer with `visible_height = 3`
  - Assert `scroll_offset` and `cursor_line_col()` are consistent at every step
  - **Property 7: Unicode Safety**
  - **Validates: Requirements R1, R2, R7**

- [x] 10. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make ci` for complete validation
  - Verify no regressions across the entire test suite

## Notes

- The TDD cycle (Red-Green-Refactor) must be followed: write each test task first, verify it fails, then implement the corresponding code task.
- `ensure_cursor_visible()` is called lazily in `render()`, not on every cursor movement. This simplifies the implementation since no key-handling changes are needed.
- The `&mut self` signature change in `render()` follows the existing pattern used by `ExpertPanelDisplay::render()`, `StatusDisplay::render()`, and `RoleSelector::render()`.
- `task_input()` in `app.rs` currently returns `&TaskInput`. Changing it to `&mut TaskInput` requires checking all call sites. The primary caller `UI::render_task_input` already takes `app: &mut TowerApp`, so this should be straightforward. However, if any immutable call sites exist, they may need a separate `task_input_ref()` accessor.
- Total rendered lines = `line_count() + 1` (content + EOF). The EOF line is virtual and never part of the editable content.
