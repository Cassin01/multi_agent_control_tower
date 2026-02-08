# Worktree Branch Launch Feature Design

## Overview

Allow launching an expert's Claude instance in a separate git worktree branch via `Ctrl+W` from the Expert selection screen. The expert's tmux pane closes Claude, creates a worktree, moves into it, and relaunches Claude with role initialization. Session data (`.macot/`) remains at the original project location via symlink.

## Requirements

1. `Ctrl+W` on Expert selection screen triggers worktree launch for the selected expert
2. Pane execution sequence: close Claude -> create worktree -> cd -> relaunch Claude -> init role
3. `.macot` session data stays at original location regardless of which branch an expert works in
4. Worktrees created at `.macot/worktrees/<branch>/`

## Architecture

### Current Data Flow

```
project_path/                       Config.with_project_path()
├── .macot/                         queue_path = project_path/.macot
│   ├── sessions/{hash}/            ContextStore base
│   │   ├── experts/expertN/        ExpertContext storage
│   │   ├── shared/                 SharedContext (decisions)
│   │   └── expert_roles.yaml       SessionExpertRoles
│   ├── reports/                    Expert reports (YAML)
│   ├── tasks/                      Task queue
│   └── messages/                   Inter-expert messaging
├── instructions/                   core_instructions_path
└── src/                            Source code
```

### Key Insight

- `queue_path` is derived from `project_path` in `Config::with_project_path()` (loader.rs:126)
- `session_hash` is SHA256(project_path) (loader.rs:162-176)
- `ContextStore` stores all data under `queue_path/sessions/{hash}/` (store.rs:17-22)
- The tower process always runs from the original project_path
- Only the expert's tmux pane changes working directory

**Session Isolation**: When launching an expert into a worktree, the existing `claude_session.session_id` must be cleared first. `launch_claude()` (claude.rs:30-38) adds `--resume <session_id>` whenever `ExpertContext.claude_session.session_id` is `Some`. A stale session ID would attempt to resume a session that was started in the original working directory, causing path confusion or outright failure in the new worktree. Calling `ExpertContext::clear_session()` before `launch_claude()` ensures the expert starts a fresh Claude session in the worktree.

### Problem

When an expert moves to a worktree at `.macot/worktrees/<branch>/`, relative paths like `.macot/reports/expert3_report.yaml` resolve against the worktree directory, not the original project. The tower still reads from the original `.macot/`.

### Solution: Symlink Strategy

```
project_path/
├── .macot/
│   ├── sessions/...                  # Unchanged, tower reads/writes here
│   ├── reports/...                   # Tower polls here
│   ├── worktrees/                    # NEW: worktree storage
│   │   └── feature-branch/           # git worktree checkout
│   │       ├── .macot -> /abs/path/to/project/.macot   # SYMLINK
│   │       ├── src/
│   │       ├── Cargo.toml
│   │       └── ...
│   └── ...
└── ...
```

When expert in worktree writes `.macot/reports/expert3_report.yaml`:
1. `.macot` symlink resolves to `/absolute/path/project/.macot`
2. File written to `/absolute/path/project/.macot/reports/expert3_report.yaml`
3. Tower reads from same absolute path -> transparent!

The recursive symlink (`.macot/worktrees/X/.macot -> .macot`) is safe because no process traverses the full recursive tree. `.macot` is `.gitignore`-d.

## Detailed Design

### 1. New File: `src/session/worktree.rs`

