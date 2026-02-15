# Design: Expert Panel Performance

## 1. Overview

This design addresses performance bottlenecks in the Expert Panel's rendering and polling pipeline. Profiling identified 7 bottlenecks across the rendering path, event loop, and tmux interaction layer. The optimizations are grouped into 4 phases by impact and complexity.

The Expert Panel is the primary UI surface for monitoring expert activity. It polls tmux panes at 250ms intervals, converts ANSI output to ratatui `Text`, and renders the content every frame. Current implementation performs redundant computation on every frame (visual line count iteration, full `Text` clone) and runs the event loop at ~1000 iterations/sec when idle.

## 2. Architecture

### Current Main Loop (`app.rs:1583-1622`)

```
while running:
  terminal.draw()           <- TUI rendering (every iteration)
  handle_events()           <- crossterm event poll (1ms timeout)
  poll_status()             <- fs read (2000ms interval)
  poll_reports()            <- fs read (3000ms interval)
  poll_messages()           <- fs read + processing (3000ms interval)
  poll_expert_panel()       <- tmux subprocess (250ms interval)
  poll_worktree_launch()
  poll_feature_executor()
```

### Expert Panel Data Flow

```
tmux capture-pane (subprocess, 250ms interval)
    |
raw: String
    |
SHA-256 hash comparison (change detection)
    | (only if changed)
ansi_to_tui::IntoText (ANSI -> Text<'static>)
    |
ExpertPanelDisplay.content = Text<'static>
    | (every frame)
render():
  - visual_line_count: iterate all lines (every frame)
  - content.clone() -> Paragraph::new()
  - frame.render_widget()
```

### Proposed Architecture Changes

The optimizations introduce:
- Cached derived state (`visual_line_count`, `display_width`) in `ExpertPanelDisplay`
- A `needs_redraw` dirty flag in `TowerApp` to skip redundant `terminal.draw()` calls
- Non-cryptographic hashing (xxh3) for content change detection
- Parallel subprocess spawning for tmux resize operations

## 3. Components and Interfaces

### 3.1 ExpertPanelDisplay (Phase 1 + Phase 3)

- **File**: `src/tower/widgets/expert_panel_display.rs`
- **Purpose**: Renders expert panel content with ANSI support, scroll mode, and line wrapping
- **Key changes**:

```rust
pub struct ExpertPanelDisplay {
    // Existing fields...

    // Phase 1: cached render state
    cached_visual_line_count: usize,
    cached_display_width: usize,

    // Phase 3: non-cryptographic hash
    content_hash: u64,  // was [u8; 32] (SHA-256)
}
```

**Phase 1 — Cache `visual_line_count`**:

`render()` currently iterates `self.content.lines` every frame to compute `visual_line_count`. The cache stores the computed value alongside the display width it was computed for. Invalidation occurs when content changes or display width changes.

Invalidation points: `try_set_content()`, `set_content()`, `enter_scroll_mode()`, `exit_scroll_mode()`, `set_expert()`.

```rust
let visual_line_count = if display_width != self.cached_display_width
    || self.cached_display_width == 0
{
    let count = if display_width > 0 {
        self.content.lines.iter()
            .map(|line| {
                let w = line.width();
                if w == 0 { 1 } else { w.div_ceil(display_width) }
            })
            .sum()
    } else {
        self.raw_line_count
    };
    self.cached_visual_line_count = count;
    self.cached_display_width = display_width;
    count
} else {
    self.cached_visual_line_count
};
```

**Phase 1 — `Text::clone()` mitigation (Option B)**:

The clone cost for a typical ~100-line terminal is only a few KB. Combined with the Phase 2 dirty flag, `render()` is only called when the display actually needs updating, reducing clone frequency to match content change rate. If profiling later shows `clone()` as a hot spot, a pre-built `Paragraph` cache (Option A) can be revisited.

**Phase 3 — Replace SHA-256 with xxh3**:

```rust
// Before
use sha2::{Digest, Sha256};
let hash: [u8; 32] = Sha256::digest(raw.as_bytes()).into();

// After
use xxhash_rust::xxh3::xxh3_64;
let hash = xxh3_64(raw.as_bytes());
```

