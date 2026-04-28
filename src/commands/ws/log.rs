//! `ws log` — aggregated commit log across every child repo in the
//! workspace.
//!
//! For each declared repo with an on-disk path, runs
//! `git -C <path> log --pretty=… -n N`, parses the per-line output,
//! combines all entries, sorts by author date descending, and takes
//! the top N. The result is rendered as a unified timeline so the
//! workspace feels like a single history at a glance — that's the
//! monorepo-feel half of the thesis applied to log.
//!
//! No spatial inference yet — `ws log` always returns the
//! workspace-wide view, regardless of where the user invokes it.
//! Per-repo log via `cd src/<repo> && git log` (passthrough). Scope
//! inference (`ws log` from inside a child becomes per-repo log)
//! lands in Slice H.
//!
//! Repos that do not exist on disk yet (typical for a freshly
//! `ws init`-ed workspace before children are cloned) are silently
//! skipped — the user already sees them as "missing on disk" in
//! `ws status`, no need to repeat the noise here.

use anyhow::{anyhow, bail, Context as _, Result};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, Write};
use std::path::Path;
use std::process::Command as ProcessCommand;

use crate::cli::{Command, Renderable};
use crate::context;
use crate::workspace::manifest::Manifest;
use crate::workspace::scope::{self, ScopePolicy};
use crate::workspace::state::StateDeclaration;

/// Default cap on commits returned (and per-repo cap when fetching).
/// Mathematically: the global top-N must be among each repo's top-N
/// (commits later within the SAME repo are more recent than earlier
/// ones), so fetching N per repo and sorting is correct.
const DEFAULT_LIMIT: usize = 20;

/// `git ws log` — show workspace activity.
pub struct WsLog {
    /// `--all` flag from the dispatcher: fetch unlimited commits per
    /// repo. Without it, both per-repo fetch and global cap default
    /// to [`DEFAULT_LIMIT`] (overridable with `-n` / `--limit`).
    pub all: bool,
    /// `--on <name>` from the dispatcher — declared-scope override.
    /// When `None`, the spatial-fallback policy applies: inside a
    /// child repo (cwd matches `<root>/src/<name>/…`), narrow the
    /// log to that one. Otherwise the full workspace.
    pub on: Option<String>,
}

impl Command for WsLog {
    type Output = WsLogOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let parsed = parse_args(args)?;

        let ctx = context::detect()?.ok_or_else(|| {
            anyhow!(
                "not in a marshal workspace.\n  \
                 Walk into a workspace (a directory tree containing `.workspace/`), \
                 or initialise one here with `ws init`."
            )
        })?;

        let manifest = Manifest::try_load_from_workspace(&ctx.root)
            .context("failed to read workspace manifest")?
            .ok_or_else(|| {
                anyhow!(
                    "workspace at {} has no manifest yet.\n  \
                     Run `ws init` here to create one, or edit \
                     `.workspace/manifest.toml` directly.",
                    ctx.root.display()
                )
            })?;

        // Effective limit: --all overrides explicit -n. Without
        // --all, -n wins; without -n either, the default kicks in.
        let per_repo_limit: Option<usize> = if self.all {
            None
        } else {
            Some(parsed.limit.unwrap_or(DEFAULT_LIMIT))
        };

        // Resolve the scope: spatial-fallback policy by default
        // (inside a child repo, narrow to that one), or the
        // explicit `--on <name>` override.
        let scope = scope::resolve(
            self.on.as_deref(),
            &manifest,
            &StateDeclaration::default(),
            ctx.current_repo.as_deref(),
            ScopePolicy::spatial_fallback(),
        )?;

        // Fetch every in-scope repo's recent commits (skip missing).
        let mut entries: Vec<LogEntry> = Vec::new();
        let mut repos_with_data = 0usize;