```rust
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

#[derive(Clone)]
pub struct WorktreeManager {
    project_path: PathBuf,
    macot_path: PathBuf,
}

impl WorktreeManager {
    pub fn new(project_path: PathBuf) -> Self {
        let macot_path = project_path.join(".macot");
        Self {
            project_path,
            macot_path,
        }
    }

    /// Directory containing all worktrees: .macot/worktrees/
    pub fn worktree_dir(&self) -> PathBuf {
        self.macot_path.join("worktrees")
    }

    /// Path for a specific worktree: .macot/worktrees/<branch>/
    pub fn worktree_path(&self, branch_name: &str) -> PathBuf {
        self.worktree_dir().join(branch_name)
    }

    /// Create a worktree at .macot/worktrees/<branch>.
    /// First tries attaching an existing branch, then falls back to creating a new one.
    pub async fn create_worktree(&self, branch_name: &str) -> Result<PathBuf> {
        let wt_path = self.worktree_path(branch_name);

        tokio::fs::create_dir_all(self.worktree_dir())
            .await
            .context("Failed to create worktrees directory")?;

        // Try using an existing branch first (no -b flag)
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                wt_path.to_str().unwrap(),
                branch_name,
            ])
            .current_dir(&self.project_path)
            .output()
            .await
            .context("Failed to run git worktree add")?;

        if output.status.success() {
            return Ok(wt_path);
        }

        // Existing branch not found or already checked out; try creating a new branch
        let output = Command::new("git")
            .args([
                "worktree",
                "add",
                wt_path.to_str().unwrap(),
                "-b",
                branch_name,
            ])
            .current_dir(&self.project_path)
            .output()
            .await
            .context("Failed to run git worktree add -b")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            anyhow::bail!("git worktree add failed: {}", stderr);
        }

        Ok(wt_path)
    }

    /// Create .macot symlink in worktree pointing to original .macot
    pub async fn setup_macot_symlink(&self, worktree_path: &Path) -> Result<()> {
        let symlink_path = worktree_path.join(".macot");
        let target = self.macot_path.canonicalize()
            .context("Failed to canonicalize .macot path")?;

        // Remove existing .macot if it exists (could be from a previous run)
        if symlink_path.exists() || symlink_path.is_symlink() {
            tokio::fs::remove_file(&symlink_path).await.ok();
            tokio::fs::remove_dir_all(&symlink_path).await.ok();
        }

        #[cfg(unix)]
        tokio::fs::symlink(&target, &symlink_path)
            .await
            .context("Failed to create .macot symlink")?;

        Ok(())
    }

    /// Check if a worktree already exists for the given branch
    pub fn worktree_exists(&self, branch_name: &str) -> bool {
        self.worktree_path(branch_name).exists()
    }

    /// Remove a worktree
    pub async fn remove_worktree(&self, branch_name: &str) -> Result<()> {
        let wt_path = self.worktree_path(branch_name);

        Command::new("git")
            .args(["worktree", "remove", wt_path.to_str().unwrap()])
            .current_dir(&self.project_path)
            .output()
            .await
            .context("Failed to remove git worktree")?;

        Ok(())
    }
}
```

#### Background Task Types

```rust
/// Result sent back from the spawned worktree-launch task.
pub struct WorktreeLaunchResult {
    pub expert_id: u32,
    pub expert_name: String,
    pub branch_name: String,
    pub worktree_path: String,
    pub claude_ready: bool,
}

/// Tracks whether a worktree launch is in progress.
/// Stored as a field on TowerApp so the main loop can poll it.
pub enum WorktreeLaunchState {
    Idle,
    InProgress {
        handle: tokio::task::JoinHandle<Result<WorktreeLaunchResult>>,
        expert_name: String,
        branch_name: String,
    },
}

impl Default for WorktreeLaunchState {
    fn default() -> Self {
        Self::Idle
    }
}
```

### 2. Modify: `src/session/mod.rs`

```rust
mod capture;
mod claude;
mod tmux;
mod worktree;  // NEW

pub use capture::{AgentStatus, CaptureManager, PaneCapture};
pub use claude::ClaudeManager;
pub use tmux::{SessionInfo, TmuxManager};
pub use worktree::WorktreeManager;  // NEW
```

### 3. Modify: `src/tower/app.rs`

#### Add field to `TowerApp`:
```rust
pub struct TowerApp {
    // ... existing fields ...
    worktree_manager: WorktreeManager,          // NEW
    worktree_launch_state: WorktreeLaunchState,  // NEW — background task tracking
}
```

#### Initialize in `TowerApp::new()`:
```rust
// In TowerApp::new():
let worktree_manager = WorktreeManager::new(config.project_path.clone());
// Add to Self { ... }
```

#### Add keybinding in `handle_events()`:
```rust
// After existing Ctrl+R binding (around line 539-544):
if key.code == KeyCode::Char('w')
    && key.modifiers.contains(KeyModifiers::CONTROL)
    && self.focus == FocusArea::TaskInput
{
    self.launch_expert_in_worktree().await?;
}
```

#### New method `launch_expert_in_worktree()`:

This method is called from `handle_events()` on `Ctrl+W`. It must **not** block the
TUI event loop, so all slow work (exit, sleep, create worktree, launch, wait, instruct)
is offloaded to a `tokio::spawn` task. The handler returns immediately after spawning.

