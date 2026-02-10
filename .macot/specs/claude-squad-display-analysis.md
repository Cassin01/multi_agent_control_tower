# Analysis: claude-squad Display Stability Techniques

## 1. Overview

claude-squad is a Go-based TUI application that manages multiple Claude Code instances running concurrently in tmux sessions. It displays a live preview of each instance's terminal output without display corruption. This document analyzes the key techniques used to achieve stable, flicker-free rendering.

## 2. Architecture Summary

```
┌─────────────────────────────────────────────────────┐
│ BubbleTea Program (tea.WithAltScreen)               │
│                                                     │
│  ┌──────────┐  ┌──────────────────────────────────┐ │
│  │   List    │  │      TabbedWindow                │ │
│  │ (30% W)  │  │  ┌───────────┐ ┌──────────────┐  │ │
│  │          │  │  │ Preview   │ │   Diff       │  │ │
│  │ Instance │  │  │ Pane      │ │   Pane       │  │ │
│  │ items    │  │  │(tmux cap) │ │ (git diff)   │  │ │
│  │          │  │  └───────────┘ └──────────────┘  │ │
│  └──────────┘  └──────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────┐ │
│  │                    Menu                        │ │
│  └────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────┐ │
│  │                   ErrBox                       │ │
│  └────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────┘
         ↕ (tmux capture-pane -p -e -J)
┌─────────────────────────────────────────────────────┐
│  tmux sessions (detached, PTY-attached)             │
│  ┌───────────┐ ┌───────────┐ ┌───────────┐         │
│  │ claude #1 │ │ claude #2 │ │ claude #3 │         │
│  └───────────┘ └───────────┘ └───────────┘         │
└─────────────────────────────────────────────────────┘
```

## 3. Key Display Stability Techniques

### 3.1 Alternate Screen Buffer (`tea.WithAltScreen`)

**File**: `app/app.go:27`

```go
p := tea.NewProgram(
    newHome(ctx, program, autoYes),
    tea.WithAltScreen(),
    tea.WithMouseCellMotion(),
)
```

The application uses BubbleTea's alternate screen mode. This is the most fundamental technique:
- Switches to an alternate terminal buffer, isolating the TUI from the normal scrollback
- When the program exits, the terminal returns to the original state
- Prevents any "garbage" output from leaking into the user's normal terminal

### 3.2 PTY-Based Detached tmux Sessions

**Files**: `session/tmux/tmux.go`, `session/tmux/pty.go`

Rather than running Claude directly in the visible terminal, each instance runs in a **detached tmux session** with a PTY attached:

```go
// Start creates a detached session, then attaches via PTY
cmd := exec.Command("tmux", "new-session", "-d", "-s", t.sanitizedName, "-c", workDir, t.program)
ptmx, err := t.ptyFactory.Start(cmd)
```

The PTY (`creack/pty` library) is used to:
1. **Create the tmux session detached** (`-d` flag) so Claude's output never directly touches the host terminal
2. **Attach via a hidden PTY** to maintain size control without rendering to stdout
3. Keep the tmux session alive independently of the TUI's rendering cycle

This is the core isolation mechanism: Claude's raw terminal output is captured and processed, never directly piped to the user's terminal.

### 3.3 Controlled Content Capture via `tmux capture-pane`

**File**: `session/tmux/tmux.go:476-496`

```go
func (t *TmuxSession) CapturePaneContent() (string, error) {
    cmd := exec.Command("tmux", "capture-pane", "-p", "-e", "-J", "-t", t.sanitizedName)
    output, err := t.cmdExec.Output(cmd)
    ...
}
```

Key flags:
- **`-p`**: Prints captured content to stdout (instead of a tmux buffer)
- **`-e`**: Preserves ANSI escape sequences (color codes) for accurate rendering
- **`-J`**: Joins wrapped lines, preventing line-break artifacts
- **`-t`**: Targets a specific session name

This gives a clean, well-formed snapshot of the terminal state at any given moment, rather than a stream of potentially fragmented output.

### 3.4 PTY Window Size Synchronization

**File**: `session/tmux/tmux.go:455-467`, `session/instance.go:328-334`

```go
func (t *TmuxSession) SetDetachedSize(width, height int) error {
    return t.updateWindowSize(width, height)
}

func (t *TmuxSession) updateWindowSize(cols, rows int) error {
    return pty.Setsize(t.ptmx, &pty.Winsize{
        Rows: uint16(rows),
        Cols: uint16(cols),
    })
}
```

The tmux session's PTY is **resized to match the preview pane dimensions**. This is critical because:
- tmux wraps content based on the terminal width known to the PTY
- If the tmux pane is wider/narrower than the preview area, lines would wrap incorrectly, causing display corruption
- By syncing sizes, the captured content fits exactly into the preview pane

This sync is triggered whenever the window resizes:

