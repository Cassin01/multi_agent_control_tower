# Contributing to macot

Thanks for contributing to `macot`.

## 5-minute setup

```bash
git clone https://github.com/Cassin01/multi_agent_control_tower.git
cd multi_agent_control_tower
cargo build
make test
```

Optional full local checks:

```bash
make ci
```

## How to pick work

Prioritize issues labeled:

- `good first issue`: newcomer friendly, narrow scope
- `help wanted`: maintainer-confirmed external contribution target
- `docs`: documentation and onboarding improvements

If your change is large, open an issue first to align on scope.

## Development workflow

1. Fork the repo and create a branch.
2. Keep PRs small and focused.
3. Add or update tests for behavior changes.
4. Update docs when command behavior or UX changes.
5. Run `make ci` before opening PR.

## Pull request checklist

- [ ] Change is scoped and reviewable
- [ ] Tests were added or updated when needed
- [ ] `make ci` passes locally
- [ ] README/docs updated if user-facing behavior changed
- [ ] Screenshots/GIF added for TUI UX changes

## Definition of done

A contribution is done when:

- the behavior is correct and tested,
- docs match the implemented behavior,
- and the PR description includes validation steps.

## Communication expectations

- Be precise and concrete in issues/PRs.
- Include reproducible steps for bugs.
- Prefer actionable suggestions over broad requests.

## Good first issue ideas

- README: add finalized `assets/demo-quickstart.gif` and screenshot captions
- docs: expand `doc/troubleshooting.md` with known tmux edge cases
- UX: improve `macot status` formatting for narrow terminals
- docs: add multi-session workflow examples to `doc/cli.md`
- CI: add docs link checker to CI workflow
- TUI: show clearer empty-state hint when no reports exist
- config: warn on unknown YAML keys with actionable messages
- contrib: refine PR template with before/after examples for UX changes
