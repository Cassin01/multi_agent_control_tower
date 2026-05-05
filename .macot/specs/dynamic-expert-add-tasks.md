# Implementation Plan: Dynamic Expert Add

## Overview

設計を **State 層 → Process 層 → UI 層** の順にボトムアップで構築する。`NamePool` / `RoleResolver` / `ManifestPersistor` といった純粋な値・I/O ユニットを先に固め、続いて `TmuxManager` 拡張、`ExpertAddService` の orchestration、最後に CLI と TUI から service を呼び出す形でつなぐ。各実装タスクには対応する Property テストを必ずペアで置き、フェーズ末ごとにチェックポイントで `make ci` を通す。並行制御 (lock) とロールバックは中盤(Phase 5)で集中的に検証する。

## Tasks

- [ ] 1. Project setup — 依存追加と feature module 配置
  - [ ] 1.1 Add new crates to `Cargo.toml`
    - Add `fs2 = "0.4"` and `notify = "6"` to `[dependencies]`
    - Verify build succeeds with `make build` after dependency resolution
    - _Requirements: 1.1_

  - [ ] 1.2 Create empty module skeletons
    - Create `src/expert/add.rs`, `src/expert/role.rs` with `mod.rs` wiring
    - Create `src/experts/names.rs`, `src/experts/persist.rs`
    - Add `pub mod` declarations in `src/experts/mod.rs` and `src/expert/mod.rs` (create the latter if absent)
    - Ensure all new modules compile as no-op (empty `pub` items)
    - _Requirements: 3.1, 3.2, 3.3, 3.4_

- [ ] 2. NamePool — 動的命名プール
  - [ ] 2.1 Implement `NamePool` with literary name pool and fallback
    - Define `LITERARY_NAMES: &[&str]` constant with 12 names listed in design §3.4
    - Implement `pub fn pick_unused(&self, used: &HashSet<&str>) -> Option<&'static str>`
    - Implement `pub fn fallback(&self, id: ExpertId) -> String` returning `format!("Expert{id:02}")`
    - Place in `src/experts/names.rs`
    - _Requirements: 3.4_

  - [ ] 2.2 Write property test for name pool selection
    - **Property: NamePool always returns a valid name (literary or fallback)**
    - Use `proptest!` to generate arbitrary `used: HashSet<String>` and arbitrary `id: u32`
    - Assert: combined output of `pick_unused` then `fallback` matches `^[A-Za-z][A-Za-z0-9_-]*$`
    - Assert: returned name not present in `used` set
    - **Validates: Requirements 3.4, 5.1 (naming regex)**

- [ ] 3. RoleResolver — role spec から system prompt 解決
  - [ ] 3.1 Implement `RoleSpec`, `BuiltinRole`, `ResolvedRole` types
    - Define enums and struct as in design §3.3
    - Embed builtin templates (architect / planner / general) using existing prompt source files; do not modify their bodies
    - Place types in `src/expert/role.rs`
    - _Requirements: 3.3, 4.3_

  - [ ] 3.2 Implement `resolve(spec, project_root)` lookup
    - Builtin: return embedded prompt directly
    - Custom: search `.macot/templates/roles/{name}.md` then `~/.config/macot/roles/{name}.md`
    - Return `RoleError::NotFound` when neither path resolves
    - Read file as UTF-8; surface read errors as `RoleError::InvalidUtf8` / `RoleError::Io`
    - _Requirements: 3.3_

  - [ ] 3.3 Write property test for role resolution determinism
    - **Property 5: Role Resolution Determinism**
    - Build a fixed tempdir fixture with known templates; call `resolve` twice for the same `RoleSpec`
    - Assert: byte-identical `ResolvedRole` (canonical_name, prompt_md, settings_template)
    - Use `proptest!` to generate random `RoleSpec` over a fixed fixture set
    - **Validates: Requirements 3.3, Property 5**

- [ ] 4. ManifestPersistor — atomic manifest I/O
  - [ ] 4.1 Implement `ExpertEntry` serde model and `ManifestPersistor::load_into_registry`
    - Add `serde(Serialize, Deserialize)` derives matching design §3.2 schema
    - On load: parse JSON array, build `ExpertRegistry`, set `next_id = max(expert_id) + 1` (0 if empty)
    - Place in `src/experts/persist.rs`
    - _Requirements: 3.2, 4.2.1_

  - [ ] 4.2 Implement `append_atomic` and `remove_by_id_atomic`
    - Write to `experts_manifest.json.tmp.{pid}` then `rename(2)` for atomic replace
    - Caller is required to hold `.macot/.lock`; document this precondition in rustdoc
    - `remove_by_id_atomic` must be idempotent (no-op if id absent)
    - _Requirements: 3.2, 4.2, 4.4, Property 1, Property 7_

  - [ ] 4.3 Write integration test for atomic write/load round-trip
    - **Property: Manifest write+load is round-trip stable**
    - Use `tempfile::tempdir` for isolated `.macot/`
    - Append 3 entries sequentially; load; assert array order = insertion order = ID ascending
    - Crash-simulate: write `.tmp` file, do not rename, then load — assert original manifest unchanged
    - **Validates: Requirements 3.2, 4.2, Property 1, Property 2**

