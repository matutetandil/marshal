# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Work in progress on `0.5.0` — Phase 3 (workspace modifications,
the three zones: declared / staged / working). Four slices shipped:
the per-developer staging area (`ws add` / `ws unstage`, Slice A),
its surfacing through `ws status` (Slice B), the first child-
working-tree-write command (`ws restore`, Slice C), and the
"clear all" counterpart (`ws reset`, Slice D), plus a
`tests/invariants.rs` meta-test crate (Slice B.5) that guards the
auto-checkable invariants from `docs/PRINCIPLES.md`. Next:
`ws commit`, `ws branch`, `ws switch`, plus the pre-flight +
parallel-execution frameworks. See
[`docs/ROADMAP.md`](docs/ROADMAP.md).

### Added

- **`ws reset` (Slice D).** Clears the per-developer staging area
  in one go. The workspace-level analogue of `git reset` (the
  mode that empties the index without touching the working tree).
  - Reads `staged.toml` (or treats missing as empty), drops every
    entry, rewrites the file with the header preserved and an
    empty body. The file's existence becomes a stable invariant
    once the first stage runs — `cat staged.toml` keeps showing
    the documentation comment after a reset.
  - Empty-staging case is benign: `was_empty: true`, message
    "Staging area was already empty — nothing to do.", no error.
  - `ws reset <repo>` and `ws reset --on <repo>` are rejected
    with a hint at `ws unstage <repo>`. Keeps the two commands
    semantically disjoint: `ws reset` for "clear all",
    `ws unstage` for "drop one".
  - `--explain` describes the load → drop → write plan.
  - JSON shape: `{root, cleared: [{name, branch, commit}, …],
    was_empty, explain_plan?}`. `cleared` is sorted alphabetically
    so the output is stable across invocations.
  - Side-effect on `StagedDeclaration`: the `repos` field gains
    `skip_serializing_if = "HashMap::is_empty"` so the post-reset
    file body is empty rather than `[repos]` empty-table noise.
    Round-trip stays correct because `#[serde(default)]` rehydrates
    the empty map on read.

- **`ws restore <repo>` (Slice C).** First Phase 3 command that
  writes to a child repo's working tree. Switches the named child
  back to the workspace's declared branch (state.toml override or
  manifest default).
  - **Pre-flight gates the operation** before any git invocation
    runs. Two tiers of obstacles, exposed via the new
    `src/workspace/preflight.rs` module:
      * **Hard blockers** (in-progress merge / rebase / cherry-pick
        / revert / bisect, working-tree conflicts, initial-empty
        repo): refused unconditionally — no flag clears them. The
        error explains each obstacle and the manual recovery
        path.
      * **Soft blockers** (staged / unstaged / untracked changes):
        refused by default (Invariant 8: Conservative Defaults).
        The user opts in to one of two resolutions:
          - `--auto-stash`        →  `git stash push --include-untracked`
                                     (preserves work; recoverable
                                     via `git stash pop`).
          - `--discard-changes`   →  `git reset --hard` + `git clean
                                     -fd` (destructive; explicit
                                     opt-in).
  - The two resolution flags are mutually exclusive — the user
    has to pick a side: preserve or destroy.
  - Already-on-declared is a no-op with a clear message; nothing
    runs against the child.
  - The error message lists every detected obstacle, marking the
    blockers (`✗`) versus those a flag would clear (`✓`), and
    suggests the next concrete step.
  - `--explain` describes the plan without executing, including
    which resolution step would run if the working tree were
    dirty (handy "rehearsal" for `--auto-stash`).
  - `--on <name>` is rejected with a hint at the canonical
    `ws restore <name>` form — single-repo restore takes a
    positional; `--on` is the multi-repo declared-scope override
    that does not apply here. The check fires before arg parsing
    so a stray `--on` always surfaces the right hint.
  - JSON shape: `{root, repo_name, path, declared_branch,
    from_branch?, from_commit?, to_branch, to_commit?, stashed,
    discarded, obstacles[], explain_plan?}`. Obstacles are
    tagged-enum (`{kind: "in_progress" | "conflicts" |
    "staged_changes" | …, …}`) — single-switch consumption,
    same precedent as `ws diff`'s `StateChange`.

  Single-repo only this slice. The multi-repo `ws restore --all`
  variant waits for the parallel-execution framework in Slice H
  (Phase 3 close).

- **`tests/invariants.rs` meta-test crate (Slice B.5).** A new
  test target where each test enforces one of the documented
  invariants from `docs/PRINCIPLES.md` from the outside, by running
  the binary against fixtures and asserting externally-visible
  properties. Test names are prefixed with `invariant_<N>_` so a
  CI failure says exactly which principle was broken.
  - Initial set: 7 tests covering Invariants 6, 8, and a doc-parity
    corollary of 10. The unit-level sync guards in `src/`
    (error_hints rule_id ↔ help topic, what_now rule_id ↔ help
    topic) stay where they are — they enforce code-adjacent
    invariants closer to the source.
  - Verified to catch a regression: removing `ws unstage` from
    the unknown-subcommand error fired
    `invariant_10_unknown_ws_subcommand_error_lists_every_known_subcommand`
    with a message naming the missing subcommand.
  - CI matrix already runs `cargo test --all-targets`, which
    picks up the new crate without `.github/workflows/ci.yml`
    changes.

- **`ws status` surfaces the staging zone (Slice B).** When a repo
  has a staging entry in `.workspace/local/staged.toml`, the
  per-repo line gains a "staged at `<branch>`@<sha7>" segment, and
  a footer summary "X repo(s) staged for commit. Run `ws commit`
  to flush staged → state.toml." appears whenever staging is
  non-empty. Staged repos are always interesting (never collapsed
  under hide-boring), even when the working state would otherwise
  read as boring — the user is mid-flight on a workspace commit
  and should see what is queued.

  - **Drift detection.** When a staged snapshot's `(branch, commit)`
    no longer matches the working state, the per-repo line gains
    "(drifted from working)" and a footer sub-line spells out the
    implication: the commit will record the staged values; re-run
    `ws add` to refresh. Drift is **not** a bug — it is the
    snapshot semantics working as designed (deploy-manifest
    atomicity), made visible.
  - **JSON shape.** `RepoStatus` gains `staging: Option<RepoStaged>`
    and `staging_drifted: bool`. Both use `skip_serializing_if`, so
    machine consumers see the fields only when they apply.
  - **`--explain` plan** gains a leading "read `.workspace/local/
    staged.toml`" step so the dry-run output reflects the full set
    of inputs.

- **`BranchInfo.oid: Option<String>`.** Porcelain v2 already emitted
  `# branch.oid <hash>` but the parser only used it to detect
  `(initial)`. The hash is now captured. Two consequences:
  - `ws add` reads HEAD's commit from the porcelain output
    directly — one shellout less per stage (no separate
    `git rev-parse HEAD`).
  - `ws status` can compare staging snapshots against the working
    state's `(branch.name, branch.oid)` to compute drift without
    extra shellouts.

