# Implementation Plan: Expert Panel Manifest Sync

## Overview

This plan fixes the bug where dynamically added experts do not appear in the tower Experts panel by making `ExpertRegistry` the single source of truth at runtime. Tasks build bottom-up: first introduce a test-only accessor and a failing property test, then extract a shared registry→display projection helper, then migrate `reload_from_manifest`, `refresh_status`, and `poll_messages` to be registry-driven, and finally lock down the contract with a Property 8' timing test, a startup-snapshot docstring on `Config::experts`, and a static-lint check that forbids future runtime reads of `self.config.experts` in `src/tower/app.rs`. Every implementation task is paired with a test task tracing back to design Properties 8' (Tower Liveness ≤1s), 11 (No Stale Mirror), 12 (Display ⊇ Registry), and Invariants I1/I2/I3.

## Tasks

- [x] 1. Add test-only accessor on `StatusDisplay` for ID-set assertions
  - Add `#[cfg(test)] pub fn expert_ids_for_test(&self) -> Vec<u32>` returning the `expert_id`s currently held by the display, in insertion order
  - Place under `src/tower/status_display.rs` (or wherever `StatusDisplay` lives) to enable Property 12 assertions in downstream tests
  - This is a non-behavioral helper; production code paths are unchanged
  - _Requirements: Property 12 (Display ⊇ Registry), I2 (Display/Registry Sync)_

  - [x] 1.1 Write property test for registry → display projection
    - **Property: T3 — StatusDisplay/Registry sync invariant (proptest, Small layer)**
    - In `src/tower/app.rs` `#[cfg(test)] mod`, add a proptest that registers a `Vec<u32>` of expert ids (size 0..16, range 0..1000) on a freshly built `TowerApp`, calls `push_registry_to_status_display()`, and asserts `HashSet(display.expert_ids_for_test()) == HashSet(input ids)`
    - Test MUST fail at this point because `push_registry_to_status_display` does not yet exist (RED step in TDD cycle)
    - **Validates: Property 12 (Display ⊇ Registry), I2 (Display/Registry Sync)**

- [x] 2. Extract `push_registry_to_status_display` projection helper
  - [x] 2.1 Add private fn `TowerApp::push_registry_to_status_display(&mut self)` to `src/tower/app.rs`
    - Snapshot `self.expert_registry.get_all_experts()` once, sort by `id` ascending for stable rendering
    - Build `Vec<ExpertEntry>` mapping each `ExpertInfo` to `{ expert_id: info.id, expert_name: info.name.clone(), state: self.detector.detect_state(info.id) }`
    - Build `HashMap<u32, String>` of `(id → role.canonical_name())` and call `self.status_display.set_expert_roles(roles)`
    - Call `self.status_display.set_experts(entries)`
    - Helper MUST NOT read `self.config.experts` (Property 11)
    - _Requirements: Property 11 (No Stale Mirror), Property 12 (Display ⊇ Registry), I2_

  - [x] 2.2 Wire helper into `reload_from_manifest` (src/tower/app.rs:422)
    - After registry rebuild and before `self.needs_redraw = true`, call `self.push_registry_to_status_display()`
    - Preserve existing `tracing::warn!` paths for `register_expert` failures (do not let one bad entry abort the projection)
    - _Requirements: Property 8' (Tower Liveness ≤1s), I1 (Registry/Disk Sync), I2_

  - [x] 2.3 Confirm task 1.1 proptest now passes
    - Run `make test` and verify the T3 proptest is GREEN
    - If failing, the registry→display projection contract is broken; do not proceed to Phase 3
    - _Requirements: Property 12, I2_

- [x] 3. Checkpoint - Projection helper landed
  - Run `make test` and `make lint`. Ensure all tests pass and clippy is clean. Ask the user if questions arise before continuing to the `refresh_status` migration.

