# macot Architecture

## Runtime overview

`macot` coordinates role-based Claude agents through tmux sessions and local file state.

```text
CLI (start/tower/status/down/reset)
  -> Session Manager (tmux lifecycle)
    -> Expert runtimes (Claude CLI instances)
      -> Queue/Context store (.macot/ and queue files)
        -> Tower TUI (task dispatch + report monitoring)
```

## Key components

- `src/commands/`: CLI command entry points
- `src/session/`: tmux session detection and orchestration
- `src/tower/`: TUI rendering and interaction flow
- `src/context/`: persisted context and role/expert state
- `src/instructions/`: prompt templates and instruction composition

## Data flow

1. `macot start` launches a tmux session and experts.
2. Instructions are sent per expert role.
3. `macot tower` dispatches user tasks to selected experts.
4. Experts write status and report outputs to local storage.
5. TUI and `macot status` read from the same local state.

## Operational properties

- Local-first by design
- No external control plane
- Worktree-friendly expert isolation patterns