### Added

- **`ws add <repo>` / `ws unstage <repo>`.** First Phase 3 slice:
  per-developer staging area, the workspace's equivalent of git's
  index. `ws add` captures the child repo's `(branch, commit)`
  *at stage time* and writes it to `.workspace/local/staged.toml`;
  `ws unstage` drops the entry. `ws commit` (a later slice) will
  flush staged entries into `state.toml` and clear the staging
  file.

  The snapshot semantics — chosen explicitly over the simpler
  "mark-only, snapshot at commit" model — protect deploy-manifest
  atomicity: what the user tested as a coordinated set across repos
  is exactly what gets committed, even if working trees drift
  between stage and commit. Re-staging a repo overwrites the
  previous snapshot, mirroring how `git add` re-stages a file's
  current content.

  - **Validation.** Both commands validate the repo against the
    manifest with the same shape as `--on bogus`: a typo errors
    out cleanly with the list of known names.
  - **Degenerate child states are refused with a recovery hint.**
    `ws add` fails clearly when the child repo has no commits
    yet (initial-empty) or is on detached HEAD — both are states
    that staging cannot represent because they do not produce a
    stable `(branch, commit)` pair a deploy manifest could pin.
  - **Idempotent unstage.** `ws unstage <never-staged-repo>` is a
    no-op rather than an error, mirroring how
    `git restore --staged <unstaged-path>` behaves benignly.
  - **Per-developer `local/` is gitignored automatically.** The
    first time `staged.toml` is written, marshal seeds
    `.workspace/local/.gitignore` with `*` so per-developer
    staging never accidentally lands in the workspace repo's
    history. Idempotent — a customised `.gitignore` is never
    overwritten.
  - **Header comment in `staged.toml`.** A user who opens the file
    directly sees what it is for and what entries look like.
  - `--explain` and `--json` work via the existing namespace globals.

- **`StagedDeclaration`** in `src/workspace/staged.rs`. Mirrors
  `state.rs` structurally (`HashMap<String, RepoStaged>`) but
  `RepoStaged.commit` is **required** (vs optional in `state.toml`)
  — a snapshot must be reproducible. Three-way load semantics
  match the manifest and state loaders: `Ok(None)` on missing,
  `Ok(Some)` on loaded, `Err` on malformed. `save_to_workspace`
  creates `.workspace/local/`, seeds the gitignore, and writes
  `staged.toml` (header + body) atomically per call.

### Changed

- `LOCAL_DIR` and `git::rev_parse` lose their
  `#[allow(dead_code)]` — both have real consumers now (the new
  staging slice). `commands::ws::staged::{get, is_empty}` keep
  individual `#[allow(dead_code)]` until Slice B (`ws status`
  integration) and Slice E (`ws commit`) consume them.
- The `ws` unknown-subcommand hint mentions `stage` and `unstage`
  alongside the existing subcommands.

## [0.4.0] — 2026-04-28

Phase 2 complete. Marshal grew workspace awareness: `git ws` is now a
namespace next to plain git, with five subcommands that observe a
workspace from outside-in (`init`, `status`, `log`, `diff`, `clone`),
a scope-inference engine that decides "which repos do I operate on?"
from the cwd plus an explicit `--on <name>` override, and a global
`--explain` flag that turns every operation into a dry-run plan
(Invariant 6: Explainable Operations). The release stays read-only —
no workspace command modifies anything beyond what `ws init` and
`ws clone` write to disk on first creation. Modifications open in
Phase 3.

### Added

- **`ws clone <url> [<dest>]`.** The last operational gap in Phase 2.
  Clones the workspace repo synchronously, reads its
  `.workspace/manifest.toml`, and fans out to every declared child
  in **parallel**, with one Docker-style progress bar per child
  driven from `git clone --progress`'s stderr stream.
  - Threading: `std::thread::scope` runs one worker per child;
    one shared `indicatif::MultiProgress` keeps the bars stacked.
    Non-TTY environments (pipes, CI) gracefully fall back to
    inert bars — output stays clean.
  - Progress parser handles the canonical phases (Counting,
    Compressing, Receiving, Resolving) plus Enumerating, Updating
    files, and Filtering content. The `remote: ` prefix is stripped
    transparently. Lines we do not recognise still surface as the
    bar's free-form message so the user always sees something.
  - Partial failures are tolerated (Invariant 5: Partial Failure
    is Acceptable). A child whose clone fails is recorded as
    `kind = "failed"` with the error string; siblings continue
    cloning; the operation still exits 0. The human form summarises
    `Cloned X/Y child repos` and lists failed entries in a footer.
  - JSON shape mirrors `ws diff`'s tagged-enum precedent:
    `{ workspace_url, workspace_root, no_children, manifest_present,
    children: [{ kind: "success" | "failed", name, url, path,
    duration_ms?, error? }, …] }`. Single-switch consumption.
  - `--no-children` skips the fan-out — the workspace repo is
    cloned, the manifest stays untouched, no `src/<name>` directories
    appear. Useful for inspecting a workspace before committing to
    cloning hundreds of repos.
  - `--explain` describes the plan (workspace clone + manifest read +
    parallel child invocations) without executing anything; the
    same dry-run safety property as `ws init --explain`.
  - A non-Marshal git repo (no `.workspace/manifest.toml` after the
    workspace clone) is still a valid target: `ws clone` falls
    through to "plain clone, no fan-out" and reports
    `manifest_present: false` in JSON. Pointing at any random repo
    URL produces a sensible result rather than an error.
- **`indicatif`** is now a real dependency. Declared in `Cargo.toml`
  since Phase 0 for exactly this slice, it lights up here for the
  first time. No other module imports it yet.

