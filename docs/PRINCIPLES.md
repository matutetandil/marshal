# Design Principles

> **Read this before making any design or implementation decision.**

This document is intentionally short. It contains the axioms that govern every design choice in this project. When in doubt, come back here.

## The Thesis

> **Looks like a monorepo for the developer, is multi-repo administered underneath, with granular scope.**

The developer experience mirrors working in a monorepo. The storage reality is N independent Git repositories coordinated by the tool. Features that fight this thesis don't belong in the project.

## The Governing Principle

> **Git Recursive: the workspace is Git applied one level up.**

Every concept, operation, and edge-case handling in the workspace has a direct analogue in Git. When designing anything new, the question is: *"How does Git handle the equivalent at the repo level?"* — and then apply the same approach at the workspace level.

This applies to:
- **Form**: working/stage/commit, branches, log, diff, status all exist at workspace level with the same semantics one level up.
- **Strategy**: how we handle impossible states, manual edits, partial failures, and conflicts mirrors how Git handles them.

If something we're designing has no natural Git analogue, suspect it's wrong.

## The Nine Invariants

Every feature, command, and behavior must satisfy these. A proposed feature that violates any invariant is rejected or redesigned.

### 1. Repo Independence

Every child repo in a workspace is a valid, clonable, usable Git repository without the tool. If the workspace disappears, the repos survive intact. A repo can belong to multiple workspaces simultaneously.

### 2. Reversibility to Pure Git

Every workspace operation must be expressible as a sequence of standard Git commands. The `--explain` flag shows this translation. If an operation cannot be explained in pure Git terms, it doesn't belong.

### 3. Ecosystem Orthogonality

GitHub, GitLab, IDEs, CI pipelines, Git hooks — all continue to work without knowing the workspace exists. The workspace is an invisible coordination layer for external tools.

### 4. Manifest as Source of Truth

The workspace state is derived from (a) the manifest in the workspace repo and (b) the actual state of the child repos read via Git. There is no hidden state that cannot be reconstructed from these two sources.

### 5. Partial Failure is Acceptable

A workspace operation can complete partially if that makes sense. The tool executes what can be executed, reports what couldn't, and exits with a meaningful status. It never aborts mid-operation leaving things half-done — except when explicitly marked atomic.

### 6. Explainable Operations

Every operation responds to `--explain` by showing its plan — the exact Git commands that would be executed — before executing. No magic, no hidden behavior.

### 7. Sync via Git Mechanisms Only

All synchronization between developers happens through existing Git mechanisms (push/pull of the workspace repo, push/pull of child repos). No custom protocols, no daemons, no external coordination services.

### 8. Conservative Defaults

The default behavior is always the most conservative option. Actions that modify state beyond what the user explicitly requested require opt-in flags with descriptive names. Pre-flight checks validate before execution; failed preconditions abort cleanly with actionable error messages.

### 9. Developer Flow Is Preserved

The individual developer's daily Git flow does not change. Workspace capabilities are additive and optional. A developer can work productively without ever invoking a workspace-specific command. Workspace features exist for those who need them, not as a mandatory ritual.

## Applying This Document

When reviewing code or design:

- Does this change respect all nine invariants? If not, stop.
- Does this follow the Git Recursive principle? If not, justify or rethink.
- Does this strengthen the thesis (monorepo feel, multi-repo reality) or dilute it?

When a tension seems to exist between invariants, check `docs/ARCHITECTURE.md` — most apparent tensions are resolved there by distinguishing phases (pre-flight vs execution), optionality (feature vs ritual), or domains (form vs strategy).

When a genuinely new case arises that the framework doesn't cover: resist the urge to improvise. Return here, return to architecture, derive the answer from first principles. If the answer requires modifying the framework, that's a significant event — document it explicitly.
