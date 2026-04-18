# Changelog

All notable changes to this project will be documented in this file.

The format is based on Keep a Changelog.

## [Unreleased]

### Added

- Contribution and governance scaffolding (`CONTRIBUTING.md`, issue templates, PR template)
- README onboarding funnel, live badges, and architecture/configuration cross-links
- Initial architecture docs

### Fixed

- Expert panel: Claude output now reflows to the panel's actual width. Previously, tmux panes were clamped to 80x24 because the session was created without `window-size manual` and `resize-pane` cannot grow a single-pane detached window. Sessions are now created with `window-size manual` and resize requests use `resize-window` (requires tmux >= 2.9).