### Changed

- `commands/ws/mod.rs` gains a `Some("clone")` arm in `dispatch`;
  the unknown-subcommand hint now mentions `ws clone <url>`.
- The integration test suite gained a `url` dev-dependency for
  `Url::from_file_path`, used by the `ws clone` tests to build
  `file://` URLs from temp paths portably (raw `format!("file://{}",
  path.display())` produced backslash-laden URLs on Windows that
  TOML rejected as invalid Unicode escapes).

### Added

- **`--explain` flag** on every workspace command. Closes the
  architectural promise from PRINCIPLES.md Invariant 6
  (Explainable Operations): every operation can show its plan —
  the exact Git commands or filesystem operations it would
  perform — without performing them. Implementation:
  - Global flag extracted in `cli::dispatch_ws`, stripped from
    argv, set as a `bool` on each `Command` struct.
  - Each Output type gains an
    `explain_plan: Option<Vec<String>>`; the renderer routes to
    a shared "Plan for `ws <…>`:" + numbered-steps format when
    the field is populated.
  - `ws init --explain` lists what it would create and **does
    not write anything** (the safety property — explain is a
    dry run, not a "show then run" preview).
  - `ws status --explain` enumerates the per-repo
    `rev-parse --git-dir` and `status --porcelain=v2 --branch`
    invocations. Honours `--on` to narrow the plan.
  - `ws log --explain` lists the per-repo `git log` invocation
    with the cap (`-n N`) visible.
  - `ws diff --explain` describes the workspace-side state.toml
    read and the `git show HEAD:.workspace/state.toml` baseline
    fetch.
  - JSON form carries `explain_plan` under --explain (skipped
    otherwise — machine consumers can branch on its presence).
  - Marshal namespace commands (`config`, `help`, `what-now`)
    are not covered by `--explain` in this slice — those are
    metadata/render rather than coordinated operations. Will
    extend if a marshal command grows into one.

- **Scope inference engine.** The Phase 0 scaffold of
  `src/workspace/scope.rs` (5 dimensions, 7 predefined policies,
  `infer()` with 3 unit tests) goes live in Slice H. New
  `scope::resolve()` is the single entry point every workspace
  command uses to compute "which repos do I operate on?" — it
  captures both the `--on <name>` declared-scope override (with
  manifest validation) and the fall-through to the command's
  policy via `infer()`. `Material`, `Temporal`, and `Structural`
  dimensions and their policy constructors stay
  `#[allow(dead_code)]` until the Phase 3+ commands that consume
  them ship.
- **Global `--on <name>` flag** on the `ws` namespace. Mirrors the
  shape of `--json` / `--all`: extracted in `cli::dispatch_ws`,
  stripped from argv, threaded to each workspace Command via a
  dedicated `on: Option<String>` field. Supports both
  `--on <value>` (separated) and `--on=<value>` (equals) forms;
  rejects empty values and repeated flags. Validated against the
  manifest's declared repos before running so a typo errors out
  with the list of known names.
- **`ws log` spatial inference.** Without `--on`, `ws log` now
  obeys the spatial-fallback policy: when invoked from inside a
  child repo (cwd matches `<root>/src/<name>/…`), the log narrows
  to that one repo. From the workspace root or a sibling
  directory, the log stays workspace-wide. `--on <name>`
  overrides spatial, even from inside a child.
- **`ws status` and `ws diff` accept `--on <name>`** as a
  declared-scope filter. Status filters its repos list to one
  entry; diff filters the change list to entries matching that
  name.

- **`ws diff`.** Third aggregated read-only command. Compares the
  working-tree `state.toml` against the version at `HEAD` and
  translates the difference into per-repo "added / removed /
  changed" entries — domain-aware interpretation rather than the
  raw TOML hunk `git diff` would show.
  - Output renders with per-line symbol prefixes:
    `~ service-a   \`main\` → \`feat/payment\``,
    `+ service-c   declared on \`feat/api\``,
    `- service-b   declaration removed (was \`main\`)`.
  - Empty-changes case: "No state declarations changed since HEAD."
    plus a pointer to plain `git diff` for non-state files.
  - Graceful degrade: no commits yet, no state.toml at HEAD, or
    the root not being a git repo all collapse to "empty at HEAD"
    — every current entry reads as an addition.
  - JSON form uses a tagged enum: each `StateChange` carries a
    `kind` field (`"added"` / `"removed"` / `"changed"`) and the
    per-variant fields side-by-side. Single-switch consumption.
  - Marshal deliberately does not reimplement plain `git diff`
    for the rest of the workspace files — `cd <root> && git diff`
    is the right tool for manifest.toml / Dockerfile / README diffs.

- **`ws log`.** Second aggregated read-only command. Walks every
  declared child repo, runs `git -C <path> log --pretty=… -n N`,
  parses tab-separated entries (hash, ISO author date, author,
  subject), combines and sorts by date descending, and renders
  the top N as a unified timeline. The "monorepo feel" of the
  thesis applied to log: one timeline across all repos.
  - `-n <N>` / `--limit <N>` (also `-n20` shorthand and
    `--limit=N` equals form) caps the result. Default: 20.
  - `--all` (the global flag) lifts the cap and skips the
    truncation footer.
  - Stable ordering: descending date primary, repo name then
    hash as tie-breakers. Two same-minute commits always render
    in the same order between invocations.
  - Empty repos (`does not have any commits yet`) and repos
    missing on disk are silently skipped — `ws status` already
    surfaces missing repos; no need for log to repeat the noise.
  - JSON form: full `{workspace, entries[], sampled,
    limit_applied?}`. `sampled` reflects the entries collected
    before global truncation (useful when you want to know
    whether the cap kicked in).
  - No spatial inference yet: `ws log` always returns the
    workspace-wide view, regardless of where the user invokes
    it. Per-repo log via `cd src/<repo> && git log` (passthrough).
    Scope inference (e.g. `ws log` inside a child becomes
    per-repo log) lands in a future Phase 2 slice.