- [x] 4. Migrate `refresh_status` to registry-driven projection
  - [x] 4.1 Write reload-then-display regression test (T1, Medium layer)
    - **Property: T1 — Reload-then-display: N → N+1**
    - In `src/tower/app.rs` `#[cfg(test)] mod`, build a `TowerApp` with `config.experts.len() == 2`, then via `ManifestPersistor::append_atomic` add an entry with `expert_id = 2`
    - Call `app.poll_manifest_changes()` and assert it returns `true`
    - Assert `app.status_display.expert_ids_for_test().len() == 3` and contains `2`
    - Call `app.refresh_status().await` and assert the count remains `3` (idempotency)
    - Test MUST fail before task 4.2 (RED step)
    - **Validates: Property 8' (Tower Liveness ≤1s), I2 (Display/Registry Sync)**

  - [x] 4.2 Rewrite `refresh_status` (src/tower/app.rs:560) to use the helper
    - Replace the existing `(0..self.config.experts.len() as u32).collect()` and `self.config.experts.iter().enumerate()` blocks with a single call to `self.push_registry_to_status_display()`
    - Keep the existing injections that are NOT registry-derived: `self.tmux.get_all_pane_current_paths().await` → `set_expert_working_dirs`, and `set_project_path(self.config.project_path...)`
    - For `expert_roles`, source the role set from registry (per §3.2 of design) so that newly added experts that are absent from `session_roles.assignments` still get a consistent default
    - MUST NOT introduce any new read of `self.config.experts`
    - _Requirements: Property 8' (Tower Liveness ≤1s), Property 11 (No Stale Mirror), I2_

  - [x] 4.3 Confirm task 4.1 regression test now passes
    - Run `make test`; T1 must be GREEN
    - _Requirements: Property 8', I2_

- [x] 5. Migrate `poll_messages` to registry-driven iteration
  - [x] 5.1 Write poll-messages-iterates-registry test (T4, Medium layer)
    - **Property: T4 — `poll_messages` iterates registry, not config**
    - Set up a `TowerApp` where `config.experts.len() == 2` but, after running `reload_from_manifest` once with a 3-entry manifest, `registry.get_all_experts().len() == 3`
    - Mock the detector so that `detect_state(2)` returns `Processing`
    - Call `app.poll_messages().await`
    - Assert `router.expert_registry().get_expert(2).state == Processing`
    - Test MUST fail before task 5.2 (RED step), because the current loop iterates `config.experts` (length 2) and never visits id 2
    - **Validates: Property 11 (No Stale Mirror), I2 (Display/Registry Sync)**

  - [x] 5.2 Rewrite `poll_messages` loop (src/tower/app.rs:680)
    - Replace `for (i, _) in self.config.experts.iter().enumerate()` with a two-phase pattern: collect `expert_ids: Vec<u32>` from `router.expert_registry().get_all_experts()` first (immutable borrow ends), then iterate `expert_ids` and call `router.expert_registry_mut().update_expert_state(expert_id, state)` per id
    - Borrow ordering MUST honor §3.3 of design: drop the `&` borrow of `expert_registry()` before taking `&mut` in the loop body
    - Preserve existing `tracing::warn!` on `update_expert_state` errors
    - _Requirements: Property 11 (No Stale Mirror), I2_

  - [x] 5.3 Confirm task 5.1 test now passes
    - Run `make test`; T4 must be GREEN
    - _Requirements: Property 11, I2_

- [x] 6. Checkpoint - All three runtime sites are registry-driven
  - Run `make test`, `make lint`, and `make ci`. All must pass. Manually grep `rg -n 'self\.config\.experts' src/tower/app.rs` and confirm the only remaining hit is the startup snapshot construction site inside `TowerApp::new` (around src/tower/app.rs:188). Ask the user if any unexpected hits remain.

