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

## Phase 1: Wrapper — UX Improvements over Git — ✅ shipped as `0.2.0` + `0.3.0` (2026-04-24 / 2026-04-27)

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
- [x] Actionable error messages for common Git errors — 13 rules shipped
  in `0.3.0` covering the high-friction failures
  (`not-a-git-repository`, `dubious-ownership`, `empty-ident`,
  `ssh-publickey-denied`, `https-auth-failed`, `host-resolution-failed`,
  `push-non-fast-forward`, `upstream-not-configured`,
  `src-refspec-no-match`, `pathspec-no-match`, `ambiguous-argument`,
  `local-changes-would-be-overwritten`, `unrelated-histories`).
  Stderr capture mode in `commands::passthrough` is gated by
  `errors.actionable_hints` (default on; off restores byte-exact
  passthrough). The remaining ~7 low-frequency rules (cannot lock ref,
  branch -D not fully merged, dubious permissions on `~/.ssh`, …) are
  deferred to opportunistic future cycles.
- [x] `help` command with context-awareness — `marshal help` lands on
  a context-aware overview (in-repo recommends `what-now`/`git status`;
  outside recommends `git init`/`cd`). Five topics ship: `overview`,
  `config`, `hints`, `modernize`, `what-now`. `--json` works
  automatically via the Command/Renderable substrate. *(0.3.0)*
- [x] `what-now` command — proactive counterpart to actionable error
  hints. State read via `git status --porcelain=v2 --branch` plus
  `.git/*` filesystem markers, fed through a Strategy registry of 9
  advice rules priority-ordered: conflict → in-progress → initial →
  detached → uncommitted → diverged → behind → ahead → clean. *(0.3.0)*
- [x] Output modes: human (default) and JSON — global `--json` flag
  in `cli::dispatch` switches every marshal-namespace command's
  stdout. The Command + Renderable substrate makes adding a new
  command light up `--json` automatically (Invariant 10). Colourised
  human output remains a follow-up — currently the human form is
  plain text. *(0.3.0)*

### Modernization Policy

The wrapper may *observe* legacy command forms (e.g. `git checkout -b`) and print a modernization tip, but by **default it never rewrites the command the user typed** — the invocation is forwarded to Git unchanged. This respects Invariant 8 (Conservative Defaults) from `PRINCIPLES.md` and the "don't improve Git in passthrough" rule from `CLAUDE.md`.

Users who want the wrapper to silently substitute modern equivalents can opt in via configuration (e.g. `marshal config set modernize.rewrite = true`). Opt-in only; no magic by default.

**Deliverable:** a tool that enhances plain Git usage without any workspace features. Adoptable by users who have no intention of using workspaces.

## Phase 2: Workspace Core — Read-Only Operations — ✅ shipped as `0.4.0` (2026-04-28)

**Goal:** workspace detection and passive operations. No state modification yet.

- [x] Context detection (walk filesystem upward, find `.workspace/`).
  Lives in `src/context.rs`; consumed by the new `ws` namespace.
  `Context { root, current_repo }` — `current_repo` identified by
  the `<root>/src/<name>/…` convention (will be reconciled against
  the manifest in the manifest-parsing slice). Reachable via
  `git ws` (the bare command in the new `ws` sibling namespace).
- [x] Workspace initialization: `ws init` (creates `.workspace/` structure).
  `--name` and `--default-branch` flags with sensible defaults (cwd
  basename, `git config init.defaultBranch`). Refuses to run inside
  an existing workspace; `--force` overrides. Manifest is written
  minimal (no empty `repos = []` / `[affinities]` noise) thanks to
  `skip_serializing_if`; `state.toml` carries a header comment but
  an empty body (every repo defaults until pinned).
- [x] Manifest parsing and validation. `Manifest::try_load_from_workspace`
  with `Ok(None)` / `Ok(Some)` / `Err` semantics so a partially-initialised
  workspace (no manifest yet) is not an error. `git ws` shows the
  workspace name + default branch + declared repo list; reconciles
  `current_repo` against the manifest's declared repos.
- [x] State.toml parsing and validation. Same three-way load semantics
  as the manifest. `git ws` shows per-repo declared branches with
  the universal hide-boring abbreviation: pinned repos listed
  individually, default-branch repos collapsed into a count line.
  Global `--all` flag (mirrors `--json` shape) overrides the
  abbreviation. Threshold for unconditional expansion: 5 repos.