- **`ws status`.** First aggregated read-only command across child
  repos. For each repo declared in the manifest, resolves the
  on-disk path, fetches its per-repo state via the shared
  `git::porcelain::RepoState::detect_at`, and reconciles against
  the workspace's declared intent (state.toml override, or manifest
  default). Output (`WsStatusOutput`) carries the workspace info
  plus a `RepoStatus` per declared repo with `clean_on_declared`,
  `missing_from_disk`, and the full `RepoState` snapshot.
  - Hide-boring rendering: clean repos collapse to a count
    ("5 other repos clean and on declared branch. Run with `--all`
    to list them."); only "interesting" repos (dirty, off-branch,
    ahead/behind, in-progress, missing, detached, initial) are
    listed individually with a one-line detail.
  - All-clean fast path: "All N repos clean and on declared branch."
    when nothing is interesting and N is large enough to abbreviate.
  - `--all` expands every repo with full detail; small N (≤ 5)
    expands by default.
  - JSON form: full per-repo data — workspace + every `RepoStatus`
    + the embedded `RepoState`. `--all` is a no-op for JSON.
- **Porcelain types now `Serialize`.** `RepoState`, `BranchInfo`,
  `WorkingTreeInfo`, and `InProgressOp` all derive `serde::Serialize`
  so they embed cleanly in `WsStatusOutput`'s JSON shape.
- **`RepoState::detect_at(path)`.** The shared porcelain substrate
  gains a path-aware entry point (extracted in Slice E1, consumed
  by `ws status` here). `git -C <path>` shellouts replace the
  cwd-implicit ones; `marshal what-now` continues to use the
  cwd-bound `detect()` (one-line wrapper over `detect_at`).

### Changed

- `marshal what-now`'s state extraction moves from
  `commands/what_now/state.rs` into `git::porcelain` (Slice E1).
  Pure refactor — no functional change. `commands/what_now/state.rs`
  is removed; rule files now import from `crate::git::porcelain`.
  All 12 parser/in-progress unit tests move with the code.

- **`ws init`.** First workspace command that writes to disk.
  Creates `<cwd>/.workspace/` with a starter `manifest.toml`
  (`[workspace] name = "…" default_branch = "…"`) and an empty
  `state.toml` carrying a header comment. Refuses to run when
  the cwd is already inside a workspace (Invariant 8: Conservative
  Defaults); `--force` overrides and overwrites the manifest +
  state in place, leaving any other files in `.workspace/`
  untouched.
  - `--name <name>` / `--name=<name>`: workspace name. Default:
    cwd basename.
  - `--default-branch <branch>` / `--default-branch=<branch>`:
    manifest's default branch. Default: `git config --get
    init.defaultBranch`, then `"main"`. Aligns the workspace
    default with what plain `git init` would have used.
  - `--force`: overwrite existing manifest/state.
- **`Manifest` serialisation tightened.** `repos` and `affinities`
  gain `skip_serializing_if` so a freshly-`ws init`-ed manifest
  is minimal — `[workspace]` block only, no empty arrays or
  empty `[affinities]` table polluting the file. Round-trip
  parsing is unchanged because `#[serde(default)]` rehydrates
  the empty collections on read.

- **State.toml parsing.** The Phase 0 scaffold of
  `src/workspace/state.rs` (parser + 3 unit tests) goes live in
  Slice C. New `StateDeclaration::try_load_from_workspace(root)`
  with the same three-way semantics as the manifest loader:
  `Ok(None)` when no state.toml exists (every repo defaults),
  `Ok(Some)` when loaded, `Err` when malformed.
- **Global `--all` flag** on the `ws` namespace. Mirrors the shape
  of `--json`: extracted in `cli::dispatch_ws`, stripped from
  argv, threaded to each workspace Command through a dedicated
  field on the Command struct (the trait surface stays unchanged).
- **Hide-boring presentation pattern** for workspace commands.
  Workspaces can hold dozens or hundreds of child repos; raw
  enumeration produces unreadable output. Default behaviour:
  surface only "interesting" repos (pinned to a non-default
  branch by state.toml) individually; collapse repos on the
  default branch into a single count line. `--all` overrides the
  abbreviation. Threshold for unconditional expansion: total
  `≤ 5` repos. JSON consumers always get the full data —
  `--all` only affects the human form.
- **`git ws` enriched with state info.** Output now reports per-repo
  declared branches (state.toml override or manifest default),
  with the hide-boring abbreviation by default. Four rendering
  modes:
  - All repos on default → one summary line
    ("all 8 repos on default branch.").
  - Few pinned, many defaulted → list pinned, count defaulted.
  - Many pinned and many defaulted → counts only.
  - `--all` → every repo listed in full.
- **`current_repo.declared_branch`** in the JSON form: the
  effective branch state.toml declares for the cwd's child repo
  (or the manifest default if state.toml says nothing). Human
  form shows it inline:
  `Current repo: service-a (declared, state declares \`feat/x\`)`.

- **Manifest parsing.** The Phase 0 scaffold of
  `src/workspace/manifest.rs` (parser + validator + 4 unit tests)
  goes live in Slice B. New `Manifest::try_load_from_workspace(root)`
  with three-way semantics: `Ok(None)` when the manifest file does
  not exist (a workspace can be partially initialised), `Ok(Some)`
  when loaded and valid, `Err` when malformed. Matches Invariant 4
  (Manifest as Source of Truth) — a missing manifest is a state of
  incompleteness, not an error.
- **`git ws` enriched with manifest data.** When the workspace has
  a manifest, the bare `git ws` command shows the workspace name,
  default branch, and the comma-joined list of declared repos;
  the JSON form returns the structured summary. When there is no
  manifest yet, the human form announces the gap and the JSON
  omits the `manifest` field entirely.
- **`current_repo` reconciliation against the manifest.** The
  `current_repo` field on `WsContextOutput` is now
  `Option<CurrentRepo { name, declared }>`. `declared` is `true`
  when a repo with this name is in the manifest; `false` when the
  cwd matches the convention path (`<root>/src/<name>/`) but the
  manifest does not know about it — a hint that the directory is
  rogue, mistyped, or the manifest has fallen behind reality.
- **`ws` namespace.** Sibling to the `marshal` namespace; with
  marshal aliased to git, the user invokes workspace operations
  as `git ws <…>`. The choice of a separate top-level namespace
  (rather than nesting under `marshal` or intercepting plain
  `git` commands) is dictated by Invariant 9 (Developer Flow
  Preserved): workspace features are additive, opt-in, behind a
  recognisable prefix. A user who never types `ws` keeps git's
  exact behaviour.