### 3.2 TowerApp (Phase 2 + Phase 4)

- **File**: `src/tower/app.rs`
- **Purpose**: Main application loop with event handling and polling orchestration
- **Key changes**:

```rust
pub struct TowerApp {
    // Existing fields...

    // Phase 2: conditional rendering
    needs_redraw: bool,
}
```

**Phase 2 — Event poll timeout increase**:

```rust
// Before: ~1000 loop iterations/sec when idle
if event::poll(Duration::from_millis(1))? {

// After: ~60 fps cap, 16ms is imperceptible
if event::poll(Duration::from_millis(16))? {
```

**Phase 2 — Dirty flag for conditional rendering**:

```rust
while self.is_running() {
    if self.needs_redraw {
        terminal.draw(|frame| UI::render(frame, self))?;
        self.needs_redraw = false;
    }
    self.handle_events().await?;
    self.poll_status().await?;
    self.poll_reports().await?;
    self.poll_messages().await?;
    self.poll_expert_panel().await?;
    self.poll_worktree_launch().await?;
    self.poll_feature_executor().await?;
}
```

Set `needs_redraw = true` when: any key/mouse event processed, `poll_status()` detects state change, `poll_reports()` loads new data, `poll_messages()` delivers messages, `poll_expert_panel()` updates content, terminal resize event, focus/panel/modal toggle, worktree/feature executor state change.

**Phase 4 — Parallel tmux resize**:

```rust
// Before (sequential)
for id in 0..self.config.num_experts() {
    if let Err(e) = self.claude.resize_pane(id, w, h).await {
        tracing::warn!("Failed to resize pane for expert {}: {}", id, e);
    }
}

// After (parallel via join_all)
let resize_futures: Vec<_> = (0..self.config.num_experts())
    .map(|id| async move {
        if let Err(e) = self.claude.resize_pane(id, w, h).await {
            tracing::warn!("Failed to resize pane for expert {}: {}", id, e);
        }
    })
    .collect();
futures::future::join_all(resize_futures).await;
```

Note: Requires verifying `ClaudeManager` borrow rules. If `&self.claude` cannot be shared across futures, wrap in `Arc` or use `tokio::spawn`.

### 3.3 Cargo.toml (Phase 3)

- **File**: `Cargo.toml`
- **Purpose**: Dependency management
- **Key changes**:

```toml
# Add
xxhash-rust = { version = "0.8", features = ["xxh3"] }
```

Note: `sha2` is also used for session hash in `config.rs`. Keep both dependencies unless all usages are migrated.

## 4. Data Models

### Cached Render State

| Field | Type | Default | Invalidation |
|-------|------|---------|-------------|
| `cached_visual_line_count` | `usize` | `0` | Content change, scroll mode toggle, expert switch |
| `cached_display_width` | `usize` | `0` | Implicitly via width mismatch check in `render()` |
| `content_hash` | `u64` | `0` | Replaced on every `try_set_content()` call |
| `needs_redraw` | `bool` | `true` | Set by events/polls, cleared after `terminal.draw()` |

### Bottleneck Summary

| # | Location | Issue | Impact |
|---|----------|-------|--------|
| B1 | `expert_panel_display.rs:215-230` | `visual_line_count` recalculated every frame | High |
| B2 | `expert_panel_display.rs:260` | `self.content.clone()` deep-copies `Text` every frame | Medium |
| B3 | `expert_panel_display.rs:165` | SHA-256 used for content change detection | Medium |
| B4 | `app.rs:584` | `event::poll(Duration::from_millis(1))` ~1000 iter/sec idle | Medium |
| B5 | `app.rs:1587` | `terminal.draw()` called every iteration unconditionally | Medium |
| B6 | `app.rs:514-518` | Sequential `tmux resize-pane` for all experts | Low |
| B7 | `tmux.rs:99-116` | `tmux capture-pane` spawns subprocess every 250ms | Low |

## 5. Error Handling

### Phase 1 (Cache)

No new error paths. Cache miss falls through to existing computation. Invalid cache state (width=0) triggers recomputation safely.

### Phase 2 (Dirty Flag)

