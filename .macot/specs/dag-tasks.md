# Implementation Plan: DAG-Based Task Scheduling

## Overview

The decomposition builds bottom-up: parser changes first (data model), then the new scheduler module (core logic), then executor integration, config wiring, and finally the app-level call-site adaptation. Each phase is independently testable and paired with property-based tests referencing the design's correctness properties.

## Tasks

- [ ] 1. Extend `TaskEntry` with `dependencies` field
  - Add `pub dependencies: Vec<String>` to `TaskEntry` in `src/feature/task_parser.rs`
  - Update the regex from `^(\s*)- \[([ x])\] (\d+(?:\.\d+)?)\.\s+(.+)$` to `^(\s*)- \[([ x])\] (\d+(?:\.\d+)*)\.\s+(.+?)(?:\s+\[deps:\s*([^\]]*)\])?\s*$`
  - Parse the optional captured group by splitting on `,` and trimming each token
  - Ensure title capture is non-greedy so `[deps: ...]` is excluded from title
  - Update all existing test helper code that constructs `TaskEntry` to include `dependencies: vec![]`
  - _Requirements: FR-1, FR-2_

  - [ ] 1.1 Write tests for dependency parsing
    - **Property 5: Backward Compatibility** — Tasks without `[deps: ...]` produce `dependencies == vec![]`
    - **Property 10: Idempotent Parsing** — Parsing identical content twice yields identical results
    - Test cases: `parse_tasks_with_deps`, `parse_tasks_without_deps`, `parse_tasks_mixed_deps_and_no_deps`, `parse_tasks_deps_with_dot_notation`, `parse_tasks_deps_empty_bracket`, `parse_tasks_deps_whitespace_variants`, `parse_tasks_multi_level_dot_notation`, `parse_tasks_title_preserved_with_deps`
    - **Validates: Requirements FR-1, FR-2, FR-6**

- [ ] 2. Checkpoint - Parser changes compile and all tests pass
  - Run `make test` to confirm existing tests still pass with the new field
  - Run `make lint` to confirm no warnings
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 3. Create `scheduler.rs` module with core types
  - Create `src/feature/scheduler.rs`
  - Define `SchedulerMode` enum (`Dag`, `Sequential`) with `Default`, `Serialize`, `Deserialize`
  - Define `BlockedDiagnostic`, `BlockedTask`, `ScheduleResult` types per design Section 3.2
  - Register `pub mod scheduler;` in `src/feature/mod.rs`
  - _Requirements: FR-3, FR-7_

- [ ] 4. Implement `select_runnable()` function
  - Implement the DAG algorithm: build completed set, compute missing deps, classify runnable vs blocked
  - Implement the Sequential algorithm: return uncompleted tasks in file order
  - Implement cycle detection heuristic per design Section 3.2
  - _Requirements: FR-3, FR-4, FR-5_

  - [ ] 4.1 Write tests for scheduler DAG mode
    - **Property 1: Dependency Ordering** — A task is never runnable while deps are incomplete
    - **Property 2: Parallel Maximization** — All dependency-satisfied tasks appear in runnable set
    - **Property 3: Blocked Detection** — Uncompleted + 0 runnable = `Blocked`, never `AllDone`
    - **Property 7: Missing Dependency Safety** — Non-existent dep references are treated as unsatisfied
    - **Property 8: Cycle Detection** — Mutual deps yield `has_cycle = true`
    - Test cases: `dag_simple_chain`, `dag_parallel_after_common_dep`, `dag_blocked_when_deps_incomplete`, `dag_cycle_detected`, `dag_missing_dep_blocks_task`, `dag_no_deps_always_runnable`, `dag_all_done`
    - **Validates: Requirements FR-3, FR-5, FR-6**

  - [ ] 4.2 Write tests for scheduler Sequential mode
    - **Property 4: Sequential Equivalence** — All uncompleted tasks returned in file order regardless of deps
    - Test cases: `sequential_ignores_deps`, `sequential_all_done`
    - **Validates: Requirements FR-7**

