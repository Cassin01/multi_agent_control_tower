# Implementation Plan: Worktree Branch Launch

## Overview

This plan decomposes the Worktree Branch Launch feature into incremental tasks following TDD. The build order is: data model extensions first (ExpertContext), then core logic (WorktreeManager), then TUI integration (app.rs keybinding + background task), and finally help text. Each implementation task is paired with tests, and checkpoints validate correctness at phase boundaries.

## Tasks

- [x] 1. Add worktree fields to `ExpertContext`
  - Add `worktree_branch: Option<String>` and `worktree_path: Option<String>` fields with `#[serde(default)]` to `ExpertContext` in `src/context/expert.rs`
  - Add `set_worktree(branch, path)` and `clear_worktree()` methods
  - Remove `#[allow(dead_code)]` from `clear_session()` since it will be called by the worktree launch flow
  - _Requirements: 2, 3_

- [x] 1.1 Write tests for ExpertContext worktree fields
  - **Property: Serialization Round-trip with Worktree Fields**
    - Test that `set_worktree()` stores branch and path correctly
    - Test that `clear_worktree()` resets both fields to `None`
    - Test YAML serialization/deserialization with worktree fields present
    - Test backward-compatible deserialization (YAML without worktree fields deserializes to `None`)
    - Test that `set_worktree()` calls `touch()` (updates `updated_at`)
  - **Validates: Requirements 2, 3**

- [x] 2. Checkpoint - ExpertContext changes compile and tests pass
  - Run `make test` to confirm all existing tests still pass with the new fields
  - Verify backward compatibility with existing serialized data

- [x] 3. Create `WorktreeManager` in `src/session/worktree.rs`
  - Create new file `src/session/worktree.rs`
  - Implement `WorktreeManager` struct with `project_path` and `macot_path` fields, deriving `Clone`
  - Implement `new(project_path)`, `worktree_dir()`, `worktree_path(branch_name)`, `worktree_exists(branch_name)`
  - Implement `create_worktree(branch_name)` — tries existing branch first, falls back to `-b`
  - Implement `setup_macot_symlink(worktree_path)` — creates `.macot` symlink with `#[cfg(unix)]`
  - Implement `remove_worktree(branch_name)`
  - _Requirements: 2, 3, 4_

- [x] 3.1 Write unit tests for `WorktreeManager` path construction
  - **Property: Path Construction Correctness**
    - Test `worktree_dir()` returns `<project_path>/.macot/worktrees/`
    - Test `worktree_path(branch)` returns `<project_path>/.macot/worktrees/<branch>/`
    - Test `worktree_exists()` returns false for nonexistent paths
  - **Validates: Requirements 4**

- [x] 3.2 Write property tests for branch name uniqueness
  - **Property: Worktree Path Never Collides**
    - Generate arbitrary expert names and timestamps; verify `worktree_path()` produces unique paths for each combination
  - **Validates: Requirements 4**

- [x] 4. Export worktree module from `src/session/mod.rs`
  - Add `mod worktree;` declaration
  - Add `pub use worktree::{WorktreeManager, WorktreeLaunchResult, WorktreeLaunchState};`
  - _Requirements: 2_

- [x] 5. Checkpoint - WorktreeManager compiles and all tests pass
  - Run `make test` to confirm the new module integrates correctly
  - Verify path construction tests pass

- [x] 6. Define background task types in `src/session/worktree.rs`
  - Add `WorktreeLaunchResult` struct with fields: `expert_id`, `expert_name`, `branch_name`, `worktree_path`, `claude_ready`
  - Add `WorktreeLaunchState` enum: `Idle`, `InProgress { handle, expert_name, branch_name }`
  - Implement `Default` for `WorktreeLaunchState` returning `Idle`
  - _Requirements: 2_

- [x] 6.1 Write tests for `WorktreeLaunchState`
  - **Property: Default State is Idle**
    - Test `WorktreeLaunchState::default()` matches `Idle`
  - **Validates: Requirements 2**

- [x] 7. Add `worktree_manager` and `worktree_launch_state` to `TowerApp`
  - Add `worktree_manager: WorktreeManager` field to `TowerApp` struct in `src/tower/app.rs`
  - Add `worktree_launch_state: WorktreeLaunchState` field
  - Initialize `WorktreeManager::new(config.project_path.clone())` in `TowerApp::new()`
  - Initialize `worktree_launch_state: WorktreeLaunchState::Idle`
  - Update imports in `src/tower/app.rs` to include `WorktreeManager` and `WorktreeLaunchState`
  - _Requirements: 2_

