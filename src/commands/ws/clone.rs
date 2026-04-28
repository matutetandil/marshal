//! `ws clone` — clone a workspace repo plus, in parallel, every child
//! repo it declares.
//!
//! Two-step operation:
//!
//! 1. Clone the workspace repo synchronously to `<dest>` (default:
//!    basename of the URL). This is the file that carries
//!    `.workspace/manifest.toml`; we cannot proceed without it.
//! 2. Read the manifest. For every declared repo, spawn a worker
//!    thread that runs `git clone --progress <url> <abs>`, parses
//!    git's stderr for the four canonical progress phases (Counting,
//!    Compressing, Receiving, Resolving), and drives a per-repo
//!    [`indicatif::ProgressBar`] under one shared [`MultiProgress`].
//!
//! Partial failures are accepted (Invariant 5: Partial Failure is
//! Acceptable). A child that fails to clone marks itself failed in
//! the [`ChildResult`] list; the operation still returns success
//! and other children continue cloning. Failures surface in the
//! human form's footer and as `kind = "failed"` entries in JSON.
//!
//! `--no-children` skips the child phase entirely (workspace-only
//! clone). `--explain` prints the plan — workspace clone + every
//! child invocation — without running anything.

use anyhow::{anyhow, bail, Context as _, Result};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Serialize;
use std::ffi::OsString;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command as ProcessCommand, Stdio};
use std::time::Instant;

use crate::cli::{Command, Renderable};
use crate::context::{MANIFEST_FILE, WORKSPACE_MARKER};
use crate::workspace::manifest::{Manifest, RepoEntry};

pub struct WsClone {
    pub explain: bool,
}

impl Command for WsClone {
    type Output = WsCloneOutput;

    fn run(&self, args: &[OsString]) -> Result<Self::Output> {
        let parsed = parse_args(args)?;
        let dest = parsed
            .dest
            .clone()
            .unwrap_or_else(|| basename_from_url(&parsed.url));
        if dest.is_empty() {
            bail!(
                "ws clone: could not derive a destination directory from URL `{}`. \
                 Pass a destination explicitly: `ws clone <url> <dir>`.",
                parsed.url
            );
        }
        let dest_path = PathBuf::from(&dest);

        // --explain: build a plan listing the workspace clone, the
        // manifest read, and a placeholder line per child slot. We
        // don't run git or read the manifest off disk here — explain
        // is a dry run.
        if self.explain {
            let mut plan = vec![format!("git clone {} {}", parsed.url, dest)];
            if parsed.no_children {
                plan.push(format!(
                    "skip child clones (--no-children); leave `{dest}` as a workspace repo only"
                ));
            } else {
                plan.push(format!(
                    "read `{dest}/.workspace/manifest.toml` to discover declared child repos"
                ));
                plan.push(
                    "for each declared child, in parallel: \
                     `git clone --progress <url> <dest>/<repo.path or src/<name>>`"
                        .to_string(),
                );
                plan.push(
                    "render a Docker-style progress bar per child via indicatif::MultiProgress"
                        .to_string(),
                );
            }
            return Ok(WsCloneOutput {
                workspace_url: parsed.url,
                workspace_root: dest,
                no_children: parsed.no_children,
                manifest_present: false,
                children: Vec::new(),
                explain_plan: Some(plan),
            });
        }

        // 1. Workspace clone — synchronous, inherits stdio so the user
        //    sees git's own progress output exactly as plain git does.
        clone_workspace(&parsed.url, &dest_path)?;

        // 2. Manifest read. The clone may have succeeded against a
        //    plain git repo that isn't a Marshal workspace; that's
        //    still a valid `git clone`, we just have no children to
        //    fan out.
        let manifest_present = manifest_path(&dest_path).exists();
        if !manifest_present || parsed.no_children {
            return Ok(WsCloneOutput {
                workspace_url: parsed.url,
                workspace_root: absolute(&dest_path),
                no_children: parsed.no_children,
                manifest_present,
                children: Vec::new(),
                explain_plan: None,
            });
        }

        let manifest = Manifest::load(&manifest_path(&dest_path))
            .context("failed to read workspace manifest after clone")?;

        let children = clone_children_parallel(&dest_path, &manifest.repos);

        Ok(WsCloneOutput {
            workspace_url: parsed.url,
            workspace_root: absolute(&dest_path),
            no_children: parsed.no_children,
            manifest_present: true,
            children,
            explain_plan: None,
        })
    }
}