- [x] Workspace clone: `ws clone <url> [<dest>]`. Clones the workspace
  repo synchronously, reads `.workspace/manifest.toml`, and fans out
  to every declared child in parallel with one Docker-style
  `indicatif::ProgressBar` per child under a shared `MultiProgress`.
  Threading via `std::thread::scope`; per-child stderr parsed for the
  four canonical `git clone --progress` phases (Counting, Compressing,
  Receiving, Resolving). Partial failures are tolerated (Invariant 5):
  a failed child becomes a `kind = "failed"` entry in the result list
  and the operation still exits 0. `--no-children` skips the
  fan-out; `--explain` prints the plan without executing it; a target
  without a manifest falls through to "plain clone, no children".
- [x] Workspace status: aggregated view of all repos, divergence reporting.
  `git ws status` walks every declared repo via `RepoState::detect_at(path)`
  (extracted to `crate::git::porcelain` from `marshal what-now`'s state
  extractor), reconciles the on-disk state against the manifest +
  state.toml's declared branch, and renders with the universal
  hide-boring pattern. `--all` expands; JSON form returns the full
  per-repo payload. **Phase 3 / Slice B extension:** the staging
  zone (`.workspace/local/staged.toml`) is also surfaced. Each repo
  with a staging entry shows a "staged at `<branch>`@<sha>" segment,
  is excluded from hide-boring (always interesting), and contributes
  to a footer "X repos staged for commit." Drift between the staged
  snapshot and the current working state is flagged informationally —
  a deliberate consequence of the snapshot semantics, not a bug.
- [x] Workspace log: aggregated cross-repo activity. `git ws log`
  walks every declared repo, fetches per-line tab-separated entries
  via `git -C <path> log --pretty=…`, sorts by descending ISO author
  date, and renders the top-N as a unified timeline. `-n <N>`
  customises the cap (default 20); the global `--all` flag lifts
  it. Per-repo log via `cd src/<repo> && git log` (passthrough);
  the "context-aware" half (inside a child, behave like git log of
  that repo) waits for the scope inference engine.
- [x] Workspace diff: semantic interpretation of state.toml changes.
  `git ws diff` compares the working-tree `state.toml` against the
  version at `HEAD` and translates the difference into per-repo
  "added / removed / changed" entries. Graceful degrade for
  no-HEAD cases. Tagged-enum JSON shape (`{kind, name, …}`). Raw
  file diffs of manifest.toml / Dockerfile / README are intentionally
  left to `cd <root> && git diff` — Marshal only reimplements the
  bits where domain interpretation adds value over plain git output.
- [x] Scope inference engine. The Phase 0 scaffold of
  `src/workspace/scope.rs` (5 dimensions, 7 policies, the `infer()`
  function) is live, fed by a thin `resolve()` entry point that
  encapsulates the `--on <name>` declared-scope override and the
  fall-through to the command's policy. `ws log` uses
  `spatial_fallback` (narrows to current repo when inside one);
  `ws status` and `ws diff` use `full_workspace` (no spatial,
  but `--on` still filters). Material/Temporal/Structural
  dimensions and their policy constructors are scaffolded for
  Phase 3+ commands.
- [x] `--explain` flag implementation. Global flag on the `ws`
  namespace; every workspace command (`init`, `status`, `log`,
  `diff`) shows its plan — exact `git -C <path> …` invocations
  or filesystem operations — without running them. Closes the
  architectural promise from Invariant 6. JSON form carries the
  plan in an `explain_plan` field. Marshal namespace commands
  are intentionally out of scope (they are metadata/render
  rather than coordinated operations); extend later if one grows
  into a workspace operation.

**Deliverable:** developers can clone a workspace and see its state clearly. No modifications yet.

## Phase 3: Workspace Modifications — The Three Zones — 🟡 first slice shipped

**Goal:** full workspace CRUD with staging model.

The three zones, mental model:
- **Working** — what every child repo's HEAD actually is right now.
- **Staged** — `.workspace/local/staged.toml`, populated by `ws add`.
  Per-repo `(branch, commit)` snapshots taken at stage time
  (`git add`-style: re-staging refreshes the snapshot, drift in
  working trees does not propagate). Per-developer, gitignored.