- **Context detection.** `src/context.rs` is no longer
  `#![allow(dead_code)]`. `context::detect()` walks up the
  filesystem from cwd looking for a `.workspace/` marker — same
  pattern git uses for `.git/`. Returns the workspace root and
  identifies the current child repo by `<root>/src/<name>/…`
  convention (the manifest will refine this in Slice B).
- **`git ws` (no arg) — first ws subcommand.** Prints the
  current workspace context as `Workspace at: <root>` plus
  `Current repo: <name>` (or `(workspace root)`). Outside any
  workspace it errors cleanly with a helpful message and exits
  non-zero. JSON form: `{root, current_repo?}` — `current_repo`
  is skipped when at the workspace root.
- **`cli::dispatch_ws`** sibling of `dispatch_marshal`. Both
  extract `--json` and thread the format. The `main.rs`
  top-level routing gains a `ws` arm; a new `run_namespace`
  helper centralises the anyhow → ExitCode translation shared
  by both namespaces.

### Changed

- `cli::dispatch` is renamed to `cli::dispatch_marshal` to make
  room for `cli::dispatch_ws`.
- The marshal-namespace overview output (`git marshal` no arg)
  now advertises the `ws` namespace so users discover it without
  reading docs.
- `marshal help` overview topic now mentions the `ws` namespace
  alongside the marshal subcommands.

## [0.3.0] — 2026-04-27

Phase 1 complete. Marshal grew from a passthrough wrapper that emits
modernization tips into a tool with its own voice across stderr
(reactive hints) and stdout (proactive advice and help), backed by a
Strategy/Command substrate that makes adding a subcommand or output
mode mechanical. Five major capabilities shipped, all reachable
through `git marshal <…>` when aliased.

### Added

- **`marshal help` (and `marshal help <topic>`).** On-CLI reference
  built on the same Strategy + Registry pattern as the rest of the
  namespace. `Help` is a `Command` whose output (`HelpOutput`) carries
  a topic name, title, and a list of `HelpSection { heading, body[] }`.
  Implements `Renderable` for the human form and derives
  `serde::Serialize` so `--json` works automatically.
- **`HelpContext`** detects whether the cwd is inside a git repository
  (one cheap `git rev-parse --is-inside-work-tree` shellout, stdio
  silenced). Detection failures degrade to "outside" — the worst-case
  interpretation never makes the help wrong, only less specific.
- **Five topics shipped with the slice:**
  - `overview` — landing screen, context-aware first section: in a
    repo it recommends `marshal what-now` and `git status`; outside
    it recommends `git init` / `cd`. Lists subcommands, every
    configuration key (sourced from `ConfigKey::all()` so future
    keys auto-show), available topics, the `--json` global flag,
    and pointers to project + design docs.
  - `config` — three-tier model, every command form, both Unix and
    Windows paths per layer, every known key, env var overrides
    (`MARSHAL_CONFIG`, …), and the malformed-config robustness note.
  - `hints` — summary, output format, the `errors.actionable_hints`
    toggle, and a tabulated list of all 13 currently-shipped rule
    ids with the stderr substring each matches.
  - `modernize` — summary, output format, the two settings
    (`modernize.tips`, `modernize.rewrite`), and the four families
    covered (12 patterns / 11 rule impls).
  - `what-now` — summary, invocation forms (human / `--json`), the
    9-rule priority chain, and the data sources (`git status
    --porcelain=v2 --branch` + `<git-dir>/*` markers; explicit
    "no human-readable git output is parsed" disclaimer).
- **Sync guards** on `hints` and `what-now` topics: tests assert that
  every rule id shipped in `error_hints/rules/` and every priority
  step in `what_now/rules/` is mentioned in the corresponding help
  body. Adding a new rule without touching the topic body fails the
  test.

### Changed

- `cli::dispatch` gains a `Some("help")` arm calling
  `run_command(Help, args[1..], format)` — one registration line, no
  modification of any existing command (Invariant 10).
- The marshal-namespace overview output (`git marshal` no arg) now
  lists `help` alongside `config` and `what-now`.

### Added

- **Invariant 10 — Open/Closed via Strategy.** Promoted to `docs/PRINCIPLES.md` (`The Nine Invariants` → `The Ten Invariants`). Locks the de facto codebase pattern (Strategy + Registry across `modernize/`, `error_hints/`, `what_now/`, the config layer registry) as inviolable. CLAUDE.md reference updated.
- **Command + Renderable substrate** in `cli.rs`. `Command` trait with `type Output: Renderable + Serialize` and `fn run(args) -> Result<Output>`; `Renderable` trait writing the human form to a `&mut dyn Write`; `OutputFormat` enum (Human / Json); `run_command` helper that the dispatcher invokes once a concrete `Command` is selected.
- **Global `--json` flag** accepted anywhere in argv. The dispatcher strips it, sets the active `OutputFormat`, and threads the format into every subcommand. Concrete commands stay invariant — they never see `--json`. Adding a new marshal-namespace command lights up `--json` automatically.
- New dependency: `serde_json = "1.0"`.

### Changed

- `commands::config` is now a directory with one file per operation (`get.rs`, `set.rs`, `unset.rs`, `list.rs`, `helpers.rs`, `mod.rs`). Each operation implements `Command`; each carries its own typed output (`GetOutput`, `SetOutput`, `UnsetOutput`, `ListOutput` + `ListEntry`). `set` and `unset` stay silent on success in the human form (matches pre-migration behaviour); both emit a structured payload in JSON.
- `commands::what_now::run()` is replaced by a `WhatNow` `Command` impl. `Advice` gains `serde::Serialize` (derived) and an `impl Renderable for Advice` taking over what `Advice::render_to_stdout()` did. The previous "registry produced no advice" branch becomes an `anyhow::Error` (the catch-all `clean` rule guarantees this is unreachable).
- `cli::dispatch` is the only place where `--json` is recognised and the output format is selected. Per Invariant 10, this is the only place that needs to change to add a cross-cutting output mode.

### Added

