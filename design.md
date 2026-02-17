# Multi Agent Control Tower (macot)

A CLI tool for orchestrating multiple Claude CLI instances working on the same codebase in parallel.

## 1. Overview

### Purpose
macot enables parallel code tasks by managing multiple Claude agents (experts) working on the same codebase simultaneously. Users manually assign tasks to specific experts through a central control tower.

### Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        Control Tower                             │
│                      (macot tower UI)                            │
│                                                                  │
│   ┌──────────────┐    ┌──────────────┐    ┌──────────────┐     │
│   │ Task Queue   │    │ Status       │    │ Report       │     │
│   │ Management   │    │ Monitor      │    │ Collector    │     │
│   └──────────────┘    └──────────────┘    └──────────────┘     │
└─────────────────────────────────────────────────────────────────┘
         │                    │                    ▲
         │ assign             │ monitor            │ report
         ▼                    ▼                    │
┌─────────────────────────────────────────────────────────────────┐
│                  tmux Session (macot-{hash})                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐        │
│  │ Expert 0 │  │ Expert 1 │  │ Expert 2 │  │ Expert N │        │
│  │ (Claude) │  │ (Claude) │  │ (Claude) │  │ (Claude) │        │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘        │
└─────────────────────────────────────────────────────────────────┘
         │              │              │              │
         └──────────────┴──────────────┴──────────────┘
                               │
                               ▼
                    ┌──────────────────┐
                    │    Codebase      │
                    └──────────────────┘
```

### Communication Model
- **Hub-spoke pattern**: All communication flows through the control tower
- **No direct agent-to-agent communication**: Prevents race conditions and conflicts
- **File-based queue system**: Tasks and reports are exchanged via YAML files

---

## 2. Components

### Control Tower (`macot tower`)
The central coordination UI that provides:
- Expert selection interface
- Task assignment input
- Status monitoring dashboard
- Report viewing

### Expert Agents
Claude CLI instances running in tmux windows:
- Each expert has a unique ID and name
- Experts receive tasks directly via the control tower prompt
- Experts write reports upon task completion
- Experts follow instructions from their instruction file

### Queue System
File-based report exchange:
- `.macot/reports/expert{ID}_report.yaml` - Completion reports per expert

### tmux Session Manager
Manages the underlying terminal sessions:
- Session naming: `macot-{hash}` where hash is first 8 chars of SHA256(absolute_project_path)
- One window per expert agent
- Color-coded prompts for visual distinction

**Session Environment Variables:**
Each tmux session stores metadata in environment variables:
```bash
MACOT_PROJECT_PATH    # Absolute path to the project directory
MACOT_NUM_EXPERTS     # Number of expert agents in this session
MACOT_CREATED_AT      # Session creation timestamp (ISO 8601)
```

**Session Discovery:**
```bash
# List all macot sessions
tmux list-sessions -F "#{session_name}" | grep "^macot-"

# Get project path for a session
tmux showenv -t {session_name} MACOT_PROJECT_PATH
```

---

## 3. Configuration

### Config File (`~/.config/macot/config.yaml`)

```yaml
# Number of expert agents to spawn
num_experts: 4

# Session prefix (used in session naming: {prefix}-{hash})
session_prefix: "macot"

# Expert configuration
experts:
  - name: "architect"
  - name: "planner"
  - name: "general"
  - name: "debugger"

# Timeouts (in seconds)
timeouts:
  agent_ready: 30
  task_completion: 600
```

### Session Naming Convention

Sessions are named using a hash of the absolute project path to enable multiple concurrent sessions:

```
Format: {session_prefix}-{hash}
Where:
  - session_prefix: Configurable prefix (default: "macot")
  - hash: First 8 characters of SHA256(absolute_project_path)

Example:
  project_path: /Users/user/myproject
  absolute_path: /Users/user/myproject (resolved)
  SHA256: a1b2c3d4e5f6...
  session_name: macot-a1b2c3d4
```

This ensures:
- Unique session per project directory
- Deterministic naming (same path → same session name)
- Multiple projects can run simultaneously

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MACOT_CONFIG` | Path to config file | `~/.config/macot/config.yaml` |

**Note:** `MACOT_PROJECT_PATH` and `MACOT_NUM_EXPERTS` are stored in tmux session environment (see tmux Session Manager section), not as shell environment variables.

---

## 4. Commands

### `macot start [project_path] [options]`
Initialize the expert session with Claude agents for a specific project.

**Arguments:**
- `project_path`: Path to project directory (default: current directory)

**Options:**
- `-n, --num-experts <N>`: Number of experts (overrides config)
- `-c, --config <path>`: Custom config file path

