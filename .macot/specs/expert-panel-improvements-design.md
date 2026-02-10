# Design: Expert Panel Display Improvements

## 1. Overview

The `ExpertPanelDisplay` widget provides live tmux pane viewing for the currently selected expert. This document analyzes the current implementation against best practices identified from the claude-squad reference implementation and proposes concrete improvements to address dead code warnings, display stability, and performance.

## 2. Current Architecture

```
┌──────────────────────────────────────────────┐
│ TowerApp (app.rs)                            │
│                                              │
│  poll_expert_panel() [1000ms interval]       │
│    │                                         │
│    ├─ claude.capture_pane_with_escapes(id)    │
│    │   └─ tmux capture-pane -e -p -t {pane}  │
│    │                                         │
│    ├─ ExpertPanelDisplay::parse_ansi(&raw)    │
│    │   └─ ansi_to_tui::IntoText              │
│    │                                         │
│    └─ panel.set_content(text, line_count)     │
│        └─ auto_scroll → scroll_offset = N-1  │
│                                              │
│  handle_expert_panel_keys()                  │
│    ├─ PageUp → scroll_up()                   │
│    ├─ PageDown → scroll_down()               │
│    └─ other → send_keys to tmux              │
│                                              │
│  UI::render() (ui.rs)                        │
│    └─ Constraint::Percentage(40) for panel   │
│       └─ panel.render(frame, area)           │
│          └─ Paragraph + content.clone()      │
└──────────────────────────────────────────────┘
```

## 3. Issues Found

### 3.1 Dead Code Warnings (Compiler)

**Severity: Medium**

Four methods have `dead_code` warnings because they are only used in tests:

| Method | Used in tests | Used in production |
|--------|--------------|-------------------|
| `show()` | Yes (`show_and_hide` test) | No — `toggle()` is used at `app.rs:587` |
| `hide()` | Yes (`show_and_hide` test) | No — `toggle()` is used |
| `is_focused()` | Yes (`starts_unfocused`, `set_focused_changes_state`) | No — `set_focused()` is used but `is_focused()` only in tests |
| `scroll_to_bottom()` | Yes (`scroll_to_bottom_enables_auto_scroll`) | No — not wired to any key binding |

**Root cause**: These methods were written for the test suite and future use, but production code paths use different approaches (`toggle()` instead of `show()`/`hide()`, `set_focused()` without querying `is_focused()`).

**Options**:
- A) Wire unused methods into production code (e.g., `scroll_to_bottom` → Home/End key binding, `is_focused` → used in `render()` conditional logic)
- B) Add `#[allow(dead_code)]` or `#[cfg(test)]` annotations
- C) Remove unused methods from production API, keep only in `#[cfg(test)]` blocks

**Recommendation**: Option A for `scroll_to_bottom` (useful feature) and `is_focused` (already used in render). Option B for `show`/`hide` since `toggle` is the preferred API.

### 3.2 Missing `-J` Flag in capture-pane

**Severity: High** | **File**: `src/session/tmux.rs:58-75`

Current implementation:
```rust
.args(["capture-pane", "-e", "-p", "-t", &format!(...)])
```

The `-J` flag (join wrapped lines) is missing. claude-squad uses `-p -e -J` consistently. Without `-J`, lines that are soft-wrapped by tmux (because the pane is narrower than the output) appear as multiple lines in the captured output, causing:
- Incorrect `raw_line_count` calculation
- Scroll offset pointing to wrong positions
- Visual line-break artifacts in the rendered panel

### 3.3 No Content Hash Change Detection

**Severity: Medium** | **File**: `src/tower/app.rs:447-483`

Every 1000ms poll cycle runs `capture_pane_with_escapes()`, `parse_ansi()`, and `set_content()` regardless of whether the pane content actually changed. claude-squad uses SHA-256 hashing to detect content changes and skip unnecessary processing.

Impact:
- Unnecessary ANSI parsing overhead on every poll
- Unnecessary `Text` allocation on every poll
- `set_content()` triggers scroll offset recalculation even when nothing changed

### 3.4 Slow Poll Interval (1000ms)

**Severity: Medium** | **File**: `src/tower/app.rs:457`

The panel poll interval is 1000ms, compared to claude-squad's 100ms for preview updates. This creates a noticeable lag between Claude producing output and the panel showing it. While the 500ms input pause helps prevent interference, the base 1000ms interval is too slow for a live preview.

**Recommendation**: Reduce to 200-300ms for a good balance between responsiveness and CPU usage.

### 3.5 No PTY/Pane Size Synchronization

**Severity: High** | **File**: `src/tower/widgets/expert_panel_display.rs:104-138`

The tmux pane dimensions are never synchronized with the panel's rendered area. claude-squad calls `SetDetachedSize(width, height)` on every resize to ensure tmux wraps content at exactly the panel width.

Without this sync:
- tmux may wrap lines at a different width than the panel area, causing misaligned or double-wrapped output
- The `Wrap { trim: false }` in `render()` wraps already-wrapped lines a second time
- Line count calculations are inaccurate since they don't account for the actual visible width

### 3.6 Expensive `content.clone()` in render()

