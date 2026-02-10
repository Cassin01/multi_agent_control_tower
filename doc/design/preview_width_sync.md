# Preview Width Synchronization - Architecture Design

## Overview

This document describes the architecture for preventing unwanted line wrapping in the Expert Panel by synchronizing the tmux PTY size with the panel's actual display width. The design is adapted from [claude-squad](https://github.com/smtg-ai/claude-squad)'s padding control mechanism.

## Background: Why Wrapping Occurs

When the Expert Panel displays tmux output:

1. The tmux pane has its own PTY size (columns × rows)
2. Programs running inside tmux (e.g., Claude CLI) format their output based on the PTY column count
3. `tmux capture-pane` returns the output as-is, already wrapped at the PTY width
4. ratatui's `Paragraph` widget with `Wrap { trim: false }` renders the captured text within the panel's inner area
5. **If the PTY width ≠ the panel's inner width**, double-wrapping or truncation occurs

The solution: **set the tmux PTY width to match the panel's usable display width**.

---

## Reference: claude-squad's Approach

claude-squad uses a layered size chain with a 30%/70% horizontal split:

```
Terminal (W × H)
    ↓
① listWidth = W × 0.3, tabsWidth = W - listWidth
    ↓
② TabbedWindow.SetSize(tabsWidth, contentHeight)
   → w.width = AdjustPreviewWidth(tabsWidth) = tabsWidth × 0.9
    ↓
③ contentHeight = height - tabHeight - windowStyle.GetVerticalFrameSize() - 2
   contentWidth  = w.width - windowStyle.GetHorizontalFrameSize()
    ↓
④ preview.SetSize(contentWidth, contentHeight)
    ↓
⑤ GetPreviewSize() → list.SetSessionPreviewSize()
   → instance.SetPreviewSize() → tmuxSession.SetDetachedSize()
   → pty.Setsize(ptmx, {Rows: height, Cols: width})
```

Key design choices:
- `AdjustPreviewWidth()` applies a **90% factor** for visual margin within the TabbedWindow
- `SetSessionPreviewSize()` propagates dimensions to **all active instances**
- Size is set via `pty.Setsize()` (direct PTY ioctl)

---

## macot Adaptation

### Structural Differences

| Aspect | claude-squad | macot |
|--------|-------------|-------|
| Language | Go (bubbletea/lipgloss) | Rust (ratatui/crossterm) |
| Layout | 30% list / 70% preview (horizontal split) | 100% width, vertical stack |
| Tab system | TabbedWindow with tab bar | No tabs (single expert panel) |
| tmux resize | `pty.Setsize()` (direct PTY ioctl) | `tmux resize-pane -x -y` (command) |
| Instance management | Multiple instances with `SetSessionPreviewSize()` | Single active expert view |
| Width adjustment | 90% factor (`AdjustPreviewWidth`) | Not needed (no tab margin) |

### All-Expert Resize on Terminal Resize

When the terminal is resized (panel size changes), **all** expert panes are resized in a loop — mirroring claude-squad's `SetSessionPreviewSize()` (list.go:86-98). This ensures that when switching experts, the newly viewed pane is already at the correct width.

When only the **viewed expert changes** (no size change), only the new expert's pane is resized — avoiding redundant tmux commands.

The `last_resized_expert_id` field tracks which expert was last resized to detect expert switches. Combined with `last_preview_size` for size change detection, this gives two-axis change detection:

| Condition | Action |
|-----------|--------|
| Size changed | Resize all N expert panes |
| Expert switched (same size) | Resize only new expert's pane |
| Neither changed | No resize (skip) |

### Key Decision: No 90% Factor

In claude-squad, `AdjustPreviewWidth(width × 0.9)` creates visual margin within the TabbedWindow. In macot, the Expert Panel has no such wrapper — ratatui's `Layout` handles spacing via `margin(1)` and the panel's `Borders::ALL`. Therefore, the 90% factor is **not applicable**.

Instead, we apply a **1-column safety margin** to prevent edge-case wrapping caused by:
- Wide characters (CJK) at width boundaries
- Programs outputting lines at exactly the terminal width
- ANSI escape sequence length discrepancies after `-J` join

---

## Architecture

### Current Size Chain (Before)

