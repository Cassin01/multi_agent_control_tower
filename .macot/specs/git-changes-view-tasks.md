# Implementation Plan: Git Changes View

## Overview

The implementation follows a bottom-up strategy: data layer (GitRepo) first, then individual widgets (GitFileList, GitDiffView), then the container (GitChangesView), and finally integration into TowerApp. Each layer is independently testable. The `git2` dependency is added at the start since all subsequent work depends on it.

## Tasks

- [ ] 1. Add `git2` dependency to Cargo.toml
  - Add `git2 = { version = "0.20", default-features = false }` to `[dependencies]` in `Cargo.toml`
  - Verify `make build` succeeds with the new dependency
  - _Requirements: Library choice (Section 1)_

- [ ] 2. Implement `GitRepo` (git2 wrapper)
  - Create `src/tower/widgets/git_repo.rs`
  - Implement `GitRepo::open(project_path: &Path) -> Result<Self>`
  - Implement `GitRepo::status() -> Result<GitStatusResult>` with classification by `git2::Status` bitflags
  - Implement helper functions `index_status_to_file_status` and `wt_status_to_file_status`
  - Implement `GitRepo::diff(file: &str, staged: bool) -> Result<Vec<DiffLine>>` using `Diff::print()` callback
  - Implement `GitRepo::stage(file: &str) -> Result<()>` using `Index::add_path()`
  - Implement `GitRepo::unstage(file: &str) -> Result<()>` using `Repository::reset_default()`
  - Implement `GitRepo::stage_all()` and `GitRepo::unstage_all()`
  - Handle initial commit edge case for `unstage` (fallback to `index.remove_path()`)
  - Define shared types: `GitFileEntry`, `GitFileStatus`, `DiffLine`, `DiffLineKind`, `GitStatusResult`
  - _Requirements: Section 3.4, Section 4.1, 4.2, Properties 5, 9_

  - [ ] 2.1 Write tests for `GitRepo`
    - **Property 5: Stage/Unstage Correctness** — Verify `stage()` matches `git add`, `unstage()` matches `git reset HEAD --`
    - **Property 9: Repository Consistency** — Verify `open()` reflects on-disk state, error on non-repo path
    - Test `status()` correctly classifies files into staged/unstaged/untracked using `git2::Status` bitflags
    - Test `diff()` returns correctly typed `DiffLine` entries (Added, Removed, Context, HunkHeader)
    - Test `stage_all()` and `unstage_all()` with multiple files
    - Test renamed file detection via `INDEX_RENAMED` / `WT_RENAMED`
    - Test initial commit edge case (empty HEAD) for unstage
    - Use `git2::Repository::init()` with `tempfile::TempDir` for isolation
    - **Validates: Properties 5, 9**

- [ ] 3. Checkpoint - Verify GitRepo layer
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 4. Implement `GitFileList` (left pane widget)
  - Create `src/tower/widgets/git_file_list.rs`
  - Implement `GitFileList` struct with `staged`, `unstaged`, `untracked` vectors and `selected_index`
  - Implement `set_files()` to populate all three sections
  - Implement flat-index navigation: `next()`, `prev()` across all sections
  - Implement `selected_file()` and `selected_section()` to resolve flat index to entry/section
  - Implement `total_count()`, `is_empty()`
  - Implement `set_focused()` for visual focus indication
  - Implement `render()` with section headers ("Staged (N)", "Unstaged (N)", "Untracked (N)"), status icons (M/A/D/R/?), selection highlight, and colors
  - Define `FileSection` enum
  - _Requirements: Section 3.2, Section 4.3, Properties 2, 3_

  - [ ] 4.1 Write tests for `GitFileList`
    - **Property 2: Section Integrity** — Every entry appears in exactly the correct section; section header counts match entry counts
    - **Property 3: Selection Bounds** — `selected_index` is always in `[0, total_count)` when non-empty, `0` when empty
    - Test `set_files` categorizes entries correctly
    - Test `next()`/`prev()` navigation wraps at boundaries
    - Test `selected_file()` returns correct entry for any valid index
    - Test empty state: `is_empty()` returns true, `total_count()` returns 0
    - Test flat-index mapping across multiple sections (e.g., 2 staged + 3 unstaged + 1 untracked → indices 0-5)
    - **Validates: Properties 2, 3**

- [ ] 5. Implement `GitDiffView` (right pane widget)
  - Create `src/tower/widgets/git_diff_view.rs`
  - Implement `GitDiffView` struct with `lines`, `scroll_offset`, `visible_height`
  - Implement `set_diff()` and `clear()`
  - Implement scroll operations: `scroll_up()`, `scroll_down()`, `page_up()`, `page_down()`, `scroll_to_top()`, `scroll_to_bottom()`
  - Implement `total_lines()`, `set_focused()`
  - Implement `render()` with color coding: green for Added, red for Removed, cyan for HunkHeader, dim for FileHeader, "Binary file" for Binary, placeholder for empty diff
  - _Requirements: Section 3.3, Section 4.2, Property 7_

  - [ ] 5.1 Write tests for `GitDiffView`
    - **Property 7: Scroll Invariant** — `scroll_offset` is always `<= max(0, total_lines - visible_height)`; clamped at both ends
    - Test `set_diff` stores lines correctly
    - Test `scroll_up`/`scroll_down` clamp at boundaries (never negative, never past end)
    - Test `page_up`/`page_down` move by `visible_height`
    - Test `scroll_to_top` sets offset to 0, `scroll_to_bottom` sets to max
    - Test empty diff renders placeholder
    - Test `DiffLineKind::Binary` triggers "Binary file" display
    - **Validates: Property 7**