- **Declared** — `.workspace/state.toml`, the committed source-of-
  truth. Updated by `ws commit` (flushes staged into declared and
  clears staged).

- [x] `ws add <repo>` — capture the child's `(branch, commit)`
  into `.workspace/local/staged.toml` at stage time. Refuses clean
  on detached HEAD or initial-empty repos (no stable snapshot
  available). Re-staging an already-staged repo overwrites with
  the current snapshot, surfaced as "(was branch@commit)" in the
  human form and in the JSON `previous_snapshot` field. The
  per-developer `local/` directory is gitignored automatically on
  first stage.
- [x] `ws unstage <repo>` — drop the entry from `staged.toml`.
  Idempotent: unstaging a never-staged repo is a no-op, not an
  error.
- [x] `ws restore <repo>` — bring a child back to the declared
  branch (state.toml override or manifest default). First Phase 3
  command that writes to a child working tree. Pre-flight gates
  the operation: hard blockers (in-progress merge / rebase /
  cherry-pick / revert / bisect, working-tree conflicts,
  initial-empty repo) refuse unconditionally; soft blockers
  (uncommitted changes) refuse by default and are resolved via
  the mutually-exclusive `--auto-stash` (preserve via
  `git stash push --include-untracked`) or `--discard-changes`
  (destructive `git reset --hard` + `git clean -fd`). Already-on-
  declared is a clean no-op; `--explain` describes the plan
  including which resolution step would run on a dirty tree.
  Single-repo only this slice — multi-repo `ws restore --all`
  waits for the parallel-execution framework.
- [x] `ws reset` — clears the per-developer staging area in one
  go. Counterpart to `ws unstage <repo>` (single-repo). Rewrites
  `.workspace/local/staged.toml` with the header preserved and
  an empty body. Empty-staging case is a benign no-op. Refuses
  positional arguments and `--on <name>` with a hint at
  `ws unstage` so the two commands stay semantically disjoint.
- [x] `ws commit` — flushes the staging area into a workspace
  commit. Reads `.workspace/local/staged.toml`, upserts every
  entry into `.workspace/state.toml`, runs
  `git commit -- <state-path>` (with `--only` semantics so other
  staged paths in the workspace repo's git index stay staged),
  and clears `staged.toml` on success. `-m <msg>` /
  `--message <msg>` / `--message=<msg>` are accepted; without a
  message git's commit inherits stdio so `$EDITOR` takes over the
  terminal exactly as plain `git commit` does. Empty staging and
  "every staged entry already matches the declared state" are
  distinct errors with distinct hints. `--json` requires `-m`
  (editor mode is incompatible with structured output);
  `--explain` describes the plan without invoking git. Single-
  shot only — there is no per-repo `ws commit <repo>` (use
  `ws unstage <repo>` to drop a staged entry instead).
- [x] Workspace branching: `ws branch <name>` (thin, Slice F) —
  the workspace-level analogue of `git branch <name>`. Creates a
  new branch in the workspace-repo from current HEAD; child repos
  are not touched. The new workspace branch's `state.toml` starts
  as a copy of the parent branch's tree (git branch copies the
  tree); the user populates the per-child mapping later via
  `ws add` / `ws commit` on the new branch. Initial-empty refusal
  is clearer than the raw git error; branch-already-exists
  propagates git's diagnostic verbatim. `--on <name>` is rejected
  (workspace-repo-only operation). The richer **granular-scope**
  variant — auto-detect divergent children, create their branches,
  record in state.toml — lives in Slice F.5, post Slice H so it
  can use the parallel-execution framework.
- [ ] Workspace switching: `ws switch <name>` with state materialization
- [x] Pre-flight checks framework — `src/workspace/preflight.rs`
  hosts the `Obstacle` enum (tagged-enum JSON: in_progress,
  conflicts, staged_changes, unstaged_changes, untracked_files,
  initial_empty), the per-state `obstacles(state)` classifier,
  and the `is_hard_blocker` / `cleared_by_auto_stash` /
  `cleared_by_discard` predicates. Single consumer today
  (`ws restore`); Slice H consolidates the framework into a
  Strategy + Registry shared by every coordinated multi-repo
  operation.
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
