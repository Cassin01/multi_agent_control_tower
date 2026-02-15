# CLAUDE.md

## Build Commands

* `make build`      : Compile source files (release)
* `make test`       : Run test suite
* `make lint`       : Run clippy (`-D warnings`)
* `make fmt-check`  : Verify formatting
* `make ci`         : Run build/lint/format/test checks
* `make clean`      : Remove generated files

## Project Structure & Rules

* **Documentation**: `doc/`

## Test-Driven Development (TDD)

**Core Philosophy**: Tests are the specification. Correctness is ensured by writing tests that describe expected behavior *before* implementation.

### Red-Green-Refactor Cycle

1. **RED** (Failing Test): Write a test describing the expected behavior. Run it to confirm it fails.
2. **GREEN** (Minimal Code): Write just enough code to make the test pass.
3. **REFACTOR** (Improve): Clean up and optimize the code while keeping tests green.

**Critical Rule**: Never skip steps. Never write implementation code without a failing test first.

### Workflow

* **Adding a New Feature**:
1. Create or open the corresponding test file.
2. Write a test for the new behavior (must fail).
3. Run `make test` to confirm the expected error.
4. Implement minimal code in the source file.
5. Run `make test` to confirm passage.
6. Refactor if needed.


* **Fixing a Bug**:
1. Write a test that reproduces the bug (must fail/crash).
2. Fix the bug in the source.
3. Run `make test` to confirm the fix and ensure no regressions.


* **Refactoring**:
1. Ensure all existing tests pass.
2. Refactor code.
3. Run `make test` to ensure all tests still pass.



## Testing Guidelines

* **Assertion Pattern**: Use assertions that compare `actual` vs `expected` with a clear message format: `"function-name: concise description of expectation"`.
* **Scope**: Test **observable behavior** (return values, side effects) rather than internal state or private variables.
* **Mocking Strategy**:
* Mock external dependencies and environment APIs *before* requiring the module under test.
* Use mocks to simulate edge cases and inputs without relying on the real environment.


* **Coverage Checklist**:
* All public functions have tests.
* Edge cases tested (nil, empty input, invalid input).
* Error conditions handled and tested.



## Anti-Patterns

* **DO NOT** write code before tests.
* **DO NOT** require/load the module under test before setting up its mocks (dependencies must be mocked first).
* **DO NOT** test implementation details (e.g., internal counters); test the public API results instead.

## Git Conventions

* **Commit Messages**: English
* **PR Titles/Descriptions**: English
