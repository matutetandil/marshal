# Development Roadmap

The product is built in phases, each self-contained and releasable. Each phase establishes a foundation for the next.

## Phase 0: Foundation

**Goal:** scaffold the project, establish architecture, set up tooling.

- [x] Design documents (`ARCHITECTURE.md`, `PRINCIPLES.md`, `GLOSSARY.md`)
- [ ] Cargo project structure
- [ ] Basic CLI skeleton with `clap`
- [ ] Alias / passthrough mechanism (invoke with any command, forward to git)
- [ ] Context detection (walk filesystem, find `.workspace/`)
- [ ] Logging infrastructure
- [ ] CI pipeline (test, lint, build on major platforms)

**Deliverable:** a binary that can be aliased to `git`, forwards everything transparently, and detects workspace context without doing anything with it yet.

## Phase 1: Wrapper — UX Improvements over Git

**Goal:** useful wrapper for plain Git repos. Pure value-add, no workspace logic required.

- [ ] Command interception with pass-through default
- [ ] Improved status output (better colors, structure)
- [ ] Actionable error messages for common Git errors (top 20)
- [ ] `help` command with context-awareness
- [ ] `what-now` command that analyzes current state and suggests next actions
- [ ] Configuration system (user preferences, project settings)
- [ ] Output modes: human (colors, interactive) and machine (JSON, scripting)

**Deliverable:** a tool that enhances plain Git usage without any workspace features. Adoptable by users who have no intention of using workspaces.

## Phase 2: Workspace Core — Read-Only Operations

**Goal:** workspace detection and passive operations. No state modification yet.

- [ ] Workspace initialization: `ws init` (creates `.workspace/` structure)
- [ ] Manifest parsing and validation
- [ ] State.toml parsing and validation
- [ ] Workspace clone: `ws clone <url>` (clones workspace + all child repos in parallel)
- [ ] Workspace status: aggregated view of all repos, divergence reporting
- [ ] Workspace log: aggregated or per-repo depending on context
- [ ] Workspace diff: diff of workspace repo, semantic interpretation of state.toml changes
- [ ] Scope inference engine
- [ ] `--explain` flag implementation

**Deliverable:** developers can clone a workspace and see its state clearly. No modifications yet.

## Phase 3: Workspace Modifications — The Three Zones

**Goal:** full workspace CRUD with staging model.

- [ ] `ws stage <repo>` — mark divergence for inclusion in next workspace commit
- [ ] `ws unstage <repo>` — remove from staging
- [ ] `ws restore <repo>` — return repo to declared state
- [ ] `ws reset` — clear staging
- [ ] `ws commit` — commit staged changes as new state.toml in workspace repo
- [ ] Workspace branching: `ws branch <name>` with scope inference
- [ ] Workspace switching: `ws switch <name>` with state materialization
- [ ] Pre-flight checks framework
- [ ] Parallel execution framework with error aggregation

**Deliverable:** complete workspace model operational. Developers can create workspaces, work in them, curate state, and coordinate changes across repos.

## Phase 4: Coordinated Operations

**Goal:** network and CI operations that leverage workspace structure.

- [ ] `ws pull` — parallel pull with affinity-based ordering
- [ ] `ws push` — push only repos with changes on current branch
- [ ] `ws fetch` — parallel fetch of all repos
- [ ] `ws sync` — reconcile declared vs actual state
- [ ] Partial operation flags: `--where-possible`, `--strict`, etc.
- [ ] Operation log (oplog) with `ws undo` support
- [ ] Affinity-aware execution (dependency ordering)

**Deliverable:** coordinated multi-repo workflows feel native. Push, pull, and sync respect workspace structure.

## Phase 5: Differentiating Features

**Goal:** features that make the workspace significantly better than alternatives.

- [ ] `ws absorb` — intelligent commit absorption (port of git-absorb)
- [ ] `ws explain <concept>` — integrated documentation for concepts and errors
- [ ] `ws auth <provider>` — frictionless credential setup
- [ ] `ws where <branch>` — find which repos have a branch
- [ ] `ws affected <change>` — dependency-aware impact analysis
- [ ] `ws graph` — visual workspace state
- [ ] Workspace branch protection policies (declared in manifest)
- [ ] Integration with GitHub/GitLab APIs for cross-repo PRs

**Deliverable:** the workspace is no longer just a coordinator — it's the best place to manage multi-repo development.

## Phase 6: Advanced & Optional

**Goal:** power features for teams at scale.

- [ ] Manifest profiles (partial workspace clones for subsets of repos)
- [ ] Atomic cross-repo operations (best-effort with rollback)
- [ ] Workspace bisect (cross-repo bisect coordinated by workspace)
- [ ] Semantic merge drivers (language-aware conflict resolution)
- [ ] Workspace-wide hooks
- [ ] TUI mode for complex operations

**Deliverable:** enterprise-grade tooling for large teams and codebases.

---

## Release Milestones

- **0.1.0** — Phase 0 + partial Phase 1. Functional alias with some UX improvements.
- **0.2.0** — Complete Phase 1. Useful standalone wrapper.
- **0.3.0** — Phase 2 complete. Read-only workspace operations.
- **0.5.0** — Phase 3 complete. Full workspace model (MVP of workspace product).
- **0.7.0** — Phase 4 complete. Coordinated operations.
- **1.0.0** — Phase 5 complete. Production-ready differentiated tool.
- **1.x+** — Phase 6 features and beyond.

## Testing Strategy

Each phase requires:

- Unit tests for core logic (scope inference, state diffing, manifest parsing).
- Integration tests with real Git repositories in temporary directories.
- Documentation updates in `docs/` for any new concepts or commands.
- Examples in `examples/` showing the feature in use.

## Design Discipline

Every phase change must:

1. Pass all nine invariants from `PRINCIPLES.md`.
2. Have documented scope policies for any new operations.
3. Include `--explain` support for any new commands.
4. Be reviewable against the architecture without requiring implementation knowledge.