- [x] 7. Lock down Property 8' timing bound and structural guarantees
  - [x] 7.1 Write Property 8' timing-bound test (T2, Medium layer)
    - **Property: T2 — Property 8' (Tower Liveness Under Add, refined bound ≤1s)**
    - Build `TowerApp` with N=2. Capture `let t0 = Instant::now();`
    - Call `ManifestPersistor::append_atomic(entry id=2)`
    - Spin-wait (16ms sleep per iteration) until `app.poll_manifest_changes()` returns `true`, with a 1s hard timeout
    - Assert `app.status_display.expert_ids_for_test()` contains `2`
    - Assert `t0.elapsed() <= Duration::from_secs(1)`
    - If macOS flakiness occurs, fall back to existing `flaky_test` mechanism if present; otherwise mark Large-tier and gate behind `#[cfg(not(target_os = "macos"))]` with a tracking note
    - **Validates: Property 8' (Tower Liveness Under Add, refined bound ≤1s)**

  - [x] 7.2 Document `Config::experts` as a startup snapshot
    - Add the docstring from design §3.4 to the `experts: Vec<ExpertConfig>` field in `src/config/loader.rs`
    - Docstring MUST state: "Startup snapshot only" + "not updated at runtime when `experts_manifest.json` changes" + cross-reference to `.macot/specs/expert-panel-manifest-sync-design.md` §2.3 and to `ExpertRegistry::get_all_experts()`
    - Behavior unchanged; this is a documentation-only edit
    - _Requirements: Property 11 (No Stale Mirror), I3 (Config Snapshot)_

  - [x] 7.3 Add static-lint task for `self.config.experts` reads
    - Add `rg -n 'self\.config\.experts' src/tower/app.rs` invocation to the PR checklist (and `Makefile` target if a `make lint` extension is appropriate, e.g. `make check-no-stale-mirror`)
    - Allowed output: only the line(s) inside `TowerApp::new` startup snapshot construction
    - Any other hit is a blocking PR review issue
    - _Requirements: Property 11 (No Stale Mirror)_

  - [x] 7.4 Update CHANGELOG / PR description
    - Add a one-line note: "`config.experts` is startup-snapshot only; runtime expert list is owned by `ExpertRegistry`"
    - Reference design doc path so future readers can find §2.3
    - _Requirements: Property 11_

- [x] 8. Checkpoint - Property 8' bound enforced
  - Run `make ci`. Confirm T1, T2, T3, T4 are all GREEN. Confirm the static `rg` lint surfaces only the allowed `TowerApp::new` site. Ask the user if any timing test is flaky on the host platform before proceeding.

- [ ] 9. (Optional) Extend E2E coverage for the live tower TUI path
  - [ ] 9.1 Add E2E case to `tests/e2e_dynamic_expert_add.rs` (Large layer)
    - Scenario: launch tower TUI with N=2, run `macot expert add -r general -n Smerdyakov` in a sibling shell, assert tower's `status_display` reports 3 experts within 1s
    - May require gating under a feature flag if the TUI harness is heavy; if so, document the flag in the test file
    - **Validates: Property 8' (end-to-end), §7.4 manual acceptance scenario**
    - _Requirements: Property 8' (Tower Liveness ≤1s)_

  - [ ] 9.2 Capture manual acceptance scenario in release notes
    - Copy the 6-step scenario from design §7.4 into the release-note check-off list for the next version
    - Includes the "status marker hot-swap within 2s" sub-step which exercises T4 in production
    - _Requirements: Property 8', I2_

- [ ] 10. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make ci` once more. Verify: (a) T1/T2/T3/T4 GREEN, (b) static `rg` lint clean, (c) `Config::experts` docstring present, (d) no new clippy warnings, (e) existing `poll_manifest_changes_reports_no_change_for_quiescent_disk` and Property 8 reload-on-rename tests still pass. Ask the user if questions arise before opening the PR.

## Notes

- **TDD discipline (per CLAUDE.md)**: every implementation task (2.1, 2.2, 4.2, 5.2) is preceded by a test task (1.1, 4.1, 5.1, 7.1) that MUST be observed failing before the implementation lands. Do not collapse pairs.
- **Single source of truth**: the entire plan rests on the design §2.1 decision that `ExpertRegistry` is authoritative at runtime. If a future task is tempted to read `self.config.experts` outside `TowerApp::new`, that is a Property 11 violation — stop and revisit the design.
- **Borrow discipline in task 5.2**: `expert_registry()` (immutable) must be fully released — by collecting ids into an owned `Vec<u32>` — before `expert_registry_mut()` is called. Inline iteration will not compile.
- **Helper reuse**: `push_registry_to_status_display` is the single projection point reused by both `reload_from_manifest` (immediate path, ~16ms) and `refresh_status` (2s periodic, fail-safe). This is the structural reason Property 8' bound holds even if the watcher misses an event.
- **Test layering**: T3 is Small (proptest, in-process), T1/T4 are Medium (in-process with mocks), T2 is Medium with wall-clock assertion (1s bound), T9.1 is Large (E2E). Match each to existing test directories — do not collocate with source.
- **Out-of-scope**: schema changes to `experts_manifest.json`, new error types, changes to `ManifestWatcher`, and changes to `Config::load`. The design explicitly leaves these untouched (§4, §5, §3.5).
