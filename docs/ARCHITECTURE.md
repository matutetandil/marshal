# Architecture

> The complete conceptual design of the workspace system. Read [`PRINCIPLES.md`](PRINCIPLES.md) first.

## Table of Contents

1. [The Problem](#the-problem)
2. [The Thesis](#the-thesis)
3. [Adoption Strategy](#adoption-strategy)
4. [Workspace Architecture](#workspace-architecture)
5. [State Model](#state-model)
6. [The Three Zones](#the-three-zones)
7. [Scope Inference](#scope-inference)
8. [Operation Semantics](#operation-semantics)
9. [Edge Cases and Their Handling](#edge-cases-and-their-handling)
10. [The Git Recursive Principle](#the-git-recursive-principle)

---

## The Problem

Git handles single repositories excellently. It handles coordinating multiple related repositories poorly. The two native mechanisms (submodules and subtrees) both have well-known usability problems. The ecosystem's response has been to push toward monorepos.

But monorepo adoption is often driven by tooling limitations rather than architectural preferences. Teams choose monorepos because coordinating multi-repo setups with Git is painful — not because their architecture is naturally monolithic. This is the same pattern as choosing a mainframe in 2005 because distributed computing was hard: a tooling-driven architectural choice that becomes obsolete when tooling catches up.

Companies operating at scale (Google with Piper, Meta with Sapling) have built custom version control systems because Git genuinely cannot handle their needs. But their solution — replace Git — is inaccessible to teams that depend on the Git ecosystem (GitHub, GitLab, existing CI, hooks, clients).

There is a gap: a coordination layer over Git that enables multi-repo architectures without requiring VCS replacement.

## The Thesis

> **Looks like a monorepo for the developer, is multi-repo administered underneath, with granular scope.**

Three claims in one sentence:

**"Looks like a monorepo"** — The daily developer experience should feel like working in a single unified repository. Switch branches, commit changes, push, pull, resolve conflicts — these operations should have the ergonomics of a monorepo even though underneath they operate on multiple repos.

**"Is multi-repo administered"** — The underlying storage remains independent repositories. Each repo has its own history, remote, CI, ownership, and release cycle. They are coordinated by the tool, not merged.

**"With granular scope"** — A feature branch exists only in the repos actually involved in that feature. A bug fix in `service-a` does not pollute the history of `service-b`. The scope of any change reflects the natural scope of the work.

The analogy is with microservices: an application feels monolithic to its users while being a distributed system underneath. Kubernetes made this viable by providing the coordination layer. This tool aims to do the equivalent for version control.

## Adoption Strategy

### Wrapper via Alias

The tool is installed as a standalone binary. Users configure `alias git=<tool>` (or use a symlink/shim named `git` in PATH). From that point:

- Every invocation of `git` passes through the wrapper first.
- The wrapper inspects the command and the filesystem context.
- If the context is not a workspace, the command is passed transparently to Git. Zero behavioral change.
- If the context is a workspace, the wrapper activates workspace coordination logic.

This approach is modeled on Podman's relationship with Docker: same CLI, different underlying behavior when beneficial, complete compatibility when not.

### Context Detection

On every invocation, the tool walks up the filesystem from the current directory looking for a `.workspace/` directory (or equivalent marker). If found, it establishes a workspace context: the workspace root, the manifest, and which child repo (if any) the current directory belongs to.

This is identical to how Git finds the `.git/` directory of the current repo. The developer never declares context; it is inferred from location.

### Zero Lock-in

Uninstalling the tool leaves:
- All child repos as ordinary Git repos, usable with any Git client.
- The workspace repo as an ordinary Git repo containing some TOML files (the manifest and state descriptions).
- No modifications to any `.git/` directory, no custom object types, no extended refs.

A workspace is always and only an organizational layer. It adds information without modifying fundamentals.

## Workspace Architecture

### Physical Structure

```
my-workspace/                    ← workspace repo (ordinary Git repo)
├── .workspace/
│   ├── manifest.toml            ← structural: which repos compose the workspace
│   ├── state.toml               ← operational: declared state for current branch
│   └── local/                   ← gitignored local state per developer
│       ├── staged.toml          ← workspace staging area
│       └── oplog.db             ← operation log
├── README.md, Dockerfile, docs/ ← workspace-level content, versioned normally
└── src/
    ├── service-a/               ← child repo (independent Git repo)
    ├── service-b/               ← child repo (independent Git repo)
    └── shared-lib/              ← child repo (independent Git repo)
```

### The Workspace Repo

The workspace root itself is a Git repository. This repo:

- Contains workspace-level files (documentation, Docker configurations, development tooling) versioned normally.
- Contains `.workspace/manifest.toml` declaring which repos form the workspace and their relationships.
- Contains `.workspace/state.toml` declaring the expected state of each child repo for the currently active branch of the workspace repo.
- Has its own branches, history, and remote. It is pushed/pulled like any Git repo.

Changes to workspace structure (adding a repo, declaring dependencies) are commits in this repo. They go through normal Git flows: branches, pull requests, reviews, merges. Conflicts are ordinary Git conflicts resolved through ordinary Git means.

### Child Repos

Child repos are ordinary Git repositories cloned inside `src/` (by convention). They:

- Have their own remote, history, and CI.
- Know nothing about the workspace. They are unmodified Git repos.
- Can be cloned independently by developers who don't use the tool.
- Can participate in multiple workspaces simultaneously (the workspace → repo relationship is declared externally in the manifest, not embedded in the repo).

### Manifest and State

Two TOML files in `.workspace/`:

**`manifest.toml`** — Structural definition. Changes rarely, with intent, reviewed through PRs.

```toml
[workspace]
name = "my-project"
default_branch = "main"

[[repos]]
name = "service-a"
url = "git@github.com:org/service-a.git"
kind = "service"

[[repos]]
name = "service-b"
url = "git@github.com:org/service-b.git"
kind = "service"

[[repos]]
name = "shared-lib"
url = "git@github.com:org/shared-lib.git"
kind = "library"

[affinities]
"service-a" = { depends_on = ["shared-lib"] }
"service-b" = { depends_on = ["shared-lib"] }
```

**`state.toml`** — Declared state for the current branch of the workspace repo. Different branches have different `state.toml` contents, versioned normally.

```toml
# .workspace/state.toml on branch "feature/new-payment"
[repos."service-a"]
branch = "feat/payment-api"

[repos."service-b"]
branch = "feat/payment-ui"

[repos."shared-lib"]
branch = "main"
```

When checking out a workspace branch, the tool reads the branch's `state.toml` and adjusts each child repo accordingly.

## State Model

### Two Levels of State

**Shared state (versioned in workspace repo):**
- Manifest (structure)
- State declarations per branch (intent)
- Workspace-level content (docs, configs)

Changes via normal Git workflow: PRs, reviews, merges. Distributed via push/pull.

**Local state (per developer, never versioned):**
- Staging area (workspace-level)
- Operation log
- Preferences and caches

Lives in `.workspace/local/`, gitignored. Personal to each developer. Reconstructible from the shared state if lost.

### Declared vs. Actual State

At any moment, there are two descriptions of the workspace state:

**Declared state** — what `state.toml` on the current workspace branch says each repo should be in. This is versioned intent.

**Actual state** — what each child repo currently has checked out (branch, commits, working tree). This is observable via plain Git in each repo.

These two can diverge. Divergence is **information, not error**. Examples:

- Developer fixes a bug in `service-b` by creating a local branch. `state.toml` still declares `service-b` should be on `main`. Divergence exists until the fix is merged or explicitly absorbed.
- Developer manually switches a repo to explore. Divergence is intentional and transient.

The tool's job is to:
1. Detect divergence.
2. Report it clearly.
3. Offer options to resolve it (absorb into declared state, restore to declared state, or leave as-is).
4. Never automatically reconcile without explicit instruction.

## The Three Zones

The workspace has the same three-zone model as Git, applied one level up. This is the clearest expression of the Git Recursive principle.

### Working State

The current actual state of child repos. Every divergence between `state.toml` and reality sits here until staged or resolved.

Analogous to Git's working directory: modifications exist but are not marked for inclusion in the next commit.

### Staging

Divergences explicitly marked for inclusion in the next workspace commit. Lives in `.workspace/local/staged.toml`.

Analogous to Git's index. Developers curate which divergences represent workspace-level intent and which are transient local work.

### Commit (of the workspace repo)

A commit in the workspace repo that materializes `state.toml` with staged divergences absorbed. Part of the workspace repo's history, reviewable, revertible, bisectable.

Analogous to a Git commit. Each workspace commit tells a coherent story about the coordinated state of the system.

### The Cycle

```
  modify child repos  →  working state (divergence)
            ↓
       `ws stage`        →  staging (prepared intent)
            ↓
       `ws commit`       →  workspace commit (recorded intent)
            ↓
       `git push`        →  shared with team
```

Each transition has a Git analogue. `ws stage` ≈ `git add`. `ws unstage` ≈ `git restore --staged`. `ws restore` ≈ `git checkout <file>` (returns to declared state). `ws reset` ≈ `git reset` (clears staging).

### Optional Engagement

Critically, **the three zones are opt-in, not required**. A developer can work indefinitely without ever staging or committing at the workspace level. Their work generates divergences that remain in working state. These divergences:

- Do not block any operations that are compatible with them.
- Are reported by `ws status` but not as errors.
- Resolve naturally when underlying changes are merged upstream in the child repos.
- Can be absorbed into workspace intent later, by the same developer or a different role (tech lead curating a release).

The three zones exist to enable deliberate coordination for those who need it. They do not impose ceremony on those who don't.

## Scope Inference

### The Problem

For the tool to "feel like a monorepo," operations must do the right thing by default without requiring the developer to specify which repos they apply to. In a monorepo, `git commit` commits whatever is staged, wherever it is. The workspace equivalent must infer scope from context.

### The Five Dimensions

Scope is inferred from a combination of signals:

**1. Spatial scope** — where the developer's current directory is. Inside a child repo, operations default to that repo. At the workspace root, operations default to the workspace.

**2. Material scope** — what files are modified. If only `service-a` has changes, `ws commit` commits in `service-a` only.

**3. Temporal scope** — what the active workspace branch declares. Operations may filter or prioritize based on `state.toml`.

**4. Structural scope** — affinities declared in the manifest. A change to `shared-lib` may imply relevance to its dependents.

**5. Declared scope** — explicit `--on <target>` flag. This overrides all inference.

### Scope Policies

Each operation has a declared scope policy. This is part of the design, not implementation detail.

| Operation | Default scope policy | Primary dimensions |
|-----------|---------------------|---------------------|
| `status` | Entire workspace | — |
| `log`, `diff` | Workspace if at root, repo if inside | Spatial |
| `commit` | Repos with staged changes | Material (limited by spatial) |
| `add` | Indicated file/directory | Strict spatial |
| `switch <branch>` | What destination branch declares | Temporal |
| `branch <name>` | Repos with changes, otherwise interactive | Material, fallback interactive |
| `fetch` | All repos | — |
| `pull` | All repos, ordered by affinity | Structural |
| `push` | Repos with unpushed commits on current branch | Material + Temporal |
| `test` | Changed repos + affected dependents | Material + Structural |
| `sync` | Repos with declared-vs-actual divergence | Temporal vs actual |

### The "on" Keyword

Explicit scope targeting uses `--on` (or the keyword `on` as syntactic sugar when unambiguous):

```
ws switch feat/auth --on service-a
ws switch feat/auth on service-a         # sugar equivalent
ws commit -m "msg" --on service-a,service-b
ws push --on kind:service                # by manifest attribute
ws fetch --on "service-*"                # by glob pattern
```

Targeting supports: explicit lists, manifest attributes (`kind:service`), glob patterns, negation (`!repo-legacy`), and state predicates (`dirty`, `ahead`).

### Transparency of Inference

The default report shows inferred scope when non-trivial:

```
$ ws commit -m "add payment flow"
Detected scope: service-a, shared-lib (material + structural)

✓ Committed in service-a
✓ Committed in shared-lib (affected dependency)
```

`--explain` shows the full reasoning:

```
$ ws commit -m "msg" --explain
Scope inference:
  - Current directory: workspace root (no spatial constraint)
  - Modified files in: service-a/src/*, shared-lib/src/*
  - Material scope: {service-a, shared-lib}
  - Structural affinity: service-a depends on shared-lib (coherent)
  - Final scope: {service-a, shared-lib}

Plan:
  1. cd service-a && git commit -m "msg"
  2. cd shared-lib && git commit -m "msg"

Execute? [y/N]
```

## Operation Semantics

### Workspace-Aware Commands

Operations that behave differently inside a workspace context. Their behavior is:

1. **Detect** — walk the filesystem, read manifest and state, infer scope.
2. **Pre-flight** — verify preconditions in all affected repos. If any precondition fails, abort cleanly with actionable error message. No repo is modified.
3. **Execute** — run the operation across affected repos. Report progress.
4. **Report** — show aggregated results, including partial failures.

The pre-flight phase is critical. It guarantees Invariant 8 (conservative defaults): the tool never modifies state without being certain the operation can complete.

### Pass-through Commands

Commands that don't have workspace semantics pass transparently to Git. The wrapper records them in the operation log but does not modify behavior.

This means Git commands the tool doesn't know about automatically work. If Git adds a new subcommand tomorrow, it works immediately without tool updates.

### Explicit Passthrough

Users can bypass workspace logic explicitly:

```
ws --raw git rebase main     # skip workspace coordination, run git directly
```

Useful for debugging or for cases where the developer wants exact Git behavior.

## Edge Cases and Their Handling

The Git Recursive principle extends to edge case handling. When designing how to handle a strange situation, ask: *"How does Git handle the equivalent at the repo level?"* — and apply the same strategy.

### Impossible Declared State

`state.toml` declares `service-a` should be on `feat/X`, but that branch exists nowhere (local or remote).

**Git analogue**: `git switch nonexistent-branch` → error with clear message.

**Workspace behavior**: error at the point of trying to materialize the state. Clear message explaining the branch doesn't exist, listing possible causes (was it deleted? never created? missing fetch?), offering actions (fetch and retry, fix the declaration, accept current state).

### Manual Edits to Workspace State

Someone edits `state.toml` by hand and commits in the workspace repo.

**Git analogue**: someone edits a tracked file by hand and commits it. Git accepts it; if the resulting state is broken (syntax error, invalid reference), subsequent operations will fail and report the problem.

**Workspace behavior**: accept the edit as ordinary Git commit. On next read, parse `state.toml`. If valid, use it. If malformed, report clearly, let user fix and recommit.

### Manual Edits to Local State

Someone edits `.workspace/local/staged.toml` by hand and corrupts it.

**Git analogue**: corrupted `.git/index`. Git detects it, reports error, suggests recovery commands.

**Workspace behavior**: detect corruption on read, report clearly, offer `ws reset` to clear local state and start fresh.

### Accumulated Divergence

A developer works locally for days, generating divergences across multiple repos without interacting with the workspace.

**Git analogue**: working directory with many unstaged changes while time passes.

**Workspace behavior**: no limit. Divergences accumulate in working state. Operations compatible with the divergences still work. Operations that require resolution report clearly. The developer curates when they want — or never, if their work never requires workspace-level coordination.

### Operations with Partial Conflict

`ws pull` with some repos clean and some with local changes.

**Git analogue**: `git pull` with local changes that don't conflict with incoming → proceeds. With conflicting changes → aborts and reports.

**Workspace behavior**: per-repo behavior mirrors `git pull`. Repos that can be pulled cleanly are pulled. Repos with conflicting local changes are reported, not modified. Final summary shows what was pulled, what was skipped, and why.

A `--where-possible` flag makes the skipping silent for scripting.

### Branches with Shared State Across Workspace Branches

Workspace branch `main` declares `shared-lib` should be on `main`. Workspace branch `feature/X` also declares `shared-lib` should be on `main` (because feature/X doesn't touch shared-lib).

**Git analogue**: two branches in a repo where most files are identical. Git stores them efficiently via object deduplication. Switching between them is nearly free for unchanged files.

**Workspace behavior**: `state.toml` in each workspace branch explicitly declares all repos' states. If two branches declare the same state for a repo, the file contents are identical and Git deduplicates storage. Switching between the branches doesn't re-checkout unchanged repos. Branching is branching.

### Developer Modifies a Child Repo Outside the Tool

Developer opens `service-b` in a different Git client, creates a branch, commits. The tool doesn't see this action in real time.

**Workspace behavior**: awareness is primarily via the alias wrapper (catching every `git` invocation). But as a safety net, any `ws status` reads actual state via `git` in each repo and detects divergence by comparison with `state.toml`. The tool is robust to actions that bypass the wrapper; it just detects them on next inspection rather than in real time.

## The Git Recursive Principle

The workspace is Git applied one level up. This is not a metaphor — it is a structural fact of the design and a rigorous guide for decisions.

### What This Means in Form

- The workspace is itself a Git repository.
- The workspace has branches (Git branches of the workspace repo).
- The workspace has commits (Git commits in the workspace repo).
- The workspace has a log, diff, status, staging — each a direct analogue of its Git counterpart.

### What This Means in Strategy

- Impossible states raise errors (as Git does with missing branches).
- Manual edits are accepted if valid, rejected if invalid (as Git does with file edits).
- Partial failures are reported, not silently patched (as Git does with partial merges).
- Local state is tolerant to damage and reconstructible (as Git does with the index).

### What This Means for Future Decisions

When a new feature is proposed, the first question is: *"What is the Git analogue of this, at the repo level?"*

- If there's a natural analogue, the feature likely fits. Its design mirrors Git's approach.
- If there's no analogue, suspect the feature is misaligned with the model. Rethink before accepting.

This principle is the primary defense against feature creep. It keeps the system coherent as it grows and predictable for users (who already know Git).

### What This Means for the User

Anyone who understands Git understands the workspace. There are no new concepts — only familiar concepts applied to a new domain. This minimizes cognitive load for adoption and makes documentation natural (every feature can be documented by analogy to Git).

---

## Summary Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  USER                                                            │
│    Writes `git <command>` (alias to workspace tool)              │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  WRAPPER                                                         │
│    1. Detect context: is this a workspace?                       │
│    2. If no: pass through to git unchanged                       │
│    3. If yes: engage workspace logic                             │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  MODELING LAYER                                                  │
│    - Read manifest.toml (structure)                              │
│    - Read state.toml (declared intent)                           │
│    - Read actual repo states (via git)                           │
│    - Infer scope per operation policy                            │
│    - Build execution plan                                        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  EXECUTION LAYER                                                 │
│    - Pre-flight check across affected repos                      │
│    - If all pass: execute plan (possibly in parallel)            │
│    - If any fail: abort with actionable error, modify nothing    │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  REPORTING LAYER                                                 │
│    - Aggregate results                                           │
│    - Format for human (colors, structure) or machine (--json)    │
│    - Record in operation log                                     │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│  GIT (unchanged, authoritative for all storage)                  │
│    - Workspace repo: manifest, state.toml, workspace branches    │
│    - Child repos: ordinary git repositories, untouched           │
└─────────────────────────────────────────────────────────────────┘
```

---

## Open Design Questions

These are not resolved in this document and are deferred to future design iterations:

1. **Migration monorepo ↔ workspace** — How to split a monorepo into a workspace preserving history, and how to merge a workspace back into a monorepo. Critical to the "trivial migration" claim of the thesis.

2. **CI coordination** — How per-repo CI combines with cross-repo CI for workspace branches. The workspace enables granular CI naturally; cross-repo validation for coordinated changes is the open design.

3. **Multi-developer collaboration** — How two developers work on the same workspace branch, how workspace-level PRs work, how conflicts in `state.toml` are handled at scale.

These open questions do not block initial implementation. The core model is sufficient to build a working tool. These are refinements that become important as usage matures.
