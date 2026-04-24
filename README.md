# Marshal

> Looks like a monorepo for the developer. Is multi-repo administered underneath, with granular scope.

A Git workspace manager that gives you the ergonomics of a monorepo while keeping the architectural benefits of independent repositories.

**Marshal** (verb): *to arrange, organize, and coordinate resources or entities into effective formation*. That's what this tool does — it marshals independent Git repositories into a coherent workspace.

## What is this?

Git does many things well, but coordinating multiple related repositories isn't one of them. Submodules are painful, subtrees are confusing, and the ecosystem's answer has been to push everyone into monorepos — often for tooling reasons rather than architectural ones.

This tool proposes a different path: keep your repositories independent, but give developers an experience that feels unified. Like microservices feel monolithic to end users while remaining distributed underneath, a workspace feels like a monorepo to developers while remaining a coordinated set of independent Git repositories.

## Status

🚧 **Early development.** Currently on `0.2.0`: the first slice of Phase 1 lands on top of the `0.1.0` passthrough core. Marshal now speaks in its own voice (through the `marshal` subcommand namespace), emits modernization tips for deprecated Git forms, and has a three-tier configuration system mirroring Git's own `system < global < local` model. Any command Marshal does not intercept still passes through to `git` byte-for-byte. Workspace features arrive in later releases — see [`docs/ROADMAP.md`](docs/ROADMAP.md).

### Portability

Marshal must compile and run wherever Git does: Windows, macOS, and Linux, on both x86_64 and ARM64. The passthrough implementation is portable by construction — it shells out to `git` via the OS `PATH` and inherits stdio directly, so there are no platform-specific assumptions embedded in the wrapper. CI validates every commit against this matrix (native tests on Linux x86_64/ARM64, macOS ARM64, Windows x86_64; cross-build for macOS x86_64).

## Wrapper features (0.2.0)

When Marshal is aliased to `git`, almost every invocation passes through unchanged. A small number of behaviours sit on top.

### Modernization tips

Legacy Git forms get a one-line tip on stderr pointing at the modern equivalent. The command still runs as typed:

```
$ git checkout -b feat/auth
marshal: tip: try `git switch -c feat/auth` instead of `git checkout -b feat/auth`
             `switch` was split out of `checkout` in Git 2.23 for branch-only operations.
Switched to a new branch 'feat/auth'
```

Covered families: `checkout → switch/restore` (Git 2.23 split, 8 patterns), `reset <file> → restore --staged`, `stash save → stash push`, `remote rm → remote remove`. Stdout is never touched — pipes stay clean.

Tips can be silenced, or — if you prefer — replaced with automatic rewriting:

```
git marshal config set modernize.tips false         # silence tips
git marshal config set modernize.rewrite true       # rewrite to the modern form before running
```

### Three-tier configuration

Mirrors Git: `system < global < local`, precedence flowing left to right.

```
git marshal config get modernize.tips
git marshal config get --show-origin modernize.tips   # shows which layer won
git marshal config set --system modernize.tips false  # machine-wide (needs sudo on Unix)
git marshal config set --global modernize.tips true   # per-user
git marshal config set --local modernize.rewrite true # per-repo (inside .git/marshal/)
git marshal config list
```

| Level    | Unix                                      | Windows                               |
|----------|-------------------------------------------|---------------------------------------|
| system   | `/etc/marshal/config.toml`                | `%ProgramData%\marshal\config.toml`   |
| global   | `$XDG_CONFIG_HOME/marshal/config.toml`    | `%APPDATA%\marshal\config.toml`       |
| local    | `<git-dir>/marshal/config.toml`           | same (under the repo's `.git/`)       |

Every path can be overridden by the corresponding env var (`MARSHAL_CONFIG`, `MARSHAL_SYSTEM_CONFIG`, `MARSHAL_LOCAL_CONFIG`). A malformed config file does not abort the command — Marshal warns once to stderr and falls back to defaults.

### Version line

`git --version` identifies every tool in the chain, node+npm / php+xdebug style:

```
$ git --version
git version 2.50.1 (Apple Git-155)
marshal version 0.2.0
```

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
- `0.1.0` — 2026-04-24. Phase 0 complete. Pure alias/passthrough. Tagged as [`v0.1.0`](https://github.com/matutetandil/marshal/releases/tag/v0.1.0).
- `0.2.0` — 2026-04-24. First slice of Phase 1: command interception, 11 modernization rules covering the 12 canonical Git deprecations, three-tier config system, `--version` augmentation. Tagged as [`v0.2.0`](https://github.com/matutetandil/marshal/releases/tag/v0.2.0). Not yet published to crates.io — publication will be automated from GitHub when it's time. Install from source: `cargo install --git https://github.com/matutetandil/marshal --tag v0.2.0`.
