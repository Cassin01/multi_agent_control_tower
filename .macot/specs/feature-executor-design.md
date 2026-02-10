# Feature Executor — Design Document

## 1. Overview

The Feature Executor automates sequential task execution from a spec task file (`.macot/specs/{feature}-tasks.md`). It assigns batches of tasks to a selected expert, polls for completion, and repeats until all tasks are done — with periodic `/compact` to manage Claude's context window.

### Core Loop

```
User triggers → Validate spec files → Send task batch → Wait N sec → Poll status
→ Expert idle? → Re-read task file → More tasks? → Send next batch → ...
→ Every 10 tasks: /compact → Wait 4 min → Resume
→ All tasks done → Exit execution mode
```

## 2. Requirements

### Functional Requirements

| ID   | Requirement |
|------|-------------|
| F1   | User inputs a feature name (from TaskInput content) |
| F2   | Validate `.macot/specs/{feature}-tasks.md` exists; error if missing |
| F3   | `.macot/specs/{feature}-design.md` is optional; omit from prompt if absent |
| F4   | Parse task file: extract task numbers, descriptions, and completion status (`- [ ]` / `- [x]`) |
| F5   | Assign next batch of uncompleted tasks to the currently selected expert |
| F6   | After sending, wait `poll_delay` seconds before polling expert status |
| F7   | When expert status becomes `pending` (idle), re-read task file and send next batch |
| F8   | Every `compact_interval` tasks, send `/compact` to the expert and wait `compact_wait` seconds |
| F9   | Show execution badge/icon in Expert List panel title while running |
| F10  | Configurable: `batch_size` (default 4), `poll_delay` (default 30s), `compact_interval` (default 10), `compact_wait` (default 240s) |
| F11  | Allow cancellation of running execution (keybinding) |
| F12  | Show progress (tasks completed / total) in status bar message |

### Non-Functional Requirements

| ID   | Requirement |
|------|-------------|
| NF1  | Non-blocking: execution loop runs within the existing async main loop via timer-based state transitions |
| NF2  | TUI remains fully responsive during feature execution |
| NF3  | Task file is re-read from disk each cycle because the expert modifies it |

## 3. Architecture

### 3.1 New Module: `src/feature/mod.rs`

```
src/feature/
├── mod.rs           // Module exports
├── executor.rs      // FeatureExecutor state machine
└── task_parser.rs   // Task file parser
```

### 3.2 State Machine: `FeatureExecutionState`

```
                  ┌──────────────────────────────────────────┐
                  │                                          │
                  ▼                                          │
Idle ──trigger──▶ Validating ──ok──▶ SendingBatch            │
                      │                  │                   │
                    error              send                  │
                      │                  │                   │
                      ▼                  ▼                   │
                   (toast)         WaitingPollDelay           │
                                       │                   │
                                    N sec                  │
                                       │                   │
                                       ▼                   │
                                  PollingStatus             │
                                       │                   │
                              expert_status==pending        │
                                       │                   │
                                       ▼                   │
                              CheckingCompletion            │
                                   │        │              │
                             more tasks   all done          │
                                   │        │              │
                                   ▼        ▼              │
                            NeedCompact?  Completed ───────┘
                              │      │        │
                            yes      no       │
                              │      │        │
                              ▼      │        │
                        SendCompact  │        │
                              │      │        │
                          4 min wait │        │
                              │      │        │
                              ▼      ▼        │
                           SendingBatch ◄─────┘
```

States:

```rust
pub enum ExecutionPhase {
    Idle,
    SendingBatch,
    WaitingPollDelay {
        started_at: Instant,
    },
    PollingStatus,
    WaitingCompact {
        started_at: Instant,
    },
    Completed,
    Failed(String),
}
```

### 3.3 `FeatureExecutor` Struct

```rust
pub struct FeatureExecutor {
    // Configuration
    feature_name: String,
    expert_id: u32,
    batch_size: usize,           // default: 4
    poll_delay: Duration,        // default: 30s
    compact_interval: usize,     // default: 10
    compact_wait: Duration,      // default: 240s

    // State
    phase: ExecutionPhase,
    tasks_sent_count: usize,     // total tasks sent (for compact interval tracking)
    tasks_sent_since_compact: usize,  // tasks sent since last compact
    current_batch: Vec<TaskId>,  // task numbers in current batch

    // File paths
    tasks_file: PathBuf,         // .macot/specs/{feature}-tasks.md
    design_file: Option<PathBuf>, // .macot/specs/{feature}-design.md (if exists)

    // Progress tracking
    total_tasks: usize,
    completed_tasks: usize,
}
```

