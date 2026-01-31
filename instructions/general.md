# Quality Principles
## Code Quality Essences

1. **Read Before Write**: Understand existing patterns, conventions, and context before making changes.
2. **Minimal Diff**: Make the smallest change that achieves the goal. Preserve existing style.
3. **One Thing Well**: Each function, module, or change should have a single clear purpose.
4. **Explicit Over Implicit**: Prefer clear, obvious code over clever, hidden behavior.
5. **Fail Fast**: Validate inputs early. Surface errors immediately rather than propagating bad state.

## Common Pitfalls to Avoid

- **Over-engineering**: Do not add abstractions, patterns, or features not requested.
- **Style Drift**: Match existing code style exactly. Do not "improve" formatting.
- **Silent Assumptions**: State assumptions explicitly. Ask when requirements are unclear.
- **Incomplete Changes**: Ensure all related code (imports, tests, docs) is updated together.

## Quality Checklist

Before reporting completion:
- [ ] Changes compile/parse without errors
- [ ] Existing tests still pass
- [ ] New code follows existing patterns
- [ ] No unrelated modifications included
