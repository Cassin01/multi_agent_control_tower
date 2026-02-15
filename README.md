# macot

[![Crates.io](https://img.shields.io/badge/crates.io-macot-blue)](https://crates.io/crates/macot)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](LICENSE)
[![CI](https://img.shields.io/badge/ci-passing-brightgreen)](./.github)

**Multi Agent Control Tower for your terminal**: orchestrate parallel coding agents with one Rust-native command line and a focused TUI.

## Key Features

- `⚡ Parallel orchestration`: run multiple experts concurrently in isolated tmux panes.
- `🧠 Role-based execution`: assign specialized roles (`architect`, `frontend`, `backend`, `tester`, and more).
- `🖥️ Control Tower TUI`: dispatch tasks, monitor states, and inspect reports from one screen.
- `🌲 Worktree-friendly workflows`: launch experts against isolated workspaces to reduce branch conflicts.
- `🧩 Configurable by design`: tune experts, roles, timeouts, and paths via YAML.
- `🔒 Local-first architecture`: file-based coordination and queue state, no external control service required.
- `🦀 Rust fundamentals`: safety, performance, and maintainability built into the runtime.

## Installation

### Prerequisites

- Rust toolchain (`rustup`, `cargo`)
- [tmux](https://github.com/tmux/tmux)
- Current runtime integration: [Claude CLI](https://docs.anthropic.com/en/docs/claude-code)

### Install from crates.io

```bash
cargo install macot
```

### Install from source

```bash
cargo install --path .
```

### Homebrew (coming soon)

```bash
brew install macot
```

### Prebuilt binaries (coming soon)

Download from GitHub Releases (TBD).

## Quick Start

Run this inside a project directory:

```bash
# 1) Start a session (defaults to current directory and 4 experts)
macot start

# 2) Open the control tower UI
macot tower

# 3) Pick an expert, enter a task, submit
```

Within ~30 seconds, you should see experts move from idle to active and reports start appearing in the TUI.

## Usage

### Command Overview

| Command | Purpose |
|---|---|
| `macot start [project_path]` | Initialize a session and launch experts |
| `macot tower [session_name]` | Open the control tower UI |
| `macot status [session_name]` | Print live session/expert status |
| `macot sessions` | List running `macot-*` sessions |
| `macot down [session_name]` | Stop a session gracefully (or force) |
| `macot reset expert <id\|name>` | Reset one expert context/runtime |

### Common Examples

```bash
# Start in current directory
macot start

# Start with explicit project path and 6 experts
macot start /path/to/project -n 6

# Start with custom config
macot start -c ./config/macot.yaml

# Open UI (auto-resolves when exactly one session exists)
macot tower

# Open UI for a specific session
macot tower macot-a1b2c3d4

# Status for active/specific session
macot status
macot status macot-a1b2c3d4

# List all sessions
macot sessions

# Graceful shutdown / force shutdown / cleanup
macot down
macot down macot-a1b2c3d4 --force
macot down macot-a1b2c3d4 --cleanup

# Reset one expert by id or name
macot reset expert 1 --session macot-a1b2c3d4
macot reset expert frontend --session macot-a1b2c3d4 --keep-history
macot reset expert backend --session macot-a1b2c3d4 --full
```

### Primary Options

- `start`
- `-n, --num-experts <N>`: override expert count
- `-c, --config <PATH>`: load custom config file
- `down`
- `-f, --force`: kill without graceful exit
- `--cleanup`: remove session context data after shutdown
- `reset expert`
- `-s, --session <NAME>`: target session explicitly
- `--keep-history`: clear working context while preserving history
- `--full`: full reset and Claude session restart

## Contributing

Contributions are welcome. `macot` follows standard Rust OSS practices: small focused PRs, clear rationale, and tests for behavior changes.

1. Open an issue for large changes to align on approach.
2. Keep PRs scoped and reviewable.
3. Update docs/tests with code changes when relevant.

```bash
# Build
make build

# Test
make test

# Lint
make lint

# Full local CI checks
make ci
```

Rust values apply here: safety, correctness, and fearless concurrency in real workflows.

## License

[Apache-2.0](LICENSE) © Cassin01
