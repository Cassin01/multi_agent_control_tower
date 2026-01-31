# macot CLI Reference

**macot** (Multi Agent Control Tower) orchestrates multiple Claude CLI instances for collaborative software development.

---

## Commands Overview

| Command | Description |
|---------|-------------|
| [`start`](#macot-start) | Initialize expert session with Claude agents |
| [`down`](#macot-down) | Gracefully shut down expert session |
| [`tower`](#macot-tower) | Launch the control tower TUI |
| [`status`](#macot-status) | Display current session status |
| [`sessions`](#macot-sessions) | List all running macot sessions |
| [`reset`](#macot-reset) | Reset expert context and instructions |

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
5. Launches Claude CLI in each pane
6. Waits for agents to become ready
7. Sends initial instructions from `instructions/core.md` and `instructions/<expert-name>.md`

### Output

```
Starting macot session for: /path/to/project
Creating session: macot-a1b2c3d4
Number of experts: 4
  [0] architect - Launching Claude...
  [1] backend - Launching Claude...
  [2] frontend - Launching Claude...
  [3] tester - Launching Claude...

Waiting for agents to be ready...
  [0] architect - Ready
  [1] backend - Ready
  [2] frontend - Ready
  [3] tester - Ready

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
| `Tab` | Switch focus between panels |
| `Enter` | Assign task to selected expert |
| `Ctrl+R` | Refresh status |
| `Ctrl+Q` / `q` | Quit |

### Interface

The TUI displays:
- Expert status panel (list of experts with current state)
- Task input panel (compose and assign tasks)
- Effort level selector

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
  [1] backend      ◐ - Thinking
  [2] frontend     ● - Executing
  [3] tester       ○ - Idle
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
