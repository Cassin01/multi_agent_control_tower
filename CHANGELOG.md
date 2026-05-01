# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog.

## [Unreleased]

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