- **`marshal what-now`.** Reads the cold state of the repository and
  prints one concrete next step on stdout. Reactive counterpart to
  the actionable error hints: hints fire on a failed git command,
  `what-now` is the user-invoked "what should I do here?" call.

  Built on the same Strategy + Registry pattern as `error_hints/`
  and `modernize/`:
  - `commands::what_now::state::RepoState` — snapshot built from
    `git status --porcelain=v2 --branch` (machine-readable, stable
    format) plus filesystem checks against `<git-dir>/MERGE_HEAD`,
    `rebase-merge/`, `rebase-apply/`, `CHERRY_PICK_HEAD`,
    `REVERT_HEAD`, `BISECT_LOG`. No human-readable git output is
    parsed. `BranchInfo` (name / detached / initial / upstream /
    ahead / behind), `WorkingTreeInfo` (staged / unstaged /
    untracked / unmerged counters), `InProgressOp` (None / Merge /
    Rebase / CherryPick / Revert / Bisect — Rebase wins over Merge
    when both markers are present).
  - `commands::what_now::rule::AdviceRule` — Strategy trait with
    `examine(&RepoState) -> Option<Advice>`. `Advice` carries a
    `rule_id`, a one-line title, and bullet suggestions. Renders to
    **stdout** (not stderr) — `what-now` is a user-invoked command
    and the advice is its output.
  - `commands::what_now::Registry` — `first_advice(&state)` returns
    the first matching rule's advice. Default seeded with the
    canonical chain.

  Nine rules in priority order:
  1. `merge-conflict` — abort command adapts to active op
     (rebase / cherry-pick / revert / merge).
  2. `*-in-progress` — one rule branching on the active op
     (rebase / cherry-pick / revert / bisect / paused-merge), each
     with its own continue / skip / abort triplet.
  3. `initial-state` — fresh repo, no commits.
  4. `detached-head` — front-loads `git switch -c <name>` so commits
     here don't get orphaned on switch.
  5. `uncommitted-changes` — title composes only the buckets that
     have files; suggestions drop irrelevant lines (no `git diff`
     when pure untracked, no `git add` when pure staged).
  6. `diverged` — both ahead and behind. `git pull --rebase`, push.
  7. `behind-upstream` — behind only.
  8. `unpushed-commits` / `unpushed-commits-no-upstream` — ahead
     only, two distinct shapes for "have upstream → `git push`"
     versus "no upstream → `git push -u origin <branch>`".
  9. `clean` — catch-all so every state produces advice.
- **CLI dispatch** — `Some("what-now")` arm in `cli::dispatch`,
  plus an entry in the `git marshal` overview output so users
  discover it.

### Added

