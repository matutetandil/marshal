# Marshal

> Looks like a monorepo for the developer. Is multi-repo administered underneath, with granular scope.

A Git workspace manager that gives you the ergonomics of a monorepo while keeping the architectural benefits of independent repositories.

**Marshal** (verb): *to arrange, organize, and coordinate resources or entities into effective formation*. That's what this tool does — it marshals independent Git repositories into a coherent workspace.

## What is this?

Git does many things well, but coordinating multiple related repositories isn't one of them. Submodules are painful, subtrees are confusing, and the ecosystem's answer has been to push everyone into monorepos — often for tooling reasons rather than architectural ones.

This tool proposes a different path: keep your repositories independent, but give developers an experience that feels unified. Like microservices feel monolithic to end users while remaining distributed underneath, a workspace feels like a monorepo to developers while remaining a coordinated set of independent Git repositories.

## Status

🚧 **Early development.** `0.2.0` is the latest tagged release. Work is in progress on `0.3.0` (closing out Phase 1) — the first six actionable error hints already landed on `main`. Marshal speaks in its own voice (through the `marshal` subcommand namespace), emits modernization tips for deprecated Git forms, has a three-tier configuration system mirroring Git's own `system < global < local` model, and appends actionable hints below git's own stderr when common commands fail. Any command Marshal does not intercept still passes through to `git`. Workspace features arrive in later releases — see [`docs/ROADMAP.md`](docs/ROADMAP.md).

### Portability

Marshal must compile and run wherever Git does: Windows, macOS, and Linux, on both x86_64 and ARM64. The passthrough implementation is portable by construction — it shells out to `git` via the OS `PATH` and inherits stdio directly, so there are no platform-specific assumptions embedded in the wrapper. CI validates every commit against this matrix (native tests on Linux x86_64/ARM64, macOS ARM64, Windows x86_64; cross-build for macOS x86_64).

## Wrapper features

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

### Actionable error hints

When `git` exits non-zero with a recognised failure, Marshal appends a short hint to stderr below git's own message — a one-line title and a list of concrete next steps:

```
$ git status                              # outside any repository
fatal: not a git repository (or any of the parent directories): .git
marshal: hint: this directory is not inside a Git repository.
  • If this is a new project, run `git init` to start one here.
  • If you meant to work in an existing repo, `cd` into it first.
```

Thirteen rules ship today, covering most of the high-friction Git failures:

| Rule                                  | Fires on                                                                       |
|---------------------------------------|--------------------------------------------------------------------------------|
| `not-a-git-repository`                | `fatal: not a git repository …`                                                |
| `dubious-ownership`                   | `detected dubious ownership in repository at …`                                |
| `empty-ident`                         | empty author identity / `Author identity unknown`                              |
| `ssh-publickey-denied`                | `Permission denied (publickey)` from a Git SSH remote                          |
| `https-auth-failed`                   | `fatal: Authentication failed for 'https://…'`                                 |
| `host-resolution-failed`              | `Could not resolve host` (DNS / network / VPN)                                 |
| `push-non-fast-forward`               | `git push` rejected because the remote moved ahead                             |
| `upstream-not-configured`             | first push of a new branch — no upstream configured                            |
| `src-refspec-no-match`                | push has nothing to send (no commits / wrong branch / detached HEAD)           |
| `pathspec-no-match`                   | `pathspec '…' did not match any file`                                          |
| `ambiguous-argument`                  | `ambiguous argument: unknown revision or path`                                 |
| `local-changes-would-be-overwritten`  | `checkout`/`switch`/`pull`/`merge`/`rebase` blocked by uncommitted changes     |
| `unrelated-histories`                 | `refusing to merge unrelated histories`                                        |

Hints fire only on git failures (exit ≠ 0) and never modify git's own output. Disable with:

```
git marshal config set errors.actionable_hints false
```

Disabling restores byte-exact passthrough — stderr inheritance is turned back on so even Marshal's stderr capture goes away.

### `marshal what-now`

The proactive counterpart to error hints: read the cold state of the repository and print one concrete next step. Useful when you walk into a repo and want a one-line "what was I doing?" without scanning `git status`.

