# Design: Expert Panel Display

## Context

The MACOT tower TUI currently allows operators to assign tasks to experts and view reports, but provides no way to see what an expert is doing in real-time without switching to the tmux session manually. The request is to display the selected expert's tmux pane content directly below the User Input panel, with both display and input capability — giving the operator a live window into each expert's activity from within the control tower.

## 1. Overview

Add an `ExpertPanelDisplay` widget that:
- Captures the selected expert's tmux pane content via `tmux capture-pane -e -p` (with ANSI escape sequences)
- Converts ANSI output to styled ratatui `Text` using the `ansi-to-tui` crate (v8, compatible with ratatui 0.29 — verified)
- Renders the content as a scrollable `Paragraph` below the Task Input area
- Forwards keystrokes to the expert's pane when focused, enabling interactive input
- Is toggleable via `Ctrl+J`

## 2. Architecture

### Layout (Panel Hidden — default, unchanged)
```
[0] Header           (Length 3)
[1] Expert List      (Length dynamic)
[2] Task Input       (Min 8)
[3] Footer           (Length 3)
```

### Layout (Panel Visible — after Ctrl+J toggle)
```
[0] Header           (Length 3)
[1] Expert List      (Length dynamic)
[2] Task Input       (Min 5)          ← shrunk
[3] Expert Panel     (Percentage 40)  ← NEW
[4] Footer           (Length 3)
```

### Data Flow
```
poll_expert_panel (1000ms) → tmux capture-pane -e -p → raw ANSI string
  → ansi_to_tui::IntoText → ratatui::Text → ExpertPanelDisplay::set_content
  → UI::render → Paragraph::scroll((offset, 0))

User keystroke (ExpertPanel focused) → handle_expert_panel_keys
  → keycode_to_tmux_key → ClaudeManager::send_keys → tmux send-keys
```

### Focus Cycle (panel visible)
```
TaskInput → ExpertPanel → TaskInput
(ExpertList is always skipped — display only)
```

**Important**: When `ExpertPanel` is focused, the panel acts as an interactive terminal embed.
All keys — including `Tab`, `Esc`, and modifier combos — are forwarded to the expert's tmux
pane, **except** the following reserved keys:

| Key | Action (local to panel, NOT forwarded) |
|-----|----------------------------------------|
| `Ctrl+J` | Toggle panel visibility (global — handled before focus dispatch) |
| `PageUp` | Scroll panel content up |
| `PageDown` | Scroll panel content down |

Since `Tab` is forwarded when ExpertPanel is focused, focus cycling via `Tab`/`BackTab` is
suppressed in this state. The operator exits the panel by pressing `Ctrl+J` (hides panel,
returns focus to TaskInput) or by using `next_focus()`/`prev_focus()` through another mechanism
if added later.

## 3. Components and Interfaces

### 3.1 ExpertPanelDisplay (NEW)

- **File**: `src/tower/widgets/expert_panel_display.rs` (new file)
- **Purpose**: Displays captured tmux pane content with scroll and focus support

```rust
pub struct ExpertPanelDisplay {
    expert_id: Option<u32>,
    expert_name: Option<String>,
    content: Text<'static>,       // Parsed ANSI → ratatui
    raw_line_count: usize,
    scroll_offset: u16,
    visible: bool,
    focused: bool,
    auto_scroll: bool,            // Auto-scroll to bottom on new content
}
```

**Key methods**:
- `toggle()` / `is_visible()` / `show()` / `hide()` — visibility control
- `set_focused(bool)` / `is_focused()` — focus state (Cyan border when focused)
- `set_expert(id, name)` — tracks selected expert; resets content/scroll on change
- `set_content(text, line_count)` — updates display content from poll
- `scroll_up()` / `scroll_down()` / `scroll_to_bottom()` — navigation
- `render(&mut self, frame, area)` — renders Paragraph with scroll and styled border

### 3.2 TmuxSender Extension

- **File**: `src/session/tmux.rs`
- **Purpose**: Add `capture_pane_with_escapes` method

Add to `TmuxSender` trait (with default fallback to `capture_pane`):
```rust
async fn capture_pane_with_escapes(&self, pane_id: u32) -> Result<String> {
    self.capture_pane(pane_id).await  // default fallback
}
```

Concrete `TmuxManager` impl uses `tmux capture-pane -e -p -t {session}:{window_id}` (the `-e` flag preserves ANSI escape sequences).

### 3.3 Key Conversion Utility

