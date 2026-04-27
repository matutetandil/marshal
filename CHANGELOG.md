# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Work in progress on `0.4.0` — Phase 2 (read-only workspace
operations). First slice shipped: the `ws` namespace exists and
context detection is live. Next: manifest + state.toml parsing,
`ws init`, `ws status`, `ws log`, `ws diff`, scope inference,
the `--explain` flag, `ws clone`. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

### Added

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