```
Terminal (W × H)
    ↓
Layout::margin(1) → W-2, H-2
    ↓
Expert Panel Rect (Percentage(40) or Length(10))
    ↓
render(): last_render_size = (area.width - 2, area.height - 2)
    ↓
poll_expert_panel(): resize_pane(last_render_size.0, last_render_size.1)  ← DIRECT
    ↓
tmux resize-pane -x {width} -y {height}
```

Problem: `last_render_size` is passed directly to `resize_pane` with no safety margin and no explicit size computation layer.

### Proposed Size Chain (After)

```
Terminal (W × H)
    ↓
① Layout::margin(1)
   available = (W - 2, H - 2)
    ↓
② Expert Panel Rect (from Layout constraint)
   panel_area = (panel_width, panel_height)
    ↓
③ ExpertPanelDisplay::render()
   inner_width  = panel_width  - BORDER_WIDTH (2)     // Borders::ALL
   inner_height = panel_height - BORDER_HEIGHT (2)     // Borders::ALL
   → stored in last_render_size
    ↓
④ ExpertPanelDisplay::preview_size()
   preview_width  = inner_width - PREVIEW_WIDTH_MARGIN (1)
   preview_height = inner_height
    ↓
⑤ poll_expert_panel()
   if size_changed: resize ALL expert panes (mirrors SetSessionPreviewSize())
   if expert_switched: resize only the newly viewed pane
    ↓
⑥ tmux resize-pane -x {preview_width} -y {preview_height}
    ↓
⑦ tmux capture-pane -e -J -p
   → output lines formatted at preview_width
    ↓
⑧ Paragraph::new(content).wrap(Wrap { trim: false })
   Lines fit within inner_width (preview_width < inner_width)
   → No unwanted re-wrapping ✓
```

### Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                     Terminal Resize Event                         │
│                         (W × H)                                  │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                      ratatui Layout                              │
│   margin(1): available = (W-2, H-2)                              │
│   Expert Panel constraint: Percentage(40) or Length(10)          │
│   → panel_area: Rect                                             │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│              ExpertPanelDisplay::render()                         │
│                                                                  │
│   Block (Borders::ALL)                                           │
│   ┌──────────────────────────────────────────┐                   │
│   │ ┌──────────────────────────────────────┐ │                   │
│   │ │  inner area                          │ │                   │
│   │ │  width  = panel_width  - 2           │ │                   │
│   │ │  height = panel_height - 2           │ │                   │
│   │ │                                      │ │ ← last_render_size│
│   │ │  Paragraph(content)                  │ │                   │
│   │ │    .wrap(Wrap { trim: false })       │ │                   │
│   │ │    .scroll(offset)                   │ │                   │
│   │ └──────────────────────────────────────┘ │                   │
│   └──────────────────────────────────────────┘                   │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│              ExpertPanelDisplay::preview_size()    [NEW]          │
│                                                                  │
│   preview_width  = inner_width - PREVIEW_WIDTH_MARGIN (1)        │
│   preview_height = inner_height                                  │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│              TowerApp::poll_expert_panel()                        │
│                                                                  │
│   size_changed  = preview_size ≠ last_preview_size               │
│   expert_changed = last_resized_expert_id ≠ expert_id            │
│                                                                  │
│   if size_changed:                                               │
│     for id in 0..num_experts:                                    │
│       resize_pane(id, preview_width, preview_height)             │
│     last_preview_size = preview_size                             │
│   else if expert_changed:                                        │
│     resize_pane(expert_id, preview_width, preview_height)        │
│   last_resized_expert_id = expert_id                             │
│                                                                  │
│   capture_pane_with_escapes(expert_id)                           │
│     → try_set_content(raw)                                       │
└───────────────────────────┬─────────────────────────────────────┘
                            │
                            ▼