        for repo in manifest.repos.iter().filter(|r| scope.contains(&r.name)) {
            let rel_path = repo
                .path
                .clone()
                .unwrap_or_else(|| format!("src/{}", repo.name));
            let abs_path = ctx.root.join(&rel_path);
            if !abs_path.exists() {
                continue;
            }
            match fetch_repo_log(&abs_path, &repo.name, per_repo_limit) {
                Ok(per_repo) => {
                    if !per_repo.is_empty() {
                        repos_with_data += 1;
                        entries.extend(per_repo);
                    }
                }
                Err(_) => {
                    // Repo path exists but git failed (not a repo,
                    // permissions, …). Skip silently — the user
                    // already sees these as "unreadable" in
                    // `ws status`.
                    continue;
                }
            }
        }

        // Sort by date descending. Ties broken by repo name then
        // hash so the order is stable.
        entries.sort_by(|a, b| {
            b.date
                .cmp(&a.date)
                .then_with(|| a.repo.cmp(&b.repo))
                .then_with(|| a.hash.cmp(&b.hash))
        });

        // Apply global cap (same as per-repo by default; `--all`
        // disables it).
        let sampled = entries.len();
        if !self.all {
            if let Some(limit) = per_repo_limit {
                entries.truncate(limit);
            }
        }

        Ok(WsLogOutput {
            workspace: WorkspaceInfo {
                root: ctx.root.to_string_lossy().into_owned(),
                name: manifest.workspace.name.clone(),
                total_repos_declared: manifest.repos.len(),
                repos_with_data,
            },
            entries,
            sampled,
            limit_applied: per_repo_limit,
            all: self.all,
        })
    }
}