### 3.4 Task File Parser

The parser reads `.macot/specs/{feature}-tasks.md` and extracts tasks.

**Task format** (from `instructions/planner.md`):

```markdown
- [ ] 1. Main task title
  - Description
  - _Requirements: X.Y_

  - [ ] 1.1 Sub-task title
    - Description

- [x] 2. Completed task
  - Description
```

**Parsed structure**:

```rust
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub number: String,      // "1", "1.1", "2", "2.1", etc.
    pub title: String,       // Task title text
    pub completed: bool,     // true if [x], false if [ ]
    pub indent_level: usize, // 0 for top-level, 1 for sub-tasks
}
```

**Parser rules**:
1. Match lines starting with `- [ ]` or `- [x]` followed by a task number (integer or dot-notation)
2. Regex: `^(\s*)- \[([ x])\] (\d+(?:\.\d+)?)\.\s+(.+)$`
3. Indent level determined by leading whitespace (0 or 2+ spaces)
4. Non-matching lines (descriptions, requirements) are ignored by the parser

### 3.5 Prompt Template

When design file exists:

```
Below are the design specifications and task list for {feature}.

@.macot/specs/{feature}-design.md
@.macot/specs/{feature}-tasks.md

Implement the tasks in order.
Execute Tasks {task_numbers}. After completing each task, Mark them as finished in the task file.
```

When design file is absent:

```
Below is the task list for {feature}.

@.macot/specs/{feature}-tasks.md

Implement the tasks in order.
Execute Tasks {task_numbers}. After completing each task, Mark them as finished in the task file.
```

Where `{task_numbers}` is a comma-separated list like `15, 16, 17, 18`.

### 3.6 Batch Calculation

1. Parse task file → get all `TaskEntry` items
2. Filter to `completed == false`
3. Take first `batch_size` uncompleted tasks
4. Extract their task numbers for the prompt

Example: If tasks 1-14 are `[x]` and 15-22 are `[ ]`, with `batch_size=4`:
- First batch: `15, 16, 17, 18`
- After those complete: `19, 20, 21, 22`

### 3.7 Compact Logic

Tracked by `tasks_sent_since_compact` counter:
1. After each batch is sent, add batch size to counter
2. Before sending next batch, check: `tasks_sent_since_compact >= compact_interval`
3. If threshold reached:
   - Send `/compact` to expert via tmux
   - Enter `WaitingCompact` state (4 min timer)
   - Reset `tasks_sent_since_compact = 0`
4. After compact wait expires, transition to `SendingBatch`

## 4. Integration with TowerApp

### 4.1 TowerApp Changes

```rust
// New field in TowerApp struct
feature_executor: Option<FeatureExecutor>,
```

### 4.2 Keybinding

`Ctrl+G` (when focused on TaskInput) — triggers feature execution.

Flow:
1. Read feature name from `task_input.content()`
2. Get selected expert ID from `status_display.selected_expert_id()`
3. Create `FeatureExecutor::new(feature_name, expert_id, config)`
4. Call `executor.validate()` → check files exist
5. If valid, set `self.feature_executor = Some(executor)`
6. Clear task input
7. Show toast: "Feature execution started: {feature}"

Cancel: `Ctrl+G` again while executing → cancels and returns to `Idle`.

### 4.3 Main Loop Integration

Add `poll_feature_executor()` to the main loop in `run()`:

```rust
// In the main loop, after existing polls:
self.poll_feature_executor().await?;
```

The poll method drives the state machine:

