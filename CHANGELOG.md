# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog.

## [Unreleased]

## [0.1.13] - 2026-07-30

### Fixed

- Expert Panel showed only the last screenful of Claude's output and scroll mode
  (`PageUp`) had nothing to scroll. Claude Code's `fullscreen` renderer draws
  into the terminal's alternate screen, where tmux keeps no scrollback, so
  `capture-pane` could never see anything above the visible viewport. Expert
  sessions now pin `"tui": "default"` via the generated `--settings` document,
  which overrides a global `settings.json` set to `fullscreen`. Relaunch experts
  to pick this up.
- `Ctrl+G` (task batch execution) relaunched Claude in the macot project root
  instead of the expert's worktree, so a worktree expert silently fell back to
  the wrong working directory. The working dir is now resolved the same way
  `reset_expert` resolves it.

## [0.1.12] - 2026-07-28

### Added

- Dynamic expert add (`macot expert add` / `macot expert list`): add a new
  expert to a running session without restarting it. Supports built-in roles
  (`architect`, `planner`, `general`) and custom roles via
  `.macot/templates/roles/{name}.md` or `~/.config/macot/roles/{name}.md`,
  with `--dry-run` and `--json` for automation.
- Tower TUI: `F2` opens the Add Expert modal; the `notify`-based manifest
  watcher reloads the Expert Panel automatically on changes.
- New dependencies: `fs2` (advisory `.macot/.lock`) and `notify` (manifest
  change events).
- New `make test-e2e` target running the dynamic-add E2E suite (`cargo test
  -- --ignored`, requires `tmux` on PATH).
- New `make check-no-stale-mirror` target enforcing Property 11 (no runtime
  reads of `self.config.experts` in `src/tower/app.rs`); now part of `make ci`.

### Changed

- `Config::experts` is now treated as a **startup snapshot only**; the
  runtime list of experts is owned by `ExpertRegistry` (single source of
  truth). `TowerApp::reload_from_manifest`, `refresh_status`, and
  `poll_messages` now drive iteration off the registry so dynamically added
  experts surface in the Experts panel and message-state polling within
  ≤ 1s of `macot expert add` (Property 8'). See
  `.macot/specs/expert-panel-manifest-sync-design.md` §2.3.
- Experts are now launched with `claude --permission-mode auto` instead of
  `--dangerously-skip-permissions`, so expert sessions no longer bypass the
  permission system. Readiness detection matches the `auto mode on` status
  line accordingly.

### Fixed

- Expert Panel did not reflect experts added at runtime via `macot expert
  add` or the `F2` modal; the panel and `poll_messages` iterated the stale
  startup snapshot in `Config::experts`.

## [0.1.11] - 2026-05-01

### Added

- Mouse wheel scrolling support in the Expert Panel
- Contribution and governance scaffolding (`CONTRIBUTING.md`, issue templates, PR template)
- README onboarding funnel, live badges, and architecture/configuration cross-links
- Initial architecture docs

### Changed

- Sync expert registries on worktree return so panel state stays consistent

### Fixed

- Expert panel: Claude output now reflows to the panel's actual width. Previously, tmux panes were clamped to 80x24 because the session was created without `window-size manual` and `resize-pane` cannot grow a single-pane detached window. Sessions are now created with `window-size manual` and resize requests use `resize-window` (requires tmux >= 2.9).
- Expert status now resets to pending correctly on exit+relaunch flows
- Removed pending-status write-back after expert exit to avoid stale states
- Removed `MessageRecipient::ExpertName` variant that caused message delivery failures
- tmux silent failures, non-UTF-8 path panics, and opaque cycle detection