```rust
pub async fn launch_expert_in_worktree(&mut self) -> Result<()> {
    // Concurrency guard — only one launch at a time
    if !matches!(self.worktree_launch_state, WorktreeLaunchState::Idle) {
        self.set_message("Worktree launch already in progress".to_string());
        return Ok(());
    }

    let expert_id = match self.status_display.selected_expert_id() {
        Some(id) => id,
        None => {
            self.set_message("No expert selected".to_string());
            return Ok(());
        }
    };

    let expert_name = self.config.get_expert_name(expert_id);
    let branch_name = format!(
        "expert-{}-{}",
        expert_id,
        chrono::Utc::now().format("%Y%m%d-%H%M%S")
    );

    if self.worktree_manager.worktree_exists(&branch_name) {
        self.set_message(format!("Worktree '{}' already exists", branch_name));
        return Ok(());
    }

    self.set_message(format!("Creating worktree '{}'...", branch_name));

    // Clone everything the spawned task needs (all types are Clone)
    let claude = self.claude.clone();
    let context_store = self.context_store.clone();
    let worktree_manager = self.worktree_manager.clone();
    let config = self.config.clone();
    let session_hash = config.session_hash();
    let instruction_role = self
        .session_roles
        .get_role(expert_id)
        .map(|s| s.to_string())
        .unwrap_or_else(|| config.get_expert_role(expert_id));
    let core_path = config.core_instructions_path.clone();
    let role_path = config.role_instructions_path.clone();
    let expert_name_clone = expert_name.clone();
    let branch_clone = branch_name.clone();
    let ready_timeout = config.timeouts.agent_ready;

    let handle = tokio::spawn(async move {
        // 1. Exit Claude in the pane
        claude.send_exit(expert_id).await?;
        tokio::time::sleep(Duration::from_secs(3)).await;

        // 2. Create worktree (tries existing branch first, then creates new)
        let worktree_path = worktree_manager.create_worktree(&branch_clone).await?;

        // 3. Setup .macot symlink
        worktree_manager.setup_macot_symlink(&worktree_path).await?;

        let wt_path_str = worktree_path.to_str().unwrap().to_string();

        // 4. Clear stale session to prevent --resume in the new worktree
        let mut expert_ctx = context_store
            .load_expert_context(&session_hash, expert_id)
            .await?
            .unwrap_or_else(|| {
                ExpertContext::new(expert_id, expert_name_clone.clone(), session_hash.clone())
            });
        expert_ctx.clear_session();
        expert_ctx.set_worktree(branch_clone.clone(), wt_path_str.clone());
        context_store.save_expert_context(&expert_ctx).await?;

        // 5. Launch Claude (reads context with no session_id → no --resume)
        claude
            .launch_claude(expert_id, &session_hash, &wt_path_str)
            .await?;

        // 6. Wait for Claude to be ready
        let ready = claude.wait_for_ready(expert_id, ready_timeout).await?;

        // 7. Send role instruction
        if ready {
            let instruction_result = load_instruction_with_template(
                &core_path,
                &role_path,
                &instruction_role,
                expert_id,
                &expert_name_clone,
            )?;
            if !instruction_result.content.is_empty() {
                claude
                    .send_instruction(expert_id, &instruction_result.content)
                    .await?;
            }
        }

        Ok(WorktreeLaunchResult {
            expert_id,
            expert_name: expert_name_clone,
            branch_name: branch_clone,
            worktree_path: wt_path_str,
            claude_ready: ready,
        })
    });

    self.worktree_launch_state = WorktreeLaunchState::InProgress {
        handle,
        expert_name,
        branch_name,
    };

    Ok(())
}
```

#### New method `poll_worktree_launch()`:

Called from the main `run()` loop alongside `poll_messages()`. Checks if the
background task has finished and transitions state back to `Idle`.

```rust
async fn poll_worktree_launch(&mut self) -> Result<()> {
    let state = std::mem::take(&mut self.worktree_launch_state);
    match state {
        WorktreeLaunchState::InProgress { handle, expert_name, branch_name } => {
            if handle.is_finished() {
                match handle.await {
                    Ok(Ok(result)) => {
                        let msg = if result.claude_ready {
                            format!("{} launched in worktree '{}'", result.expert_name, result.branch_name)
                        } else {
                            format!("Worktree '{}' created but Claude may still be starting", result.branch_name)
                        };
                        self.set_message(msg);
                    }
                    Ok(Err(e)) => {
                        self.set_message(format!("Worktree launch failed: {}", e));
                    }
                    Err(e) => {
                        self.set_message(format!("Worktree launch panicked: {}", e));
                    }
                }
                self.worktree_launch_state = WorktreeLaunchState::Idle;
            } else {
                // Still running — put state back
                self.worktree_launch_state = WorktreeLaunchState::InProgress {
                    handle,
                    expert_name,
                    branch_name,
                };
            }
        }
        WorktreeLaunchState::Idle => {
            self.worktree_launch_state = WorktreeLaunchState::Idle;
        }
    }
    Ok(())
}
```

#### Integration with `run()` loop:

Add `self.poll_worktree_launch().await?;` after `poll_messages()` in the main loop
(app.rs `run()` method):

```rust
self.poll_messages().await?;
self.poll_worktree_launch().await?;  // NEW
```

### 4. Modify: `src/context/expert.rs`

Add worktree tracking to `ExpertContext`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExpertContext {
    // ... existing fields ...
    #[serde(default)]
    pub worktree_branch: Option<String>,
    #[serde(default)]
    pub worktree_path: Option<String>,
}

impl ExpertContext {
    // ... existing methods ...