```rust
async fn poll_feature_executor(&mut self) -> Result<()> {
    let executor = match &self.feature_executor {
        Some(e) => e,
        None => return Ok(()),
    };

    match executor.phase() {
        ExecutionPhase::Idle => {}

        ExecutionPhase::SendingBatch => {
            // Parse task file, calculate batch
            let tasks = executor.parse_tasks()?;
            let batch = executor.next_batch(&tasks);

            if batch.is_empty() {
                // All tasks complete
                executor.set_phase(ExecutionPhase::Completed);
            } else {
                // Check compact needed
                if executor.needs_compact() {
                    // Send /compact
                    self.claude.send_keys_with_enter(expert_id, "/compact").await?;
                    executor.reset_compact_counter();
                    executor.set_phase(ExecutionPhase::WaitingCompact {
                        started_at: Instant::now(),
                    });
                } else {
                    // Generate and send prompt
                    let prompt = executor.build_prompt(&batch);
                    self.claude.send_keys_with_enter(expert_id, &prompt).await?;
                    executor.record_batch_sent(batch.len());
                    executor.set_phase(ExecutionPhase::WaitingPollDelay {
                        started_at: Instant::now(),
                    });
                }
            }
        }

        ExecutionPhase::WaitingPollDelay { started_at } => {
            if started_at.elapsed() >= executor.poll_delay() {
                executor.set_phase(ExecutionPhase::PollingStatus);
            }
        }

        ExecutionPhase::PollingStatus => {
            // Check expert status
            let state = self.detector.detect(expert_id).await?;
            if state == ExpertState::Idle {
                // Expert finished, check task completion
                executor.set_phase(ExecutionPhase::SendingBatch);
            }
            // else: still busy, continue polling on next tick
        }

        ExecutionPhase::WaitingCompact { started_at } => {
            if started_at.elapsed() >= executor.compact_wait() {
                executor.set_phase(ExecutionPhase::SendingBatch);
            }
        }

        ExecutionPhase::Completed => {
            self.set_message(format!(
                "Feature '{}' execution completed ({}/{} tasks)",
                executor.feature_name(),
                executor.completed_tasks(),
                executor.total_tasks()
            ));
            self.feature_executor = None;
        }

        ExecutionPhase::Failed(msg) => {
            self.set_message(format!("Feature execution failed: {}", msg));
            self.feature_executor = None;
        }
    }

    Ok(())
}
```

### 4.4 Status Display Badge

When `self.feature_executor.is_some()`, modify the Expert List panel title:

```
"Experts [▶ {feature}]"          // while running
"Experts [⏸ compact...]"         // during compact wait
"Experts"                         // normal (no execution)
```

Implementation: In `StatusDisplay::render()`, accept an optional `execution_badge: Option<String>` parameter and append it to the block title.

### 4.5 Progress in Status Bar

While executing, show progress in the toast/status area:

```
"▶ {feature}: {completed}/{total} tasks | Batch: {current_batch_numbers}"
```

During compact:
```
"⏸ {feature}: compact ({remaining}s) | {completed}/{total} tasks"
```

## 5. Configuration

Add to `Config`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureExecutionConfig {
    #[serde(default = "default_batch_size")]
    pub batch_size: usize,           // default: 4

    #[serde(default = "default_poll_delay")]
    pub poll_delay_secs: u64,        // default: 30

    #[serde(default = "default_compact_interval")]
    pub compact_interval: usize,     // default: 10

    #[serde(default = "default_compact_wait")]
    pub compact_wait_secs: u64,      // default: 240
}
```

In `config.yaml`:

```yaml
feature_execution:
  batch_size: 4
  poll_delay_secs: 30
  compact_interval: 10
  compact_wait_secs: 240
