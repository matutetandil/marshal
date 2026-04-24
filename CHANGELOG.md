# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Work begins on `0.3.0` — completing Phase 1: actionable error messages for
common Git failures, context-aware `help`, the `what-now` command, and
JSON output modes. See [`docs/ROADMAP.md`](docs/ROADMAP.md).

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

[Unreleased]: https://github.com/matutetandil/marshal/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/matutetandil/marshal/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/matutetandil/marshal/compare/v0.0.0-reserved...v0.1.0
[0.0.0-reserved]: https://github.com/matutetandil/marshal/releases/tag/v0.0.0-reserved