- **File**: `src/tower/app.rs` (private function)
- **Purpose**: Convert crossterm `KeyCode` + `KeyModifiers` to tmux key strings

```rust
fn keycode_to_tmux_key(code: KeyCode, modifiers: KeyModifiers) -> Option<String>
```

**Conversion table**:

| crossterm input | tmux key string | Notes |
|----------------|-----------------|-------|
| `Char('a')` | `"a"` | Plain character |
| `Char('A')` (Shift) | `"A"` | Uppercase character |
| `Ctrl+Char('c')` | `"C-c"` | Ctrl modifier prefix |
| `Enter` | `"Enter"` | |
| `Backspace` | `"BSpace"` | |
| `Tab` | `"Tab"` | Forwarded (NOT reserved) |
| `BackTab` (Shift+Tab) | `"BTab"` | Forwarded (NOT reserved) |
| `Esc` | `"Escape"` | Forwarded (NOT reserved) |
| `Up` | `"Up"` | |
| `Down` | `"Down"` | |
| `Left` | `"Left"` | |
| `Right` | `"Right"` | |
| `Home` | `"Home"` | |
| `End` | `"End"` | |
| `PageUp` | `None` | Reserved for local panel scroll |
| `PageDown` | `None` | Reserved for local panel scroll |

Returns `None` only for `PageUp`/`PageDown` (reserved for local panel scrolling). All other keys are converted and forwarded.

### 3.4 Changes to Existing Components

#### 3.4.1 Structural additions

**`FocusArea` enum** (`src/tower/app.rs:28`): Add `ExpertPanel` variant.

**`LayoutAreas` struct** (`src/tower/app.rs:36`): Add `expert_panel: Rect` field.

**`TowerApp` struct** (`src/tower/app.rs`):
- Add fields: `expert_panel_display: ExpertPanelDisplay`, `last_panel_poll: Instant`
- Add accessor: `expert_panel_display(&mut self) -> &mut ExpertPanelDisplay`
- Add method: `poll_expert_panel()` — polls every 1000ms, respects 500ms input debounce
- Add method: `handle_expert_panel_keys(code, modifiers)` — async, forwards keys via `send_keys`
- Update: `next_focus()` / `prev_focus()` — conditionally include `ExpertPanel` when visible
- Update: `update_focus()` — set `expert_panel_display.set_focused(...)`
- Update: `handle_mouse_click()` — check `expert_panel` rect
- Update: `handle_events()` — see §3.4.2 for detailed flow
- Update: `run()` — add `poll_expert_panel().await` call in main loop

#### 3.4.2 Event handling flow (async dispatch detail)

The `handle_events()` method is already `async fn`. The key challenge is integrating the `ExpertPanel` focus arm, which needs to call async `handle_expert_panel_keys().await`, into the existing synchronous `match self.focus { ... }` block.

**Current flow** (simplified):
```rust
// 1. Global Ctrl handlers (early return)
if key.modifiers.contains(KeyModifiers::CONTROL) {
    match key.code {
        Char('c') | Char('q') => { self.quit(); return Ok(()); }
        Char('i') => { self.help_modal.toggle(); return Ok(()); }
        _ => {}
    }
}
// 2. Modal intercepts (help, report detail, role selector — early return)
// 3. Focus-specific dispatch
match self.focus {
    ExpertList => {}
    TaskInput => self.handle_task_input_keys(code, modifiers),  // sync
}
// 4. Tab/BackTab focus cycling
if key.code == KeyCode::Tab { ... }
// 5. Other context-specific handlers (Ctrl+S, Ctrl+R, Ctrl+W, etc.)
```

**Updated flow**:
```rust
// 1. Global Ctrl handlers — ADD Ctrl+J toggle (early return)
if key.modifiers.contains(KeyModifiers::CONTROL) {
    match key.code {
        Char('c') | Char('q') => { self.quit(); return Ok(()); }
        Char('h') => { self.help_modal.toggle(); return Ok(()); }
        Char('j') => {                                    // ← NEW
            self.expert_panel_display.toggle();
            if !self.expert_panel_display.is_visible()
                && self.focus == FocusArea::ExpertPanel
            {
                self.set_focus(FocusArea::TaskInput);     // P2
            }
            return Ok(());
        }
        _ => {}
    }
}
// 2. Modal intercepts (unchanged)
// 3. Focus-specific dispatch — ADD ExpertPanel arm with .await
match self.focus {
    ExpertList => {}
    TaskInput => self.handle_task_input_keys(code, modifiers),
    ExpertPanel => {                                      // ← NEW
        self.handle_expert_panel_keys(code, modifiers).await?;
        return Ok(());  // ← early return: skip Tab cycling and
                        //   other TaskInput-specific handlers below
    }
}
// 4. Tab/BackTab focus cycling (unchanged — only reached when NOT ExpertPanel)
// 5. Other context-specific handlers (unchanged)
```

