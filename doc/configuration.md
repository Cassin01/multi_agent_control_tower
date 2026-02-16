# Configuration Guide

`macot` supports YAML-based configuration passed via `--config`.

```bash
macot start -c ./config/macot.yaml
macot tower --config ./config/macot.yaml
```

## Example config

```yaml
experts:
  - name: architect
    role: architect
  - name: backend
    role: backend
  - name: frontend
    role: frontend
  - name: tester
    role: tester

runtime:
  startup_timeout_seconds: 30
  graceful_shutdown_timeout_seconds: 10

paths:
  instructions_dir: ./instructions
```

## Guidance

- Keep expert names stable for predictable task routing.
- Keep startup/shutdown timeouts realistic for your machine.
- Store custom instruction files in a versioned directory.