// ── Workspace clone (foreground, inherited stdio) ─────────────────

fn clone_workspace(url: &str, dest: &Path) -> Result<()> {
    let status = ProcessCommand::new("git")
        .arg("clone")
        .arg(url)
        .arg(dest)
        .status()
        .with_context(|| format!("failed to spawn `git clone {url} {}`", dest.display()))?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| format!("{c}"))
            .unwrap_or_else(|| "?".to_string());
        bail!(
            "ws clone: workspace clone failed (`git clone {url} {}` exited with status {code}). \
             Verify the URL, your network, and credentials.",
            dest.display()
        );
    }
    Ok(())
}

// ── Children clone (parallel, indicatif progress bars) ────────────

/// Coordinates the parallel child-repo clones. One thread per repo,
/// one [`ProgressBar`] per repo under a shared [`MultiProgress`].
/// `std::thread::scope` lets us borrow `manifest_repos` and the
/// bars without forcing `'static`.
fn clone_children_parallel(workspace_root: &Path, repos: &[RepoEntry]) -> Vec<ChildResult> {
    if repos.is_empty() {
        return Vec::new();
    }

    let multi = MultiProgress::new();
    let bar_style = progress_bar_style();
    let prefix_width = repos.iter().map(|r| r.name.len()).max().unwrap_or(0).max(8);

    // Pre-create one bar per repo so they appear together immediately
    // (rather than streaming in as threads start).
    let bars: Vec<ProgressBar> = repos
        .iter()
        .map(|repo| {
            let bar = multi.add(ProgressBar::new(0));
            bar.set_style(bar_style.clone());
            bar.set_prefix(format!("{:<width$}", repo.name, width = prefix_width));
            bar.set_message("queued");
            bar
        })
        .collect();

    // The work: each thread clones one repo and updates its bar.
    // Results are collected in declaration order so the JSON output
    // is stable across runs.
    let results: Vec<ChildResult> = std::thread::scope(|scope| {
        let mut handles = Vec::with_capacity(repos.len());
        for (repo, bar) in repos.iter().zip(bars.iter()) {
            let abs_path = workspace_root.join(repo_relative_path(repo));
            let url = repo.url.clone();
            let name = repo.name.clone();
            let display_path = abs_path.to_string_lossy().into_owned();
            let bar_handle = bar.clone();
            handles.push(scope.spawn(move || {
                let started = Instant::now();
                let outcome = clone_one_with_progress(&url, &abs_path, &bar_handle);
                let duration_ms = started.elapsed().as_millis();
                match outcome {
                    Ok(()) => {
                        bar_handle
                            .finish_with_message(format!("✓ cloned in {}", format_ms(duration_ms)));
                        ChildResult::Success {
                            name,
                            url,
                            path: display_path,
                            duration_ms,
                        }
                    }
                    Err(err) => {
                        let msg = err.to_string();
                        // Truncate noisy multi-line errors to a single
                        // line on the bar; full error stays in the
                        // JSON `error` field for machine consumers.
                        let one_line = msg.lines().next().unwrap_or(&msg).to_string();
                        bar_handle.finish_with_message(format!("✗ failed: {one_line}"));
                        ChildResult::Failed {
                            name,
                            url,
                            path: display_path,
                            error: msg,
                        }
                    }
                }
            }));
        }
        handles.into_iter().map(|h| h.join().unwrap()).collect()
    });

    // Make sure every bar has a final state and the multi cleared its
    // line so subsequent stdout writes are not interleaved with bars.
    let _ = multi.clear();

    results
}