**Behavior:**
1. Resolve `project_path` to absolute path
2. Generate session name: `{prefix}-{SHA256(absolute_path)[:8]}`
3. Check if session already exists (error if running)
4. Create tmux session with N windows
5. Store metadata in tmux environment:
   ```bash
   tmux setenv -t {session} MACOT_PROJECT_PATH {absolute_path}
   tmux setenv -t {session} MACOT_NUM_EXPERTS {num_experts}
   tmux setenv -t {session} MACOT_CREATED_AT {timestamp}
   ```
6. Set unique prompt colors per window
7. Launch Claude CLI in each window with `--dangerously-skip-permissions`
8. Wait for all agents to be ready
9. Send instruction prompts to each expert

**Example:**
```bash
# Start session for current directory
macot start

# Start session for specific project
macot start /path/to/myproject

# Start with custom number of experts
macot start /path/to/myproject -n 6
```

### `macot sessions`
List all running macot sessions.

**Output format:**
```
SESSION           PROJECT PATH                         EXPERTS  CREATED
macot-a1b2c3d4    /Users/user/project1                 4        2024-01-15 10:30
macot-e5f6g7h8    /Users/user/project2                 3        2024-01-15 11:45
```

**Behavior:**
1. Find all tmux sessions matching `{prefix}-*`
2. For each session, retrieve environment variables
3. Display formatted table with session info

### `macot down [session_name]`
Gracefully shut down a specific expert session.

**Arguments:**
- `session_name`: Name of the session to stop (e.g., `macot-a1b2c3d4`)
  - If omitted and only one session exists, stops that session
  - If omitted and multiple sessions exist, shows error with list

**Behavior:**
1. Validate session exists
2. Send exit commands to all Claude instances
3. Wait for graceful termination (timeout: 10s)
4. Force kill remaining processes
5. Destroy tmux session

**Example:**
```bash
# Stop specific session
macot down macot-a1b2c3d4

# Stop only running session (if single session)
macot down
```

### `macot stop` (deprecated alias)
Alias for `macot down`. Kept for backward compatibility.

**Note:** Prefer using `macot down` for new scripts and documentation.

### `macot tower`
Launch the control tower UI.

**Prerequisite:** Expert session must be running.

**UI Features:**
- Expert selection menu (numbered list)
- Task input field (multi-line supported)
- Status display per expert (idle/busy/offline)
- Report viewer for completed tasks

### `macot status [session_name]`
Display current session status without entering tower UI.

**Arguments:**
- `session_name`: Name of the session to check (optional)
  - If omitted and only one session exists, shows that session
  - If omitted and multiple sessions exist, shows error with list

**Output:**
```
Session: macot-a1b2c3d4 (running)
Project: /Users/user/myproject
Experts:
  [0] architect  - idle
  [1] planner    - busy
  [2] general    - idle
  [3] debugger   - offline
```

---

## 5. Directory Structure

### Project Layout
```
multi_agent_control_tower/
├── src/
│   ├── main.rs
│   ├── lib.rs
│   ├── cli.rs
│   ├── utils.rs
│   ├── commands/
│   │   ├── start.rs
│   │   ├── down.rs
│   │   ├── tower.rs
│   │   ├── status.rs
│   │   ├── sessions.rs
│   │   └── reset.rs
│   ├── config/
│   │   └── loader.rs
│   ├── session/
│   │   ├── tmux.rs
│   │   ├── detector.rs
│   │   ├── claude.rs
│   │   └── worktree.rs
│   ├── tower/
│   │   ├── app.rs
│   │   └── ui.rs
│   ├── queue/
│   │   ├── manager.rs
│   │   └── router.rs
│   ├── models/
│   │   ├── expert.rs
│   │   ├── message.rs
│   │   ├── queued_message.rs
│   │   └── report.rs
│   ├── experts/
│   │   └── registry.rs
│   ├── instructions/
│   │   ├── template.rs
│   │   ├── defaults.rs
│   │   └── schema.rs
│   └── context/
│       ├── expert.rs
│       ├── role.rs
│       ├── shared.rs
│       └── store.rs
├── instructions/
│   ├── templates/
│   │   └── core.md.tmpl
│   ├── architect.md
│   ├── backend.md
│   ├── debugger.md
│   ├── frontend.md
│   ├── planner.md
│   └── general.md
├── doc/
├── .macot/
│   ├── reports/
│   └── status/
├── design.md
├── Cargo.toml
└── Makefile
```

### Queue Directories
```
.macot/
└── reports/
    ├── expert0_report.yaml
    ├── expert1_report.yaml
    └── ...
```

---

## 6. Workflows

