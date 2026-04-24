# Marshal

> Looks like a monorepo for the developer. Is multi-repo administered underneath, with granular scope.

A Git workspace manager that gives you the ergonomics of a monorepo while keeping the architectural benefits of independent repositories.

**Marshal** (verb): *to arrange, organize, and coordinate resources or entities into effective formation*. That's what this tool does — it marshals independent Git repositories into a coherent workspace.

## What is this?

Git does many things well, but coordinating multiple related repositories isn't one of them. Submodules are painful, subtrees are confusing, and the ecosystem's answer has been to push everyone into monorepos — often for tooling reasons rather than architectural ones.

This tool proposes a different path: keep your repositories independent, but give developers an experience that feels unified. Like microservices feel monolithic to end users while remaining distributed underneath, a workspace feels like a monorepo to developers while remaining a coordinated set of independent Git repositories.

## Status

🚧 **Early development.** Phase 0 shipped as `0.1.0`: pure alias/passthrough. Aliased to `git`, the binary forwards every invocation to the real `git` verbatim — same stdout, same stderr, same exit code, same behavior for interactive commands. No context detection, no command rewriting, no workspace logic yet; those arrive in later releases (see [`docs/ROADMAP.md`](docs/ROADMAP.md)).

### Portability

Marshal must compile and run wherever Git does: Windows, macOS, and Linux, on both x86_64 and ARM64. The passthrough implementation is portable by construction — it shells out to `git` via the OS `PATH` and inherits stdio directly, so there are no platform-specific assumptions embedded in the wrapper. CI validates every commit against this matrix (native tests on Linux x86_64/ARM64, macOS ARM64, Windows x86_64; cross-build for macOS x86_64).

## Design Principles

1. **Looks like monorepo, is multi-repo.** The developer experience mirrors working in a monorepo; the storage reality is N independent Git repositories.
2. **Git recursive.** Everything the workspace does is Git applied one level up. No new paradigms.
3. **Wrapper, not replacement.** Git remains the source of truth. The tool orchestrates, observes, and reports — never invents mechanisms Git already provides.
4. **Zero lock-in.** Every operation translates to pure Git commands. Uninstall the tool and your repos are untouched.
5. **Opt-in workspace features.** Developers can work normally without ever invoking workspace-specific commands. The coordination layer is there for those who need it.

See [`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) for the invariants that govern all design decisions.

## Quick concept

```
my-workspace/                    # workspace repo (git)
├── .workspace/
│   ├── manifest.toml            # which repos, affinities, groups
│   └── state.toml               # declared state per workspace-branch
├── docs/, Dockerfile, etc.      # workspace-level content
└── src/
    ├── service-a/               # independent git repo
    ├── service-b/               # independent git repo
    └── shared-lib/              # independent git repo
```

The workspace repo has branches. Each branch declares what state the child repos should be in. Developers work inside the child repos with plain Git; the wrapper observes and helps coordinate when asked.

## Documentation

- [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) — full system design
- [`docs/PRINCIPLES.md`](docs/PRINCIPLES.md) — invariants and rules (read this first)
- [`docs/GLOSSARY.md`](docs/GLOSSARY.md) — terminology
- [`docs/ROADMAP.md`](docs/ROADMAP.md) — development phases

## License

MIT — see [`LICENSE`](LICENSE).

## Releases

- `0.0.0-reserved` — name reservation on [crates.io](https://crates.io/crates/marshal). No functional code. Tagged as [`v0.0.0-reserved`](https://github.com/matutetandil/marshal/releases/tag/v0.0.0-reserved) on branch `release/0.0.0-reserved`.
- `0.1.0` — 2026-04-24. Phase 0 complete. Pure alias/passthrough. Tagged as [`v0.1.0`](https://github.com/matutetandil/marshal/releases/tag/v0.1.0). Not yet published to crates.io — publication will be automated from GitHub when it's time. Install from source meanwhile: `cargo install --git https://github.com/matutetandil/marshal --tag v0.1.0`.
