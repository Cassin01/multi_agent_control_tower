# Implementation Plan: Feature Executor

## Overview

This plan decomposes the Feature Executor into incremental tasks that build bottom-up: task parser first, then the executor state machine, then configuration, and finally TUI integration (keybinding, main loop, badge, progress). Each implementation task is paired with tests. Checkpoints validate accumulated work before proceeding to the next phase.

## Tasks

- [x] 1. Create `src/feature/mod.rs` module declaration
  - Create `src/feature/` directory
  - Add `mod.rs` with `pub mod executor;` and `pub mod task_parser;`
  - Add `pub mod feature;` to `src/lib.rs`
  - _Requirements: F4, NF1_

- [x] 2. Implement `TaskEntry` struct and task file parser in `src/feature/task_parser.rs`
  - Define `TaskEntry` struct with fields: `number: String`, `title: String`, `completed: bool`, `indent_level: usize`
  - Implement `parse_tasks(content: &str) -> Vec<TaskEntry>` function
  - Regex: `^(\s*)- \[([ x])\] (\d+(?:\.\d+)?)\.\s+(.+)$`
  - Indent level: 0 for no leading whitespace, 1+ based on leading spaces
  - Ignore non-matching lines (descriptions, requirements, headings)
  - _Requirements: F4_

- [x] 2.1. Write tests for task file parser
  - **Property P2: Batch Correctness** (parser foundation)
  - Test parsing `- [ ]` lines as incomplete tasks
  - Test parsing `- [x]` lines as completed tasks
  - Test parsing sub-tasks with dot notation (e.g., `1.1`, `2.3`)
  - Test indent level detection (top-level vs sub-task)
  - Test ignoring non-task lines (descriptions, headings, blank lines)
  - Test mixed completed/incomplete task file
  - Test empty file returns empty vec
  - **Validates: Requirements F4**

- [x] 3. Checkpoint - Parser correctness
  - Run `make test` to ensure all parser tests pass.

- [x] 4. Add `FeatureExecutionConfig` to `src/config/loader.rs`
  - Define `FeatureExecutionConfig` struct with: `batch_size: usize` (default 4), `poll_delay_secs: u64` (default 30), `exit_wait_secs: u64` (default 3), `ready_timeout_secs: u64` (default 60)
  - Add `#[serde(default)]` `feature_execution: FeatureExecutionConfig` field to `Config`
  - Implement `Default` for `FeatureExecutionConfig` with specified defaults
  - _Requirements: F10_

- [x] 4.1. Write tests for `FeatureExecutionConfig` defaults
  - **Property P2: Batch Correctness** (config foundation)
  - Test that default config has `batch_size=4`, `poll_delay_secs=30`, `exit_wait_secs=3`, `ready_timeout_secs=60`
  - Test deserialization from YAML with partial overrides
  - **Validates: Requirements F10**

- [x] 5. Implement `ExecutionPhase` enum and `FeatureExecutor` struct in `src/feature/executor.rs`
  - Define `ExecutionPhase` enum: `Idle`, `ExitingExpert { started_at: Instant }`, `RelaunchingExpert { started_at: Instant }`, `SendingBatch`, `WaitingPollDelay { started_at: Instant }`, `PollingStatus`, `Completed`, `Failed(String)`
  - Define `FeatureExecutor` struct with config fields, state, file paths, progress tracking, and reset context
  - Implement `FeatureExecutor::new(feature_name, expert_id, config)` constructor
  - Implement `validate()` method: check `tasks_file` exists, optionally set `design_file` if it exists
  - Implement `parse_tasks()` method: read task file from disk, delegate to `task_parser::parse_tasks`
  - Implement `next_batch(tasks)` method: filter incomplete, take first `batch_size`
  - Implement `build_prompt(batch)` method: generate prompt string per design section 3.5
  - Implement accessor methods: `phase()`, `feature_name()`, `exit_wait()`, `ready_timeout()`, `poll_delay()`, `completed_tasks()`, `total_tasks()`, `working_dir()`, `instruction_file()`
  - Implement `set_phase()`, `record_batch_sent()`, `cancel()` methods
  - _Requirements: F1, F2, F3, F4, F5, F7, F8, F10, F11_

- [x] 5.1. Write tests for `FeatureExecutor` validation
  - **Property P1: File Validation**
  - Test `validate()` succeeds when tasks file exists
  - Test `validate()` fails when tasks file is missing
  - Test `validate()` sets `design_file` to `Some` when design file exists
  - Test `validate()` leaves `design_file` as `None` when design file is absent
  - **Validates: Requirements F1, F2, F3**

- [x] 5.2. Write tests for batch calculation
  - **Property P2: Batch Correctness**
  - Test `next_batch()` returns first `batch_size` uncompleted tasks
  - Test `next_batch()` returns fewer than `batch_size` when fewer remain
  - Test `next_batch()` returns empty vec when all tasks completed
  - Test `next_batch()` skips completed tasks correctly
  - **Validates: Requirements F5, F10**