- [ ] 5. ExpertRegistry persistence integration
  - [ ] 5.1 Re-define `next_id` semantics as derived view
    - Update `register_expert(AUTO_ASSIGN_ID)` flow to support rollback: expose `decrement_next_id_after_failed_commit()` (crate-internal)
    - On lock acquisition, callers must reload manifest and reconcile `next_id = max(disk_max + 1, mem_next_id)`
    - Document new lifecycle in `src/experts/registry.rs` module-level rustdoc
    - _Requirements: 3.2, 4.2.1_

  - [ ] 5.2 Write test for `next_id` reconciliation across processes
    - **Property 2 + Property 3 reconciliation**
    - Simulate: registry A loads manifest=[0,1], allocates 2 in-memory but does not commit. Registry B loads after A commits 2 → B should see `next_id=3`
    - Assert: no two registries ever return same ID after lock+reload protocol
    - **Validates: Property 2, Property 3, Requirement 4.2.1**

- [ ] 6. Locking primitive — `.macot/.lock`
  - [ ] 6.1 Implement `MacotLock` wrapper around `fs2::FileExt`
    - New file `src/state/lock.rs` (create `state` module if absent)
    - `MacotLock::acquire(project_root, timeout=5s)` — spin retry with `try_lock_exclusive`, sleep 50ms between attempts
    - RAII guard: lock released on drop
    - Map errors to `ExpertAddError::LockBusy`
    - _Requirements: 2.4, 5.2, Property 3_

  - [ ] 6.2 Write test for lock contention
    - **Property 3: Lock-Serialized Critical Section**
    - Spawn 2 tasks via `tokio::spawn`; first holds lock 2s, second times out at 5s and gets `LockBusy` only if first never releases (here second should succeed within 2s)
    - Reverse case: first holds 6s — second returns `LockBusy`
    - Assert: serialization observable; no interleaved writes
    - **Validates: Property 3, Property 10, Requirement 2.4**

- [ ] 7. TmuxManager extension — window spawn/kill
  - [ ] 7.1 Add `spawn_expert_window` and `kill_expert_window` async methods
    - File `src/session/tmux.rs` (existing)
    - Mirror existing `create_session` shape: `tmux new-window -t {session}: -n expert{N} -c {cwd} -d` then `send-keys "claude --append-system-prompt … --settings …"`
    - `kill_expert_window` is idempotent (Ok(()) if window absent)
    - _Requirements: 3.5, Property 4_

  - [ ] 7.2 Lift `TmuxBackend` trait to cover new methods
    - Add `spawn_expert_window` / `kill_expert_window` signatures to existing trait used by tests
    - Provide in-memory mock impl in test module that records calls and can be wired to fail on demand
    - _Requirements: 3.5, 7.3_

  - [ ] 7.3 Write integration test for window lifecycle via mock
    - **Property: spawn → kill is idempotent**
    - Use mock backend; call `spawn_expert_window(N=4)` → assert single recorded call with name `expert4`
    - Call `kill_expert_window(N=4)` twice → assert no error on second call
    - **Validates: Requirements 3.5, Property 4 (process-layer side)**