- [x] 8. Implement `launch_expert_in_worktree()` method on `TowerApp`
  - Add concurrency guard: return early if not `Idle`
  - Get selected expert ID, generate branch name with format `expert-<name>-<YYYYMMDD-HHMMSS>`
  - Check `worktree_exists()`, return early if already exists
  - Clone all needed shared state (`claude`, `context_store`, `worktree_manager`, `config`, etc.)
  - Spawn `tokio::spawn` background task that performs the 8-step sequence:
    1. `send_exit()` to close Claude
    2. `sleep(3s)` for Claude to exit
    3. `create_worktree()` for git worktree
    4. `setup_macot_symlink()` for `.macot` symlink
    5. `clear_session()` + `set_worktree()` + `save_expert_context()` for context cleanup
    6. `launch_claude()` with worktree path
    7. `wait_for_ready()` with timeout
    8. `send_instruction()` with role instructions
  - Set `worktree_launch_state` to `InProgress` with the `JoinHandle`
  - _Requirements: 1, 2, 3_

- [x] 8.1 Write tests for concurrency guard and branch name generation
  - **Property: Concurrency Guard**
    - Verify that calling `launch_expert_in_worktree()` when state is `InProgress` returns early with message
  - **Property: Branch Name Format**
    - Verify branch name follows `expert-<name>-<YYYYMMDD-HHMMSS>` pattern
  - **Validates: Requirements 1, 2**

- [x] 9. Implement `poll_worktree_launch()` method on `TowerApp`
  - Use `std::mem::take()` to extract state
  - If `InProgress` and `handle.is_finished()`: await the result, set success/error message, transition to `Idle`
  - If `InProgress` and not finished: put state back
  - If `Idle`: stay `Idle`
  - _Requirements: 2_

- [x] 9.1 Write tests for `poll_worktree_launch()` state transitions
  - **Property: Poll Completion Transitions to Idle**
    - Verify successful completion sets correct message and transitions to `Idle`
    - Verify failure sets error message and transitions to `Idle`
  - **Validates: Requirements 2**

- [x] 10. Add `Ctrl+W` keybinding in `handle_events()`
  - Add key match for `KeyCode::Char('w')` with `KeyModifiers::CONTROL` when `focus == FocusArea::TaskInput`
  - Call `self.launch_expert_in_worktree().await?`
  - _Requirements: 1_

- [x] 11. Integrate `poll_worktree_launch()` into `run()` loop
  - Add `self.poll_worktree_launch().await?;` after `self.poll_messages().await?;` in the main loop
  - _Requirements: 2_

- [x] 12. Checkpoint - Full worktree launch integration compiles and tests pass
  - Run `make test` to confirm all tests pass
  - Verify the TUI event loop integration compiles correctly

- [x] 13. Add `Ctrl+W` to help modal
  - Add `Self::key_line("Ctrl+W", "Launch expert in worktree branch")` in `build_help_lines()` under "Expert Operations" subsection in `src/tower/widgets/help_modal.rs`
  - _Requirements: 1_

- [x] 13.1 Write test for help text content
  - **Property: Help Text Includes Worktree Shortcut**
    - Verify `build_help_lines()` output contains "Ctrl+W" text
  - **Validates: Requirements 1**

- [x] 14. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make test` to confirm all tests pass
  - Verify `make` (build) completes without warnings
  - Review edge cases from the design document are handled:
    - Branch already exists fallback
    - Symlink already exists cleanup
    - Concurrency guard for multiple launches
    - Session clearing before launch
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- The background task pattern (`tokio::spawn` + poll) keeps the TUI responsive during the multi-second worktree creation and Claude relaunch sequence.
- `clear_session()` is critical before `launch_claude()` in the worktree context to prevent stale `--resume` flags.
- The symlink strategy allows `.macot` paths to resolve correctly from within the worktree without changing any existing path logic in the tower.
- All async operations in the spawned task use cloned values (`Clone` on `ClaudeManager`, `ContextStore`, `WorktreeManager`, `Config`).
- The `#[cfg(unix)]` guard on symlink creation means Windows is not supported for this feature.
- Property tests for branch name uniqueness use proptest, which is already a dependency in this project.
