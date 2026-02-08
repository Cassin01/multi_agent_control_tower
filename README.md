# macot

**Multi Agent Control Tower** — Orchestrate multiple Claude CLI instances working on your codebase in parallel.

macot spawns a team of specialized Claude agents inside tmux, then gives you a terminal UI to assign tasks, monitor progress, and collect results — all from a single command.

## Features

- **Parallel AI agents** — Run multiple Claude CLI instances simultaneously, each in its own tmux pane
- **Role-based experts** — Assign specialized roles (architect, frontend, backend, tester) with dedicated instruction sets
- **Terminal UI dashboard** — Monitor agent status, assign tasks, and review reports through a ratatui-powered TUI
- **Hub-spoke coordination** — All communication flows through the control tower, preventing race conditions between agents
- **Git worktree isolation** — Launch agents in separate worktrees so parallel changes never conflict
- **File-based messaging** — Agents exchange reports and context via YAML files, no external services required
- **YAML configuration** — Customize expert names, roles, colors, and timeouts to match your workflow

## Prerequisites

- [tmux](https://github.com/tmux/tmux) (session management)
- [Claude CLI](https://docs.anthropic.com/en/docs/claude-code) (AI agent runtime)

## Installation

### From source

```sh
cargo install --path .
```

### With cargo

```sh
cargo install macot
```

<!--
### Homebrew (coming soon)

```sh
brew install macot
```
-->

## Quick Start

```sh
# 1. Start a session in your project directory
macot start

# 2. Launch the control tower UI
macot tower

# 3. Select an expert, type a task, and press Enter
```

That's it. Four Claude agents are now working on your project. Use the TUI to assign tasks and watch results come in.

## Usage

```
macot <COMMAND>

Commands:
  start      Initialize expert session with Claude agents
  tower      Launch the control tower UI
  status     Display current session status
  sessions   List all running macot sessions
  down       Gracefully shut down expert session
  reset      Reset expert context and instructions
```

### `macot start [project_path]`

Spin up a tmux session with Claude agents.

```sh
# Current directory, default 4 experts
macot start

# Specific project with 6 experts
macot start /path/to/project -n 6

# Custom config
macot start -c ./my-config.yaml
```

### `macot tower [session_name]`

Open the TUI dashboard. Requires a running session.

| Key | Action |
|-----|--------|
| `Tab` | Cycle focus between panels |
| `Enter` | Send task to selected expert |
| `?` | Toggle help |
| `q` / `Ctrl+C` | Quit |

### `macot status`

Print expert states without entering the TUI.

```
Session: macot-a1b2c3d4 (running)
Project: /Users/you/myproject
Experts:
  [0] Alyosha (architect)  - idle
  [1] Ilyusha (frontend)   - in_progress
  [2] Grigory (backend)    - done
  [3] Katya   (tester)     - idle
```

### `macot down`

Gracefully stop all agents and destroy the tmux session.

## Configuration

Default config location: `~/.config/macot/config.yaml`

```yaml
session_prefix: "macot"

experts:
  - name: "Alyosha"
    color: "red"
    role: "architect"
  - name: "Ilyusha"
    color: "blue"
    role: "frontend"
  - name: "Grigory"
    color: "green"
    role: "backend"
  - name: "Katya"
    color: "yellow"
    role: "tester"

timeouts:
  agent_ready: 30
  task_completion: 600
```

### Custom roles

Place Markdown instruction files in the `instructions/` directory of your project. Each file defines a role's responsibilities and output format. macot ships with built-in roles: `architect`, `frontend`, `backend`, `tester`, `planner`, and `general`.

## Architecture

```
┌─────────────────────────────────────────────────┐
│              Control Tower (TUI)                │
│                                                 │
│  ┌────────────┐  ┌────────────┐  ┌───────────┐  │
│  │ Task Queue │  │  Status    │  │  Report   │  │
│  │ Management │  │  Monitor   │  │ Collector │  │
│  └────────────┘  └────────────┘  └───────────┘  │
└──────────┬──────────────┬──────────────▲────────┘
           │ assign       │ monitor      │ report
           ▼              ▼              │
┌─────────────────────────────────────────────────┐
│           tmux Session (macot-{hash})           │
│  ┌─────────┐ ┌─────────┐ ┌─────────┐ ┌────────┐ │
│  │Expert 0 │ │Expert 1 │ │Expert 2 │ │Expert N│ │
│  │(Claude) │ │(Claude) │ │(Claude) │ │(Claude)│ │
│  └─────────┘ └─────────┘ └─────────┘ └────────┘ │
└──────────────────────┬──────────────────────────┘
                       ▼
              ┌────────────────┐
              │   Codebase     │
              └────────────────┘
```

## Contributing

Contributions are welcome. This project follows standard Rust conventions.

```sh
# Build
make build

# Run tests
make test

# Lint
make lint

# Format
make fmt
```

Please open an issue before submitting large changes to discuss the approach.

## License

[MIT](LICENSE)
