# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and this project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

Phase 0 implementation underway — target release `0.1.0`. See
[`docs/ROADMAP.md`](docs/ROADMAP.md) for the active phase and its scope.

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
  self-contained releases. `0.1.0` becomes pure alias/passthrough only;
  Phase 1 UX work ships across `0.2.0` and `0.3.0`; read-only workspace
  (Phase 2) lands in `0.4.0`.
- Context detection moved from Phase 0 to Phase 2, where it is actually
  consumed. Avoids writing unreachable code in `0.1.0`.
- `main.rs` now goes straight to passthrough; `cli.rs`, `context.rs`, and the
  workspace command scaffolds are kept in the tree and compile, but are not
  wired into `main` in `0.1.0`. They will be re-enabled in later releases.
- `src/git/mod.rs` dropped its `run_interactive` helper; the passthrough path
  owns its own `Command` construction to keep behavior and responsibility in
  one place.

### Portability
- Marshal's portability contract added to `README.md`: the binary must run
  wherever Git runs (Windows, macOS, Linux; x86_64 and ARM64). The passthrough
  implementation honours this by relying only on `std::process::Command`,
  `std::ffi::OsString`, and inherited stdio — no platform-specific assumptions.

## [0.0.0-reserved] — 2026-04-24

Name reservation on [crates.io](https://crates.io/crates/marshal). Contains no
functional code; exists only to claim the `marshal` crate name for the
project. Real releases begin at `0.1.0`.

Published from branch `release/0.0.0-reserved` and tagged `v0.0.0-reserved`.
Not merged to `main` by design — the branch is an isolated one-off publish,
while `main` continues with the Phase 0 scaffold.

[Unreleased]: https://github.com/matutetandil/marshal/compare/v0.0.0-reserved...HEAD
[0.0.0-reserved]: https://github.com/matutetandil/marshal/releases/tag/v0.0.0-reserved
