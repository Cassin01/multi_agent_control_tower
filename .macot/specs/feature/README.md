# Feature Specs

Per-feature artifact directory for the MACOT multi-expert pipeline.

## Directory Structure

```
.macot/specs/feature/{feature-name}/
├── requirements.md   # Input: user stories, acceptance criteria
├── design.md         # Output: architect's design document
└── plan.md           # Output: planner's implementation plan
```

## Workflow

1. Control tower (or user) creates `requirements.md` for a new feature
2. Architect expert reads requirements, produces `design.md`
3. Planner expert reads requirements + design, produces `plan.md`
4. Implementer experts execute tasks from `plan.md`

## Naming Convention

Feature directories use kebab-case: `user-authentication`, `api-rate-limiting`, etc.
