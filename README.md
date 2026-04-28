# Marshal

> Looks like a monorepo for the developer. Is multi-repo administered underneath, with granular scope.

Marshal is a Git workspace manager. It gives developers the ergonomics of a monorepo — switch a branch, commit, push, get a unified view — on top of independent Git repositories that each keep their own history, remote, and CI. Microservices proved the same idea for runtime architecture; Marshal applies it to source control.

## Why

Git handles single repositories well. Coordinating multiple related repositories is where it falls short — submodules are painful, subtrees are confusing, and the ecosystem's answer has been to push everyone into monorepos for tooling reasons rather than architectural ones. Marshal is the missing coordination layer: independent repositories at rest, unified workflow at use.

## Status

🚧 **Early development.** Currently at `v0.4.0` — Phase 2 complete (read-only workspace).

What's shipped today is the wrapper layer that sits on top of plain `git`, plus the workspace observation layer: detect, inspect, and clone workspaces without modifying anything beyond the manifest/state files that `ws init` and `ws clone` create on first use. Modifications open in the next phase. When Marshal is aliased to `git`, every plain-git invocation still passes through unchanged unless Marshal has something useful to add.

| Phase                         | Status                          | What it covers                                                                                                       |
|-------------------------------|---------------------------------|----------------------------------------------------------------------------------------------------------------------|
| 0 — Foundation                | ✅ shipped (`v0.1.0`)            | Pure alias/passthrough, cross-platform CI, release plumbing.                                                         |
| 1 — Wrapper UX                | ✅ shipped (`v0.2.0` + `v0.3.0`) | Modernization tips, actionable error hints, three-tier config, `marshal what-now`, `marshal help`, `--json` everywhere. |
| 2 — Workspace (read-only)     | ✅ shipped (`v0.4.0`)            | `git ws` namespace: `init`, `status`, `log`, `diff`, `clone` (parallel children with progress bars), scope inference, `--explain`. |
| 3+ — Workspace (full)         | 📋 designed                     | The three zones, branching, coordinated push/pull, oplog, undo, differentiating features.                            |

See [`docs/ROADMAP.md`](docs/ROADMAP.md) for the full plan.

## Install

```sh
cargo install --git https://github.com/matutetandil/marshal --tag v0.4.0
alias git=marshal
```

Verify:

```sh
$ git --version
git version 2.50.1
marshal version 0.4.0
```

Run `git marshal help` for the rest of the surface.

## Portability

Marshal compiles and runs wherever Git does — Linux, macOS, Windows on x86_64 and ARM64. Every commit is validated on the full matrix in CI.

## Documentation

- [`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) — the ten invariants and the Git Recursive principle. **Start here.**
- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — full system design.
- [`docs/GLOSSARY.md`](docs/GLOSSARY.md) — terminology.
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — development phases and release schedule.

For the in-CLI reference (every command, every config key, every hint and advice rule), run `git marshal help` and `git marshal help <topic>`.

## License

MIT — see [`LICENSE`](LICENSE).

## Releases

| Tag | Date | Highlights |
|-----|------|------------|
| [`v0.0.0-reserved`](https://github.com/matutetandil/marshal/releases/tag/v0.0.0-reserved) | 2026-04-24 | Name reservation on crates.io. No functional code. |
| [`v0.1.0`](https://github.com/matutetandil/marshal/releases/tag/v0.1.0)                   | 2026-04-24 | Phase 0 — pure alias/passthrough.                  |
| [`v0.2.0`](https://github.com/matutetandil/marshal/releases/tag/v0.2.0)                   | 2026-04-24 | First slice of Phase 1.                            |
| [`v0.3.0`](https://github.com/matutetandil/marshal/releases/tag/v0.3.0)                   | 2026-04-27 | Phase 1 complete.                                  |
| [`v0.4.0`](https://github.com/matutetandil/marshal/releases/tag/v0.4.0)                   | 2026-04-28 | Phase 2 complete — read-only workspace.            |

Not yet published to crates.io. Install from source: `cargo install --git https://github.com/matutetandil/marshal --tag <tag>`.