```go
// app.go:167-170
previewWidth, previewHeight := m.tabbedWindow.GetPreviewSize()
if err := m.list.SetSessionPreviewSize(previewWidth, previewHeight); err != nil {
    log.ErrorLog.Print(err)
}
```

### 3.5 Debounced Window Resize Events

**File**: `session/tmux/tmux_unix.go:16-78`

```go
func (t *TmuxSession) monitorWindowSize() {
    winchChan := make(chan os.Signal, 1)
    signal.Notify(winchChan, syscall.SIGWINCH)

    // Debounce resize events
    debouncedWinch := make(chan os.Signal, 1)
    go func() {
        var resizeTimer *time.Timer
        for {
            select {
            case <-t.ctx.Done():
                return
            case <-winchChan:
                if resizeTimer != nil {
                    resizeTimer.Stop()
                }
                resizeTimer = time.AfterFunc(50*time.Millisecond, func() {
                    select {
                    case debouncedWinch <- syscall.SIGWINCH:
                    case <-t.ctx.Done():
                    }
                })
            }
        }
    }()
    ...
}
```

When the user resizes the terminal:
- SIGWINCH signals are **debounced with a 50ms timer** to prevent rapid-fire resize operations
- This avoids display glitches from partially-rendered intermediate states during continuous resizing
- The PTY is resized only once the user stops resizing

### 3.6 Tick-Based Polling with Separate Cadences

**File**: `app/app.go:174-185, 191-199, 203-222`

Two independent polling loops run at different frequencies:

1. **Preview update tick (100ms)**: Updates the visible preview pane content
```go
case previewTickMsg:
    cmd := m.instanceChanged()
    return m, tea.Batch(cmd, func() tea.Msg {
        time.Sleep(100 * time.Millisecond)
        return previewTickMsg{}
    })
```

2. **Metadata update tick (500ms)**: Checks instance status changes (Running/Ready/Prompt)
```go
var tickUpdateMetadataCmd = func() tea.Msg {
    time.Sleep(500 * time.Millisecond)
    return tickUpdateMetadataMessage{}
}
```

This separation prevents expensive operations (diff stats, status hashing) from blocking the fast preview refresh, reducing perceived lag and flicker.

### 3.7 Content Hash-Based Change Detection

**File**: `session/tmux/tmux.go:204-267`

```go
type statusMonitor struct {
    prevOutputHash []byte
}

func (t *TmuxSession) HasUpdated() (updated bool, hasPrompt bool) {
    content, err := t.CapturePaneContent()
    ...
    if !bytes.Equal(t.monitor.hash(content), t.monitor.prevOutputHash) {
        t.monitor.prevOutputHash = t.monitor.hash(content)
        return true, hasPrompt
    }
    return false, hasPrompt
}
```

SHA-256 hashing of pane content means:
- Status changes are only triggered when content actually changes
- Unnecessary re-renders are avoided when Claude's output is idle
- Memory is saved by storing only hashes rather than full content strings

### 3.8 lipgloss-Based Strict Layout Control

**File**: `app/app.go:147-172, 724-754`

All layout computation uses `lipgloss` functions that guarantee consistent dimensions:

```go
func (m *home) updateHandleWindowSizeEvent(msg tea.WindowSizeMsg) {
    listWidth := int(float32(msg.Width) * 0.3)
    tabsWidth := msg.Width - listWidth
    contentHeight := int(float32(msg.Height) * 0.9)
    menuHeight := msg.Height - contentHeight - 1
    ...
}
```

The View function uses `lipgloss.JoinHorizontal` and `lipgloss.JoinVertical` to compose the layout:

```go
func (m *home) View() string {
    listAndPreview := lipgloss.JoinHorizontal(lipgloss.Top, listWithPadding, previewWithPadding)
    mainView := lipgloss.JoinVertical(lipgloss.Center, listAndPreview, m.menu.String(), m.errBox.String())
    ...
}
```

This approach:
- Ensures each component fills exactly its allocated space
- Prevents content overflow causing misaligned lines
- Uses `lipgloss.Place()` for exact positioning within allocated boxes

### 3.9 Preview Pane Height Padding and Truncation

**File**: `ui/preview.go:160-179`

```go
availableHeight := p.height - 1  // 1 for ellipsis
lines := strings.Split(p.previewState.text, "\n")

if len(lines) > availableHeight {
    lines = lines[:availableHeight]
    lines = append(lines, "...")
} else {
    // Pad with empty lines to fill available height
    padding := availableHeight - len(lines)
    lines = append(lines, make([]string, padding)...)
}
```

The preview pane always outputs **exactly the same number of lines**:
- If content is too long: truncate and show "..."
- If content is too short: pad with empty lines

This prevents the layout from "jumping" when Claude's output changes in length, which is the most common cause of display corruption in terminal UIs.

### 3.10 Width Control via `go-runewidth`

**Files**: `ui/list.go`, `ui/err.go`