- [ ] 5. Checkpoint - Scheduler module compiles and all tests pass
  - Run `make test` and `make lint`
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 6. Add `scheduler_mode` to `FeatureExecutionConfig`
  - Add `#[serde(default)] pub scheduler_mode: SchedulerMode` to `FeatureExecutionConfig` in `src/config/loader.rs`
  - Add `use crate::feature::scheduler::SchedulerMode;` import
  - Update `Default` impl for `FeatureExecutionConfig` to include `scheduler_mode: SchedulerMode::Dag`
  - _Requirements: FR-7, NFR-1_

  - [ ] 6.1 Write tests for config backward compatibility
    - **Property 6: Config Backward Compatibility** — Config missing `scheduler_mode` deserializes to `Dag`
    - Test cases: config YAML without `scheduler_mode` deserializes correctly, config YAML with `scheduler_mode: sequential` deserializes correctly
    - **Validates: Requirements FR-7, NFR-1**

- [ ] 7. Modify `FeatureExecutor` to use scheduler
  - Add `scheduler_mode: SchedulerMode` field to `FeatureExecutor` struct in `src/feature/executor.rs`
  - Update `FeatureExecutor::new()` to accept and store `scheduler_mode` from config
  - Change `next_batch` return type from `Vec<&'a TaskEntry>` to `Result<Vec<&'a TaskEntry>, String>`
  - Delegate to `scheduler::select_runnable()` in `next_batch`, apply `batch_size` truncation
  - Implement `format_blocked_message()` helper function
  - _Requirements: FR-3, FR-4, FR-5_

  - [ ] 7.1 Write tests for executor integration with scheduler
    - **Property 9: Batch Size Enforcement** — Batch never exceeds `batch_size`
    - Test cases: `next_batch_dag_mode_respects_deps`, `next_batch_dag_mode_batch_size_limit`, `next_batch_dag_mode_blocked_returns_error`, `next_batch_sequential_mode_unchanged`
    - Update existing executor tests for the new `Result` return type
    - **Validates: Requirements FR-3, FR-4, FR-5**

- [ ] 8. Checkpoint - Executor changes compile and all tests pass
  - Run `make test` and `make lint`
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 9. Update `poll_feature_executor` in `app.rs`
  - Modify the `SendingBatch` handler in `src/tower/app.rs` to use `match` on the `Result` from `next_batch`
  - Handle `Ok(batch)` with empty check for `Completed` transition (existing logic)
  - Handle `Err(blocked_msg)` to transition to `ExecutionPhase::Failed(blocked_msg)`
  - Update any other call sites of `next_batch` if they exist
  - Pass `scheduler_mode` from config when constructing `FeatureExecutor`
  - _Requirements: FR-5, NFR-2_

  - [ ] 9.1 Write integration tests for phase transitions
    - **Property 1: Dependency Ordering** (end-to-end)
    - **Property 3: Blocked Detection** (end-to-end)
    - **Property 6: Config Backward Compatibility** (end-to-end)
    - Test cases: `SendingBatch` to `Failed` with diagnostic, `SendingBatch` to `Completed` when all done, default config uses DAG mode
    - **Validates: Requirements FR-5, NFR-1, NFR-2**

- [ ] 10. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make ci` (build + lint + format + test)
  - Verify backward compatibility: task files without `[deps: ...]` work identically to before
  - Verify config without `scheduler_mode` defaults to DAG
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- The `next_batch` signature change from `Vec<&TaskEntry>` to `Result<Vec<&TaskEntry>, String>` is a breaking change for callers. Task 9 must update all call sites (primarily `poll_feature_executor` in `app.rs`).
- Existing tests in `executor.rs` that call `next_batch` must be updated in task 7.1 to handle the new `Result` return type.
- The scheduler module is fully independent and testable in isolation (tasks 3-4) before wiring into executor.
- `SchedulerMode::default()` returns `Dag` to satisfy NFR-1 backward compatibility.
- Cycle detection is a heuristic (not full topological sort) per design — sufficient for the batch-selection use case.