    /// Must be called together with clear_session() when switching an expert
    /// to a worktree, so that launch_claude() does not pass --resume with a
    /// stale session ID from the original working directory.
    pub fn set_worktree(&mut self, branch: String, path: String) {
        self.worktree_branch = Some(branch);
        self.worktree_path = Some(path);
        self.touch();
    }

    pub fn clear_worktree(&mut self) {
        self.worktree_branch = None;
        self.worktree_path = None;
        self.touch();
    }
}
```

### 5. Modify: `src/tower/widgets/help_modal.rs`

Add Ctrl+W to the help text:

```rust
// In build_help_lines(), under "Expert Operations" subsection:
Self::key_line("Ctrl+W", "Launch expert in worktree branch"),
```

## Execution Sequence Diagram

The handler returns immediately after spawning; the main loop polls for completion.

```
User presses Ctrl+W
        │
        ▼
  ┌──────────────────────────────┐
  │ Concurrency guard            │   if not Idle → return
  │ Get selected expert_id       │
  │ Generate branch name         │   "expert-3-20260207-120000"
  │ Clone shared state           │   (claude, context_store, worktree_manager, config)
  └──────────┬───────────────────┘
             │ tokio::spawn
             │ (returns immediately — UI stays responsive)
             │
    ┌────────┴────────────────────────────────────────────┐
    │  BACKGROUND TASK                                    │
    │                                                     │
    │  1. /exit to Claude       send_exit()               │
    │  2. sleep 3s                                        │
    │  3. git worktree add      create_worktree()         │
    │     (try existing branch, then -b new)              │
    │  4. ln -s .macot          setup_macot_symlink()     │
    │  5. clear_session()       prevent stale --resume    │
    │     set_worktree()        save_expert_context()     │
    │  6. cd <wt> && claude     launch_claude()           │
    │  7. wait_for_ready                                  │
    │  8. Send role instruction send_instruction()        │
    │                                                     │
    │  → WorktreeLaunchResult { expert_id, branch, ... }  │
    └────────┬────────────────────────────────────────────┘
             │
             ▼
  ┌──────────────────────────────┐
  │ poll_worktree_launch()       │   called each tick in run() loop
  │ handle.is_finished()?        │
  │  → yes: set success/error    │
  │         message, go Idle     │
  │  → no:  keep InProgress      │
  └──────────────────────────────┘
```

## Files Summary

| File | Action | Description |
|------|--------|-------------|
| `src/session/worktree.rs` | NEW | WorktreeManager (Clone), WorktreeLaunchResult, WorktreeLaunchState, create/symlink/remove |
| `src/session/mod.rs` | MODIFY | Export worktree module and new types |
| `src/tower/app.rs` | MODIFY | Ctrl+W keybinding, spawn-based launch_expert_in_worktree(), poll_worktree_launch(), run() loop integration |
| `src/context/expert.rs` | MODIFY | Add worktree_branch/worktree_path fields; clear_session() + set_worktree() used together |
| `src/tower/widgets/help_modal.rs` | MODIFY | Add Ctrl+W to help text |

## Edge Cases

1. **Branch already exists**: `create_worktree()` first tries `git worktree add <path> <branch>` (attach existing branch). If that fails (branch doesn't exist or is already checked out elsewhere), it falls back to `git worktree add <path> -b <branch>` (create new). If both fail, the error propagates.
2. **Expert is busy**: Check `AgentStatus` before sending `/exit`, warn user
3. **Symlink already exists**: Remove existing before creating new one
4. **Claude doesn't exit cleanly**: 3-second timeout after `/exit`, proceed anyway
5. **git worktree add fails**: Catch error in spawned task; `poll_worktree_launch()` displays the error message
6. **macOS vs Linux symlinks**: Use `#[cfg(unix)]` for symlink, both platforms supported
7. **Stale worktree state**: If a previous `git worktree add` partially succeeded (directory exists but git metadata is inconsistent), `git worktree remove` or manual cleanup may be needed. Consider calling `git worktree prune` before retrying.

## Testing Strategy

1. **Unit tests for WorktreeManager**: Mock filesystem, test path construction
2. **Unit test for branch name generation**: Deterministic format validation
3. **Integration test**: ExpertContext serialization with new worktree fields
4. **Property test**: Worktree path never collides for different expert+timestamp combos
5. **Concurrency guard**: Verify that a second `Ctrl+W` while `InProgress` shows a message and does not spawn another task
6. **Session clearing**: Verify that `clear_session()` is called before `launch_claude()`, so no `--resume` flag is passed in the worktree context
7. **Branch fallback**: Test `create_worktree()` with an existing branch name (first `git worktree add` succeeds) and a new branch name (falls back to `-b`)
8. **Poll completion**: Verify `poll_worktree_launch()` transitions from `InProgress` to `Idle` on success/failure and sets the correct status message