┌─────────────────────────────────────────────────────────────────┐
│                tmux Session (per expert)                          │
│                                                                  │
│   resize-pane -x {preview_width} -y {preview_height}            │
│   → PTY cols = preview_width                                     │
│   → Claude CLI formats output at preview_width columns           │
│                                                                  │
│   capture-pane -e -J -p                                          │
│   → returns lines already wrapped at preview_width               │
└─────────────────────────────────────────────────────────────────┘
```

---

## Padding Constants

| Constant | Value | Location | Purpose |
|----------|-------|----------|---------|
| `Layout::margin(1)` | 1 per side | `ui.rs` | Global outer margin |
| `Borders::ALL` width | 2 total | `expert_panel_display.rs` | Left + right border |
| `Borders::ALL` height | 2 total | `expert_panel_display.rs` | Top + bottom border |
| `PREVIEW_WIDTH_MARGIN` | 1 | `expert_panel_display.rs` | Safety margin for edge cases |

### Why PREVIEW_WIDTH_MARGIN = 1

The 1-column margin ensures:
- Lines at exactly `preview_width` characters fit within `inner_width` (`preview_width + 1`)
- Wide characters (2-column) near the boundary don't overflow
- `Wrap { trim: false }` has no opportunity to re-wrap

This differs from claude-squad's 10% factor because macot has no TabbedWindow visual margin to account for — the padding is purely for wrapping prevention.

---

## Numeric Examples

### Terminal 120×40

| Step | Width | Height | Calculation |
|------|-------|--------|-------------|
| Terminal | 120 | 40 | Raw |
| After margin(1) | 118 | 38 | -1 per side |
| Panel Rect (40%) | 118 | 15 | `Percentage(40)` of 38 |
| Inner (borders) | 116 | 13 | -1 per side |
| **Preview size** | **115** | **13** | -1 margin |
| tmux PTY | 115 cols × 13 rows | | `tmux resize-pane` |

### Terminal 80×24

| Step | Width | Height | Calculation |
|------|-------|--------|-------------|
| Terminal | 80 | 24 | Raw |
| After margin(1) | 78 | 22 | -1 per side |
| Panel Rect (Length(10)) | 78 | 10 | Small terminal → fixed |
| Inner (borders) | 76 | 8 | -1 per side |
| **Preview size** | **75** | **8** | -1 margin |
| tmux PTY | 75 cols × 8 rows | | `tmux resize-pane` |

### Terminal 200×60

| Step | Width | Height | Calculation |
|------|-------|--------|-------------|
| Terminal | 200 | 60 | Raw |
| After margin(1) | 198 | 58 | -1 per side |
| Panel Rect (40%) | 198 | 23 | `Percentage(40)` of 58 |
| Inner (borders) | 196 | 21 | -1 per side |
| **Preview size** | **195** | **21** | -1 margin |
| tmux PTY | 195 cols × 21 rows | | `tmux resize-pane` |

---

## Implementation

### Changes by File

#### 1. `src/tower/widgets/expert_panel_display.rs`

Add `PREVIEW_WIDTH_MARGIN` constant and `preview_size()` method.

```rust
/// Safety margin subtracted from inner width when setting tmux PTY size.
/// Prevents edge-case line wrapping at width boundaries.
const PREVIEW_WIDTH_MARGIN: u16 = 1;

impl ExpertPanelDisplay {
    /// Returns the effective dimensions for tmux PTY synchronization.
    ///
    /// The preview size is smaller than the render inner size by
    /// PREVIEW_WIDTH_MARGIN columns. This ensures that tmux output
    /// (formatted at preview_width) fits within the display area
    /// (inner_width) without triggering ratatui's Wrap.
    ///
    /// Size chain:
    ///   Terminal → Layout margin(1) → Panel Rect → Borders::ALL
    ///   → inner size (last_render_size)
    ///   → preview size (inner - PREVIEW_WIDTH_MARGIN)
    ///   → tmux resize-pane
    pub fn preview_size(&self) -> (u16, u16) {
        let (w, h) = self.last_render_size;
        (w.saturating_sub(PREVIEW_WIDTH_MARGIN), h)
    }
}
```

#### 2. `src/tower/app.rs`

Update `poll_expert_panel()` to use `preview_size()` instead of `last_render_size()`.

Rename `last_panel_size` → `last_preview_size` for clarity.

```rust
// In TowerApp struct:
last_preview_size: (u16, u16),  // renamed from last_panel_size