/// Run `git log` against one repo and parse the output. Returns the
/// per-repo entries already tagged with the repo name. Errors only
/// when git itself fails — empty output (no commits) is `Ok(vec![])`.
fn fetch_repo_log(
    repo_path: &Path,
    repo_name: &str,
    per_repo_limit: Option<usize>,
) -> Result<Vec<LogEntry>> {
    // Pretty format: `<hash>\t<ISO author date>\t<author name>\t<subject>`.
    // `%x09` is a literal tab — separator that won't appear inside
    // any field.
    let mut cmd = ProcessCommand::new("git");
    cmd.current_dir(repo_path)
        .args(["log", "--pretty=format:%H%x09%aI%x09%an%x09%s"]);
    if let Some(n) = per_repo_limit {
        cmd.arg(format!("-n{n}"));
    }

    let output = cmd
        .output()
        .with_context(|| format!("failed to invoke `git log` in {}", repo_path.display()))?;

    if !output.status.success() {
        // A repo with no commits yet exits non-zero; treat that as
        // "no entries", not an error.
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not have any commits yet")
            || stderr.contains("bad default revision")
        {
            return Ok(Vec::new());
        }
        bail!(
            "git log failed in {}: {}",
            repo_path.display(),
            stderr.trim()
        );
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut entries = Vec::new();
    for line in stdout.lines() {
        // Skip blank lines (git appends a trailing newline).
        if line.is_empty() {
            continue;
        }
        let parts: Vec<&str> = line.splitn(4, '\t').collect();
        if parts.len() != 4 {
            // Malformed line — skip rather than crash. `git log`
            // can occasionally emit unexpected forms (e.g. when
            // the repo's config has weird format settings); we
            // don't want one weird repo to break the whole view.
            continue;
        }
        entries.push(LogEntry {
            repo: repo_name.to_string(),
            hash: parts[0].to_string(),
            date: parts[1].to_string(),
            author: parts[2].to_string(),
            summary: parts[3].to_string(),
        });
    }
    Ok(entries)
}

#[derive(Serialize)]
pub struct WsLogOutput {
    pub workspace: WorkspaceInfo,
    pub entries: Vec<LogEntry>,
    /// How many entries were collected before the global cap was
    /// applied. With `--all`, equals `entries.len()`. Otherwise
    /// reflects the sample taken across repos (each repo
    /// contributed up to `limit_applied` entries) before the
    /// global truncation. Always ≥ `entries.len()`.
    pub sampled: usize,
    /// `Some(n)` when a cap was applied, `None` when `--all` lifted
    /// the cap. JSON consumers can branch on this.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_applied: Option<usize>,
    /// Render-time only: forces the renderer to skip the
    /// "showing K of N" footer (since with `--all` everything is
    /// shown). Excluded from JSON.
    #[serde(skip)]
    pub all: bool,
}

#[derive(Serialize)]
pub struct WorkspaceInfo {
    pub root: String,
    pub name: String,
    pub total_repos_declared: usize,
    /// Number of repos that actually contributed entries (i.e.
    /// declared, on disk, with at least one commit). The complement
    /// is "missing on disk + empty repos".
    pub repos_with_data: usize,
}

#[derive(Serialize)]
pub struct LogEntry {
    pub repo: String,
    pub hash: String,
    /// ISO-8601 author date (`%aI`), suitable for lexical sort and
    /// for tooling consumers parsing dates.
    pub date: String,
    pub author: String,
    pub summary: String,
}

impl Renderable for WsLogOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        writeln!(
            w,
            "Workspace `{}` — recent activity across {} of {} repos",
            self.workspace.name,
            self.workspace.repos_with_data,
            self.workspace.total_repos_declared
        )?;
        writeln!(w, "Root: {}", self.workspace.root)?;
        writeln!(w)?;

        if self.entries.is_empty() {
            writeln!(
                w,
                "(No commits yet — every declared repo is empty or missing.)"
            )?;
            return Ok(());
        }

        // Compute column widths so repo + author align. Date is
        // truncated to the date+hour portion ("yyyy-mm-dd hh:mm")
        // for compactness; full ISO is in the JSON form.
        let repo_pad = self.entries.iter().map(|e| e.repo.len()).max().unwrap_or(0);
        let author_pad = self
            .entries
            .iter()
            .map(|e| e.author.len())
            .max()
            .unwrap_or(0);

        for entry in &self.entries {
            let short_hash = if entry.hash.len() >= 7 {
                &entry.hash[..7]
            } else {
                &entry.hash[..]
            };
            let short_date = format_short_date(&entry.date);
            writeln!(
                w,
                "  {short_date}  {:<repo_pad$}  {short_hash}  {:<author_pad$}  {}",
                entry.repo,
                entry.author,
                entry.summary,
                repo_pad = repo_pad,
                author_pad = author_pad
            )?;
        }

        // Footer when a cap was applied. We deliberately don't
        // claim a specific total — counting all commits across
        // every repo would mean a `git rev-list --count` per repo
        // (extra shellouts) and the user only needs the escape
        // hatch, not a precise count.
        if !self.all && self.limit_applied.is_some() {
            writeln!(w)?;
            writeln!(
                w,
                "Showing top {}. Use `--all` for every commit, \
                 `-n <N>` for a different cap, or `cd src/<repo>` \
                 for a single repo's log.",
                self.entries.len()
            )?;
        }

        Ok(())
    }
}

/// Trim an ISO-8601 timestamp to "yyyy-mm-dd hh:mm" for display.
/// Falls back to the raw string when the input is shorter than
/// expected (a malformed `%aI` from git would be unusual but we
/// don't want to panic on it).
fn format_short_date(iso: &str) -> String {
    // `%aI` looks like `2026-04-27T15:33:21+00:00`. Take the date
    // and the HH:MM portion.
    if let Some((date, rest)) = iso.split_once('T') {
        if rest.len() >= 5 {
            return format!("{date} {}", &rest[..5]);
        }
        return date.to_string();
    }
    iso.to_string()
}

// ── Argument parsing ──────────────────────────────────────────────

#[derive(Debug, Default)]
struct ParsedArgs {
    limit: Option<usize>,
}

