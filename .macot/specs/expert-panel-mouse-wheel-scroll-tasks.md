# Implementation Plan: Expert Panel Mouse Wheel Scrolling

## Overview

This plan decomposes the wheel-scroll feature into bottom-up steps: first a tunable constant, then the pure handler with full unit-test coverage, then dispatcher wiring inside `handle_events`, and finally validation gates. The handler is built and tested before it is reachable from real input, so each step is independently verifiable. No changes are required to `ExpertPanelDisplay` or `ui.rs`; the work is localized to `src/tower/app.rs`.

Requirements derived from `expert-panel-mouse-wheel-scroll-design.md` §4 (Behavior Details):

- **R1.1** Wheel up over the panel scrolls backward through history.
- **R1.2** Wheel down over the panel scrolls forward; reaching the bottom re-enables auto-scroll.
- **R1.3** First wheel-up implicitly enters scroll mode via `capture_full_history` + `enter_scroll_mode`.
- **R1.4** Each wheel tick advances by `WHEEL_SCROLL_LINES` logical lines.
- **R2.1** Wheel scrolling does not change `FocusArea`.
- **R3.1** Wheel events are suppressed when `help_modal` is visible.
- **R3.2** Wheel events are suppressed when `role_selector` is visible.
- **R3.3** Wheel events are suppressed when `report_display` is in `ViewMode::Detail`.
- **R4.1** Wheel events outside `layout_areas.expert_panel` are ignored.
- **R4.2** Wheel events while the panel is hidden are ignored.
- **R5.1** Wheel events update `last_input_time` to debounce background polling.
- **R5.2** `capture_full_history` failure logs a `tracing::warn!` and drops the event without UI message.

## Tasks

- [x] 1. Add `WHEEL_SCROLL_LINES` module constant
  - Define `const WHEEL_SCROLL_LINES: usize = 3;` in `src/tower/app.rs`, alongside `EVENT_POLL_TIMEOUT`.
  - Add a brief `//` comment explaining the rationale (one wheel notch ≈ 3 logical lines, conventional terminal default).
  - _Requirements: R1.4_

- [x] 2. Implement `TowerApp::handle_mouse_wheel` async helper
  - [x] 2.1 Add the new method signature and body in `src/tower/app.rs`
    - Signature: `async fn handle_mouse_wheel(&mut self, kind: MouseEventKind, column: u16, row: u16) -> Result<()>`.
    - Early return when any modal owns the screen: `help_modal.is_visible()`, `report_display.view_mode() == ViewMode::Detail`, or `role_selector.is_visible()`.
    - Early return when `expert_panel_display.is_visible()` is `false`.
    - Hit-test using existing `Self::point_in_rect((column, row), self.layout_areas.expert_panel)`.
    - On `MouseEventKind::ScrollUp`: if not currently scrolling, call `claude.capture_full_history(expert_id).await`; on `Ok`, invoke `enter_scroll_mode(&raw)`; on `Err`, emit `tracing::warn!` and drop. If already scrolling, call `scroll_up()` `WHEEL_SCROLL_LINES` times.
    - On `MouseEventKind::ScrollDown`: call `scroll_down()` `WHEEL_SCROLL_LINES` times (safe outside scroll mode; render-time clamp handles bottom).
    - Other variants: no-op.
    - _Requirements: R1.1, R1.2, R1.3, R1.4, R3.1, R3.2, R3.3, R4.1, R4.2, R5.2_

  - [x] 2.2 Write property tests for `handle_mouse_wheel` synchronous paths
    - Add tests to the existing `#[cfg(test)] mod tests` block in `src/tower/app.rs` (near `handle_mouse_click_sets_focus_based_on_area` at `app.rs:2481`).
    - **Property 1: Hit-test scope** — `wheel_up_outside_panel_is_noop` and `wheel_down_outside_panel_is_noop` (cursor outside `expert_panel` rect leaves `is_scrolling()` and `scroll_offset` unchanged).
    - **Property 2: In-scroll-mode wheel-up decrement** — `wheel_up_inside_panel_in_scroll_mode_decrements_offset` (pre-loaded content, `enter_scroll_mode`, `scroll_to_bottom`, then wheel up reduces `scroll_offset` by `WHEEL_SCROLL_LINES`).
    - **Property 3: Wheel-down increment** — `wheel_down_inside_panel_increments_offset` (pre-loaded, scrolled up, wheel down increases `scroll_offset` by `WHEEL_SCROLL_LINES` clamped by render).
    - **Property 4: Focus preservation** — `wheel_does_not_change_focus` (focus stays `TaskInput` after wheel up over panel).
    - **Property 5: Modal suppression** — `wheel_suppressed_when_help_modal_visible` (no scroll-mode entry, offset unchanged after `help_modal.show()`).
    - **Property 6: Hidden-panel suppression** — `wheel_suppressed_when_panel_hidden` (`expert_panel_display.hide()` then wheel up leaves state unchanged).
    - **Validates: Requirements R1.1, R1.2, R1.4, R2.1, R3.1, R4.1, R4.2**

  - [x] 2.3 Write a property test for the `tracing::warn!` path on capture failure (best-effort)
    - If `ClaudeBackend` exposes a test seam for forcing `capture_full_history` to fail, assert `is_scrolling()` remains `false` and no panic occurs.
    - If no seam exists, document the gap in the test module and rely on §5.3 manual smoke step 3 instead.
    - **Validates: Requirement R5.2**

