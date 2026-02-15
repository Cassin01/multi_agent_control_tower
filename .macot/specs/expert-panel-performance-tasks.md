# Implementation Plan: Expert Panel Performance

## Overview

This plan decomposes the 4-phase performance optimization into incremental coding tasks. Each phase targets independent bottlenecks: Phase 1 caches visual line count in `ExpertPanelDisplay`, Phase 2 adds conditional rendering via a dirty flag and increased poll timeout in `TowerApp`, Phase 3 replaces SHA-256 with xxh3 for content hashing, and Phase 4 parallelizes tmux resize operations. Tasks are ordered bottom-up — data model changes first, then rendering logic, then integration — with test tasks paired to each implementation step.

## Requirements Traceability

The design document defines 7 correctness properties (Section 6) referenced below:

- **P1**: Cache Consistency
- **P2**: Dirty Flag Completeness
- **P3**: Hash Equivalence
- **P4**: Scroll State Preservation
- **P5**: Render Idempotency
- **P6**: Resize Atomicity
- **P7**: Event Responsiveness

## Tasks

### Phase 1: Cache `visual_line_count`

- [x] 1. Add cached render state fields to `ExpertPanelDisplay`
  - Add `cached_visual_line_count: usize` and `cached_display_width: usize` fields to `ExpertPanelDisplay` struct in `src/tower/widgets/expert_panel_display.rs`
  - Initialize both to `0` in `new()`
  - Reset both to `0` in invalidation points: `set_content()`, `try_set_content()`, `enter_scroll_mode()`, `exit_scroll_mode()`, `set_expert()`
  - _Requirements: P1, P4_

- [x] 1.1 Write tests for cache invalidation
  - **Property 1: Cache Consistency** — verify `cached_visual_line_count` matches full recomputation after content change
  - **Property 4: Scroll State Preservation** — verify scroll offset and mode are unchanged after cache invalidation
  - Tests: `visual_line_count_cache_invalidated_on_content_change`, `visual_line_count_cache_invalidated_on_width_change`, `visual_line_count_cache_reused_when_unchanged`, `scroll_state_preserved_after_cache_invalidation`
  - **Validates: Requirements P1, P4**

- [x] 2. Replace per-frame `visual_line_count` computation with cache lookup in `render()`
  - In `render()`, replace `let visual_line_count = self.raw_line_count;` with the cached computation logic from design Section 3.1
  - When `display_width != self.cached_display_width || self.cached_display_width == 0`: recompute by iterating `self.content.lines`, store result in `cached_visual_line_count` and update `cached_display_width`
  - Otherwise: use `self.cached_visual_line_count` directly
  - _Requirements: P1, P5_

- [x] 2.1 Write tests for render idempotency with cache
  - **Property 5: Render Idempotency** — verify calling `render()` multiple times with unchanged state produces identical output regardless of cache state
  - **Validates: Requirements P1, P5**

- [x] 3. Checkpoint — Phase 1 validation
  - Run `make test` to ensure all 52+ existing `expert_panel_display` tests pass
  - Verify new cache tests pass
  - Ensure no regressions in scroll mode behavior

### Phase 2: Conditional rendering and event poll timeout

- [x] 4. Add `needs_redraw` dirty flag to `TowerApp`
  - Add `needs_redraw: bool` field to `TowerApp` struct in `src/tower/app.rs`
  - Initialize to `true` in constructor
  - _Requirements: P2_

- [x] 4.1 Write tests for `needs_redraw` flag state transitions
  - **Property 2: Dirty Flag Completeness** — verify `needs_redraw` is set to `true` by all state-changing operations: key/mouse events, poll updates, terminal resize, focus/panel/modal toggles
  - **Validates: Requirements P2**

- [x] 5. Wrap `terminal.draw()` call with `needs_redraw` guard
  - In the main loop (`app.rs:1583-1622`), wrap `terminal.draw()` in `if self.needs_redraw { ... self.needs_redraw = false; }`
  - Set `self.needs_redraw = true` at every state mutation point: after any `Event` in `handle_events()`, when `poll_status()` detects change, when `poll_reports()` loads data, when `poll_messages()` delivers messages, when `poll_expert_panel()` updates content, on terminal resize, on focus/panel/modal toggle, on worktree/feature executor state change
  - _Requirements: P2_