fn parse_args(args: &[OsString]) -> Result<ParsedArgs> {
    let mut limit: Option<usize> = None;

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws log: argument {} is not valid UTF-8: {:?}",
                i,
                arg.to_string_lossy()
            )
        })?;

        if s == "-n" || s == "--limit" {
            let value = consume_value(args, i, s)?;
            assign_limit(&mut limit, &value, s)?;
            i += 2;
        } else if let Some(value) = s.strip_prefix("--limit=") {
            assign_limit(&mut limit, value, "--limit")?;
            i += 1;
        } else if let Some(value) = s.strip_prefix("-n") {
            // `-n20` shorthand.
            if value.is_empty() {
                bail!("ws log: '-n' expects a value (e.g. `-n 20`).");
            }
            assign_limit(&mut limit, value, "-n")?;
            i += 1;
        } else {
            bail!(
                "ws log: unexpected argument '{s}'. \
                 Expected -n <N>, --limit <N>, or no args (default {DEFAULT_LIMIT})."
            );
        }
    }

    Ok(ParsedArgs { limit })
}

fn consume_value(args: &[OsString], idx: usize, flag: &str) -> Result<String> {
    args.get(idx + 1)
        .ok_or_else(|| anyhow!("ws log: '{flag}' expects a value"))?
        .to_str()
        .map(|s| s.to_string())
        .ok_or_else(|| anyhow!("ws log: value for '{flag}' is not valid UTF-8"))
}

fn assign_limit(slot: &mut Option<usize>, value: &str, flag: &str) -> Result<()> {
    if slot.is_some() {
        bail!("ws log: '{flag}' specified more than once");
    }
    let n: usize = value.parse().with_context(|| {
        format!("ws log: '{flag}' expects a non-negative integer, got '{value}'")
    })?;
    *slot = Some(n);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn os(strings: &[&str]) -> Vec<OsString> {
        strings.iter().map(|s| OsString::from(*s)).collect()
    }

    #[test]
    fn parse_empty_yields_default() {
        let p = parse_args(&[]).unwrap();
        assert!(p.limit.is_none());
    }

    #[test]
    fn parse_n_with_separated_value() {
        let p = parse_args(&os(&["-n", "5"])).unwrap();
        assert_eq!(p.limit, Some(5));
    }

    #[test]
    fn parse_n_with_attached_value() {
        let p = parse_args(&os(&["-n10"])).unwrap();
        assert_eq!(p.limit, Some(10));
    }

    #[test]
    fn parse_limit_with_equals() {
        let p = parse_args(&os(&["--limit=42"])).unwrap();
        assert_eq!(p.limit, Some(42));
    }

    #[test]
    fn parse_limit_separated() {
        let p = parse_args(&os(&["--limit", "7"])).unwrap();
        assert_eq!(p.limit, Some(7));
    }

    #[test]
    fn parse_rejects_unknown_flag() {
        let err = parse_args(&os(&["--bogus"])).unwrap_err();
        assert!(err.to_string().contains("unexpected argument"));
    }

    #[test]
    fn parse_rejects_non_integer_limit() {
        let err = parse_args(&os(&["-n", "abc"])).unwrap_err();
        assert!(err.to_string().contains("non-negative integer"));
    }

    #[test]
    fn parse_rejects_double_limit() {
        let err = parse_args(&os(&["-n", "5", "--limit", "10"])).unwrap_err();
        assert!(err.to_string().contains("more than once"));
    }

    #[test]
    fn format_short_date_trims_iso_to_minute() {
        assert_eq!(
            format_short_date("2026-04-27T15:33:21+00:00"),
            "2026-04-27 15:33"
        );
    }

    #[test]
    fn format_short_date_handles_unexpected_input() {
        // No `T` → return the raw string (don't panic).
        assert_eq!(format_short_date("not-iso"), "not-iso");
    }
}