**Severity: Low** | **File**: `src/tower/widgets/expert_panel_display.rs:132`

```rust
let paragraph = Paragraph::new(self.content.clone())
```

`Text<'static>` contains `Vec<Line<'static>>` which holds `Vec<Span<'static>>` — cloning the entire tree on every frame. For large pane captures this is O(n) allocation per frame.

**Options**:
- Use `&self.content` with a reference-based Paragraph (requires API check)
- Pre-render the paragraph once on `set_content()` and cache it
- Accept the cost if pane content is small

### 3.7 `scroll_offset` Uses `u16`

**Severity: Low** | **File**: `src/tower/widgets/expert_panel_display.rs:15`

The `scroll_offset` field is `u16` (max 65535). For very long-running Claude sessions with extensive output, this could overflow. ratatui's `Paragraph::scroll()` also takes `(u16, u16)`, so this is constrained by the downstream API, but it's worth noting as a limitation.

### 3.8 Auto-scroll Offset Calculation

**Severity: Medium** | **File**: `src/tower/widgets/expert_panel_display.rs:83-85`

```rust
if self.auto_scroll && line_count > 0 {
    self.scroll_offset = line_count.saturating_sub(1) as u16;
}
```

This sets the scroll to `line_count - 1`, which positions the scroll at the second-to-last page of content. The correct auto-scroll behavior should scroll to `max(0, line_count - visible_height)` to show the last screen of content. However, `visible_height` is not known at `set_content()` time — it's only available during `render()`.

The `render()` method does clamp: `self.scroll_offset = self.scroll_offset.min(max_scroll)`, which corrects it, but the intent of the auto-scroll calculation in `set_content()` is conceptually wrong.

### 3.9 Layout Uses Fixed Percentage(40)

**Severity: Low** | **File**: `src/tower/ui.rs:51`

The expert panel always takes 40% of the vertical space via `Constraint::Percentage(40)`. In small terminals (< 30 rows), this squeezes other widgets significantly. A `Constraint::Min(10)` combined with a max percentage would be more adaptive.

### 3.10 No Scroll Position Indicator

**Severity: Low** | **File**: `src/tower/widgets/expert_panel_display.rs:104-138`

When the user scrolls up, there is no visual indicator of the current scroll position or that auto-scroll is disabled. claude-squad shows an "ESC to exit scroll mode" footer. A similar indicator (e.g., "Auto-scroll OFF — PageUp/Down to scroll" in the title bar) would improve UX.

### 3.11 `scroll_down()` Does Not Re-enable Auto-scroll at Bottom

**Severity: Medium** | **File**: `src/tower/widgets/expert_panel_display.rs:93-95`

```rust
pub fn scroll_down(&mut self) {
    self.scroll_offset = self.scroll_offset.saturating_add(1);
}
```

Unlike `scroll_up()` which disables `auto_scroll`, `scroll_down()` does not re-enable it when reaching the bottom. The user must call `scroll_to_bottom()` to re-enable auto-scroll, but this method is not wired to any key binding. This means once a user scrolls up and then scrolls back down to the bottom, new content will not auto-scroll.

## 4. Prioritized Improvement Plan

### Phase 1: Critical Display Quality (High Impact)

1. **Add `-J` flag** to `capture_pane_with_escapes` in `src/session/tmux.rs`
2. **Add PTY size sync**: Send `tmux resize-pane` when the panel area changes
3. **Wire `scroll_to_bottom()`** to a key binding (e.g., `Home` for top, `End` for bottom)
4. **Fix `scroll_down()` auto-scroll**: Re-enable auto_scroll when offset reaches max_scroll

### Phase 2: Performance Optimization (Medium Impact)

5. **Add content hash detection**: Store SHA-256 of captured content, skip processing when unchanged
6. **Reduce poll interval** from 1000ms to 250ms
7. **Avoid `content.clone()`** in render if possible

### Phase 3: Polish (Low Impact)

8. **Resolve dead_code warnings**: `#[allow(dead_code)]` for `show`/`hide`, wire `is_focused` into conditional rendering
9. **Add scroll position indicator** in title bar
10. **Adaptive panel height**: Use `Constraint::Min(10)` with percentage cap

## 5. Correctness Properties

1. **Line Join Invariant** — Captured pane content must have wrapped lines joined (`-J` flag) before line counting.
2. **Size Sync Invariant** — The tmux pane width must equal the panel's inner width (area.width - 2 for borders) at all times.
3. **Auto-scroll Consistency** — When auto_scroll is true, `scroll_offset` must always be `max(0, line_count - visible_height)` after `render()`.
4. **Content Change Idempotency** — If pane content hasn't changed (same hash), `set_content()` must not be called, preserving scroll state.
5. **Scroll Bound Safety** — `scroll_offset` must never exceed `line_count.saturating_sub(visible_height)` after any operation.

## 6. Testing Strategy

- **Unit tests**: Verify `scroll_to_bottom()` re-enables auto_scroll, `scroll_down()` at max re-enables auto_scroll
- **Integration tests**: Verify `-J` flag produces correct line counts for wrapped output
- **Property tests**: For any sequence of scroll_up/scroll_down/set_content, scroll_offset stays within [0, max_scroll]
