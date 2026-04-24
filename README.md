# Marshal

> Parece monorepo para el dev. Es multi-repo administrado por debajo, con granular scope.

> **Note:** The `0.0.0-reserved` release on crates.io is a name reservation only — it contains no functional code. Real releases begin at `0.1.0`.

A Git workspace manager that gives you the ergonomics of a monorepo while keeping the architectural benefits of independent repositories.

**Marshal** (verb): *to arrange, organize, and coordinate resources or entities into effective formation*. That's what this tool does — it marshals independent Git repositories into a coherent workspace.

## What is this?

Git does many things well, but coordinating multiple related repositories isn't one of them. Submodules are painful, subtrees are confusing, and the ecosystem's answer has been to push everyone into monorepos — often for tooling reasons rather than architectural ones.

This tool proposes a different path: keep your repositories independent, but give developers an experience that feels unified. Like microservices feel monolithic to end users while remaining distributed underneath, a workspace feels like a monorepo to developers while remaining a coordinated set of independent Git repositories.

## Status

🚧 **Early development.** Design is solidified (see [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md)), implementation is starting.

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
