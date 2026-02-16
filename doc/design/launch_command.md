# `macot launch` Command Design

## Overview

`macot launch` combines `macot start` and `macot tower` into a single command. It initializes the expert session and immediately opens the control tower TUI, without requiring the user to run two separate commands.

## Requirements

1. `macot launch` accepts the same arguments as `macot start`
2. Session initialization (tmux session, queue, context store, metadata) runs synchronously
3. Expert agent launching runs asynchronously in the background
4. The tower TUI starts immediately after session infrastructure is ready
5. The TUI displays expert startup progress in real-time (experts transition from "pending" to "ready")

## Current Workflow (Before)

```
$ macot start          # blocks until all experts are ready (~30s)
$ macot tower          # separate command to open TUI
```

## Proposed Workflow (After)

```
$ macot launch         # initializes session + opens TUI immediately
```

## Architecture

### Command Arguments

Identical to `start::Args`:

```rust
#[derive(ClapArgs)]
pub struct Args {
    /// Path to project directory (default: current directory)
    #[arg(default_value = ".")]
    pub project_path: PathBuf,

    /// Number of experts (overrides config)
    #[arg(short = 'n', long)]
    pub num_experts: Option<u32>,

    /// Custom config file path
    #[arg(short, long)]
    pub config: Option<PathBuf>,
}
```

### Execution Flow

```
macot launch
│
├── 1. Resolve project path
├── 2. Load config (apply --num-experts, --config)
├── 3. Validate no existing session
│
├── 4. Initialize session infrastructure (synchronous)
│   ├── QueueManager::init()
│   ├── ExpertStateDetector: set all markers to "pending"
│   ├── ContextStore::init_session()
│   ├── TmuxManager::create_session()
│   └── TmuxManager::init_session_metadata()
│
├── 5. Spawn expert launch task (async, background)
│   └── tokio::spawn → for each expert:
│       ├── set_pane_title()
│       ├── launch_claude()
│       └── wait_for_ready()  (runs in background, not blocking TUI)
│
└── 6. Launch tower TUI (foreground)
    ├── WorktreeManager::resolve()
    └── TowerApp::new(config).run()
```

### Key Design Decision: Separation Point

The `start` logic splits into two phases:

| Phase | Operations | Blocking |
|-------|-----------|----------|
| **Infrastructure** (steps 1-4) | session creation, queue, metadata | Yes (required before TUI) |
| **Expert launch** (step 5) | Claude CLI spawn + readiness wait | No (async background) |

The tower TUI already polls expert status via `ExpertStateDetector`, so experts transitioning from "pending" to "ready" is reflected in the UI automatically.

### CLI Registration

```rust
// cli.rs
#[derive(Subcommand)]
pub enum Commands {
    Start(start::Args),
    Down(down::Args),
    Tower(tower::Args),
    /// Initialize expert session and launch the control tower UI
    Launch(launch::Args),
    Status(status::Args),
    Sessions,
    Reset(reset::Args),
}
```

```rust
// main.rs
Commands::Launch(args) => commands::launch::execute(args).await,
```

### Implementation Skeleton

```rust
// src/commands/launch.rs
pub async fn execute(args: Args) -> Result<()> {
    // Phase 1: Infrastructure (reuse start logic)
    let project_path = args.project_path.canonicalize()?;
    let mut config = Config::load(args.config)?.with_project_path(project_path.clone());
    if let Some(n) = args.num_experts {
        config = config.with_num_experts(n);
    }

    let tmux = TmuxManager::from_config(&config);
    if tmux.session_exists().await {
        bail!("Session {} already exists. Run 'macot down' first.", config.session_name());
    }

    // Initialize queue, status markers, context store, tmux session, metadata
    // (same as start::execute steps)
    init_session_infrastructure(&config, &tmux, &project_path).await?;

    // Phase 2: Spawn expert launch (background)
    let claude = ClaudeManager::new(config.session_name());
    spawn_experts_async(&config, &tmux, &claude, &project_path);

    // Phase 3: Launch tower TUI (foreground)
    let worktree_manager = WorktreeManager::resolve(project_path).await?;
    let mut app = TowerApp::new(config, worktree_manager);
    app.run().await?;

    Ok(())
}
```

### Code Reuse Strategy

Extract shared infrastructure initialization from `start::execute` into a reusable function:

```
src/commands/
├── start.rs        → calls init_session_infrastructure() + wait for experts
├── launch.rs       → calls init_session_infrastructure() + spawn experts async + tower
└── common.rs       → init_session_infrastructure() (extracted)
```

Alternatively, `launch.rs` can directly call the initialization steps inline. The choice depends on whether `start` and `launch` diverge in future.

**Recommended approach**: Extract `init_session_infrastructure()` and `spawn_expert_tasks()` into `common.rs` to avoid duplication between `start` and `launch`.

## File Changes

| File | Change |
|------|--------|
| `src/commands/launch.rs` | New file: launch command implementation |
| `src/commands/mod.rs` | Add `pub mod launch;` |
| `src/cli.rs` | Add `Launch(launch::Args)` variant |
| `src/main.rs` | Add `Commands::Launch` match arm |
| `src/commands/common.rs` | Extract shared session init logic |
| `src/commands/start.rs` | Refactor to use extracted common logic |

## Error Handling

- Infrastructure initialization failures (phase 1-4) abort before TUI starts, with a clear error message
- Expert launch failures (phase 5) are non-fatal; the TUI displays expert status in real-time, and the user can observe and retry from within the TUI
- If the TUI exits (user quits), the background expert launch tasks are dropped (experts already running in tmux continue independently)

## Edge Cases

| Case | Behavior |
|------|----------|
| Session already exists | Bail with "Run 'macot down' first" (same as `start`) |
| TUI exits before experts ready | Experts continue launching in tmux; user can `macot tower` to reconnect |
| Expert launch fails | TUI shows expert as stuck in "pending"; user can reset via TUI |
| `--config` invalid path | Bail before any session creation |
