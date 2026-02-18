# Implementation Plan: Worktree Message Isolation

## Overview

This plan builds worktree-scoped message isolation bottom-up: first the data model (`ExpertInfo`), then the registry layer, then the routing layer, and finally the integration point in `app.rs` and the messaging template. Each phase pairs implementation with tests, and checkpoints validate correctness before moving on.

## Tasks

- [x] 1. Add `worktree_path` field to `ExpertInfo`
  - Add `pub worktree_path: Option<String>` to `ExpertInfo` in `src/models/expert.rs`
  - Add `#[serde(default, skip_serializing_if = "Option::is_none")]` for backward-compatible serialization
  - Update `ExpertInfo::new()` to initialize `worktree_path: None`
  - _Requirements: 3.1_

- [x] 1.1 Add `set_worktree_path` and `same_worktree` methods to `ExpertInfo`
  - Implement `pub fn set_worktree_path(&mut self, path: Option<String>)` in `src/models/expert.rs`
  - Implement `pub fn same_worktree(&self, other: &ExpertInfo) -> bool` using exact `Option<String>` equality (`None == None`, `Some(X) == Some(X)`, all other combinations `false`)
  - _Requirements: 3.1_

- [x] 1.2 Write tests for `ExpertInfo` worktree methods
  - **Property 1: Worktree Isolation** — `same_worktree` returns `false` for `(None, Some(X))`, `(Some(X), None)`, `(Some(X), Some(Y))`
  - **Property 2: Main Repo Affinity** — `same_worktree` returns `true` for `(None, None)`
  - **Property 5: Backward Compatibility** — Serialize `ExpertInfo` without `worktree_path`, deserialize, verify field is `None`; serialize with `worktree_path`, deserialize, verify field is `Some`
  - Test `set_worktree_path` updates the field correctly
  - Test symmetry: `same_worktree(a, b) == same_worktree(b, a)` for all affinity matrix combinations
  - Test reflexivity: `same_worktree(a, a) == true` for both `None` and `Some(X)`
  - **Validates: Requirements 3.1, Properties 1, 2, 5**

- [x] 2. Checkpoint - ExpertInfo model tests pass
  - Run `make test` to confirm all new and existing tests pass.
  - Ensure no regressions in serialization/deserialization.

- [x] 3. Add worktree-aware methods to `ExpertRegistry`
  - Implement `pub fn update_expert_worktree(&mut self, expert_id: ExpertId, worktree_path: Option<String>) -> Result<(), RegistryError>` in `src/experts/registry.rs`
  - Implement `pub fn get_idle_experts_by_role_str_in_worktree(&self, role: &str, worktree_path: &Option<String>) -> Vec<ExpertId>` in `src/experts/registry.rs`
    - Reuse `find_by_role_str` then filter by `is_idle()` AND `same_worktree` with a reference expert's worktree_path
  - _Requirements: 3.2_

- [x] 3.1 Write tests for `ExpertRegistry` worktree methods
  - **Property 3: Role Scoping** — Register experts with different worktree paths and roles; verify `get_idle_experts_by_role_str_in_worktree` returns only those sharing the caller's worktree
  - **Property 2: Main Repo Affinity** — `get_idle_experts_by_role_str_in_worktree` with `None` worktree returns only experts with `None` worktree
  - Test `update_expert_worktree` sets the path correctly on an existing expert
  - Test `update_expert_worktree` for nonexistent expert returns `RegistryError`
  - **Validates: Requirements 3.2, Properties 2, 3**

- [x] 4. Checkpoint - ExpertRegistry tests pass
  - Run `make test` to confirm all new and existing tests pass.

- [x] 5. Add worktree isolation to `MessageRouter`
  - Add `sender_id: ExpertId` parameter to `find_recipient` in `src/queue/router.rs`
  - Implement private `fn worktree_matches(&self, sender_id: ExpertId, recipient_id: ExpertId) -> bool` that retrieves both `ExpertInfo` from registry and calls `same_worktree`
  - Modify `find_recipient` ID arm: after finding the expert, call `worktree_matches(sender_id, found_id)`; return `None` if mismatch
  - Modify `find_recipient` Name arm: after finding the expert, call `worktree_matches(sender_id, found_id)`; return `None` if mismatch
  - Modify `find_recipient` Role arm: replace `get_idle_experts_by_role_str` with `get_idle_experts_by_role_str_in_worktree`, passing the sender's worktree path
  - Update `attempt_delivery` to pass `sender_id` (from `queued_message.message.from_expert_id`) to `find_recipient`
  - When delivery fails due to worktree mismatch (ID/Name targeting), set error message `"Expert {id} is in a different worktree"`
  - _Requirements: 3.3_

