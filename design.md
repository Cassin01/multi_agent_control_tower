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
│                     tmux Session (expert)                        │
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
Claude CLI instances running in tmux panes:
- Each expert has a unique ID and name
- Experts read tasks from their designated queue file
- Experts write reports upon task completion
- Experts follow instructions from their instruction file

### Queue System
File-based task and report exchange:
- `queue/tasks/expert{ID}.yaml` - Task assignments per expert
- `queue/reports/expert{ID}_report.yaml` - Completion reports per expert

### tmux Session Manager
Manages the underlying terminal sessions:
- Session name: `expert`
- One pane per expert agent
- Color-coded prompts for visual distinction

---

## 3. Configuration

### Config File (`~/.config/macot/config.yaml`)

```yaml
# Number of expert agents to spawn
num_experts: 4

# Project path (defaults to current directory)
project_path: .

# Expert configuration
experts:
  - name: "architect"
    color: "red"
  - name: "frontend"
    color: "blue"
  - name: "backend"
    color: "green"
  - name: "tester"
    color: "yellow"

# Timeouts (in seconds)
timeouts:
  agent_ready: 30
  task_completion: 600

# tmux session name
session_name: "expert"
```

### Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MACOT_CONFIG` | Path to config file | `~/.config/macot/config.yaml` |
| `MACOT_PROJECT_PATH` | Override project path | Current directory |
| `MACOT_NUM_EXPERTS` | Override number of experts | From config |

---

## 4. Commands

### `macot start [options]`
Initialize the expert session with Claude agents.

**Options:**
- `-n, --num-experts <N>`: Number of experts (overrides config)
- `-p, --project <path>`: Project directory path
- `-c, --config <path>`: Custom config file path

**Behavior:**
1. Create tmux session with N panes
2. Set unique prompt colors per pane
3. Launch Claude CLI in each pane with `--dangerously-skip-permissions`
4. Wait for all agents to be ready
5. Send instruction prompts to each expert

### `macot stop`
Gracefully shut down the expert session.

**Behavior:**
1. Send exit commands to all Claude instances
2. Wait for graceful termination (timeout: 10s)
3. Force kill remaining processes
4. Destroy tmux session

### `macot tower`
Launch the control tower UI.

**Prerequisite:** Expert session must be running.

**UI Features:**
- Expert selection menu (numbered list)
- Task input field (multi-line supported)
- Status display per expert (idle/in_progress/done)
- Report viewer for completed tasks

### `macot status`
Display current session status without entering tower UI.

**Output:**
```
Session: expert (running)
Experts:
  [0] architect  - idle
  [1] frontend   - in_progress
  [2] backend    - done
  [3] tester     - idle
```

---

## 5. Directory Structure

### Project Layout
```
multi_agent_control_tower/
├── cmd/
│   └── macot/
│       └── main.go
├── internal/
│   ├── config/
│   │   └── config.go
│   ├── session/
│   │   └── tmux.go
│   ├── tower/
│   │   └── ui.go
│   └── queue/
│       └── queue.go
├── instructions/
│   ├── core.md
│   ├── architect.md
│   ├── frontend.md
│   ├── backend.md
│   └── tester.md
├── queue/
│   ├── tasks/
│   │   └── .gitkeep
│   └── reports/
│       └── .gitkeep
├── design.md
├── go.mod
└── Makefile
```

### Queue Directories
```
queue/
├── tasks/
│   ├── expert0.yaml
│   ├── expert1.yaml
│   └── ...
└── reports/
    ├── expert0_report.yaml
    ├── expert1_report.yaml
    └── ...
```

---

## 6. Workflows

### Session Initialization Flow
```
User runs `macot start`
         │
         ▼
┌─────────────────────────────┐
│ 1. Load configuration       │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 2. Create tmux session      │
│    with N panes             │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 3. Configure pane prompts   │
│    (colors, titles)         │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Launch Claude CLI        │
│    in each pane             │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 5. Wait for ready signal    │
│    (poll for prompt)        │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 6. Send instruction files   │
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
│ 3. Write task to            │
│    queue/tasks/expert{ID}   │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Send wakeup signal       │
│    via tmux send-keys       │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 5. Expert reads task file   │
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
│    queue/reports/expert{ID} │
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
User runs `macot stop`
         │
         ▼
┌─────────────────────────────┐
│ 1. Send /exit to all Claude │
│    instances                │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 2. Wait for graceful exit   │
│    (timeout: 10s)           │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 3. Kill remaining processes │
└─────────────────────────────┘
         │
         ▼
┌─────────────────────────────┐
│ 4. Destroy tmux session     │
└─────────────────────────────┘
         │
         ▼
      Session Terminated
```

---

## 7. File Formats

### Task YAML Schema (`queue/tasks/expert{ID}.yaml`)

```yaml
task_id: "task-2024-01-15-001"
expert_id: 0
expert_name: "architect"
status: "pending"  # pending | in_progress | done | failed
created_at: "2024-01-15T10:30:00Z"
description: |
  Review the authentication module and propose improvements
  for better security and maintainability.
context:
  files:
    - "internal/auth/handler.go"
    - "internal/auth/middleware.go"
  notes: "Focus on JWT token validation"
priority: "high"  # low | normal | high | critical
```

### Report YAML Schema (`queue/reports/expert{ID}_report.yaml`)

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
1. Read task from `queue/tasks/expert{ID}.yaml`
2. Update status to `in_progress`
3. Execute the assigned task
4. Write report to `queue/reports/expert{ID}_report.yaml`
5. Notify control tower upon completion
6. Update status to `done`

## Communication Protocol
- Do NOT communicate directly with other experts
- All coordination goes through the control tower
- Use the report file for all outputs

## Task File Location
Your task file: `queue/tasks/expert{ID}.yaml`
Your report file: `queue/reports/expert{ID}_report.yaml`
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