- [x] 6. Increase event poll timeout from 1ms to 16ms
  - Change `event::poll(Duration::from_millis(1))` to `event::poll(Duration::from_millis(16))` in `handle_events()` at `app.rs:584`
  - _Requirements: P7_

- [x] 6.1 Write tests for event responsiveness
  - **Property 7: Event Responsiveness** — verify that input events are still processed within acceptable latency after timeout change (test that key events trigger expected state changes without delay)
  - **Validates: Requirements P2, P7**

- [x] 7. Checkpoint — Phase 2 validation
  - Run `make test` to ensure all existing `app.rs` tests pass
  - Verify dirty flag tests pass
  - Confirm the timing instrumentation in the main loop still logs correctly

### Phase 3: Replace SHA-256 with xxh3

- [x] 8. Add `xxhash-rust` dependency
  - Add `xxhash-rust = { version = "0.8", features = ["xxh3"] }` to `Cargo.toml`
  - Verify `sha2` is still needed for `src/utils.rs` session hash — do NOT remove `sha2`
  - _Requirements: P3_

- [x] 9. Replace SHA-256 with xxh3 in `ExpertPanelDisplay`
  - Change `content_hash` field type from `[u8; 32]` to `u64` in the struct definition
  - Update `new()` to initialize `content_hash: 0`
  - Update `set_expert()` to reset `content_hash` to `0`
  - Replace `use sha2::{Digest, Sha256};` with `use xxhash_rust::xxh3::xxh3_64;` in imports
  - In `try_set_content()`: replace `Sha256::digest(raw.as_bytes()).into()` with `xxh3_64(raw.as_bytes())`
  - Update hash comparison from `[u8; 32]` equality to `u64` equality
  - _Requirements: P3_

- [x] 9.1 Write tests for hash equivalence
  - **Property 3: Hash Equivalence** — verify identical inputs produce identical hashes and distinct inputs produce distinct hashes
  - Test that `try_set_content()` correctly detects changed vs unchanged content with the new hash
  - **Validates: Requirements P3**

- [x] 10. Checkpoint — Phase 3 validation
  - Run `make test` to ensure all tests pass
  - Run `make build` to verify clean compilation with no warnings
  - Verify `sha2` import is removed from `expert_panel_display.rs` but remains in `utils.rs`

### Phase 4: Parallel tmux resize

- [x] 11. Investigate `ClaudeManager` borrow compatibility for parallel futures
  - Check whether `self.claude.resize_pane()` takes `&self` (shared ref) or `&mut self`
  - If `&self`: proceed with `futures::future::join_all` approach
  - If `&mut self`: document the constraint and use `tokio::spawn` with `Arc` wrapping, or note that parallelization requires a refactor beyond this scope
  - _Requirements: P6_

- [x] 12. Parallelize sequential resize loop in `poll_expert_panel()`
  - Replace the sequential `for id in 0..self.config.num_experts()` loop at `app.rs:514-518` with `futures::future::join_all`
  - Each future independently handles its own error via `tracing::warn!`
  - Ensure one pane failure does not prevent others from completing
  - _Requirements: P6_

- [x] 12.1 Write tests for resize atomicity
  - **Property 6: Resize Atomicity** — verify that after a resize event, all expert panes are eventually resized and one failure does not block others
  - Extend existing `resize_pane` test infrastructure in `src/session/claude.rs` and `src/session/tmux.rs`
  - **Validates: Requirements P6**

- [x] 13. Final checkpoint — Ensure all tests pass and system integration works
  - Run `make test` to confirm all tests pass across all phases
  - Run `make build` to verify clean compilation
  - Ensure no regressions in the 52+ `expert_panel_display` tests and 20+ `app` tests

## Notes

- Phases 1-3 are independent and can be implemented in parallel by separate experts
- Phase 4 depends on investigating `ClaudeManager` borrow rules (task 11) before implementation
- The `Text::clone()` cost (B2) is deferred — the dirty flag in Phase 2 naturally reduces clone frequency to match content change rate
- B7 (tmux capture-pane subprocess) is out of scope as noted in the design
- Keep `sha2` in `Cargo.toml` — it is used in `src/utils.rs` for session hashing
- All performance claims should be validated by manual testing per Section 7 of the design doc