- [ ] 8. ExpertAddService — orchestration core
  - [ ] 8.1 Implement input validation and session resolution
    - Validate name regex `^[A-Za-z][A-Za-z0-9_-]*$`, length 1..=32
    - If `req.session` is None and `macot sessions` returns exactly one — use it; else `AmbiguousSession`
    - Verify session exists via `tmux has-session`; else `SessionNotFound`
    - File `src/expert/add.rs`
    - _Requirements: 3.1, 5.1_

  - [ ] 8.2 Implement state-write phase (lock-bounded)
    - Acquire `MacotLock`
    - Reload manifest via `ManifestPersistor::load_into_registry` and reconcile `next_id`
    - Resolve `RoleSpec` via `RoleResolver`
    - Pick name via `NamePool::pick_unused` or fallback
    - Render & write: `system_prompt/expert{N}.md`, `system_prompt/expert{N}_settings.json`, `status/expert{N}` (= "pending"), `sessions/{h}/experts/expert{N}/context.yaml`, append `expert_roles.yaml`
    - Commit: `experts_manifest.json` via `append_atomic`
    - Release lock before tmux operations (Property 10)
    - _Requirements: 3.1, 4.1, 4.4, Property 1, Property 10_

  - [ ] 8.3 Implement tmux-launch phase and rollback
    - Call `TmuxManager::spawn_expert_window`; on failure re-acquire lock and unwind state files + manifest entry + role assignment
    - Surface terminal error as `TmuxLaunch`; if rollback itself fails, return `RollbackFailure { original, rollback }`
    - Decrement `ExpertRegistry::next_id` only when manifest commit failed (per §4.2.1 step 3)
    - _Requirements: 3.1, 5.2, Property 1, Property 7_

  - [ ] 8.4 Write integration test for happy path
    - **Property 1 + Property 2 (filesystem layer)**
    - Run `add_expert` against tempdir + mock tmux that succeeds
    - Assert: manifest gains exactly one entry with `expert_id = prev_max + 1`
    - Assert: all 4 paired files exist (`expert{N}.md`, `_settings.json`, `status/expert{N}`, `context.yaml`)
    - **Validates: Property 1, Property 2, Requirements 3.1, 4.1**

  - [ ] 8.5 Write integration test for tmux-failure rollback
    - **Property 1 + Property 7: No-Op on Failure**
    - Configure mock tmux to fail on `spawn_expert_window`
    - Capture manifest hash and `expert_roles.yaml` hash before
    - Assert: `add_expert` returns `TmuxLaunch`; both hashes unchanged after; no `expert{N}.*` files exist on disk
    - **Validates: Property 1, Property 7, Requirements 5.2**

  - [ ] 8.6 Write integration test for state-write failure rollback
    - **Property 1 + Property 7 (write-time failure)**
    - Inject `Err` at each state write step (4 sub-cases via failure-injection trait)
    - Assert in each: no partial files left behind; manifest unchanged; `next_id` cache rolled back
    - **Validates: Property 1, Property 7**

  - [ ] 8.7 Write integration test for concurrent adds
    - **Property 3: Lock Serialization**
    - Spawn 2 `add_expert` calls in parallel via `tokio::join!`
    - Assert: both succeed with distinct IDs; manifest array has 2 new entries; no duplicate name
    - Run 20× to amplify chance of catching races
    - **Validates: Property 3, Property 6, Requirement 4.4**

  - [ ] 8.8 Write integration test for lock-release-before-tmux
    - **Property 10: Bounded Lock Hold Time**
    - Inside mock tmux's `spawn_expert_window`, attempt `MacotLock::try_acquire` — must succeed
    - Assert: tmux phase observes lock free
    - **Validates: Property 10**

- [ ] 9. Checkpoint — service layer green
  - Run `make ci` and ensure all tasks 1.1〜8.8 tests pass
  - Ensure all tests pass; ask the user if questions arise

- [ ] 10. CLI integration
  - [ ] 10.1 Add `Expert` subcommand group to `src/commands/mod.rs`
    - Add `Expert(ExpertArgs)` to `Command` enum (preserves existing `Reset`)
    - Create `src/commands/expert.rs` with `ExpertArgs`, `ExpertCmd::Add { … }`, `ExpertCmd::List { … }` exactly as in design §3.6
    - Do NOT add `Remove` variant in this PR (Future Work)
    - _Requirements: 3.6_

  - [ ] 10.2 Wire `Add` to `ExpertAddService::add_expert`
    - Translate clap args → `ExpertAddRequest`
    - On `--worktree`: invoke existing Ctrl+W worktree flow after `add_expert` returns
    - On `--dry-run`: short-circuit before lock acquisition; print planned ID, prompt path, template source, name to stdout
    - On `--json`: print serialized `ExpertAdded` only; suppress human line
    - Default: print `Added expert {id} ({name}, {role}) in session {sess} (window {idx})`
    - _Requirements: 3.6, 5.4_

  - [ ] 10.3 Implement `Expert List` subcommand
    - Read `experts_manifest.json` (no lock needed for read), print table: id / name / role / worktree
    - _Requirements: 3.6, 4.1_

  - [ ] 10.4 Write CLI integration test
    - **Property: CLI parses & dispatches correctly**
    - Use `assert_cmd` to invoke `macot expert add --dry-run -r general --name TestUser` against tempdir fixture
    - Assert: stdout contains planned ID; manifest unchanged
    - Assert: `--json` output is parseable JSON matching `ExpertAdded` schema
    - **Validates: Requirements 3.6, 5.4**