// In poll_expert_panel():
async fn poll_expert_panel(&mut self) -> Result<()> {
    // ... (existing visibility/debounce checks) ...

    if let Some(expert_id) = self.expert_panel_display.expert_id() {
        // Use preview_size() which applies safety margin,
        // not raw last_render_size()
        let preview_size = self.expert_panel_display.preview_size();
        if preview_size != self.last_preview_size
            && preview_size.0 > 0
            && preview_size.1 > 0
        {
            if let Err(e) = self
                .claude
                .resize_pane(expert_id, preview_size.0, preview_size.1)
                .await
            {
                tracing::warn!(
                    "Failed to resize window for expert {}: {}",
                    expert_id, e
                );
            }
            self.last_preview_size = preview_size;
        }

        match self.claude.capture_pane_with_escapes(expert_id).await {
            Ok(raw) => {
                self.expert_panel_display.try_set_content(&raw);
            }
            Err(e) => {
                tracing::warn!(
                    "Failed to capture expert {} window: {}",
                    expert_id, e
                );
            }
        }
    }

    Ok(())
}
```

### Tests

#### `expert_panel_display.rs`

```rust
#[test]
fn preview_size_subtracts_margin_from_render_size() {
    let mut panel = ExpertPanelDisplay::new();
    panel.set_content(Text::raw("hello"), 1);

    use ratatui::{Terminal, backend::TestBackend};
    let backend = TestBackend::new(40, 10);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| panel.render(frame, frame.area()))
        .unwrap();

    // inner = (40-2, 10-2) = (38, 8)
    assert_eq!(
        panel.last_render_size(),
        (38, 8),
        "preview_size: last_render_size should be inner dimensions"
    );
    // preview = (38-1, 8) = (37, 8)
    assert_eq!(
        panel.preview_size(),
        (37, 8),
        "preview_size: should subtract PREVIEW_WIDTH_MARGIN from width"
    );
}

#[test]
fn preview_size_saturates_at_zero() {
    let panel = ExpertPanelDisplay::new();
    // last_render_size = (0, 0) by default
    assert_eq!(
        panel.preview_size(),
        (0, 0),
        "preview_size: should saturate at zero, not underflow"
    );
}

#[test]
fn preview_size_with_narrow_terminal() {
    let mut panel = ExpertPanelDisplay::new();
    panel.set_content(Text::raw("x"), 1);

    use ratatui::{Terminal, backend::TestBackend};
    // Minimum viable: 3 wide (border + 1 content col + border)
    let backend = TestBackend::new(3, 5);
    let mut terminal = Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| panel.render(frame, frame.area()))
        .unwrap();

    // inner = (3-2, 5-2) = (1, 3)
    // preview = (1-1, 3) = (0, 3)
    assert_eq!(
        panel.preview_size(),
        (0, 3),
        "preview_size: narrow terminal should saturate width to 0"
    );
}
```

---

## Future Enhancements (Out of Scope)

### Configurable Margin

If users experience wrapping issues, `PREVIEW_WIDTH_MARGIN` could be made configurable:

```yaml
# macot.yaml
ui:
  preview_width_margin: 1  # default
```

### Vertical Padding

Add height margin for a potential truncation indicator (like claude-squad's `"..."` line):

```rust
const PREVIEW_HEIGHT_MARGIN: u16 = 0;  // future use

pub fn preview_size(&self) -> (u16, u16) {
    let (w, h) = self.last_render_size;
    (
        w.saturating_sub(PREVIEW_WIDTH_MARGIN),
        h.saturating_sub(PREVIEW_HEIGHT_MARGIN),
    )
}
```

---

## Comparison with claude-squad

### Equivalent Mapping

| claude-squad | macot | Notes |
|-------------|-------|-------|
| `AdjustPreviewWidth(w × 0.9)` | N/A | Not needed (no TabbedWindow margin) |
| `TabbedWindow.SetSize()` | `Layout` + `render()` | ratatui handles layout |
| `windowStyle.GetHorizontalFrameSize()` | `Borders::ALL` (-2) | Same concept |
| `windowStyle.GetVerticalFrameSize()` | `Borders::ALL` (-2) | Same concept |
| `tabHeight` (-3) | N/A | No tab bar in macot |
| `- 2` (spacing) | N/A | No tab-content gap |
| `preview.SetSize()` | `last_render_size` | Stored during render |
| `GetPreviewSize()` | `preview_size()` | **NEW**: with safety margin |
| `SetSessionPreviewSize()` | `poll_expert_panel()` all-expert loop | Resize all on size change, single on expert switch |
| `pty.Setsize()` | `tmux resize-pane` | Different mechanism, same effect |

### Why It's Simpler in macot

1. **No horizontal split**: macot's expert panel takes full width → no `listWidth`/`tabsWidth` calculation
2. **No tab bar**: No `tabHeight` or tab border subtraction
3. **ratatui Layout handles sizing**: No manual `SetSize()` chain needed
4. **Single mechanism**: `tmux resize-pane` command vs direct PTY ioctl

The core principle is identical: **make the tmux PTY column count match the display area width**.
