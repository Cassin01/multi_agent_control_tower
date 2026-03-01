# Implementation Plan: Expert Row Execution Status

## Overview

Move the execution indicator from the Experts panel title badge to an inline two-line row for the executing expert. The plan builds bottom-up: data model first, then producer (FeatureExecutor), then consumer (StatusDisplay), then integration (UI::render). Each implementation step is paired with tests and old badge code is removed incrementally.

## Tasks

- [ ] 1. Define `ExpertRowStatus` struct
  - Add `ExpertRowStatus` with `expert_id: u32`, `text: String`, `color: Color` to `src/tower/widgets/status_display.rs`
  - Derive `Debug, Clone`
  - Make all fields `pub`
  - _Requirements: 3.1_

- [ ] 1.1 Write tests for `ExpertRowStatus` construction
  - **Property 2: Single-Expert Targeting** — struct carries exactly one `expert_id`
  - **Validates: Requirements 3.1**

- [ ] 2. Add `expert_row_status()` to `FeatureExecutor`
  - Add `pub fn expert_row_status(&self) -> Option<ExpertRowStatus>` to `src/feature/executor.rs`
  - Map `ExitingExpert`/`RelaunchingExpert` → `"⟳ resetting..."` with `Color::Yellow`
  - Map `SendingBatch`/`WaitingPollDelay`/`PollingStatus` → `"▶ {feature} {completed}/{total}"` with `Color::Magenta`
  - Map `Idle`/`Completed`/`Failed` → `None`
  - Truncate feature name if total text exceeds 25 chars
  - Set `expert_id` from `self.expert_id`
  - _Requirements: 3.2, 4_

- [ ] 2.1 Write tests for `expert_row_status()` phase mapping
  - **Property 5: Phase-Accurate Display** — each `ExecutionPhase` maps to the correct text and color
  - **Property 7: Lifecycle Consistency** — `Idle`/`Completed`/`Failed` return `None`
  - **Property 8: Progress Accuracy** — `completed/total` numbers match `completed_tasks()`/`total_tasks()`
  - Tests: `expert_row_status_none_when_idle`, `expert_row_status_resetting_during_exit`, `expert_row_status_resetting_during_relaunch`, `expert_row_status_running_during_sending_batch`, `expert_row_status_running_during_waiting_poll`, `expert_row_status_running_during_polling`, `expert_row_status_none_when_completed`, `expert_row_status_none_when_failed`, `expert_row_status_truncated_for_long_feature_name`, `expert_row_status_expert_id_matches_executor`
  - **Validates: Requirements 3.2, 4**

- [ ] 3. Checkpoint — `ExpertRowStatus` struct and `FeatureExecutor::expert_row_status()` tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Replace `execution_badge` field with `expert_execution` in `StatusDisplay`
  - In `src/tower/widgets/status_display.rs`, remove field `execution_badge: Option<String>`
  - Add field `expert_execution: Option<ExpertRowStatus>`
  - Remove methods `set_execution_badge()` and `execution_badge()`
  - Add method `set_expert_execution(status: Option<ExpertRowStatus>)`
  - _Requirements: 3.3_

- [ ] 4.1 Add `total_row_lines()` method to `StatusDisplay`
  - Returns `experts.len() + 1` when `expert_execution` is `Some`, else `experts.len()`
  - _Requirements: 3.3, 4_

- [ ] 4.2 Write tests for `StatusDisplay` field and method changes
  - **Property 9: Height Accuracy** — `total_row_lines()` returns correct values with/without execution
  - **Property 2: Single-Expert Targeting** — `set_expert_execution` stores and clears correctly
  - Tests: `set_expert_execution_stores_status`, `set_expert_execution_none_clears`, `total_row_lines_without_execution`, `total_row_lines_with_execution`
  - **Validates: Requirements 3.3, 4**

- [ ] 5. Update `StatusDisplay::render()` for two-line rows
  - Change the title from `format!("Experts [{badge}]")` logic to always `"Experts"`
  - In the expert row map, check if `expert_execution.expert_id` matches the current entry
  - If match: build `ListItem::new(vec![line1, line2])` with line2 showing indented execution status
  - If no match: build `ListItem::new(line1)` as before
  - Line2 indent: 6 spaces to align with name column after `[N] ● `
  - Apply `exec.color` style to line2 status text
  - _Requirements: 3.3, 4_

- [ ] 5.1 Write test for title invariant
  - **Property 1: Title Invariant** — panel title is always `"Experts"` regardless of execution state
  - Test: `title_always_experts`
  - **Validates: Requirements 3.3**

- [ ] 6. Checkpoint — `StatusDisplay` changes compile and all tests pass
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 7. Update `UI::render()` to use new API
  - In `src/tower/ui.rs`, replace `execution_badge()` call with `expert_row_status()`
  - Replace `set_execution_badge(badge)` with `set_expert_execution(exec_status)`
  - Replace `expert_count() + 2` with `total_row_lines() + 2` for height calculation
  - _Requirements: 3.4_

- [ ] 8. Remove old `execution_badge()` from `FeatureExecutor`
  - Delete `pub fn execution_badge(&self) -> Option<String>` from `src/feature/executor.rs`
  - _Requirements: 3.2_

- [ ] 8.1 Migrate old tests
  - Remove `execution_badge_*` tests from `src/feature/executor.rs` (replaced by task 2.1 tests)
  - Remove `execution_badge_*` tests from `src/tower/widgets/status_display.rs` (replaced by task 4.2/5.1 tests)
  - _Requirements: 7_

- [ ] 9. Final checkpoint — Ensure all tests pass and system integration works
  - Run `make ci` to validate build, lint, format, and tests
  - Visually verify: panel title is always `"Experts"`, executing expert row has two lines, non-executing experts are single-line
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- The `ExpertRowStatus` struct is defined in `status_display.rs` alongside `StatusDisplay` since it is the primary consumer
- `FeatureExecutor` needs to import `ExpertRowStatus` and `ratatui::style::Color`
- The old `execution_badge` field/methods in both `FeatureExecutor` and `StatusDisplay` are fully replaced (not deprecated)
- ListState selection index is unaffected by multi-line `ListItem`s — ratatui handles this natively (Property 10)
- Feature name truncation threshold is 25 chars total for the status text line