- [ ] 11. Tower TUI integration
  - [ ] 11.1 Add `F2` keybinding to open Add-Expert modal
    - Edit `src/tui/keymap.rs` to bind `F2` (Expert Panel focus) → `OpenAddExpertModal` action
    - Document rejection of `Ctrl+A` / `Ctrl+N` due to input-mode collisions (rustdoc on action)
    - _Requirements: 3.7_

  - [ ] 11.2 Implement Add-Expert modal in `src/tui/expert_panel.rs`
    - Fields: Role (select among architect / planner / general / `<custom path>`), Name (optional input), Worktree (checkbox)
    - Enter → call `ExpertAddService::add_expert` with current form values
    - Show error toast on failure; close modal on success
    - _Requirements: 3.7_

  - [ ] 11.3 Implement `notify`-based manifest watcher
    - Spawn `notify::RecommendedWatcher` watching `.macot/experts_manifest.json` rename events
    - On event: re-read manifest and rebuild `Vec<ExpertCell>`; preserve focus + scroll keyed by `expert_id`
    - No polling fallback (per Property 8 bound; Ctrl+L manual refresh is the documented escape hatch)
    - _Requirements: 3.7, Property 8_

  - [ ] 11.4 Write test for tower reload on manifest change
    - **Property 8: Tower Liveness Under Add**
    - Drive `notify::EventHandler` directly with a synthetic `Event::new(EventKind::Modify(ModifyKind::Name(_)))`
    - Assert within 1s timeout: `Vec<ExpertCell>` reflects new manifest contents
    - Assert: focus on previously-selected `expert_id` retained
    - **Validates: Property 8, Requirement 3.7**

- [ ] 12. Checkpoint — UI surfaces green
  - Run `make ci` and ensure CLI + TUI tests pass
  - Ensure all tests pass; ask the user if questions arise

- [ ] 13. End-to-end / acceptance
  - [ ] 13.1 Implement E2E test for dynamic add against real tmux
    - **Property 4 + Property 9**
    - Place dummy `claude` on PATH (writes to log file then sleeps)
    - `macot launch . -n 2` → `macot expert add -r general -n Smerdyakov`
    - Assert: `tmux list-windows` shows window named `expert3`
    - Run `macot reset expert 3 --full`; assert window relaunched, context.yaml regenerated
    - Marked CI-optional via `#[ignore]`; runnable via `make test-e2e`
    - **Validates: Property 4, Property 9, Requirement 3.5**

  - [ ] 13.2 Implement E2E test for namepool exhaustion fallback
    - Add experts repeatedly without `--name` until literary pool drains
    - Assert: at least one returned name matches `^Expert\d{2}$`
    - **Validates: Requirement 3.4**

  - [ ] 13.3 Implement E2E test for `macot down --cleanup` after dynamic add
    - Add 1 expert, run `down --cleanup`, assert `.macot/` cleaned of dynamic state
    - **Validates: Requirement 4.1, Requirement 4.4**

- [ ] 14. Documentation & manual acceptance
  - [ ] 14.1 Update `doc/` with new CLI surface and TUI key
    - Add `macot expert add` flags table; note `F2` binding; note dependency additions
    - _Requirements: 3.6, 3.7_

  - [ ] 14.2 Run §7.4 manual acceptance scenarios
    - Execute the 9-step checklist from design §7.4 by hand and record outcome
    - File issues for any deviation observed
    - _Requirements: 7.4_

- [ ] 15. Final checkpoint — Ensure all tests pass and system integration works
  - Run `make ci` (build + lint + fmt-check + test) — must be green
  - Run E2E suite (`make test-e2e` or `cargo test -- --ignored`) — must be green
  - Verify Properties 1〜10 are each cited by at least one passing test
  - Ensure all tests pass; ask the user if questions arise

## Notes

- **TDD enforcement**: Each `*.x` implementation task is paired with a `*.y` test task. The test task MUST be authored first (RED), confirmed failing, then implementation made GREEN per project CLAUDE.md.
- **Locking discipline**: All manifest writes happen under `MacotLock`; tmux/Claude I/O happens *outside* the lock (Property 10). Tests in 6.2, 8.7, 8.8 specifically guard this boundary.
- **Rollback completeness**: Three failure surfaces are tested independently — state-write failure (8.6), tmux-launch failure (8.5), and concurrent contention (8.7). Property 1 + Property 7 must be cited together to assert atomicity-on-failure.
- **No `expert remove`**: Explicitly out of scope per design §1; CLI enum must not include `Remove` variant in this PR.
- **Mock boundaries**: `TmuxBackend` trait (task 7.2) is the seam between Medium-tier integration tests and Large-tier E2E tests. Failure injection layers attach here.
- **Build order rationale**: NamePool / RoleResolver / ManifestPersistor are dependency-free → buildable independently. Lock + Tmux + Service layer integrate them. CLI / TUI consume the service. Each phase is independently green-able.
- **Cargo.toml is a critical-path dependency** for Phase 1; without `fs2` and `notify` no later phase compiles. Do not split task 1.1.