- [x] 5.3. Write tests for prompt building
  - **Property P11: Design File Optional**
  - Test `build_prompt()` includes design file reference when `design_file` is `Some`
  - Test `build_prompt()` omits design file reference when `design_file` is `None`
  - Test `build_prompt()` includes comma-separated task numbers
  - **Validates: Requirements F3, F5**

- [x] 6. Checkpoint - Executor core logic
  - Run `make test` to ensure all executor and parser tests pass.

- [x] 7. Add `feature_executor: Option<FeatureExecutor>` field to `TowerApp` in `src/tower/app.rs`
  - Add the field to the `TowerApp` struct
  - Initialize as `None` in constructor
  - _Requirements: NF1_

- [x] 8. Implement `Ctrl+G` keybinding handler in `src/tower/app.rs`
  - In the key event handler, detect `Ctrl+G` when focused on TaskInput
  - If `feature_executor` is `Some` (already running): cancel execution, set `feature_executor = None`, show toast
  - If `feature_executor` is `None`: read feature name from `task_input.content()`, get selected expert ID, create `FeatureExecutor::new()`, call `validate()`, if valid set field and clear input, show toast; if invalid show error toast
  - Check expert is idle before starting; refuse with warning toast if busy
  - _Requirements: F1, F2, F11_

- [x] 8.1. Write tests for `Ctrl+G` keybinding
  - **Property P7: Cancellation Safety**
  - Test that `Ctrl+G` with valid feature name starts execution
  - Test that `Ctrl+G` while executing cancels and returns to idle
  - Test that `Ctrl+G` with missing task file shows error toast
  - **Validates: Requirements F1, F2, F11**

- [x] 9. Implement `poll_feature_executor()` method in `src/tower/app.rs`
  - Add async method that matches on `executor.phase()` and drives state transitions per design section 4.3
  - Handle `ExitingExpert`: check elapsed vs `exit_wait`, then relaunch Claude
  - Handle `RelaunchingExpert`: poll pane for "bypass permissions", check timeout
  - Handle `SendingBatch`: parse tasks, calculate batch, send prompt or complete
  - Handle `WaitingPollDelay`: check elapsed vs `poll_delay`
  - Handle `PollingStatus`: check expert state, re-read tasks, decide next action
  - Handle `Completed`: show completion message, clear executor
  - Handle `Failed`: show error message, clear executor
  - _Requirements: F5, F6, F7, F8, NF1, NF2, NF3_

- [x] 9.1. Integrate `poll_feature_executor()` into main loop
  - Add `self.poll_feature_executor().await?;` call in `run()` method after existing polls
  - _Requirements: NF1, NF2_

- [x] 10. Checkpoint - Core execution loop
  - Run `make test` and `make` to ensure compilation and all tests pass.

- [x] 11. Implement execution badge in Expert List panel title
  - Modify `StatusDisplay` to accept an optional `execution_badge: Option<String>` in its render method
  - Display `"Experts [> {feature}]"` while running, `"Experts [~ resetting...]"` during reset, `"Experts"` normally
  - Update `src/tower/ui.rs` to pass the badge from `TowerApp.feature_executor` state to `StatusDisplay`
  - _Requirements: F9_

- [x] 11.1. Write tests for execution badge rendering
  - **Property P10: Progress Accuracy** (badge correctness)
  - Test badge shows feature name during execution
  - Test badge shows "resetting..." during ExitingExpert/RelaunchingExpert phases
  - Test badge is absent when no executor is active
  - **Validates: Requirements F9**

- [x] 12. Implement progress display in status bar
  - While executing, show: `"> {feature}: {completed}/{total} tasks | Batch: {current_batch_numbers}"`
  - During session reset, show: `"~ {feature}: resetting expert... | {completed}/{total} tasks"`
  - Update the message in `poll_feature_executor()` at appropriate state transitions
  - _Requirements: F12_

- [x] 12.1. Write tests for progress display
  - **Property P10: Progress Accuracy**
  - Test progress message format during execution
  - Test progress message format during reset
  - Test progress counts reflect actual task file state
  - **Validates: Requirements F12**

- [x] 13. Add `Ctrl+G` keybinding hint to help modal
  - Add entry in `src/tower/widgets/help_modal.rs` for `Ctrl+G: Execute feature / Cancel execution`
  - _Requirements: F11_

- [x] 14. Checkpoint - UI integration
  - Run `make test` and `make` to ensure all tests pass and the full build succeeds.

- [x] 15. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make test` to confirm all tests pass
  - Run `make` to confirm clean compilation
  - Verify all requirements F1-F12, NF1-NF3 are covered
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- The project uses inline `#[cfg(test)] mod tests` blocks rather than a separate `tests/` directory. All new tests should follow this pattern.
- `ExpertStateDetector::detect_state()` is synchronous (reads files directly), while `ClaudeManager` methods are async. The `poll_feature_executor()` method must be async.
- The existing `ClaudeManager` already provides `send_exit()`, `launch_claude()`, `send_keys_with_enter()`, and `capture_pane_with_escapes()` -- all needed by the executor.
- Task file is re-read from disk each cycle (requirement NF3) because the expert modifies it. Never cache parsed results across cycles.
- The `Ctrl+G` keybinding is not currently used in the codebase, so there are no conflicts.
