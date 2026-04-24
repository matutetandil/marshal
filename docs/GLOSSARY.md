# Glossary

Terminology used throughout the project. When in doubt about what a term means, come here.

## Core Concepts

**Workspace** — A coordinated set of Git repositories presented as a unified development environment. Consists of a workspace repo (containing manifest and state) and one or more child repos. The workspace is an organizational layer; it does not contain the repos in any Git sense.

**Workspace Repo** — The Git repository at the root of a workspace. Contains workspace-level files (docs, configs), the manifest, and state declarations. Has branches, history, and a remote like any Git repo.

**Child Repo** — An independent Git repository that participates in a workspace. Lives inside the workspace directory (conventionally under `src/`). Unaware of the workspace; can be used independently with plain Git.

**Manifest** — Declarative description of the workspace structure. Lives at `.workspace/manifest.toml`. Versioned in the workspace repo. Lists child repos, their URLs, and their affinities.

**State Declaration** — Per-branch description of what each child repo should be on when that workspace branch is active. Lives at `.workspace/state.toml`. Versioned in the workspace repo.

**Workspace Branch** — A branch of the workspace repo. Each workspace branch has its own `state.toml`. Checking out a workspace branch triggers child repos to move to their declared states.

**Affinity** — A declared relationship between child repos in the manifest. Can be a dependency (`depends_on`), a group membership, a synchronization constraint, or a priority tier.

## State Model

**Declared State** — What `state.toml` says each child repo should be on for the current workspace branch. Versioned intent.

**Actual State** — What each child repo currently has checked out. Read via plain Git.

**Divergence** — A difference between declared and actual state. Not an error; information about local work that hasn't been reflected in workspace intent.

**Working State** — The aggregate of all current divergences. Analogous to Git's working directory.

**Staging** — Divergences explicitly marked for inclusion in the next workspace commit. Lives in `.workspace/local/staged.toml`. Analogous to Git's index.

**Workspace Commit** — A commit in the workspace repo that materializes `state.toml` with staged divergences absorbed. Analogous to a Git commit.

## Operations

**Scope** — The set of child repos an operation applies to. Inferred from context unless explicitly declared with `--on`.

**Scope Policy** — The default scope inference rules for a specific operation. Declared as part of the operation's design.

**Pre-flight Check** — Verification phase before executing an operation. Validates that preconditions are met across all affected repos. Aborts the operation cleanly if any repo fails validation.

**Workspace-Aware Command** — A command whose behavior adjusts based on workspace context. Follows the detect → pre-flight → execute → report cycle.

**Pass-through Command** — A command without workspace semantics that is forwarded to Git unchanged. Logged but not modified.

## Scope Dimensions

**Spatial Scope** — Where the developer's current directory is. Inside a child repo implies that repo.

**Material Scope** — Which repos have actual file modifications.

**Temporal Scope** — What the current workspace branch declares.

**Structural Scope** — Affinities from the manifest (e.g., dependencies).

**Declared Scope** — Explicit `--on` targeting. Overrides inference.

## Related but Distinct

**Wrapper** — The tool itself, acting as a transparent layer over Git via alias.

**Context Detection** — The process of determining whether the current directory is inside a workspace. Walks up the filesystem looking for `.workspace/`.

**Operation Log (oplog)** — Local record of every operation the tool has processed. Used for the "undo" feature. Analogous to Git's reflog.

**Explain Mode** — `--explain` flag that shows the operation plan (which Git commands would execute) before executing.

## Anti-Terms

These terms are **not** used in this project, because they carry confusing connotations from related but different systems:

- **"Submodule"** — We don't have submodules. Child repos are independent, not embedded.
- **"Subtree"** — We don't merge histories. Repos remain independent.
- **"Monorepo"** — The workspace is not a monorepo. It feels like one, but the reality is multi-repo.
- **"Meta-repo"** — Ambiguous. Use "workspace repo" specifically.
- **"Super-project"** — Term from submodules; doesn't apply.