**Why `return Ok(())` in the ExpertPanel arm?**

When the ExpertPanel is focused, it acts as an interactive terminal. All keys (including Tab, Esc, Enter, arrow keys) must be forwarded to the tmux pane — none should leak into the Tab cycling logic (step 4) or the TaskInput-specific Ctrl+S/Ctrl+R/Ctrl+W handlers (step 5). The early return after `.await` ensures complete input isolation.

The `.await` is safe here because `handle_events()` is already `async fn` and runs within the tokio runtime in the `run()` main loop. The `send_keys` call inside `handle_expert_panel_keys` is a lightweight tmux command (~1ms) and will not block the event loop.

**`handle_expert_panel_keys` method**:
```rust
async fn handle_expert_panel_keys(
    &mut self,
    code: KeyCode,
    modifiers: KeyModifiers,
) -> Result<()> {
    // Local panel navigation — NOT forwarded to tmux
    match code {
        KeyCode::PageUp => {
            self.expert_panel_display.scroll_up();
            return Ok(());
        }
        KeyCode::PageDown => {
            self.expert_panel_display.scroll_down();
            return Ok(());
        }
        _ => {}
    }

    // Forward all other keys to the expert's tmux pane
    if let Some(expert_id) = self.expert_panel_display.expert_id() {
        if let Some(tmux_key) = keycode_to_tmux_key(code, modifiers) {
            if let Err(e) = self.claude.send_keys(expert_id, &tmux_key).await {
                tracing::warn!("Failed to forward key to expert {}: {}", expert_id, e);
                self.set_message(format!("Key send failed: {}", e));
            }
        }
    }
    Ok(())
}
```

#### 3.4.3 Other UI changes

**`UI::render`** (`src/tower/ui.rs`): Conditional layout — 6 chunks when hidden, 7 when visible.

**`UI::render_footer`** (`src/tower/ui.rs`): Add `Ctrl+J: Panel` hint.

**`widgets/mod.rs`**: Add `mod expert_panel_display` and `pub use`.

**`Cargo.toml`**: Add `ansi-to-tui = "8"`.

## 4. Error Handling

| Error | Strategy |
|-------|----------|
| `capture_pane_with_escapes` fails | `tracing::warn!`, keep last content displayed |
| `IntoText` ANSI parse error | Fall back to `Text::raw()` (plain text, no colors) |
| `send_keys` fails when forwarding input | `tracing::warn!`, show status bar message |
| No expert selected | Display "(no expert selected)" in panel title |
| Panel hidden while focused | Auto-move focus to `TaskInput` |

## 5. Correctness Properties

1. **P1 Visibility-Layout**: Hidden → 6 layout constraints. Visible → 7 constraints.
2. **P2 Focus-Visibility Coherence**: `focus == ExpertPanel` implies `is_visible() == true`. Hiding moves focus to `TaskInput`.
3. **P3 Focus Cycle Completeness**: N `next_focus()` calls (N = focusable areas) returns to start. `ExpertList` always skipped.
4. **P4 Scroll Bounds**: `scroll_offset <= max(0, line_count - visible_height)` after render. `scroll_up()` at 0 is no-op.
5. **P5 Expert Tracking**: Panel `expert_id` matches `status_display.selected_expert_id()` after poll.
6. **P6 Poll Gating**: No capture when: (a) panel hidden, (b) input within 500ms, (c) <1000ms since last poll.
7. **P7 Input Isolation**: No keys forwarded when `ExpertPanel` is not focused. When focused, only `PageUp`/`PageDown` are intercepted locally; all other keys (including `Tab`, `Esc`) are forwarded to tmux. `Ctrl+J` never reaches the panel — it is handled globally before focus dispatch.
8. **P8 Content Reset on Expert Change**: Different `expert_id` → scroll=0, content cleared.
9. **P9 Toggle Idempotency**: `toggle(); toggle()` returns to original visibility.
10. **P10 ANSI Fallback**: Parse error → raw text displayed, no panic.
11. **P11 LayoutAreas Zero Rect**: Hidden → `expert_panel = Rect::default()`. Mouse click does not match zero rect.

