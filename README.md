<div align="center">

# macot

[![CI](https://github.com/Cassin01/multi_agent_control_tower/actions/workflows/ci.yml/badge.svg)](https://github.com/Cassin01/multi_agent_control_tower/actions/workflows/ci.yml)
[![crates.io](https://img.shields.io/crates/v/macot.svg)](https://crates.io/crates/macot)
[![docs.rs](https://img.shields.io/docsrs/macot)](https://docs.rs/macot)
[![License](https://img.shields.io/badge/license-Apache--2.0-green.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.74%2B-orange.svg)](https://www.rust-lang.org/)

**Control Tower for parallel Claude workflows in your terminal**

Coordinate multiple role-based coding agents on one codebase with a Rust-native CLI + TUI.

![demo](assets/demo-quickstart.gif)

[Features](#-features) •
[Quick Start](#-quick-start) •
[Installation](#-installation) •
[Configuration](#-configuration) •
[Builtin Roles](#-builtin-roles) •
[Commands](#-commands) •
[FAQ](#-faq) •
[Contributing](#-contributing)

</div>

---

## ✨ Features

- **⚡ Parallel orchestration**: run multiple experts concurrently in isolated tmux panes.
- **🧠 Role-based execution**: assign experts as `architect`, `planner`, `backend`, `frontend`, `debugger`, or `general`.
- **🤖 Automated feature execution**: run task batches from spec files automatically.
- **📨 Async inter-expert messaging**: deliver queued messages to available experts.
- **🖥️ Control Tower TUI**: dispatch tasks, monitor status, and review reports in one screen.
- **🌲 Worktree-friendly flow**: reduce branch conflicts with isolated workspaces per expert.
- **🧩 Configurable by design**: tune experts, roles, timeouts, and paths via YAML.
- **🔒 Local-first runtime**: queue and context live locally with no external coordinator service.

## 🚀 Quick Start

Install, launch, and verify in under a minute.

```bash
# 0) Prerequisites: rust + tmux + Claude CLI
cargo install macot

# 1) Launch session + TUI in one step
macot launch
```

Or, if you prefer separate steps:

```bash
# 1) Start a session in your current project
macot start .

# 2) Open the control tower
macot tower

# 3) Verify from another terminal
macot status
```

Success looks like:

- session name appears as `macot-<hash>`
- at least one expert moves to `Thinking` or `Executing`
- a report appears in the tower report list

## 📦 Installation

Use crates.io for the fastest setup.

```bash
cargo install macot
```

<details>
<summary><b>Install from source</b></summary>

```bash
git clone https://github.com/Cassin01/multi_agent_control_tower.git
cd multi_agent_control_tower
cargo install --path .
```

</details>

<details>
<summary><b>Homebrew / prebuilt binaries</b></summary>

Homebrew formula and release binaries are planned but not published yet.

</details>

## ⚙️ Configuration

Use YAML config when you need custom experts, roles, or runtime timeouts.

```yaml
experts:
  - name: Linda
    role: architect
  - name: James
    role: planner
  - name: John
    role: general
  - name: Sarah
    role: debugger

runtime:
  startup_timeout_seconds: 30
  graceful_shutdown_timeout_seconds: 10

paths:
  instructions_dir: ./instructions
```

```bash
macot start -c ./config/macot.yaml
macot tower --config ./config/macot.yaml
```

See full reference: [`doc/configuration.md`](doc/configuration.md)

## 🎭 Builtin Roles

Roles are assigned to experts in your YAML config. Each role injects a tailored system prompt that shapes the expert's behavior.

| Role | Description |
|------|-------------|
| 🏛️ `architect` | System design, code structure, and technical decision-making. Produces design documents (`*-design.md`) for downstream experts. |
| 📝 `planner` | Task decomposition and implementation planning. Breaks requirements into structured, incremental task lists (`*-tasks.md`). |
| ⚙️ `backend` | Server-side development, APIs, databases, and data management. |
| 🎨 `frontend` | User interface development, UX, and client-side implementation. |
| 🔍 `debugger` | Investigation, root cause analysis, and diagnostic reporting for failures. Does not implement fixes — delegates to other experts. |
| 🧩 `general` | General-purpose problem-solving. Default fallback when no specific role is assigned. |

Custom roles can be added by placing a `<role-name>.md` file in the instructions directory (default: `~/.config/macot/instructions/`, overridable via `paths.instructions_dir`).

## 📋 Commands

Core command surface:

| Command | Purpose |
|---|---|
| `macot start [project_path]` | Initialize a session and launch experts |
| `macot tower [session_name]` | Open the control tower UI |
| `macot launch [project_path]` | Start a session and open the control tower in one step |
| `macot status [session_name]` | Print live session and expert status |
| `macot sessions` | List running `macot-*` sessions |
| `macot down [session_name]` | Stop a session gracefully or forcefully |
| `macot reset expert <id\|name>` | Reset one expert context/runtime |

More examples and TUI keybindings: [`doc/cli.md`](doc/cli.md)

## ❓ FAQ

### Where are session artifacts stored?

Inside your project at `.macot/`.

### How do agents communicate with each other?

Use the built-in async messaging queue. Each expert can send messages through a dedicated messaging subagent, and those messages are created as YAML files in `.macot/messages/outbox/` and routed automatically to matching experts (by id, name, or role) when recipients are idle, so a `debugger` expert can report a root cause and delegate the fix to a `backend` expert.

### How do I run tasks automatically from a spec?

In the tower Task Input, enter a feature name (for example `auth-refactor`) and press `Ctrl+G`.
`<feature>-tasks.md` is created by the `planner` expert, and `<feature>-design.md` is created by the `architect` expert.  
macot will execute tasks from `.macot/specs/<feature>-tasks.md` in batches (and also references `.macot/specs/<feature>-design.md` when present).

## 🤝 Contributing

Contributions are welcome. Contribution flow, issue templates, and PR checklist are documented in [`CONTRIBUTING.md`](CONTRIBUTING.md).

## ☕ Support

Support this project: https://buymeacoffee.com/Cassin01

## 📄 License

[Apache-2.0](LICENSE) © Cassin01