- **Actionable error hints.** When `git` exits non-zero and the captured
  stderr matches a known failure shape, Marshal appends a short hint
  to stderr below git's own message — a one-line title and a list of
  concrete next steps. Hints fire only on git failures and never
  modify git's own output.

  Thirteen rules in this cycle, organised by failure domain:

  *Repository / setup:*
  - `not-a-git-repository` — points the user at `git init` (new project)
    or `cd` (existing project).
  - `dubious-ownership` — explains that git refuses for security and
    surfaces both `safe.directory <path>` (per-repo) and
    `safe.directory '*'` (less secure, all directories) options.
  - `empty-ident` — fires on empty author identity / `Author identity
    unknown` / `Please tell me who you are`. Surfaces the exact
    `git config --global user.name/user.email` commands and the
    per-repo variant.

  *Authentication:*
  - `ssh-publickey-denied` — walks through `ssh-add -l`, host-side key
    registration, and an `ssh -T` connectivity check.
  - `https-auth-failed` — fires on `Authentication failed for 'https://…'`.
    Anchored on the `'https` prefix to stay off the SSH path. Hint
    covers PAT generation (passwords no longer accepted by GitHub since
    2021), switching the remote to SSH with `git remote set-url`, and
    setting up a `credential.helper`.

  *Network:*
  - `host-resolution-failed` — fires on `Could not resolve host`. Hint
    walks connectivity / VPN-hijacked DNS, typoed remote URL
    (`git remote -v`), and direct verification with `ping` / `nslookup`.

  *Push lifecycle (all gated on `parsed.subcommand_is("push")`):*
  - `push-non-fast-forward` — recommends `git pull --rebase && git push`
    with `--force-with-lease` (never plain `--force`) as the deliberate
    alternative.
  - `upstream-not-configured` — first push of a new branch. Surfaces
    `git push -u origin <branch>` and the `git branch --show-current`
    helper.
  - `src-refspec-no-match` — push has nothing to send. Three remediations
    walked: no commits yet (`git log --oneline -1`), wrong current
    branch (`git branch --show-current`), or detached HEAD
    (`git switch -c <new-name>`).

  *Pathspec / refs:*
  - `pathspec-no-match` — fires on `pathspec '…' did not match any file`.
    Common across `git checkout/switch/restore/add` when the user typoed
    a path or referenced a brand-new file before adding it.
  - `ambiguous-argument` — fires on `ambiguous argument '…': unknown
    revision or path`. Front-loads `git fetch` (most common cause is a
    branch/tag that exists on the remote but hasn't been fetched).

  *Working tree / merge:*
  - `local-changes-would-be-overwritten` — fires on the canonical refusal
    from `checkout`/`switch`/`pull`/`merge`/`rebase` when uncommitted
    changes block the operation. Walks through stash, commit, and `git
    restore` with the irreversibility called out.
  - `unrelated-histories` — front-loads the "did you pick the wrong
    branch?" check before mentioning `--allow-unrelated-histories`,
    since the flag is rarely the right answer.
- **`error_hints/` Strategy registry.** New module with the same shape
  as `modernize/`: `ErrorHintRule` trait, `Registry`, a `Hint` value
  type that renders to stderr in the canonical
  `marshal: hint: <title>\n  • <action>` format. Adding a rule is one
  trait impl plus one line in `register_defaults` — OCP respected.
- **`errors.actionable_hints` config key** (default `true`). Setting it
  to `false` restores byte-exact passthrough: stderr inheritance is
  turned back on and the registry is not walked.
- **Stderr capture mode in passthrough.** New `capture_stderr` argument
  on `commands::passthrough::run_returning_outcome`. When `true`,
  stderr is piped through a worker thread that forwards each chunk to
  our stderr live (preserving streaming for clone/push/fetch progress)
  and retains a copy in a 256 KiB-capped buffer for post-invocation
  pattern matching. When `false` (used by inherit-only call sites and
  by the opt-out path), the existing `Stdio::inherit()` path runs
  unchanged.
- **`Outcome::Ran` is now a struct variant** carrying both `status` and
  the optional captured stderr buffer. Existing call sites that only
  need the exit status (the `--version` augmentation) destructure with
  `..` and are unaffected.

### Changed

- `main.rs` reads `errors.actionable_hints` once per invocation and
  uses it to gate both the stderr capture mode and the registry walk.
  When the run fails and a hint matches, the hint is emitted to
  stderr after git's own output; on success or with the feature
  disabled, behaviour is identical to `0.2.0`.

### Architecture

- **Invariant 10 (Open/Closed via Strategy)** added to
  `docs/PRINCIPLES.md`. The Nine Invariants become the Ten Invariants;
  CLAUDE.md is updated. Locks the de facto codebase pattern (Strategy
  + Registry across `modernize/`, `error_hints/`, `what_now/`,
  `commands/help/`, the `ConfigSource` registry) as inviolable. The
  Command + Renderable substrate, the `--json` flag, and `marshal
  help` were all designed and shipped under this invariant.

### Release notes

- Phase 1 shipped end-to-end across `0.2.0` and `0.3.0`. The 0.3.0
  cycle covered: 13 actionable error hints (still ~7 short of the
  planned ~20, but covering the high-friction failures —
  `not-a-git-repository`, `dubious-ownership`, `empty-ident`,
  SSH publickey + HTTPS auth + DNS resolution, three push-lifecycle
  rules, pathspec + ambiguous-argument, working-tree refusal,
  unrelated-histories merge), `marshal what-now`, the
  Strategy/Command refactor, global `--json`, and `marshal help`.
- Tagged on `main` as `v0.3.0`. Not published to crates.io in this
  release; publication will be automated from a future GitHub Actions
  workflow. Until then, install from source: `cargo install --git
  https://github.com/matutetandil/marshal --tag v0.3.0`.
- Test count at 0.3.0: 228 unit + 42 integration = 270 (up from 137
  at 0.2.0).
- The remaining low-frequency hint rules (cannot lock ref,
  branch -D not fully merged, dubious permissions on `~/.ssh`,
  bad revision, …) and human-output colourisation are deferred to
  future cycles — they no longer block Phase 1.

## [0.2.0] — 2026-04-24

First slice of Phase 1 shipped. Marshal is no longer a pure passthrough —
it speaks in its own voice through the `marshal` subcommand namespace,
emits modernization tips for legacy Git forms, and has a full three-tier
configuration system. Passthrough fidelity is still the default for any
invocation marshal does not intercept.

### Added

- **Command interception architecture.**
  - New `git::parser` module splits a Git invocation's argv into
    `global_flags`, `subcommand`, and `subcommand_args`, handling
    value-taking global options (`-C`, `-c`, `--git-dir`, `--work-tree`,
    …) with both `--opt=value` and `--opt value` forms. Arguments stay
    as `OsString` throughout so non-UTF-8 paths on Unix and wide-char
    arguments on Windows survive intact.
  - `main` now parses argv and routes: if the first subcommand is
    literally `marshal`, dispatch to marshal's own namespace; otherwise,
    consult the modernization registry, then forward to git.
- **`marshal` subcommand namespace.** `git marshal` (when aliased) or
  `marshal marshal` (direct) routes to marshal's own commands without
  reaching `git`. Unknown marshal subcommands exit with a clear error
  instead of being forwarded.
- **Modernization rules as Strategy + registry.** New `modernize` module
  with a `ModernizationRule` trait, a `Registry`, and 11 rule impls
  covering the 12 canonical legacy Git forms Git itself treats as
  deprecated or succeeded:
  - **checkout → switch/restore** (Git 2.23 split, 8 patterns): `-b`,
    `-B`, `--orphan`, `--detach`, `<commit> -- <files>`,
    `HEAD [--] <files>`, `-- <files>`, bare `<branch>`.
  - **reset → restore --staged** (file-mode): `reset [HEAD] <files>`.
  - **stash save → stash push** (single rule covering both `save` and
    `save -u`; deprecated since Git 2.16).
  - **remote rm → remote remove**.

  Rules match disjoint patterns (first-match-wins is safe), preserve
  any global flags (`git -C /tmp checkout -b X` rewrites correctly), and
  carry an optional one-line historical note surfaced in the tip.
  Adding a new rule is one trait impl plus one registration line —
  OCP respected.
- **Tip emission on stderr.** When a rule fires, a canonical one-line
  tip (with optional second-line historical note) is emitted to stderr
  **before** git runs. Stdout is never touched. Example:

      marshal: tip: try `git switch -c feat/auth` instead of `git checkout -b feat/auth`
                   `switch` was split out of `checkout` in Git 2.23 for branch-only operations.

- **Optional argv rewriting.** When `modernize.rewrite = true`, marshal
  substitutes the rewritten argv before running git, so the modern form
  is what actually executes. Default off (Invariant 8, Conservative
  Defaults).
- **Three-tier configuration system** at `src/config/`. Mirrors Git's
  own `system < global < local` model:
  - **System** (`/etc/marshal/config.toml` / `%ProgramData%\marshal\config.toml`).
  - **Global** (`$XDG_CONFIG_HOME/marshal/config.toml` /
    `%APPDATA%\marshal\config.toml`).
  - **Local** (`<git-dir>/marshal/config.toml`; per-clone, inside `.git/`).

  Each layer is a `ConfigSource` Strategy; the `ConfigResolver` merges
  them with `Option<T>` field semantics (unset at layer → fall through).
  Every path can be overridden by `MARSHAL_CONFIG`,
  `MARSHAL_SYSTEM_CONFIG`, and `MARSHAL_LOCAL_CONFIG` respectively, used
  by tests and power users.
- **`marshal config` command.** `get|set|unset|list`, with
  `--system|--global|--local` flags on write operations (default:
  `--global`) and `--show-origin` on `get` (tab-separated
  `<level>\t<value>`, or `default\t<value>` when no layer has the key).
  Atomic write-then-rename protects against partial-write corruption.
- **`--version` augmentation.** `git --version` now prints git's version
  line verbatim, followed by `marshal version X.Y.Z` on stdout.
  Mirrors node+npm, php+xdebug. Only triggers when git exits
  successfully.
- **Two config keys** to start with: `modernize.tips` (default `true`,
  silences all tips when `false`) and `modernize.rewrite` (default
  `false`).

### Changed

- `main.rs` now threads through: parser → marshal-namespace route →
  effective-config load → modernize hook → passthrough. A malformed
  config file falls back to defaults with a single-line warning on
  stderr rather than aborting the command.
- `commands::passthrough::run` kept its signature; new
  `Outcome` enum and `run_returning_outcome` added so `main` can
  inspect the exit status (used by the `--version` gate).
- `cli.rs` rewritten from the Phase 2 speculative scaffold to the
  `marshal` namespace dispatcher. The Phase 2 workspace commands
  (`init`, `status`, `log`, `clone`) are no longer reachable from
  `main` in 0.2.x; they keep `#![allow(dead_code)]` until Phase 2
  wires them in properly.

### Portability

- Every config source (`system`, `global`, `local`) handles Windows and
  Unix path conventions. Local-layer discovery is pure filesystem (walk
  up looking for `.git`, follow worktree `gitdir:` pointers) — no shell
  out to `git rev-parse`.
- Unit tests that mutate process-global env vars acquire a shared
  `ENV_MUTEX` before `set_var`/`remove_var` to prevent races between
  parallel tests, which only became visible after step 5b added a
  second env-manipulating test module.
- Integration tests isolate all three config env vars
  (`MARSHAL_CONFIG`, `MARSHAL_SYSTEM_CONFIG`, `MARSHAL_LOCAL_CONFIG`)
  from the host machine. Prevents test runs from reading or writing
  any real config file on the developer's box.

### Release notes

- Tagged on `main` as `v0.2.0`. Not published to crates.io in this
  release; publication will be automated from a future GitHub Actions
  workflow. Until then, install from source: `cargo install --git
  https://github.com/matutetandil/marshal --tag v0.2.0`.
- Test count at 0.2.0: 114 unit + 23 integration = 137 (up from 23 at
  0.1.0).

## [0.1.0] — 2026-04-24

Phase 0 shipped. Marshal is now a transparent Git passthrough: aliased to
`git`, every invocation is forwarded with byte-exact fidelity.

### Added
- **Pure passthrough wrapper.** The `marshal` binary forwards every invocation
  to `git` verbatim: arguments are preserved as `OsString` (so non-UTF-8 paths
  and wide-char Windows args survive), stdin/stdout/stderr are inherited
  directly from the parent process, and `git`'s exit code is propagated
  exactly. On Unix, death-by-signal follows the shell convention `128 + signum`.
  When aliased to `git`, the binary is indistinguishable from Git itself.
- Integration tests that compare `marshal <args>` against `git <args>`
  byte-for-byte on a representative set of invocations (version, status,
  unknown subcommand, commit round-trip, Unicode arguments).
- Modernization policy documented in `docs/ROADMAP.md`: the wrapper
  suggests modern command equivalents (e.g. `checkout -b` → `switch -c`)
  but does not rewrite user input by default. Rewrite is opt-in via
  configuration. (Implementation ships with `0.2.0`.)

### Changed
- Release cadence refined: milestones now slice phases into smaller,
  self-contained releases. `0.1.0` is pure alias/passthrough only; Phase 1
  UX work ships across `0.2.0` and `0.3.0`; read-only workspace (Phase 2)
  lands in `0.4.0`.
- Context detection moved from Phase 0 to Phase 2, where it is actually
  consumed. Avoids writing unreachable code in `0.1.0`.
- `main.rs` goes straight to passthrough; `cli.rs`, `context.rs`, and the
  workspace command scaffolds are kept in the tree and compile, but are not
  wired into `main` in `0.1.0`. They are re-enabled in later releases.
- `src/git/mod.rs` dropped its `run_interactive` helper; the passthrough path
  owns its own `Command` construction to keep behavior and responsibility in
  one place.

### Portability
- Marshal's portability contract added to `README.md`: the binary must run
  wherever Git runs (Windows, macOS, Linux; x86_64 and ARM64). The passthrough
  implementation honours this by relying only on `std::process::Command`,
  `std::ffi::OsString`, and inherited stdio — no platform-specific assumptions.
- **Cross-platform CI pipeline** (`.github/workflows/ci.yml`). Every push to
  `main` and every PR runs `cargo build --release` and `cargo test` natively
  on four runners: Linux x86_64, Linux ARM64, macOS ARM64, Windows x86_64.
  macOS x86_64 is covered by a dedicated `cross-build` job that produces the
  Intel binary from the macOS ARM64 runner — the hosted `macos-13` pool is
  being wound down and queue times are unreliable; cross-compiling verifies
  the toolchain still produces a valid `x86_64-apple-darwin` artifact, which
  is the failure mode we care about for `std`-only Rust code. `cargo fmt
  --check` and `cargo clippy --all-targets -- -D warnings` run once on Linux
  x86_64. `--locked` is used everywhere so `Cargo.lock` is the contract.
  Windows ARM64 is deferred until the hosted runner leaves preview.
- `#![allow(dead_code)]` applied to Phase 2+ scaffolded modules
  (`context.rs`, `workspace/*.rs`) so the CI can enforce `clippy -D warnings`
  against the live 0.1.0 code without false positives from scaffolded code
  that will be consumed in later releases.

### Release notes
- Tagged on `main` as `v0.1.0`. Not published to crates.io in this release;
  publication will be automated from a future GitHub Actions workflow when
  it makes sense to push a build to the registry. Until then, install from
  source: `cargo install --git https://github.com/matutetandil/marshal --tag v0.1.0`.

## [0.0.0-reserved] — 2026-04-24

Name reservation on [crates.io](https://crates.io/crates/marshal). Contains no
functional code; exists only to claim the `marshal` crate name for the
project. Real releases begin at `0.1.0`.

Published from branch `release/0.0.0-reserved` and tagged `v0.0.0-reserved`.
Not merged to `main` by design — the branch is an isolated one-off publish,
while `main` continues with the Phase 0 scaffold.

[Unreleased]: https://github.com/matutetandil/marshal/compare/v0.3.0...HEAD
[0.3.0]: https://github.com/matutetandil/marshal/compare/v0.2.0...v0.3.0
[0.2.0]: https://github.com/matutetandil/marshal/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/matutetandil/marshal/compare/v0.0.0-reserved...v0.1.0
[0.0.0-reserved]: https://github.com/matutetandil/marshal/releases/tag/v0.0.0-reserved
