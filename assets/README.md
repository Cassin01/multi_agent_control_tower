# Assets

This directory contains repository presentation assets.

- `demo-quickstart.gif`: primary README hero demo (6-8 second loop)
- `demo-quickstart.tape`: VHS source used to regenerate the demo GIF

## Capture guidance

1. Show `macot start` -> `macot tower` -> task dispatch -> report visibility.
2. Keep terminal text readable on mobile-sized previews.
3. Favor short loops with clear state transitions.

## Regenerate

Prerequisites:

- `vhs`
- `tmux`
- `ffmpeg`
- `claude` (authenticated)
- `macot` on `PATH`

Commands:

```bash
make demo-gif-validate
make demo-gif
```

Acceptance check:

1. GIF shows `macot start`, `macot tower`, and `macot status`.
2. Task assignment action is visible (`Ctrl+S`).
3. Report/state transition is visible in the tower view.