```go
// list.go:143-146
widthAvail := r.width - 3 - runewidth.StringWidth(prefix) - 1
if widthAvail > 0 && runewidth.StringWidth(titleText) > widthAvail {
    titleText = runewidth.Truncate(titleText, widthAvail-3, "...")
}
```

```go
// err.go:43-45
if runewidth.StringWidth(err) > e.width-3 && e.width-3 >= 0 {
    err = runewidth.Truncate(err, e.width-3, "...")
}
```

Uses `go-runewidth` which correctly handles:
- Full-width CJK characters (2 cells wide)
- Combining characters and diacritics
- Emoji and other multi-byte characters

This prevents display corruption from characters that take more than 1 terminal column.

### 3.11 Stdin Control Sequence Filtering on Attach

**File**: `session/tmux/tmux.go:316-328`

```go
// Nuke the first bytes of stdin, up to 64, to prevent tmux from reading it.
// When we attach, there tends to be terminal control sequences like ?[?62c0;95;0c
select {
case <-timeoutCh:
default:
    log.InfoLog.Printf("nuked first stdin: %s", buf[:nr])
    continue
}
```

When attaching to a tmux session, terminals often send identification/capability sequences. These are **dropped during the first 50ms** of attachment to prevent:
- Terminal control sequences being interpreted as user input
- Display corruption from escape sequences being forwarded to the tmux pane

### 3.12 Overlay System with Background Fade

**File**: `ui/overlay/overlay.go:46-171`

```go
func PlaceOverlay(x, y int, fg, bg string, shadow bool, center bool, ...) string {
    // Apply a fade effect to the background
    fadedBgLines := make([]string, len(bgLines))
    bgColorRegex := regexp.MustCompile(`\x1b\[48;[25];[0-9;]+m`)
    fgColorRegex := regexp.MustCompile(`\x1b\[38;[25];[0-9;]+m`)
    ...
}
```

The overlay system:
- Composes foreground (dialog) over background (main UI) at the character level
- Uses ANSI-aware string manipulation to replace background with faded colors
- Properly handles escape sequences to prevent color bleed between layers
- Uses `cutLeft()` to slice ANSI-aware strings at exact column positions

### 3.13 Fixed-Size Component Architecture

Every UI component has explicit `SetSize(width, height)` methods and renders exactly within those bounds:

| Component | Size Control |
|-----------|-------------|
| `List` | `SetSize(width, height)` with `lipgloss.Place()` |
| `PreviewPane` | `SetSize(width, maxHeight)` with line padding |
| `DiffPane` | `SetSize(width, height)` with viewport |
| `TabbedWindow` | `SetSize(width, height)` with computed tab/content heights |
| `Menu` | `SetSize(width, height)` with `lipgloss.Place()` |
| `ErrBox` | `SetSize(width, height)` with `lipgloss.Place()` |

This "box model" ensures every component fills its allocated space exactly, preventing any gaps or overflow.

## 4. Summary Table

| # | Technique | Purpose | Location |
|---|-----------|---------|----------|
| 1 | Alternate Screen Buffer | Isolate TUI from scrollback | `app/app.go:27` |
| 2 | Detached tmux + PTY | Isolate Claude output from host terminal | `session/tmux/tmux.go:91-191` |
| 3 | `capture-pane -p -e -J` | Clean, joined, ANSI-preserved snapshot | `session/tmux/tmux.go:476-484` |
| 4 | PTY size sync | Match tmux output width to preview pane | `session/tmux/tmux.go:455-467` |
| 5 | Debounced SIGWINCH | Prevent resize flicker | `session/tmux/tmux_unix.go:44-63` |
| 6 | Dual polling cadences | Fast preview (100ms) / slow metadata (500ms) | `app/app.go:191-222` |
| 7 | Content hash change detection | Skip redundant re-renders | `session/tmux/tmux.go:204-267` |
| 8 | lipgloss strict layout | Pixel-exact component placement | `app/app.go:724-754` |
| 9 | Line count padding/truncation | Stable height output | `ui/preview.go:160-179` |
| 10 | `go-runewidth` width control | CJK/emoji safe truncation | `ui/list.go:143-146` |
| 11 | Stdin filter on attach | Drop terminal control sequences | `session/tmux/tmux.go:316-328` |
| 12 | ANSI-aware overlay compositing | Proper dialog rendering | `ui/overlay/overlay.go:46-171` |
| 13 | Fixed-size component boxes | No gaps or overflow | All UI components |

## 5. Key Libraries

| Library | Role |
|---------|------|
| `charmbracelet/bubbletea` | TUI framework (Elm architecture, alt screen, event loop) |
| `charmbracelet/lipgloss` | Layout composition, styling, exact placement |
| `charmbracelet/bubbles/viewport` | Scrollable content with controlled dimensions |
| `creack/pty` | PTY creation and window size control |
| `mattn/go-runewidth` | Unicode-aware string width measurement |
| `muesli/ansi` | ANSI escape sequence handling for overlay |
| `muesli/reflow/truncate` | ANSI-safe string truncation |
