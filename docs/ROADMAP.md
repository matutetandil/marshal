# Development Roadmap

The product is built in phases, each self-contained and releasable. Each phase establishes a foundation for the next.

## Phase 0: Foundation — ✅ shipped as `0.1.0` (2026-04-24)

**Goal:** scaffold the project, establish architecture, set up tooling.

- [x] Design documents (`ARCHITECTURE.md`, `PRINCIPLES.md`, `GLOSSARY.md`)
- [x] Cargo project structure
- [x] CLI skeleton with `clap` — scaffolded in `src/cli.rs`; intentionally
  **not wired** to `main` in `0.1.0` so clap stays out of the passthrough
  hot path and byte-exact fidelity is preserved. The scaffold is enabled
  from `main` in `0.2.0` when command interception begins.
- [x] Alias / passthrough mechanism (invoke with any command, forward to git)
- [x] Logging infrastructure (`tracing` + `RUST_LOG` filter, stderr writer)
- [x] CI pipeline — `cargo build --release` and `cargo test` run natively on
  Linux x86_64, Linux ARM64, macOS ARM64, and Windows x86_64. macOS x86_64
  is covered by a dedicated cross-build job from the ARM runner. `cargo
  fmt --check` and `cargo clippy -- -D warnings` run on Linux x86_64.

**Deliverable:** a binary that can be aliased to `git` and forwards every invocation transparently. No workspace awareness yet — context detection is deferred to Phase 2, where it is actually consumed.

## Phase 1: Wrapper — UX Improvements over Git — 🟡 first slice shipped as `0.2.0` (2026-04-24)

**Goal:** useful wrapper for plain Git repos. Pure value-add, no workspace logic required.

- [x] Command interception with pass-through default *(0.2.0)*
- [x] Command modernization suggestions — 11 rules covering the 12 canonical
  legacy forms: `checkout → switch/restore` (Git 2.23 split, 8 patterns),
  `reset → restore --staged` (file-mode), `stash save → stash push`,
  `remote rm → remote remove`. Tips to stderr by default; opt-in rewrite
  via `modernize.rewrite`. *(0.2.0)*
- [x] Configuration system — three-tier (`system < global < local`) with
  TOML-on-disk, `marshal config get|set|unset|list`, `--show-origin`, and
  `--system|--global|--local` flags. Per-repo local lives at
  `<git-dir>/marshal/config.toml`. *(0.2.0)*
- [x] `--version` augmentation — marshal appends its own version line after
  git's, following the node+npm / php+xdebug pattern. *(0.2.0)*
- [ ] Improved status output (better colors, structure) — deferred: the
  `PRINCIPLES.md` rule "don't improve Git in passthrough" limits what we
  can do before workspace context arrives (Phase 2). Revisit once workspace
  mode provides a natural scope for the augmentation.
- [ ] Actionable error messages for common Git errors (top 20)
- [ ] `help` command with context-awareness
- [ ] `what-now` command that analyzes current state and suggests next actions
- [ ] Output modes: human (colors, interactive) and machine (JSON, scripting)

### Modernization Policy

The wrapper may *observe* legacy command forms (e.g. `git checkout -b`) and print a modernization tip, but by **default it never rewrites the command the user typed** — the invocation is forwarded to Git unchanged. This respects Invariant 8 (Conservative Defaults) from `PRINCIPLES.md` and the "don't improve Git in passthrough" rule from `CLAUDE.md`.

Users who want the wrapper to silently substitute modern equivalents can opt in via configuration (e.g. `marshal config set modernize.rewrite = true`). Opt-in only; no magic by default.

**Deliverable:** a tool that enhances plain Git usage without any workspace features. Adoptable by users who have no intention of using workspaces.

## Phase 2: Workspace Core — Read-Only Operations

**Goal:** workspace detection and passive operations. No state modification yet.

- [ ] Context detection (walk filesystem upward, find `.workspace/`)
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

Each release is intentionally small and self-contained. Phases map loosely to milestones — a single phase may span two releases when that produces smaller, more reviewable increments.

- **0.0.0-reserved** — Name reservation on crates.io. No functional code. Published 2026-04-24.
- **0.1.0** — Phase 0 complete. Pure alias/passthrough: `alias git=marshal` behaves identically to Git for every command. Logging, CI, and release plumbing in place. No UX changes.
- **0.2.0** — First slice of Phase 1: command interception + modernization suggestions (tip-only by default; opt-in rewrite) + better status output. The wrapper starts having an identity beyond passthrough.
- **0.3.0** — Phase 1 complete. Actionable error messages, `help`/`what-now`, configuration system, human/JSON output modes. Useful standalone wrapper.
- **0.4.0** — Phase 2 complete. Context detection, read-only workspace operations (`ws init`, status, log, diff, clone, scope inference, `--explain`).
- **0.5.0** — Phase 3 complete. Full workspace model (the three zones, branching, switching). MVP of the workspace product.
- **0.7.0** — Phase 4 complete. Coordinated operations (pull/push/fetch/sync, oplog, undo).
- **1.0.0** — Phase 5 complete. Differentiating features; production-ready.
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