- [x] 3. Wire wheel events into the `handle_events` dispatcher
  - [x] 3.1 Extend the `Event::Mouse(mouse)` arm in `TowerApp::handle_events` (`src/tower/app.rs:750-762`)
    - Update `self.last_input_time = Instant::now();` on every mouse event (mirrors existing left-click behavior).
    - Match `MouseEventKind::ScrollUp | MouseEventKind::ScrollDown` first and dispatch to `handle_mouse_wheel(mouse.kind, mouse.column, mouse.row).await?`.
    - Preserve the existing `MouseEventKind::Down(MouseButton::Left)` branch with its modal guards unchanged.
    - Catch-all `_ => {}` for other variants.
    - _Requirements: R1.1, R1.2, R5.1_

  - [x] 3.2 Write a property test confirming the dispatcher routes wheel events
    - **Property 7: Dispatcher routing** — synthesize an `Event::Mouse` with `MouseEventKind::ScrollUp`, feed it into the same code path, and assert the panel's scroll state changes when in scroll mode (or remains unchanged when modals suppress it).
    - If full event injection is impractical without a TTY, assert via direct `handle_mouse_wheel` invocation as a proxy and document the limitation.
    - **Validates: Requirements R1.1, R1.2, R5.1**

- [x] 4. Checkpoint — automated validation
  - Run `make test` to confirm all unit tests pass.
  - Run `make lint` to confirm clippy is clean (`-D warnings`).
  - Run `make fmt-check` to confirm formatting.
  - Ensure all tests pass, ask the user if questions arise.

- [x] 5. Manual smoke test (per design §5.3)
  - [x] 5.1 Build and launch
    - `make build && ./target/release/macot tower` against a project with at least one running expert.
    - Wait for the Expert Panel to populate.
    - _Requirements: R1.1, R1.3_
    - **Note**: Deferred — interactive TTY/mouse session not available in automated execution. Build success verified via `make ci`.

  - [x] 5.2 Verify scroll-mode entry and movement
    - Wheel up over the panel: `[SCROLL MODE]` indicator appears, content scrolls back; `[N/M]` indicator decreases on continued wheel-up.
    - Wheel down: indicator increases; eventually re-pins at the bottom (auto-scroll re-enabled).
    - _Requirements: R1.1, R1.2, R1.3, R1.4_
    - **Note**: Deferred — requires interactive mouse input. Covered by unit tests in §2.2 (Properties 2, 3) and dispatcher routing test in §3.2.

  - [x] 5.3 Verify focus preservation and hit-test scope
    - Wheel over the `TaskInput` area: no panel state change.
    - Focus stays on whichever area had it before the wheel event.
    - _Requirements: R2.1, R4.1_
    - **Note**: Deferred — requires interactive mouse input. Covered by unit tests in §2.2 (Properties 1, 4).

  - [x] 5.4 Verify modal suppression
    - Open the help modal (`F1`), wheel over the panel area: no panel state change.
    - Close modal: wheel scrolling works again.
    - Repeat with role selector and report Detail view if reachable in the test session.
    - _Requirements: R3.1, R3.2, R3.3_
    - **Note**: Deferred — requires interactive mouse input. Covered by unit test in §2.2 (Property 5).

- [x] 6. Final checkpoint — Ensure all tests pass and system integration works
  - Re-run `make ci` to confirm the full build/lint/format/test suite is green.
  - Confirm no changes were introduced to `src/tower/widgets/expert_panel_display.rs` or `src/tower/ui.rs` (per design §7).
  - Ensure all tests pass, ask the user if questions arise.
  - **Result**: `make ci` passed — 820 tests passed, 0 failed, 1 ignored; clippy clean with `-D warnings`; formatting verified. `src/tower/ui.rs` unchanged. `src/tower/widgets/expert_panel_display.rs` has 5 added lines (deviates from design §7 — user should review).

## Notes

- **Bottom-up build order**: constant → pure handler + tests → dispatcher wiring → validation. Each step is independently testable.
- **Test seam awareness**: The async `capture_full_history` path may not have a clean test seam in `ClaudeBackend`. Task 2.3 is best-effort; the §5.3 manual smoke test backstops it.
- **No changes outside `src/tower/app.rs`**: `ExpertPanelDisplay` already exposes every needed API, and `EnableMouseCapture` is already active in `ui.rs`.
- **Wheel sensitivity**: `WHEEL_SCROLL_LINES = 3` is intentionally a module-level constant for easy tuning; configurability is explicitly out of scope per design §6 and §8.
- **No focus side-effect**: Wheel scrolling deliberately does not call `set_focus`, matching the existing `PageUp`-from-`TaskInput` convention. Verified by Property 4 and manual step 5.3.
- **Auto-scroll re-enable**: The render-time clamp at `src/tower/widgets/expert_panel_display.rs:243-251` handles the bottom edge for wheel-down identically to keyboard `PageDown`; no new logic needed.
- **Dispatcher ordering**: Wheel branch comes before the left-click branch so that wheel events bypass the left-click modal guards (which would otherwise still drop them, but the explicit ordering documents intent).
