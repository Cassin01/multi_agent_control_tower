# macot CLI Reference

**macot** (Multi Agent Control Tower) orchestrates multiple Claude CLI instances for collaborative software development.

---

## Commands Overview

| Command | Description |
|---------|-------------|
| [`start`](#macot-start) | Initialize expert session with Claude agents |
| [`down`](#macot-down) | Gracefully shut down expert session |
| [`tower`](#macot-tower) | Launch the control tower TUI |
| [`launch`](#macot-launch) | Initialize session and open TUI in one step |
| [`status`](#macot-status) | Display current session status |
| [`sessions`](#macot-sessions) | List all running macot sessions |
| [`reset`](#macot-reset) | Reset expert context and instructions |
| [`expert`](#macot-expert) | Add or list experts in a running session |

---

## macot start

Initialize expert session with Claude agents.

### Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `project_path` | PathBuf | `.` | Path to project directory |

### Options

| Option | Short | Type | Description |
|--------|-------|------|-------------|
| `--num-experts` | `-n` | u32 | Number of experts (overrides config) |
| `--config` | `-c` | PathBuf | Custom config file path |

### Examples

```bash
# Start session in current directory
macot start

# Start session in specific directory with 4 experts
macot start /path/to/project -n 4

# Start with custom config
macot start . --config ./custom-config.yaml
```

### Behavior

1. Resolves the project path to an absolute path
2. Loads configuration (from custom path or default)
3. Creates a tmux session named `macot-<hash>`
4. Initializes queue and context storage
5. Launches Claude CLI in each window
6. Waits for agents to become ready
7. Sends initial instructions from `instructions/core.md` and `instructions/<expert-name>.md`

### Output

```
Starting macot session for: /path/to/project
Creating session: macot-a1b2c3d4
Number of experts: 4
  [0] architect - Launching Claude...
  [1] planner - Launching Claude...
  [2] general - Launching Claude...
  [3] debugger - Launching Claude...

Waiting for agents to be ready...
  [0] architect - Ready
  [1] planner - Ready
  [2] general - Ready
  [3] debugger - Ready

Session started successfully!
Run 'macot tower' to open the control tower UI
Run 'tmux attach -t macot-a1b2c3d4' to view agents directly
```

---

## macot down

Gracefully shut down expert session.

### Arguments

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `session_name` | String | No | Session name (e.g., macot-a1b2c3d4) |

If `session_name` is omitted and only one session is running, that session is stopped automatically.

### Options

| Option | Short | Type | Description |
|--------|-------|------|-------------|
| `--force` | `-f` | bool | Force kill without graceful shutdown |
| `--cleanup` | - | bool | Clean up context and queue files |

### Examples

```bash
# Stop single running session (graceful shutdown)
macot down

# Stop specific session
macot down macot-a1b2c3d4

# Force kill without sending exit commands
macot down --force

# Stop and clean up all session data
macot down --cleanup

# Force kill specific session and clean up
macot down macot-a1b2c3d4 --force --cleanup
```

### Behavior

**Graceful shutdown (default):**
1. Sends `/exit` command to each Claude agent
2. Waits 10 seconds for graceful termination
3. Kills the tmux session

**Force shutdown (`--force`):**
1. Immediately kills the tmux session without sending exit commands

**Cleanup (`--cleanup`):**
- Removes context files for the session from the queue directory

---

## macot tower

Launch the control tower TUI (Terminal User Interface).

### Arguments

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `session_name` | String | No | Session name to connect to |

If `session_name` is omitted and only one session is running, connects to that session automatically.

### Options

| Option | Short | Type | Description |
|--------|-------|------|-------------|
| `--config` | `-c` | PathBuf | Custom config file path |

### Examples

```bash
# Connect to single running session
macot tower

# Connect to specific session
macot tower macot-a1b2c3d4

# Connect with custom config
macot tower --config ./custom-config.yaml
```

### TUI Controls

| Key | Action |
|-----|--------|
| **Global** | |
| `Ctrl+T` | Switch focus between panels |
| `F1` | Toggle help |
| `F2` | Open Add Expert modal (dynamic expert add) |
| `Ctrl+C` / `Ctrl+Q` | Quit application |
| **Task Input** | |
| `Ctrl+S` | Assign task to selected expert |
| `Ctrl+P` / `Ctrl+N` | Select previous/next expert |
| `Ctrl+O` | Change expert role |
| `Ctrl+R` | Reset selected expert |
| `Ctrl+W` | Launch expert in worktree |
| `Shift+Tab` | Send `BTab` to selected expert (tmux) |
| `Esc` | Clear input |
| **Report List** | |
| `j` / `↓` | Select next report |
| `k` / `↑` | Select previous report |
| `Enter` | Open report detail |
| **Report Detail** | |
| `Esc` / `q` | Close detail |

### Interface

The TUI displays:
- Expert status panel (list of experts with current state)
- Task input panel (compose and assign tasks)

---

## macot launch

Initialize expert session and open the control tower TUI in one step. Equivalent to running `macot start` followed by `macot tower`.

Session infrastructure (tmux session, queue, context store) is initialized synchronously, then expert agents are launched asynchronously in the background while the TUI starts immediately. Experts transition from "pending" to "ready" in the TUI as they come online.

### Arguments

| Argument | Type | Default | Description |
|----------|------|---------|-------------|
| `project_path` | PathBuf | `.` | Path to project directory |

### Options

| Option | Short | Type | Description |
|--------|-------|------|-------------|
| `--num-experts` | `-n` | u32 | Number of experts (overrides config) |
| `--config` | `-c` | PathBuf | Custom config file path |

### Examples

```bash
# Launch session and TUI in current directory
macot launch

# Launch with specific project path and 4 experts
macot launch /path/to/project -n 4

# Launch with custom config
macot launch . --config ./custom-config.yaml
```

### Behavior

1. **Infrastructure sync** — Resolves project path, loads configuration, creates tmux session, initializes queue and context storage
2. **Expert async** — Spawns expert agent launch in the background (Claude CLI launch + readiness wait per expert)
3. **TUI foreground** — Opens the control tower UI immediately; expert startup progress is visible in real-time

> **Note:** This is equivalent to `macot start` + `macot tower`, but the TUI opens sooner because expert readiness is not awaited before launching the UI.

---

## macot status

Display current session status.

### Arguments

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `session_name` | String | No | Session name to check |

If `session_name` is omitted and only one session is running, shows status for that session.

### Examples

```bash
# Show status of single running session
macot status

# Show status of specific session
macot status macot-a1b2c3d4
```

### Output

```
Session: macot-a1b2c3d4 (running)
Project: /path/to/project
Created: 2025-01-31 10:00:00

Experts:
  [0] architect    ○ - Idle
  [1] planner      ◐ - Thinking
  [2] general      ● - Executing
  [3] debugger     ○ - Idle
```

### Status Indicators

| Symbol | Status | Description |
|--------|--------|-------------|
| `○` | Idle | Expert is waiting for tasks |
| `◐` | Thinking | Expert is processing input |
| `●` | Executing | Expert is running tools |
| `✗` | Error | Expert encountered an error |

---

## macot sessions

List all running macot sessions.

### Arguments

None.

### Options

None.

### Examples

```bash
macot sessions
```

### Output

```
SESSION            PROJECT PATH                              EXPERTS CREATED
--------------------------------------------------------------------------------
macot-a1b2c3d4     /path/to/project                               4 2025-01-31 10:00
macot-e5f6g7h8     /path/to/another/project                       3 2025-01-31 11:30
```

If no sessions are running:
```
No macot sessions running.
```

---

## macot reset

Reset expert context and instructions.

### Subcommand: expert

Reset a specific expert's context and reinitialize with instructions.

#### Arguments

| Argument | Type | Required | Description |
|----------|------|----------|-------------|
| `expert` | String | Yes | Expert ID (0-N) or name |

#### Options

| Option | Short | Type | Description |
|--------|-------|------|-------------|
| `--session` | `-s` | String | Session name (required if multiple sessions running) |
| `--keep-history` | - | bool | Keep conversation history (only clears knowledge context) |
| `--full` | - | bool | Full reset including Claude session restart |

### Examples

```bash
# Reset expert by ID (single session)
macot reset expert 0

# Reset expert by name
macot reset expert architect

# Reset expert in specific session
macot reset expert 0 -s macot-a1b2c3d4

# Soft reset - keep conversation history
macot reset expert 0 --keep-history

# Full reset - restart Claude process entirely
macot reset expert 0 --full
```

### Reset Modes

**Standard reset (default):**
1. Clears expert context (or only knowledge if `--keep-history`)
2. Sends `/clear` command to Claude
3. Resends instructions from configuration

**Full reset (`--full`):**
1. Sends `/exit` command to Claude
2. Clears all expert context
3. Relaunches Claude process
4. Resends instructions from configuration

### Output

```
Resetting expert 0 (architect)...
  Clearing context (keep_history=false)...
  Sending /clear to Claude...
  Resending instructions...
Expert 0 reset complete.
```

---

## macot expert

Manage experts inside a **running** macot session without restarting it. Use this to add a new expert on the fly when an extra role is needed, or to inspect the current roster.

### Subcommand: `expert add`

Add a new expert to a running session. Allocates a fresh `expert_id` (monotonically increasing — IDs are never reused), writes the per-expert state files, appends the manifest atomically, and spawns a new tmux window named `expert{N}` for the agent.

#### Options

| Option | Short | Type | Default | Description |
|--------|-------|------|---------|-------------|
| `--session` | `-s` | String | auto | Target session name. Required only when multiple sessions are running. |
| `--role` | `-r` | String | `general` | Role: `architect`, `planner`, `general`, or a custom name resolved against `.macot/templates/roles/{name}.md` then `~/.config/macot/roles/{name}.md`. |
| `--name` | `-n` | String | auto | Display name. Must match `^[A-Za-z][A-Za-z0-9_-]*$`, length 1–32, and be unique in the session. Auto-picked from the literary pool when omitted; falls back to `Expert{NN}` once the pool is exhausted. |
| `--prompt-file` | - | PathBuf | - | Reserved for a future custom-prompt path; currently rejected with a hint to use the templates directory instead. |
| `--worktree` | - | bool | `false` | Marker for the worktree-launch flow. Currently CLI emits a notice asking you to use `Ctrl+W` from the tower TUI; full CLI integration is tracked as Future Work. |
| `--worktree-branch` | - | String | - | Branch to use with `--worktree` once full CLI integration lands. |
| `--dry-run` | - | bool | `false` | Validate inputs and print the planned `expert_id`, name, role, prompt path, and template source. Writes nothing to disk and does not touch tmux. |
| `--json` | - | bool | `false` | Emit the result as JSON on stdout instead of the human-readable line. Compatible with `--dry-run` (emits the planned-state JSON). |

#### Examples

```bash
# Auto-pick a name from the literary pool
macot expert add -r general

# Pin a name (must be unique in the session)
macot expert add -r planner -n Smerdyakov

# Custom role from .macot/templates/roles/qa-bot.md
macot expert add -r qa-bot

# Plan-only — see what an add would do, write nothing
macot expert add -r general --dry-run

# Machine-readable output
macot expert add -r general --json
```

#### Output

Default (human):

```
Added expert 4 (Smerdyakov, general) in session macot-8c0dda46 (window 4)
```

`--json`:

```json
{"session":"macot-8c0dda46","expert_id":4,"name":"Smerdyakov","role":"general","tmux_window_index":4}
```

`--dry-run` (human):

```
DRY RUN — no state files written, no tmux operations performed.
  session         : macot-8c0dda46
  planned id      : 4
  planned name    : Smerdyakov
  template source : builtin:general
  prompt path     : /path/to/project/.macot/system_prompt/expert4.md
```

#### Behavior

1. Resolves the session (auto-selects when exactly one is running).
2. Resolves the role (built-in template, or the first match under `.macot/templates/roles/{name}.md` → `~/.config/macot/roles/{name}.md`).
3. Acquires the `.macot/.lock` advisory file lock (5 s timeout; returns "another macot operation in progress" otherwise).
4. Allocates the next `expert_id` as `max(existing_ids) + 1`, reconciling against any concurrent on-disk progress.
5. Writes `system_prompt/expert{N}.md`, `system_prompt/expert{N}_settings.json`, `status/expert{N}` (= `pending`), `sessions/{h}/experts/expert{N}/context.yaml`, and appends to `sessions/{h}/expert_roles.yaml`.
6. Atomically commits `experts_manifest.json` (temp-file + rename) — this is the commit point.
7. Releases the file lock and spawns the tmux window outside the lock so other processes are unblocked while tmux/Claude come up.
8. On any tmux failure the manifest entry, role assignment, and per-expert files are unwound; on success the tower TUI's manifest watcher detects the change and re-renders within ~100 ms.

### Subcommand: `expert list`

List the experts registered in a session by reading `experts_manifest.json` directly. Read-only — does not require the file lock.

#### Options

| Option | Short | Type | Description |
|--------|-------|------|-------------|
| `--session` | `-s` | String | Session name. Auto-selects the only running session when omitted; falls back to the manifest in the current directory if no session is running. |

#### Example

```bash
macot expert list
```

```
 ID  NAME              ROLE              WORKTREE
------------------------------------------------------------
  0  Alyosha           architect
  1  Ilyusha           planner
  2  Grigory           general
  3  Katya             general
  4  Smerdyakov        general
```

---

## Global Behavior

### Session Name Resolution

When `session_name` is optional and not provided:
- If exactly one macot session is running, it is selected automatically
- If no sessions are running, an error is displayed
- If multiple sessions are running, a list is shown and user must specify

### Error Messages

```bash
# No sessions running
No macot sessions running

# Multiple sessions without specification
Multiple sessions running. Please specify one:
  macot-a1b2c3d4 - /path/to/project
  macot-e5f6g7h8 - /path/to/another

# Session not found
Session macot-xyz does not exist
```

---

## Configuration

macot loads configuration from:
1. Custom path specified via `--config`
2. Default configuration with sensible defaults

See [Configuration Guide](./configuration.md) for details on configuring experts, timeouts, and paths.

---

## Related Commands

```bash
# View agents directly in tmux
tmux attach -t macot-a1b2c3d4

# List tmux sessions
tmux list-sessions
```
