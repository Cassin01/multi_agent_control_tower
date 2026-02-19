# Implementation Plan: Worktree Return on Ctrl+W

## Overview

The feature adds dual-purpose behavior to Ctrl+W: when task input is empty and the expert is in a worktree, it returns the expert to the project root. Implementation builds bottom-up — starting with the `ExpertContext` data layer (`clear_worktree`), then the `TowerApp` method (`return_expert_from_worktree`), then the key handler routing change, and finally the help text update. Each step is paired with tests following existing patterns (`create_test_app`, message assertions).

## Tasks

- [x] 1. Add `clear_worktree` method to `ExpertContext`
  - Add `pub fn clear_worktree(&mut self)` to `impl ExpertContext` in `src/context/expert.rs`
  - Sets `worktree_branch` and `worktree_path` to `None`, calls `self.touch()`
  - Mirrors the structure of `set_worktree` (line 103)
  - _Requirements: 3 (Context Cleanup)_

- [x] 1.1 Write tests for `clear_worktree`
  - **Property 3: Context Cleanup** — After `clear_worktree`, both `worktree_branch` and `worktree_path` are `None`
  - **Property 3: Context Cleanup** — `clear_worktree` calls `touch()` (updated_at changes)
  - Follow existing test naming: `expert_context_clear_worktree_resets_fields_to_none`, `expert_context_clear_worktree_calls_touch`
  - **Validates: Requirements 3**

- [x] 2. Checkpoint - Verify `ExpertContext` changes
  - Run `make test` to ensure all existing + new tests pass

- [x] 3. Add `return_expert_from_worktree` method to `TowerApp`
  - Add `pub async fn return_expert_from_worktree(&mut self) -> Result<()>` to `src/tower/app.rs`
  - Check `selected_expert_id()`; if `None`, set message "No expert selected" and return
  - Load `ExpertContext` and check `worktree_path`; if `None` or empty, set message "Enter a feature name in the task input before launching worktree" and return
  - If in worktree: call `send_exit`, sleep 3s, load context, call `clear_worktree`, `clear_session`, `clear_knowledge`, save context
  - Reload instructions via `load_instruction_with_template` (same as `reset_expert`, line 1360)
  - Relaunch Claude at `config.project_path` (not the worktree path)
  - Set success message: `"{name} returned to project root"`
  - Follow the same pattern as `reset_expert` (lines 1317-1407) with the addition of `clear_worktree`
  - _Requirements: 2 (Worktree Detection), 3 (Context Cleanup), 4 (Restart Correctness), 5 (Instruction Reload), 6 (Error Fallback), 7 (Idempotency)_

- [x] 3.1 Write tests for `return_expert_from_worktree`
  - **Property 6: Error Fallback** — When no expert is selected, message is "No expert selected"
  - **Property 6, 7: Error Fallback / Idempotency** — When expert has no worktree context, the existing error message is shown
  - Follow existing test patterns: `create_test_app`, `app.message()` assertions
  - Test names: `return_expert_no_expert_selected_shows_error`, `return_expert_no_worktree_shows_error`
  - **Validates: Requirements 2, 3, 6, 7**

- [x] 4. Checkpoint - Verify `return_expert_from_worktree` method
  - Run `make test` to ensure all tests pass

- [x] 5. Modify Ctrl+W key handler to route based on input state
  - In `src/tower/app.rs` around line 882, change the Ctrl+W handler from unconditional `launch_expert_in_worktree()` to input-based routing
  - If `task_input.content().trim()` is empty: call `return_expert_from_worktree()`
  - Otherwise: call `launch_expert_in_worktree()` (existing behavior preserved)
  - _Requirements: 1 (Input-Based Routing)_

- [x] 5.1 Write tests for Ctrl+W routing logic
  - **Property 1: Input-Based Routing** — With non-empty input, the existing `launch_expert_in_worktree` path is taken (existing test `launch_expert_in_worktree_rejects_empty_feature_name` may need adjustment since empty input now routes differently)
  - Verify existing test `launch_expert_in_worktree_returns_early_when_in_progress` still passes
  - **Validates: Requirements 1**

- [x] 6. Update help modal text for Ctrl+W
  - In `src/tower/widgets/help_modal.rs`, change the Ctrl+W description from `"Launch expert in worktree (uses task input as branch name)"` to `"Launch expert in worktree / Return from worktree"`
  - _Requirements: Documentation accuracy_

- [x] 6.1 Update help modal test
  - Verify existing test `help_text_includes_worktree_shortcut` still passes (it checks for "Ctrl+W" and "worktree", both still present)
  - If the test checks exact text, update the expected string
  - **Validates: Documentation accuracy**

- [x] 7. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make ci` to verify build, lint, format, and tests all pass
  - Verify the existing `launch_expert_in_worktree_rejects_empty_feature_name` test still makes sense or update it since empty input now triggers return logic instead

## Notes

- The `return_expert_from_worktree` method closely follows `reset_expert` (lines 1317-1407) but additionally calls `clear_worktree` on the context before saving.
- The working directory for relaunch must be `config.project_path` (not `resolve_expert_working_dir`, which would resolve back to the worktree).
- The existing test `launch_expert_in_worktree_rejects_empty_feature_name` calls `launch_expert_in_worktree()` directly, so it still tests that method's own validation. However, the Ctrl+W handler no longer reaches that code path with empty input — it routes to `return_expert_from_worktree` instead.
- All tests use the existing mock infrastructure (`create_test_app`, `MockSender`).