/// Clone one repo, parsing `git clone --progress` output to feed
/// `bar`. stderr is piped (that is where git emits its progress)
/// and stdout is discarded — git puts almost nothing on stdout for
/// a clone, but piping it would risk the child blocking on a full
/// pipe if the buffer fills before we drain it.
fn clone_one_with_progress(url: &str, dest: &Path, bar: &ProgressBar) -> Result<()> {
    bar.set_message("starting…");

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create parent directory `{}`", parent.display()))?;
    }

    let mut child = ProcessCommand::new("git")
        .arg("clone")
        .arg("--progress")
        .arg(url)
        .arg(dest)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("failed to spawn `git clone --progress {url}`"))?;

    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow!("git stderr handle missing (this should not happen)"))?;

    // Buffer the *last* full stderr line so we can include it in an
    // error message when git exits non-zero. Progress chatter ends
    // in `\r`; the genuine fatal message ends in `\n`.
    let mut last_line = String::new();
    drive_progress(stderr, bar, &mut last_line);

    let status = child
        .wait()
        .with_context(|| format!("failed to wait for `git clone {url}`"))?;
    if !status.success() {
        let code = status
            .code()
            .map(|c| format!("{c}"))
            .unwrap_or_else(|| "?".to_string());
        let trailing = last_line.trim();
        if trailing.is_empty() {
            bail!("git clone exited with status {code}");
        }
        bail!("git clone exited with status {code}: {trailing}");
    }
    Ok(())
}

/// Read git's stderr byte-by-byte, splitting on either `\r` (in-place
/// progress updates) or `\n` (real lines). For each segment, attempt
/// to extract a known phase + counts and update the bar. Any line we
/// don't recognise is kept as the bar's free-form message so the user
/// sees something even during the "Cloning into ‘x’…" preamble.
fn drive_progress<R: Read>(stderr: R, bar: &ProgressBar, last_line: &mut String) {
    let mut reader = BufReader::new(stderr);
    let mut buf = Vec::with_capacity(256);
    loop {
        buf.clear();
        // Read up to either \r or \n. We can't use `read_line` because
        // git emits progress with `\r` carriage returns only.
        match read_until_either(&mut reader, &mut buf, b'\r', b'\n') {
            Ok(0) => break, // EOF
            Ok(_) => {}
            Err(_) => break, // I/O error — give up on the bar, the
                             // outer wait() will surface the failure.
        }
        // Strip the terminating delimiter.
        let len = buf.len();
        if len > 0 && (buf[len - 1] == b'\r' || buf[len - 1] == b'\n') {
            buf.pop();
        }
        if buf.is_empty() {
            continue;
        }
        let text = String::from_utf8_lossy(&buf);
        let line = text.trim();
        if line.is_empty() {
            continue;
        }

        // Stash the line for failure reporting. We overwrite freely;
        // the *last* meaningful line is what we want for diagnostics.
        last_line.clear();
        last_line.push_str(line);

        if let Some((phase, current, total)) = parse_progress(line) {
            if total > 0 {
                bar.set_length(total);
            }
            bar.set_position(current);
            bar.set_message(phase);
        } else {
            bar.set_message(line.to_string());
        }
    }
}