```
$ git marshal what-now
Working tree has 2 staged, 1 unstaged, and 3 untracked changes.
  • Review changes: `git diff` (unstaged) and `git diff --cached` (staged).
  • Stage all of it: `git add -A`. Stage selectively: `git add <path>`.
  • Commit the staged set: `git commit -m "<message>"`.
  • Save for later instead: `git stash push -m "wip"` (re-apply with `git stash pop`).
```

The advice picks the most-relevant rule for the situation; the chain runs from "things blocking everything else" down to "all clear":

| Priority | Rule(s)                         | When                                                                            |
|----------|---------------------------------|---------------------------------------------------------------------------------|
| 1        | `merge-conflict`                | Unresolved conflicts — abort command adapts to merge / rebase / cherry-pick / revert |
| 2        | `*-in-progress`                 | rebase / cherry-pick / revert / bisect / paused-merge — continue / skip / abort |
| 3        | `initial-state`                 | `git init` happened, no commits yet                                             |
| 4        | `detached-head`                 | HEAD not on a branch                                                            |
| 5        | `uncommitted-changes`           | staged / unstaged / untracked — bucket breakdown adapts to what's there         |
| 6        | `diverged`                      | Branch is ahead **and** behind upstream                                         |
| 7        | `behind-upstream`               | Branch is behind only                                                           |
| 8        | `unpushed-commits`              | Branch is ahead only — different shape with vs without configured upstream      |
| 9        | `clean`                         | Catch-all: nothing to flag                                                      |

State is read once via `git status --porcelain=v2 --branch` plus a few filesystem checks against `.git/` markers (`MERGE_HEAD`, `rebase-merge/`, `CHERRY_PICK_HEAD`, …). No human-readable git output is parsed.

### `marshal help`

The on-CLI reference. `marshal help` (no arg) prints a context-aware overview that adapts based on whether you are inside a git repository or outside one — the recommended next moves change accordingly. `marshal help <topic>` dives into a specific topic.

```
$ git marshal help
Marshal 0.3.0 — a transparent wrapper for git.

You're inside a Git repository. Quick start:
  marshal what-now           See what you should do next.
  marshal config list        Inspect Marshal's configuration.
  git status                 Standard git (passes through unchanged).

Subcommands:
  config     Manage Marshal configuration (get/set/unset/list).
  what-now   Analyse repo state and suggest the next action.
  help       Print this overview, or `help <topic>` for details.

…
```

Topics shipped:

| Topic       | What it covers                                                                  |
|-------------|---------------------------------------------------------------------------------|
| `overview`  | This screen (context-aware). Default when no topic is given.                    |
| `config`    | Three-tier configuration system (system < global < local), every key, env var overrides. |
| `hints`     | Actionable error hints, the `errors.actionable_hints` toggle, all rule ids.     |
| `modernize` | Modernization tips, the two settings, every family covered.                     |
| `what-now`  | The `what-now` rule chain in priority order, data sources, JSON shape.          |

`--json` works here too: `marshal help --json` (or `marshal help config --json`) emits the topic structure as `{topic, title, sections: [{heading, body[]}]}`.

### JSON output (`--json`)

Every command in the marshal namespace accepts a global `--json` flag (anywhere in argv) that switches stdout from the human form to a structured JSON payload. Made for scripting and tooling; the human form stays the default everywhere else.

```
$ git marshal config list --json
{
  "entries": [
    { "key": "modernize.tips",          "value": "true"  },
    { "key": "modernize.rewrite",       "value": "false" },
    { "key": "errors.actionable_hints", "value": "true"  }
  ]
}

$ git marshal what-now --json
{
  "rule_id": "clean",
  "title": "Working tree clean, on `main` up to date with `origin/main`.",
  "suggestions": [
    "Start something new: `git switch -c feat/<name>`.",
    "Or pull the latest: `git pull --rebase`."
  ]
}

$ git marshal --json config get --show-origin modernize.tips
{
  "key": "modernize.tips",
  "value": "true",
  "origin": "default"
}
```

The flag is position-independent (`marshal --json config list` and `marshal config list --json` both work) and only affects stdout — errors stay on stderr. Concrete commands never see `--json`; the dispatcher detects it once and routes the command's output type through `serde_json` instead of the human renderer. This shape is mandated by Invariant 10 ([`docs/PRINCIPLES.md`](docs/PRINCIPLES.md)): adding a new command is `impl Command` plus one registration line, and `--json` works for it automatically.

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