## 6. Testing Strategy (TDD — tests before implementation)

### Widget Tests (`src/tower/widgets/expert_panel_display.rs`)
- `panel_starts_hidden` (P9)
- `toggle_makes_visible` / `toggle_twice_returns_to_hidden` (P9)
- `starts_unfocused` / `set_focused_changes_state` (P2)
- `set_expert_tracks_id` (P5)
- `set_expert_different_resets_scroll` / `set_expert_same_preserves_scroll` (P8)
- `scroll_up_at_zero_stays_zero` / `scroll_down_increments` (P4)
- `scroll_to_bottom_enables_auto_scroll` / `scroll_up_disables_auto_scroll` (P4)

### TmuxSender Tests (`src/session/tmux.rs`)
- `capture_pane_with_escapes_default_falls_back` (P10)

### TowerApp Tests (`src/tower/app.rs`)
- `focus_cycle_without_panel_skips_expert_panel` (P3)
- `focus_cycle_with_panel_includes_expert_panel` (P3)
- `hiding_panel_while_focused_moves_to_task_input` (P2)
- `toggle_panel_visibility` (P1)
- `mouse_click_does_not_match_zero_rect` (P11)
- Property test: focus cycle roundtrip for arbitrary panel visibility (P3)

### Key Conversion Tests (`src/tower/app.rs`)
- `keycode_to_tmux_key_char` / `_ctrl_char` / `_enter` (P7)
- `keycode_to_tmux_key_tab_returns_tab_string` (P7) — Tab is forwarded as `"Tab"`, NOT None
- `keycode_to_tmux_key_esc_returns_escape_string` (P7) — Esc is forwarded as `"Escape"`, NOT None
- `keycode_to_tmux_key_page_up_returns_none` / `_page_down_returns_none` (P7) — reserved for local scroll

### ANSI Parse Tests (`src/tower/widgets/expert_panel_display.rs`)
- `ansi_parse_plain_text` / `ansi_parse_colored_text` / `ansi_parse_malformed_does_not_panic` (P10)

## 7. Implementation Order (TDD)

1. Add `ansi-to-tui = "8"` to `Cargo.toml`
2. Write `ExpertPanelDisplay` tests (RED) → implement widget (GREEN)
3. Write `capture_pane_with_escapes` tests (RED) → implement trait extension (GREEN)
4. Write focus cycling tests (RED) → extend `FocusArea`, `LayoutAreas`, focus logic (GREEN)
5. Write key conversion tests (RED) → implement `keycode_to_tmux_key` (GREEN)
6. Implement `handle_expert_panel_keys` and `poll_expert_panel` with async dispatch (§3.4.2)
7. Implement conditional layout in `ui.rs`
8. Update footer and help modal with `Ctrl+J: Panel` hint
9. Wire `widgets/mod.rs` exports
10. REFACTOR — `make test`, `make` (clippy)

## 8. Files to Modify

| File | Change |
|------|--------|
| `Cargo.toml` | Add `ansi-to-tui = "8"` |
| `src/tower/widgets/expert_panel_display.rs` | **NEW** — widget + tests |
| `src/tower/widgets/mod.rs` | Add module + pub use |
| `src/tower/app.rs` | FocusArea, LayoutAreas, TowerApp fields, focus cycling, async input handling (§3.4.2), polling, toggle, mouse click |
| `src/tower/ui.rs` | Conditional layout (6 vs 7 chunks), render expert panel, footer hint |
| `src/tower/widgets/help_modal.rs` | Add `Ctrl+J: Toggle expert panel` |
| `src/session/tmux.rs` | Add `capture_pane_with_escapes` to trait + TmuxManager impl |

## 9. Verification

1. `make test` — all existing + new tests pass
2. `make` — compiles without warnings (clippy)
3. Manual: Run `macot tower`, press `Ctrl+J` → panel appears below Task Input
4. Manual: Select an expert, verify pane content appears with colors
5. Manual: Focus panel (Tab from TaskInput when panel visible), type characters → forwarded to expert pane
6. Manual: `Tab` and `Esc` in panel are forwarded to tmux (not intercepted)
7. Manual: `PageUp`/`PageDown` scrolls panel content locally
8. Manual: `Ctrl+J` again → panel hides, focus returns to TaskInput
9. Manual: Resize terminal → layout degrades gracefully