/// `BufRead::read_until` with two delimiters. Returns the count of
/// bytes appended to `buf` (delimiter included). Wraps the standard
/// "until first match" loop without pulling in a regex dep.
fn read_until_either<R: BufRead>(
    reader: &mut R,
    buf: &mut Vec<u8>,
    a: u8,
    b: u8,
) -> io::Result<usize> {
    let mut total = 0;
    loop {
        let (consumed, done) = {
            let available = match reader.fill_buf() {
                Ok(slice) => slice,
                Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if available.is_empty() {
                return Ok(total);
            }
            match available.iter().position(|&c| c == a || c == b) {
                Some(i) => {
                    buf.extend_from_slice(&available[..=i]);
                    (i + 1, true)
                }
                None => {
                    buf.extend_from_slice(available);
                    (available.len(), false)
                }
            }
        };
        reader.consume(consumed);
        total += consumed;
        if done {
            return Ok(total);
        }
    }
}

/// Parse a `git clone --progress` line into `(phase, current, total)`.
/// Returns `None` for lines we don't recognise (e.g. the initial
/// "Cloning into 'x'…", info lines from the remote that don't carry
/// progress numbers, etc.).
fn parse_progress(line: &str) -> Option<(String, u64, u64)> {
    // Strip the optional `remote: ` prefix some phases carry.
    let line = line.strip_prefix("remote: ").unwrap_or(line);

    // Phase is the text up to the first `:` (e.g. "Counting objects").
    // Match a known phase explicitly so a future git output change
    // surfaces as "no progress" rather than as garbage.
    let known_phases: &[&str] = &[
        "Counting objects",
        "Enumerating objects",
        "Compressing objects",
        "Receiving objects",
        "Resolving deltas",
        "Updating files",
        "Filtering content",
    ];
    let phase = known_phases.iter().find(|p| line.starts_with(**p))?;
    let rest = &line[phase.len()..];
    let rest = rest.strip_prefix(':')?.trim_start();

    // Either `<percent>% (<cur>/<total>)…` or `<cur>, done.` (Counting
    // objects emits the latter on completion). Try the parens form
    // first.
    if let Some(open) = rest.find('(') {
        if let Some(close) = rest[open..].find(')') {
            let inside = &rest[open + 1..open + close];
            // `12/45` or `12/45, …` — split on `/`.
            if let Some((cur, rest_total)) = inside.split_once('/') {
                let cur: u64 = cur.trim().parse().ok()?;
                // total may be followed by `, 1.20 KiB | …` — parse
                // up to first non-digit.
                let total_str: String = rest_total
                    .trim()
                    .chars()
                    .take_while(|c| c.is_ascii_digit())
                    .collect();
                let total: u64 = total_str.parse().ok()?;
                return Some(((*phase).to_string(), cur, total));
            }
        }
    }

    // Fallback: `<cur>, done.` (Counting objects sometimes emits
    // just `Counting objects: 12, done.` without a denominator).
    let head: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
    if !head.is_empty() {
        let cur: u64 = head.parse().ok()?;
        return Some(((*phase).to_string(), cur, cur));
    }

    None
}

fn progress_bar_style() -> ProgressStyle {
    // Docker-style: green spinner, name padded, fixed-width bar,
    // counts, then a free-form trailing message.
    ProgressStyle::with_template("{spinner:.green} {prefix} [{bar:30.cyan/blue}] {pos}/{len} {msg}")
        .unwrap_or_else(|_| ProgressStyle::default_bar())
        .progress_chars("=> ")
}

fn format_ms(ms: u128) -> String {
    if ms < 1000 {
        format!("{ms}ms")
    } else {
        format!("{:.1}s", (ms as f64) / 1000.0)
    }
}

// ── Path helpers ──────────────────────────────────────────────────

fn manifest_path(workspace_root: &Path) -> PathBuf {
    workspace_root.join(WORKSPACE_MARKER).join(MANIFEST_FILE)
}

/// Where a child repo lives within the workspace tree. Default is
/// `src/<name>` per the manifest convention; an explicit
/// `path = "..."` in the manifest entry overrides.
fn repo_relative_path(repo: &RepoEntry) -> PathBuf {
    match &repo.path {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => PathBuf::from("src").join(&repo.name),
    }
}

/// Best-effort canonicalisation. Falls back to the lossy display of
/// the as-given path when the OS refuses (e.g. the directory has
/// since been moved). Used only for the user-facing `workspace_root`
/// output field.
fn absolute(p: &Path) -> String {
    p.canonicalize()
        .map(|c| c.to_string_lossy().into_owned())
        .unwrap_or_else(|_| p.to_string_lossy().into_owned())
}

/// Derive a default destination directory from a clone URL.
///
/// Mirrors what `git clone <url>` does without an explicit dest:
/// take the last path component, strip a `.git` suffix. Handles
/// HTTPS URLs (`https://host/foo/bar.git`), SSH URLs
/// (`git@host:foo/bar.git`), and bare directory paths.
fn basename_from_url(url: &str) -> String {
    // Pick the last `/` or `:` (SSH form has the org separated by
    // `:` rather than `/`). Strip any trailing slash first so a
    // URL like `https://host/foo/` derives `foo`.
    let trimmed = url.trim_end_matches('/');
    let cut = trimmed.rfind(['/', ':']).map(|i| i + 1).unwrap_or(0);
    let last = &trimmed[cut..];
    last.strip_suffix(".git").unwrap_or(last).to_string()
}

// ── Argument parsing ──────────────────────────────────────────────

#[derive(Debug)]
struct ParsedArgs {
    url: String,
    dest: Option<String>,
    no_children: bool,
}

fn parse_args(args: &[OsString]) -> Result<ParsedArgs> {
    let mut url: Option<String> = None;
    let mut dest: Option<String> = None;
    let mut no_children = false;

    for arg in args {
        let s = arg.to_str().ok_or_else(|| {
            anyhow!(
                "ws clone: argument is not valid UTF-8: {:?}",
                arg.to_string_lossy()
            )
        })?;
        if s == "--no-children" {
            no_children = true;
        } else if s.starts_with("--") {
            bail!("ws clone: unexpected flag '{s}'. Expected: <url> [<dest>] [--no-children].");
        } else if url.is_none() {
            url = Some(s.to_string());
        } else if dest.is_none() {
            dest = Some(s.to_string());
        } else {
            bail!("ws clone: too many positional arguments. Expected `<url> [<dest>]`.");
        }
    }

    let url = url.ok_or_else(|| anyhow!("ws clone: missing required <url> argument."))?;
    Ok(ParsedArgs {
        url,
        dest,
        no_children,
    })
}

// ── Output type ───────────────────────────────────────────────────

#[derive(Serialize)]
pub struct WsCloneOutput {
    pub workspace_url: String,
    pub workspace_root: String,
    pub no_children: bool,
    /// `true` when `<workspace_root>/.workspace/manifest.toml` was
    /// found after the workspace clone. `false` when the cloned repo
    /// is a plain git repo without Marshal metadata (still a valid
    /// clone, just no children to fan out to).
    pub manifest_present: bool,
    pub children: Vec<ChildResult>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub explain_plan: Option<Vec<String>>,
}

/// One result per declared child. Tagged-enum JSON shape (`kind`)
/// mirrors `ws diff`'s `StateChange`: machine consumers branch with a
/// single switch.
#[derive(Serialize, Debug)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChildResult {
    Success {
        name: String,
        url: String,
        path: String,
        duration_ms: u128,
    },
    Failed {
        name: String,
        url: String,
        path: String,
        error: String,
    },
}

