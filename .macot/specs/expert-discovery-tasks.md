# Implementation Plan: Expert Discovery Agent

## Overview

This plan decomposes the expert-discovery feature into incremental tasks building from the data layer upward: manifest generation first, then template creation, followed by integration into the rendering pipeline, and finally the refresh hooks in `TowerApp`. Each implementation task is paired with tests, and checkpoints validate integration at key milestones.

## Tasks

- [x] 1. Create `ExpertManifestEntry` struct and `generate_expert_manifest` function
  - Create new file `src/instructions/manifest.rs`
  - Define `ExpertManifestEntry` with `expert_id: u32`, `name: String`, `role: String`, `worktree_path: Option<String>`
  - Implement `generate_expert_manifest(config: &Config, session_roles: &SessionExpertRoles, registry: &ExpertRegistry) -> Result<String>`
  - Uses `session_roles` for current role assignments, falls back to config defaults
  - Uses `registry` for `worktree_path` field from `ExpertInfo`
  - Register module in `src/instructions/mod.rs`
  - _Requirements: 3.1, 4_

- [x] 1.1 Write tests for `generate_expert_manifest`
  - **Property 5: Complete Expert Visibility** — every expert in config appears in manifest
  - **Property 5 (edge case)**: empty config produces valid empty JSON array `[]`
  - **Property 1: Worktree Isolation** — manifest includes `worktree_path` from registry
  - **Property 2: Manifest Freshness** — session roles override config defaults
  - Valid JSON output verification
  - **Validates: Requirements 3.1, 4, 6.1, 6.2, 6.5**

- [x] 2. Implement `write_expert_manifest` function
  - Add `write_expert_manifest(queue_path: &Path, content: &str) -> Result<PathBuf>` to `src/instructions/manifest.rs`
  - Writes JSON content to `{queue_path}/experts_manifest.json`
  - Overwrites existing file if present
  - _Requirements: 3.1, 4_

- [x] 2.1 Write tests for `write_expert_manifest`
  - **Property 2: Manifest Freshness** — file is created at expected path
  - **Property 2: Manifest Freshness** — file is overwritten when called again
  - **Validates: Requirements 3.1, 4, 6.2**

- [x] 3. Checkpoint - Manifest generation compiles and all tests pass
  - Run `make test` to verify manifest module tests
  - Run `make build` to verify compilation

- [x] 4. Create expert-discovery agent template
  - Create `instructions/templates/agents/expert-discovery.md.tmpl`
  - Template variables: `{{ expert_id }}`, `{{ expert_name }}`, `{{ worktree_path }}`, `{{ manifest_path }}`, `{{ status_dir }}`
  - Instructions for reading manifest, filtering by `worktree_path`, reading status files, returning formatted table
  - Status mapping: `"pending"` -> idle, `"processing"` -> busy, missing -> busy
  - Template must be read-only (no writes to any file)
  - _Requirements: 3.2, 4_

- [x] 4.1 Write content tests for expert-discovery template
  - **Property 7: Signature Compatibility** — template contains `{{ manifest_path }}` variable
  - **Property 7: Signature Compatibility** — template contains `{{ status_dir }}` variable
  - **Property 1: Worktree Isolation** — template contains worktree filter instruction
  - **Property 6: Subagent Isolation** — template instructs read-only behavior
  - **Property 1: Worktree Isolation** — template contains `{{ worktree_path }}` variable
  - **Validates: Requirements 3.2, 6.1, 6.6, 6.7**

- [x] 5. Extend `render_agents_json` to include expert-discovery agent
  - Modify `src/instructions/agents.rs`
  - Add parameters: `worktree_path: Option<&str>`, `manifest_path: &str`, `status_dir: &str`
  - Load and render `expert-discovery.md.tmpl` alongside `messaging.md.tmpl`
  - Output JSON gains `"expert-discovery"` key with description and rendered prompt
  - If template does not exist, return only messaging agent (backward compatibility)
  - _Requirements: 3.3, 6.4_