- [x] 5.1 Update all call sites of `find_recipient`
  - Search for all usages of `find_recipient` outside `attempt_delivery` and update to pass `sender_id`
  - _Requirements: 3.3_

- [x] 5.2 Write tests for `MessageRouter` worktree isolation
  - **Property 1: Worktree Isolation** — `worktree_matches` returns correct results for all 5 affinity matrix combinations
  - **Property 4: ID/Name Targeting Enforcement** — `find_recipient` by ID with worktree mismatch returns `None`; by name with worktree mismatch returns `None`
  - **Property 3: Role Scoping** — `find_recipient` by role with mixed worktrees returns only same-worktree expert
  - **Property 7: Retry Semantics Preserved** — `attempt_delivery` with worktree mismatch produces `DeliveryResult` with `success: false` and appropriate error string
  - **Property 2: Main Repo Affinity** — Two experts with `None` worktree can find each other via all targeting strategies
  - **Validates: Requirements 3.3, Properties 1, 2, 3, 4, 7**

- [x] 6. Checkpoint - MessageRouter tests pass
  - Run `make test` to confirm all new and existing tests pass.
  - Run `make lint` to check for warnings.

- [x] 7. Propagate worktree path in `App::poll_worktree_launch`
  - In `src/tower/app.rs`, modify the success path of `poll_worktree_launch`:
    - Call `self.expert_registry.update_expert_worktree(result.expert_id, Some(result.worktree_path.clone()))`
    - If `self.message_router` is `Some`, also call `router.expert_registry_mut().update_expert_worktree(result.expert_id, Some(result.worktree_path.clone()))`
  - Remove `#[allow(dead_code)]` from `WorktreeLaunchResult::worktree_path` and `WorktreeLaunchResult::expert_id` if they are now used
  - _Requirements: 3.4, Property 6_

- [x] 7.1 Write test for worktree path propagation
  - **Property 6: Worktree Path Propagation** — Covered by `worktree_tests` in router.rs which validates that `update_expert_worktree` correctly propagates and that the router respects worktree paths. Direct app.rs integration test is not feasible in unit tests due to `TowerApp`'s dependency on terminal/tmux infrastructure; the correctness of propagation is verified by the combination of: (1) `update_expert_worktree` unit tests in registry.rs, (2) worktree isolation tests in router.rs, and (3) the code review of `poll_worktree_launch` changes.
  - **Validates: Requirements 3.4, Property 6**

- [x] 8. Update messaging template for worktree isolation docs
  - In `instructions/templates/agents/messaging.md.tmpl`, add a section explaining worktree isolation:
    - Messages are scoped to the sender's worktree context
    - Experts in different worktrees cannot exchange messages
    - Main repo experts (no worktree) form a default communication group
  - _Requirements: 3.5_

- [x] 9. Final checkpoint - Full integration validation
  - Run `make ci` (build, lint, format-check, test) to verify everything passes.
  - Verify no `#[allow(dead_code)]` remains on fields that are now used.
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- The `ExpertRegistry` in `TowerApp` and the one inside `MessageRouter` are separate instances. Both must be updated when worktree path changes (task 7 handles this).
- `find_recipient` signature change (adding `sender_id`) is a breaking change for all call sites; task 5.1 covers updating them.
- Backward compatibility is ensured via `serde(default)` on the new field — existing YAML files without `worktree_path` will deserialize to `None`.
- The TDD cycle requires writing tests first (tasks 1.2, 3.1, 5.2, 7.1) before or alongside implementation. In practice, since the design specifies exact interfaces, the test and implementation sub-tasks can be worked in tight red-green-refactor loops within each phase.