```

## 6. Correctness Properties

| ID   | Property | Description |
|------|----------|-------------|
| P1   | File Validation | Feature execution starts only if tasks.md exists |
| P2   | Batch Correctness | Each batch contains exactly min(batch_size, remaining) uncompleted tasks |
| P3   | No Duplicate Assignment | A task number is never sent twice across batches (re-read from disk each cycle) |
| P4   | Compact Interval | `/compact` is sent after exactly `compact_interval` tasks, not sooner or later |
| P5   | Compact Wait | No task is sent during the compact wait period |
| P6   | Status Polling Delay | Status polling does not begin until `poll_delay` seconds after task submission |
| P7   | Cancellation Safety | Cancellation stops execution immediately without sending partial batches |
| P8   | Non-blocking | TUI event loop never blocks; all waits are timer-based |
| P9   | Task Re-read | Task file is re-read from disk at each cycle, not cached |
| P10  | Progress Accuracy | Displayed progress matches actual task file state |
| P11  | Design File Optional | Prompt correctly omits design file reference when file doesn't exist |

## 7. Error Handling

| Scenario | Behavior |
|----------|----------|
| tasks.md not found | Toast error, no execution started |
| Task file parse error | Toast error with details, no execution started |
| Expert goes offline during execution | Pause execution, show warning, resume when expert returns |
| tmux send_keys failure | Retry once, then transition to Failed state |
| Task file unreadable mid-execution | Transition to Failed state with error message |
| No uncompleted tasks in file | Immediately transition to Completed |

## 8. Files Affected

| File | Change |
|------|--------|
| `src/feature/mod.rs` | **NEW** — Module declaration |
| `src/feature/executor.rs` | **NEW** — FeatureExecutor state machine |
| `src/feature/task_parser.rs` | **NEW** — Task file parser |
| `src/lib.rs` | Add `mod feature;` |
| `src/config/loader.rs` | Add `FeatureExecutionConfig` |
| `src/tower/app.rs` | Add `feature_executor` field, `poll_feature_executor()`, `Ctrl+G` handler |
| `src/tower/ui.rs` | Pass execution badge to StatusDisplay |
| `src/tower/widgets/status_display.rs` | Accept and render execution badge in panel title |
| `src/tower/widgets/help_modal.rs` | Add `Ctrl+G` keybinding hint |

## 9. Sequence Diagram

```
User          TowerApp         FeatureExecutor     TaskParser      Claude(tmux)     StatusFile
 │               │                   │                 │                │                │
 │──Ctrl+G──────▶│                   │                 │                │                │
 │               │──new(feature)────▶│                 │                │                │
 │               │                   │──validate()────▶│                │                │
 │               │                   │◀──ok────────────│                │                │
 │               │                   │──parse_tasks()─▶│                │                │
 │               │                   │◀──tasks[]───────│                │                │
 │               │                   │──next_batch()──▶│                │                │
 │               │                   │◀──[15,16,17,18]─│                │                │
 │               │◀──prompt──────────│                 │                │                │
 │               │──send_keys───────────────────────────────────────▶│                │
 │               │                   │                 │                │                │
 │               │     ... N seconds pass (WaitingPollDelay) ...       │                │
 │               │                   │                 │                │                │
 │               │──detect()──────────────────────────────────────────────────────────▶│
 │               │◀──"processing"────────────────────────────────────────────────────── │
 │               │                   │                 │                │                │
 │               │     ... keep polling ...             │                │                │
 │               │                   │                 │                │                │
 │               │──detect()──────────────────────────────────────────────────────────▶│
 │               │◀──"pending"───────────────────────────────────────────────────────── │
 │               │                   │                 │                │                │
 │               │                   │──parse_tasks()─▶│                │                │
 │               │                   │◀──tasks[]───────│                │                │
 │               │                   │──next_batch()──▶│                │                │
 │               │                   │◀──[19,20,21,22]─│                │                │
 │               │◀──prompt──────────│                 │                │                │
 │               │──send_keys───────────────────────────────────────▶│                │
 │               │                   │                 │                │                │
 │               │     ... cycle repeats ...            │                │                │
```

## 10. Open Questions / Design Decisions

### Q1: Sub-task handling in batch counting

When counting batch size, should sub-tasks (1.1, 1.2) count individually?

**Decision**: Yes. Each checkbox line (`- [ ]`) is one task unit regardless of nesting. A batch of 4 means 4 checkbox items.

### Q2: Polling interval during PollingStatus phase

How frequently should the executor check expert status after the initial poll delay?

**Decision**: Reuse existing status poll interval (2 seconds). The `poll_feature_executor()` runs every main loop tick, and expert state is already refreshed by `poll_status()` at 2s intervals. So `PollingStatus` simply reads the cached state.

### Q3: What if expert is already busy when execution starts?

**Decision**: Wait. Enter `WaitingPollDelay` state immediately and begin polling. The first batch is only sent when the expert is idle.

Actually — revised decision: Show a warning toast and require the expert to be idle. If busy, refuse to start execution.