- [x] 5.1 Write tests for extended `render_agents_json`
  - **Property 4: Backward Compatibility** — discovery agent present when template exists
  - **Property 4: Backward Compatibility** — discovery agent absent when no template
  - **Property 7: Signature Compatibility** — rendered output contains manifest path
  - **Property 7: Signature Compatibility** — rendered output contains status dir
  - **Property 1: Worktree Isolation** — rendered output contains worktree path
  - Update existing tests (`render_agents_json_returns_none_when_no_template`, `render_agents_json_returns_valid_json_with_template`, `render_agents_json_renders_expert_id`) for new signature
  - **Validates: Requirements 3.3, 6.1, 6.4, 6.7**

- [x] 6. Checkpoint - Agent rendering compiles and all tests pass
  - Run `make test` to verify agents.rs tests
  - Run `make build` to verify compilation

- [x] 7. Extend `load_instruction_with_template` signature
  - Modify `src/instructions/template.rs`
  - Add parameters: `worktree_path: Option<&str>`, `manifest_path: &str`, `status_dir: &str`
  - Pass new parameters through to `render_agents_json`
  - _Requirements: 3.5_

- [x] 7.1 Update all call sites of `load_instruction_with_template`
  - Production call sites (7 total):
    - `prepare_expert_files` in `src/commands/common.rs`
    - `reset_expert` (CLI command) in `src/commands/reset.rs`
    - `change_expert_role` in `src/tower/app.rs`
    - `reset_expert` (TowerApp method) in `src/tower/app.rs`
    - `return_expert_from_worktree` in `src/tower/app.rs`
    - `launch_expert_in_worktree` in `src/tower/app.rs`
    - `start_feature_execution` handler in `src/tower/app.rs`
  - Derive `manifest_path` and `status_dir` from `config.queue_path`
  - Derive `worktree_path` from `ExpertRegistry` or context
  - _Requirements: 3.5, 6.7_

- [x] 7.2 Update test call sites of `load_instruction_with_template` and `render_agents_json`
  - Update 6 test call sites in `src/instructions/template.rs`
  - Update 3 test call sites in `src/instructions/agents.rs`
  - Add new tests for parameter pass-through
  - **Property 7: Signature Compatibility** — manifest_path passed to agents renderer
  - **Property 7: Signature Compatibility** — status_dir passed to agents renderer
  - **Property 1: Worktree Isolation** — worktree_path passed to agents renderer
  - **Validates: Requirements 3.5, 6.1, 6.7**

- [x] 8. Checkpoint - Full rendering pipeline compiles and all tests pass
  - Run `make test` to verify all template and agents tests
  - Run `make build` to verify all call sites compile

- [x] 9. Add `refresh_expert_manifest` helper to `TowerApp`
  - Add private method `fn refresh_expert_manifest(&self) -> Result<()>` to `src/tower/app.rs`
  - Reads from `self.config`, `self.session_roles`, `self.expert_registry`
  - Calls `generate_expert_manifest` then `write_expert_manifest`
  - _Requirements: 3.4_

- [x] 9.1 Insert manifest refresh calls at state-change points
  - Call `refresh_expert_manifest` in:
    - `TowerApp::new()` — initial generation at startup
    - `TowerApp::change_expert_role()` — after role update
    - `TowerApp::reset_expert()` — after expert reset
    - `TowerApp::launch_expert_in_worktree()` — after worktree assignment
    - `TowerApp::return_expert_from_worktree()` — after worktree cleared
  - _Requirements: 3.4, 6.2_

- [x] 9.2 Write tests for manifest refresh integration
  - **Property 2: Manifest Freshness** — manifest regenerated on role change
  - **Property 2: Manifest Freshness** — manifest regenerated on worktree operations
  - **Property 5: Complete Expert Visibility** — all experts appear after refresh
  - **Validates: Requirements 3.4, 6.2, 6.5**

- [x] 10. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make ci` (build + lint + format + test)
  - Verify no compiler warnings related to new code
  - Ensure all tests pass, ask the user if questions arise

## Notes

- The `load_instruction_with_template` function grows to 9 parameters. The design doc notes this is a follow-up refactoring candidate (bundle into `AgentContext` struct), but that is out of scope for this task.
- Status files (`.macot/status/expert{id}`) are an existing mechanism, unchanged by this feature. The discovery subagent reads them but never writes.
- The manifest file is the bridge between the Rust control tower and the Claude CLI agent runtime. Agents cannot call Rust functions directly.
- Tasks 7.1 and 7.2 are the highest-risk items due to the number of call sites (7 production + 9 test). The compiler enforces correctness via required parameters.