- [ ] 6. Checkpoint - Verify widget layers
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 7. Implement `GitChangesView` (container)
  - Create `src/tower/widgets/git_changes_view.rs`
  - Implement `GitChangesView` struct with `active`, `focus: GitFocusPane`, sub-widgets, and `repo`
  - Define `GitFocusPane` enum (`FileList | DiffView`) and `GitAction` enum
  - Implement `new(project_path: PathBuf) -> Result<Self>`
  - Implement `is_active()`, `toggle()`, `activate()`, `deactivate()`
  - Implement `refresh()` with xxh3 change detection to skip redundant updates
  - Implement `handle_key()` returning `GitAction`:
    - `q`/`Esc` → `GitAction::Close`
    - `j`/`Down`, `k`/`Up` → navigate file list (when FileList focused)
    - `Tab` → switch focus pane
    - `Space`/`Enter` → stage/unstage selected file
    - `a` → stage all, `A` → unstage all
    - `r` → refresh
    - Arrow keys / `g`/`G` / `Ctrl+U`/`Ctrl+D` → scroll diff (when DiffView focused)
  - Implement `render()` with responsive two-pane layout:
    - >= 120 cols: 30/70 split
    - 80-119: fixed 25 + remaining
    - 60-79: fixed 20 + remaining
    - < 60: single-pane (file list only or toggle)
  - Automatically refresh diff when file selection changes
  - After stage/unstage, refresh file list and re-select nearest file if current disappears
  - Register module in `src/tower/widgets/mod.rs`
  - _Requirements: Section 3.1, Section 4.3, 4.4, Properties 1, 4, 6, 8_

  - [ ] 7.1 Write tests for `GitChangesView`
    - **Property 1: Toggle Idempotence** — Toggle twice returns to inactive state
    - **Property 4: Diff-Selection Consistency** — Diff updates when selection changes; after stage/unstage, diff reflects new state
    - **Property 6: Focus Isolation** — While active, handle_key processes events; while inactive, no events reach git view
    - **Property 8: Layout Responsiveness** — Both panes visible at >= 60 cols; single-pane below 60
    - Test `Tab` switches `GitFocusPane` between FileList and DiffView
    - Test `q`/`Esc` returns `GitAction::Close`
    - Test stage/unstage returns correct `GitAction` variants
    - Test xxh3 change detection skips redundant `set_files` calls
    - **Validates: Properties 1, 4, 6, 8**

- [ ] 8. Checkpoint - Verify GitChangesView container
  - Ensure all tests pass, ask the user if questions arise.

- [ ] 9. Integrate GitChangesView into TowerApp
  - Add `Option<GitChangesView>` field to `TowerApp` (in `src/tower/app.rs`)
  - Initialize in `TowerApp::new()` — call `GitChangesView::new()`, store `Some` on success, `None` on error
  - Add `Ctrl+D` handler in `handle_events` to toggle the view (guard: only if `Some`)
  - Add short-circuit guard in `handle_events`: if git view is active, route all keys to `GitChangesView::handle_key()` before normal focus dispatch (same pattern as `help_modal.is_visible()`)
  - Handle `GitAction::Close` to deactivate view
  - Handle `GitAction::StageFile`/`UnstageFile`/`StageAll`/`UnstageAll` by calling repo methods and refreshing
  - Surface errors from stage/unstage via `set_message()`
  - Auto-refresh on re-activation (when toggling back via `Ctrl+D`)
  - _Requirements: Section 2 (Architecture, Screen lifecycle, Separation from normal focus system), Property 10_

  - [ ] 9.1 Integrate GitChangesView into UI::render
    - In `src/tower/ui.rs`, check `git_changes_view.is_active()` before rendering normal layout
    - If active, delegate entire frame to `GitChangesView::render()`
    - If inactive, render normal layout unchanged
    - _Requirements: Section 2 (UI::render), Property 10_

  - [ ] 9.2 Write integration tests for TowerApp + GitChangesView
    - **Property 10: Clean Deactivation** — FocusArea and widget states return to pre-activation values after deactivation
    - Test `Ctrl+D` toggles git view on/off
    - Test key events do not leak to normal FocusArea when git view is active
    - Test `None` case: `Ctrl+D` is no-op when not in a git repo
    - Test error surfacing via `set_message()` on failed stage/unstage
    - **Validates: Properties 6, 10**

- [ ] 10. Final checkpoint - Ensure all tests pass and system integration works
  - Run `make ci` (build + lint + format + test)
  - Verify no regressions in existing tests
  - Ensure all tests pass, ask the user if questions arise.

## Notes

- **Dependency**: `git2 = { version = "0.20", default-features = false }` — no SSH/HTTPS transport needed for local-only operations.
- **Existing deps reused**: `xxhash-rust` (change detection), `tempfile` (test repos), `anyhow` (error propagation).
- **Test strategy**: `git2::Repository::init()` with `TempDir` creates real git repos in tests — no mocking needed, closer to production behavior.
- **Integration pattern**: Follows the existing modal overlay pattern (`HelpModal`, `RoleSelector`) for event short-circuiting and render delegation.
- **Shared types**: `GitFileEntry`, `GitFileStatus`, `DiffLine`, `DiffLineKind` are defined in `git_repo.rs` and re-exported via `mod.rs` for use by other widgets.
