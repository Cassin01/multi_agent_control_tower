# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog.

## [Unreleased]

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