Risk: missing a `needs_redraw = true` causes stale display. Mitigations:
1. Set `needs_redraw = true` after any `Event` (catch-all)
2. Set `needs_redraw = true` when any poll function returns a change indicator
3. No silent failures — worst case is a momentary stale frame corrected on next event

### Phase 3 (Hash)

xxh3 collision probability for sequential content comparison is effectively zero (n^2/2^65). No error handling changes needed.

### Phase 4 (Parallel Resize)

Individual resize failures are already logged via `tracing::warn!`. Parallelization preserves this behavior — each future handles its own error independently.

## 6. Correctness Properties

1. **Cache Consistency** — `cached_visual_line_count` must equal the value that would be computed by iterating `self.content.lines` for the current `cached_display_width`. After any content mutation, the cache must be invalidated.

2. **Dirty Flag Completeness** — Every state change visible in the UI must set `needs_redraw = true`. No user-visible state change may occur without a subsequent redraw.

3. **Hash Equivalence** — Content change detection using xxh3 must produce the same accept/reject decisions as SHA-256 for all practical inputs. Specifically: identical inputs must produce identical hashes; distinct sequential captures must produce distinct hashes with probability >= 1 - 2^-63.

4. **Scroll State Preservation** — Cache invalidation in scroll mode must not alter scroll offset, scroll position, or scroll mode state.

5. **Render Idempotency** — Calling `render()` multiple times with unchanged state must produce identical output regardless of cache state.

6. **Resize Atomicity** — After a terminal resize, all expert panes must eventually be resized. Failure of one pane resize must not prevent others from completing.

7. **Event Responsiveness** — Increasing the event poll timeout to 16ms must not introduce perceptible input latency (defined as < 50ms end-to-end from keypress to screen update).

## 7. Testing Strategy

### Unit Tests (Phase 1)

Cover Properties 1, 4, 5.

```rust
#[test]
fn visual_line_count_cache_invalidated_on_content_change() { ... }

#[test]
fn visual_line_count_cache_invalidated_on_width_change() { ... }

#[test]
fn visual_line_count_cache_reused_when_unchanged() { ... }
```

Extend the existing 52 tests in `expert_panel_display.rs` to verify cache behavior does not regress scroll mode, content setting, or expert switching.

### Integration Tests (Phase 2)

Cover Properties 2, 7.

Verify that `needs_redraw` is set by all relevant state changes. Test via the existing app test harness that processes events and verifies UI state.

### Benchmark (Phase 3)

Verify Property 3 by comparing hash outputs for known inputs. Measure throughput improvement over SHA-256 for typical 4KB terminal output.

| Hash | Expected Throughput |
|------|-------------------|
| SHA-256 | ~500 MB/s |
| xxh3 | ~15 GB/s |

### Existing Test Suite

All 52 tests in `expert_panel_display.rs` and 20+ tests in `app.rs` must pass after each phase. Run via `make test`.

### Manual Verification

1. Launch macot with 3+ experts
2. Observe CPU usage in idle state (expect significant reduction)
3. Type in task input — verify no visible input lag
4. Scroll mode (PageUp/Down) — verify smooth scrolling
5. Resize terminal — verify panel content re-wraps correctly
6. Switch experts — verify content updates within 250ms

## Implementation Order

| Phase | Scope | Estimated Diff | Dependencies |
|-------|-------|---------------|--------------|
| 1 | `expert_panel_display.rs` | ~40 lines | None |
| 2 | `app.rs` | ~30 lines | None |
| 3 | `expert_panel_display.rs`, `Cargo.toml` | ~20 lines | Check `sha2` usage elsewhere |
| 4 | `app.rs` | ~15 lines | Verify `ClaudeManager` borrow rules |

Phases 1-3 are independent and can be implemented in parallel. Phase 4 depends on verifying borrow compatibility.

## Out of Scope

- **B7 (tmux capture-pane subprocess)**: 250ms polling interval already limits to 4/sec. Further optimization requires architectural changes with diminishing returns.
- **Async parallel polling**: Running polls concurrently via `tokio::join!` requires restructuring mutable borrows. Deferred to future refactoring.