### Session Initialization Flow
```
User runs `macot start [project_path]`
         │
         ▼
┌─────────────────────────────┐
│ 1. Load configuration       │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 2. Resolve project_path     │
│    to absolute path         │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 3. Generate session name    │
│    hash = SHA256(abs_path)  │
│    name = macot-{hash[:8]}  │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Check if session exists  │
│    (error if already running)│
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 5. Create tmux session      │
│    with N panes             │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 6. Store session metadata   │
│    in tmux environment      │
│    (PROJECT_PATH, NUM, TIME)│
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 7. Configure pane prompts   │
│    (colors, titles)         │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 8. Launch Claude CLI        │
│    in each pane             │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 9. Wait for ready signal    │
│    (poll for prompt)        │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│10. Send instruction files   │
│    to each expert           │
└─────────────────────────────┘
         │
         ▼
      Session Ready
```

### Task Assignment Flow
```
User in `macot tower`
         │
         ▼
┌─────────────────────────────┐
│ 1. Select target expert     │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 2. Input task description   │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 3. Send task description    │
│    via tmux send-keys       │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Expert receives task     │
│    and begins execution     │
└─────────────────────────────┘
```

### Report Collection Flow
```
Expert completes task
         │
         ▼
┌─────────────────────────────┐
│ 1. Expert writes report to  │
│    .macot/reports/expert{ID} │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 2. Expert notifies tower    │
│    (say command or status)  │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 3. Tower detects completion │
│    (poll or notification)   │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Tower displays report    │
│    in UI                    │
└─────────────────────────────┘
```

### Session Teardown Flow
```
User runs `macot down [session_name]`
         │
         ▼
┌─────────────────────────────┐
│ 1. Resolve target session   │
│    (from arg or single      │
│     running session)        │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 2. Validate session exists  │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 3. Send /exit to all Claude │
│    instances in session     │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Wait for graceful exit   │
│    (timeout: 10s)           │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 5. Kill remaining processes │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 6. Destroy tmux session     │
└─────────────────────────────┘
         │
         ▼
      Session Terminated
```

---

## 7. File Formats

### Report YAML Schema (`.macot/reports/expert{ID}_report.yaml`)

```yaml
task_id: "task-2024-01-15-001"
expert_id: 0
expert_name: "architect"
status: "done"
started_at: "2024-01-15T10:31:00Z"
completed_at: "2024-01-15T10:45:00Z"
summary: |
  Reviewed authentication module and identified 3 areas for improvement.
details:
  findings:
    - description: "JWT expiration not properly validated"
      severity: "high"
      file: "internal/auth/middleware.go"
      line: 45
    - description: "Missing rate limiting on login endpoint"
      severity: "medium"
      file: "internal/auth/handler.go"
      line: 23
  recommendations:
    - "Add token expiration check before parsing claims"
    - "Implement rate limiting middleware"
  files_modified: []
  files_created: []
errors: []
```

### Instruction Markdown Format (`instructions/{expert_name}.md`)

```markdown
# Expert Instructions: {expert_name}

## Role
You are the {role_description} expert in a multi-agent development team.

## Responsibilities
- {responsibility_1}
- {responsibility_2}

## Workflow
1. Accept task from control tower prompt
2. Acknowledge task receipt
3. Execute the assigned task
4. Write report to `.macot/reports/expert{ID}_report.yaml`
5. Notify control tower upon completion
6. Update status to `done`

## Communication Protocol
- Do NOT communicate directly with other experts
- All coordination goes through the control tower
- Use the report file for all outputs

## Report File Location
Your report file: `.macot/reports/expert{ID}_report.yaml`
```

---

## 8. Error Handling

### Agent Failure Recovery
- **Detection**: Control tower polls agent status every 5s
- **Timeout**: If agent unresponsive for 60s, mark as failed
- **Recovery**:
  1. Log failure details
  2. Attempt graceful restart of specific pane
  3. Re-send instruction file
  4. Reassign pending task if any

### Session Recovery
- **Unexpected tmux session death**:
  1. Detect via `tmux has-session`
  2. Prompt user to run `macot start` again
- **Partial pane failure**:
  1. Identify failed pane(s)
  2. Recreate only failed panes
  3. Preserve working agents

### Timeout Handling
| Operation | Timeout | Action on Timeout |
|-----------|---------|-------------------|
| Agent ready | 30s | Retry 3 times, then fail |
| Task execution | 600s | Mark task as failed, notify user |
| Graceful shutdown | 10s | Force kill |
| Tower connection | 5s | Show error, suggest `macot start` |

### Error Codes
| Code | Description |
|------|-------------|
| 1 | Configuration error |
| 2 | tmux session not found |
| 3 | Agent initialization failed |
| 4 | Task assignment failed |
| 5 | Report collection failed |
| 10 | Unknown error |