impl ChildResult {
    fn name(&self) -> &str {
        match self {
            Self::Success { name, .. } | Self::Failed { name, .. } => name,
        }
    }

    fn is_failure(&self) -> bool {
        matches!(self, Self::Failed { .. })
    }
}

impl Renderable for WsCloneOutput {
    fn render_human(&self, w: &mut dyn Write) -> io::Result<()> {
        if let Some(plan) = &self.explain_plan {
            return super::render_explain_plan(w, "ws clone", plan);
        }

        writeln!(w, "Cloned workspace from {}", self.workspace_url)?;
        writeln!(w, "  Root: {}", self.workspace_root)?;

        if !self.manifest_present {
            writeln!(
                w,
                "  (No `.workspace/manifest.toml` in the cloned repo — \
                 nothing to fan out. This was effectively a plain git clone.)"
            )?;
            return Ok(());
        }

        if self.no_children {
            writeln!(w, "  --no-children was set; child repos were not cloned.")?;
            return Ok(());
        }

        if self.children.is_empty() {
            writeln!(w, "  Manifest declares no child repos.")?;
            return Ok(());
        }

        let total = self.children.len();
        let failed: Vec<&ChildResult> = self.children.iter().filter(|c| c.is_failure()).collect();
        let succeeded = total - failed.len();

        writeln!(w)?;
        writeln!(w, "Cloned {succeeded}/{total} child repos.")?;

        if !failed.is_empty() {
            writeln!(w)?;
            writeln!(w, "Failed:")?;
            let pad = failed.iter().map(|c| c.name().len()).max().unwrap_or(0);
            for child in &failed {
                if let ChildResult::Failed { name, error, .. } = child {
                    let one_line = error.lines().next().unwrap_or(error);
                    writeln!(w, "  {:<pad$}  {one_line}", name, pad = pad)?;
                }
            }
            writeln!(w)?;
            writeln!(
                w,
                "Re-run `ws clone <url>` against just the failed repos with plain `git clone`, \
                 or correct the manifest and re-run `ws clone`."
            )?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basename_from_https_with_dot_git() {
        assert_eq!(basename_from_url("https://github.com/org/repo.git"), "repo");
    }

    #[test]
    fn basename_from_https_without_dot_git() {
        assert_eq!(basename_from_url("https://github.com/org/repo"), "repo");
    }

    #[test]
    fn basename_from_ssh_form() {
        assert_eq!(basename_from_url("git@github.com:org/repo.git"), "repo");
    }

    #[test]
    fn basename_from_trailing_slash() {
        assert_eq!(basename_from_url("https://host/foo/bar/"), "bar");
    }

    #[test]
    fn basename_from_file_url() {
        // file:// is what our integration tests use to avoid network.
        // The basename of `/tmp/XYZ/source.git` is `source`.
        assert_eq!(basename_from_url("file:///tmp/abc/source.git"), "source");
    }

    #[test]
    fn parse_progress_receiving_objects() {
        let r = parse_progress("Receiving objects:  50% (10/20)").unwrap();
        assert_eq!(r.0, "Receiving objects");
        assert_eq!(r.1, 10);
        assert_eq!(r.2, 20);
    }

    #[test]
    fn parse_progress_strips_remote_prefix() {
        let r = parse_progress("remote: Counting objects:  25% (5/20)").unwrap();
        assert_eq!(r.0, "Counting objects");
        assert_eq!(r.1, 5);
        assert_eq!(r.2, 20);
    }

    #[test]
    fn parse_progress_with_trailing_data() {
        // `Receiving objects: 100% (50/50), 1.20 KiB | 0 bytes/s, done.`
        let r =
            parse_progress("Receiving objects: 100% (50/50), 1.20 KiB | 0 bytes/s, done.").unwrap();
        assert_eq!(r.1, 50);
        assert_eq!(r.2, 50);
    }

    #[test]
    fn parse_progress_resolving_deltas_done() {
        let r = parse_progress("Resolving deltas: 100% (10/10), done.").unwrap();
        assert_eq!(r.0, "Resolving deltas");
    }

    #[test]
    fn parse_progress_counting_done_form() {
        // Some git versions emit `Counting objects: 42, done.` without
        // a denominator on completion.
        let r = parse_progress("remote: Counting objects: 42, done.").unwrap();
        assert_eq!(r.0, "Counting objects");
        assert_eq!(r.1, 42);
        assert_eq!(r.2, 42);
    }

    #[test]
    fn parse_progress_unknown_line_returns_none() {
        assert!(parse_progress("Cloning into 'x'...").is_none());
        assert!(parse_progress("remote: Total 50 (delta 0), reused 0").is_none());
        assert!(parse_progress("").is_none());
    }

    #[test]
    fn repo_relative_path_defaults_to_src() {
        let repo = RepoEntry {
            name: "alpha".to_string(),
            url: "x".to_string(),
            kind: None,
            path: None,
        };
        assert_eq!(
            repo_relative_path(&repo),
            PathBuf::from("src").join("alpha")
        );
    }

    #[test]
    fn repo_relative_path_honours_explicit_path() {
        let repo = RepoEntry {
            name: "alpha".to_string(),
            url: "x".to_string(),
            kind: None,
            path: Some("services/alpha".to_string()),
        };
        assert_eq!(repo_relative_path(&repo), PathBuf::from("services/alpha"));
    }

    #[test]
    fn parse_args_minimal() {
        let p = parse_args(&[OsString::from("git@x:repo.git")]).unwrap();
        assert_eq!(p.url, "git@x:repo.git");
        assert!(p.dest.is_none());
        assert!(!p.no_children);
    }

    #[test]
    fn parse_args_url_and_dest() {
        let p = parse_args(&[OsString::from("git@x:repo.git"), OsString::from("my-dir")]).unwrap();
        assert_eq!(p.url, "git@x:repo.git");
        assert_eq!(p.dest.as_deref(), Some("my-dir"));
    }

    #[test]
    fn parse_args_no_children_flag() {
        let p = parse_args(&[
            OsString::from("git@x:repo.git"),
            OsString::from("--no-children"),
        ])
        .unwrap();
        assert!(p.no_children);
    }

    #[test]
    fn parse_args_rejects_missing_url() {
        let err = parse_args(&[]).unwrap_err();
        assert!(err.to_string().contains("missing required <url>"));
    }

    #[test]
    fn parse_args_rejects_unknown_flag() {
        let err = parse_args(&[OsString::from("--bogus")]).unwrap_err();
        assert!(err.to_string().contains("unexpected flag"));
    }

    #[test]
    fn parse_args_rejects_third_positional() {
        let err = parse_args(&[
            OsString::from("a"),
            OsString::from("b"),
            OsString::from("c"),
        ])
        .unwrap_err();
        assert!(err.to_string().contains("too many positional"));
    }

    #[test]
    fn format_ms_sub_second() {
        assert_eq!(format_ms(523), "523ms");
    }

    #[test]
    fn format_ms_seconds() {
        assert_eq!(format_ms(1500), "1.5s");
    }
}
