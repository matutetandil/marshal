# Contributing

Thanks for your interest. Read this before opening PRs or filing issues.

## Before Anything Else

1. Read `docs/PRINCIPLES.md`. The nine invariants govern everything.
2. Read `docs/ARCHITECTURE.md`. Understand the conceptual model.
3. Read `docs/GLOSSARY.md` so we're using the same terms.

## Design Changes vs. Implementation Changes

Not all contributions are equal:

**Implementation changes** (bug fixes, performance, tests, refactoring) are welcome as PRs directly. Make sure tests pass and clippy is happy.

**Design changes** (new commands, new behavior, new concepts) require discussion first. Open an issue with:
- What problem the change addresses.
- How it satisfies the nine invariants.
- What the Git analogue is (the Git Recursive principle).
- How it would be documented.

Design changes without this discussion will likely be declined regardless of code quality. The architecture is more important than any individual feature.

## Code Quality

- `cargo fmt` before every commit.
- `cargo clippy -- -D warnings` must pass.
- Every new function has a doc comment explaining *why*, not *what*.
- New commands include integration tests.
- New concepts are added to `docs/GLOSSARY.md`.

## Commit Messages

Follow Conventional Commits. Examples:

- `feat(workspace): add ws stage command`
- `fix(scope): handle empty manifest correctly`
- `docs: clarify invariant 5 in PRINCIPLES.md`
- `test(status): add regression for divergence display`

The scope should be a module name (`workspace`, `scope`, `cli`, etc.) or a command name.

## Pull Request Checklist

Before marking a PR ready for review:

- [ ] Tests pass on my machine (`cargo test`)
- [ ] `cargo fmt` applied
- [ ] `cargo clippy` clean
- [ ] New concepts documented in glossary
- [ ] Relevant sections of `ARCHITECTURE.md` updated if needed
- [ ] The change respects all nine invariants
- [ ] The change follows the Git Recursive principle

## What Gets Rejected

- Features that modify state without explicit user request (violates Invariant 8).
- Features requiring a background daemon or watcher (violates the no-infrastructure rule).
- Features that make plain Git repos behave differently (violates Invariant 9).
- "Improvements" that diverge from Git's conventions when passing through.
- Code that doesn't follow the module structure of the project.

## Questions?

Open a discussion (not an issue) for design questions. Issues are for bugs and concrete feature proposals.
