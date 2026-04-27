//! Capture a snapshot of the repository's current state.
//!
//! [`RepoState`] is the input every advice rule sees. It is built once
//! per `marshal what-now` invocation by combining two cheap shellouts —
//! `git rev-parse --git-dir` and `git status --porcelain=v2 --branch` —
//! with a handful of filesystem checks against `.git/` markers for
//! ongoing operations git can't surface in porcelain v2 (merge,
//! rebase, cherry-pick, revert, bisect).
//!
//! Stable signals only: porcelain v2 is git's documented machine-
//! readable format, expressly intended to survive across releases.
//! Filesystem markers (`MERGE_HEAD`, `rebase-merge/`, `CHERRY_PICK_HEAD`,
//! `REVERT_HEAD`, `BISECT_LOG`) are git internals but have been stable
//! for a decade — well below the change-rate of git's user-facing
//! output. We never parse human-readable `git status`.

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

/// One-shot snapshot of the repository.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct RepoState {
    pub branch: BranchInfo,
    pub working_tree: WorkingTreeInfo,
    pub in_progress: InProgressOp,
}

/// Branch identity and remote relationship.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct BranchInfo {
    /// Current branch name. `None` when HEAD is detached or the
    /// repository is at its initial empty state.
    pub name: Option<String>,
    pub is_detached: bool,
    /// `true` when the repo has no commits yet (`git init`, nothing
    /// committed). Mutually exclusive with `is_detached`.
    pub is_initial: bool,
    /// Tracking remote/branch pair, e.g. `origin/main`. `None` when
    /// no upstream is configured for the current branch.
    pub upstream: Option<String>,
    pub ahead: usize,
    pub behind: usize,
}

/// Working tree + index counters. We aggregate counts (rather than
/// keep file lists) because every advice rule decides on counts; if a
/// rule ever needs the names, we'll add a `paths` field then.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct WorkingTreeInfo {
    /// Files with non-`.` in the **index** column (changes ready to
    /// commit). Counts include renames and copies.
    pub staged: usize,
    /// Files with non-`.` in the **worktree** column (modifications
    /// not yet staged).
    pub unstaged: usize,
    pub untracked: usize,
    /// Files with merge conflicts (porcelain `u` lines, plus tracked
    /// entries with `U` in either status column).
    pub unmerged: usize,
}

impl WorkingTreeInfo {
    pub fn is_clean(&self) -> bool {
        self.staged == 0 && self.unstaged == 0 && self.untracked == 0 && self.unmerged == 0
    }

    pub fn has_unmerged(&self) -> bool {
        self.unmerged > 0
    }

    pub fn has_any_changes(&self) -> bool {
        !self.is_clean()
    }
}

/// Multi-step operations the user is in the middle of. Detected via
/// well-known files inside `<git-dir>/`.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum InProgressOp {
    #[default]
    None,
    Merge,
    Rebase,
    CherryPick,
    Revert,
    Bisect,
}

impl InProgressOp {
    pub fn is_active(self) -> bool {
        !matches!(self, Self::None)
    }
}

impl RepoState {
    /// Detect the current state. Errors when the cwd is not inside a
    /// git repository (the `git rev-parse --git-dir` shellout fails).
    pub fn detect() -> Result<Self> {
        let git_dir = git_dir_via_rev_parse()
            .context("not in a git repository (or any parent up to the filesystem root)")?;
        let porcelain = run_status_porcelain_v2()?;
        let mut state = parse_porcelain_v2(&porcelain);
        state.in_progress = detect_in_progress(&git_dir);
        Ok(state)
    }
}

fn git_dir_via_rev_parse() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--git-dir"])
        .output()
        .context("failed to invoke `git rev-parse --git-dir`")?;
    if !output.status.success() {
        bail!(
            "git rev-parse --git-dir exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let raw = String::from_utf8_lossy(&output.stdout);
    Ok(PathBuf::from(raw.trim()))
}

fn run_status_porcelain_v2() -> Result<String> {
    let output = Command::new("git")
        .args(["status", "--porcelain=v2", "--branch"])
        .output()
        .context("failed to invoke `git status --porcelain=v2 --branch`")?;
    if !output.status.success() {
        bail!(
            "git status --porcelain=v2 --branch exited {}: {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Parse the v2 porcelain output into a populated [`RepoState`].
///
/// Format reference: <https://git-scm.com/docs/git-status#_porcelain_format_version_2>.
/// The lines we read:
///
///   * `# branch.oid <commit>` or `(initial)`
///   * `# branch.head <branch>` or `(detached)`
///   * `# branch.upstream <branch>`
///   * `# branch.ab +<ahead> -<behind>`
///   * `1 <XY> …`  — ordinary tracked change
///   * `2 <XY> …`  — rename or copy
///   * `u <XY> …`  — unmerged (conflict)
///   * `? <path>`  — untracked
///   * `! <path>`  — ignored (we skip these)
fn parse_porcelain_v2(text: &str) -> RepoState {
    let mut state = RepoState::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# branch.head ") {
            if rest == "(detached)" {
                state.branch.is_detached = true;
            } else {
                state.branch.name = Some(rest.to_string());
            }
        } else if let Some(rest) = line.strip_prefix("# branch.oid ") {
            if rest == "(initial)" {
                state.branch.is_initial = true;
            }
        } else if let Some(rest) = line.strip_prefix("# branch.upstream ") {
            state.branch.upstream = Some(rest.to_string());
        } else if let Some(rest) = line.strip_prefix("# branch.ab ") {
            // "+<ahead> -<behind>" — both fields always present per the spec.
            let parts: Vec<&str> = rest.split_whitespace().collect();
            if parts.len() == 2 {
                if let Some(n) = parts[0].strip_prefix('+') {
                    state.branch.ahead = n.parse().unwrap_or(0);
                }
                if let Some(n) = parts[1].strip_prefix('-') {
                    state.branch.behind = n.parse().unwrap_or(0);
                }
            }
        } else if line.starts_with("1 ") || line.starts_with("2 ") {
            // After the prefix `<n> ` (two bytes), the next two bytes
            // are the X and Y status fields. The two-codepoint XY pair
            // is always ASCII per the spec — index by byte safely.
            let bytes = line.as_bytes();
            if bytes.len() >= 4 {
                let x = bytes[2] as char;
                let y = bytes[3] as char;
                if x == 'U' || y == 'U' {
                    state.working_tree.unmerged += 1;
                } else {
                    if x != '.' {
                        state.working_tree.staged += 1;
                    }
                    if y != '.' {
                        state.working_tree.unstaged += 1;
                    }
                }
            }
        } else if line.starts_with("u ") {
            state.working_tree.unmerged += 1;
        } else if line.starts_with("? ") {
            state.working_tree.untracked += 1;
        }
        // `! ` (ignored) lines are deliberately skipped — they don't
        // surface in any advice rule.
    }
    state
}

fn detect_in_progress(git_dir: &Path) -> InProgressOp {
    // Order matters: rebase carries a MERGE_HEAD too, so check rebase
    // first to avoid mis-classifying it as a plain merge.
    if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() {
        InProgressOp::Rebase
    } else if git_dir.join("CHERRY_PICK_HEAD").exists() {
        InProgressOp::CherryPick
    } else if git_dir.join("REVERT_HEAD").exists() {
        InProgressOp::Revert
    } else if git_dir.join("BISECT_LOG").exists() {
        InProgressOp::Bisect
    } else if git_dir.join("MERGE_HEAD").exists() {
        InProgressOp::Merge
    } else {
        InProgressOp::None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    // ── parser fixtures ────────────────────────────────────────────

    #[test]
    fn parses_clean_branch_with_upstream_and_ahead() {
        let text = "# branch.oid 1234567890abcdef\n\
                    # branch.head main\n\
                    # branch.upstream origin/main\n\
                    # branch.ab +2 -0\n";
        let s = parse_porcelain_v2(text);
        assert_eq!(s.branch.name.as_deref(), Some("main"));
        assert!(!s.branch.is_detached);
        assert!(!s.branch.is_initial);
        assert_eq!(s.branch.upstream.as_deref(), Some("origin/main"));
        assert_eq!(s.branch.ahead, 2);
        assert_eq!(s.branch.behind, 0);
        assert!(s.working_tree.is_clean());
    }

    #[test]
    fn parses_detached_head() {
        let text = "# branch.oid abcd\n# branch.head (detached)\n";
        let s = parse_porcelain_v2(text);
        assert!(s.branch.is_detached);
        assert!(s.branch.name.is_none());
        assert!(!s.branch.is_initial);
    }

    #[test]
    fn parses_initial_repo() {
        let text = "# branch.oid (initial)\n# branch.head main\n";
        let s = parse_porcelain_v2(text);
        assert!(s.branch.is_initial);
        assert_eq!(s.branch.name.as_deref(), Some("main"));
    }

    #[test]
    fn parses_diverged_branch() {
        let text = "# branch.head feat/x\n\
                    # branch.upstream origin/feat/x\n\
                    # branch.ab +3 -2\n";
        let s = parse_porcelain_v2(text);
        assert_eq!(s.branch.ahead, 3);
        assert_eq!(s.branch.behind, 2);
    }

    #[test]
    fn counts_staged_unstaged_and_untracked() {
        // `1 M. ...` = staged-only, `1 .M ...` = unstaged-only,
        // `1 MM ...` = both, `? path` = untracked.
        let text = "# branch.head main\n\
                    1 M. N... 100644 100644 100644 aaa bbb a.txt\n\
                    1 .M N... 100644 100644 100644 ccc ddd b.txt\n\
                    1 MM N... 100644 100644 100644 eee fff c.txt\n\
                    ? d.txt\n\
                    ? e.txt\n";
        let s = parse_porcelain_v2(text);
        assert_eq!(s.working_tree.staged, 2, "M. and MM both bump staged");
        assert_eq!(s.working_tree.unstaged, 2, ".M and MM both bump unstaged");
        assert_eq!(s.working_tree.untracked, 2);
        assert_eq!(s.working_tree.unmerged, 0);
        assert!(!s.working_tree.is_clean());
    }

    #[test]
    fn counts_unmerged_via_u_lines_and_via_uppercase_u_in_xy() {
        // Both shapes that indicate an unresolved conflict must land
        // in `unmerged` and never double-count as staged/unstaged.
        let text = "# branch.head main\n\
                    u UU N... 100644 100644 100644 100644 aa bb cc dd a.txt\n\
                    1 UU N... 100644 100644 100644 ee ff b.txt\n";
        let s = parse_porcelain_v2(text);
        assert_eq!(s.working_tree.unmerged, 2);
        assert_eq!(s.working_tree.staged, 0);
        assert_eq!(s.working_tree.unstaged, 0);
    }

    #[test]
    fn rename_lines_are_counted_like_modifications() {
        // `2 ` lines also start with the XY pair after the prefix.
        let text = "# branch.head main\n\
                    2 R. N... 100644 100644 100644 aa bb R100 new.txt\told.txt\n";
        let s = parse_porcelain_v2(text);
        assert_eq!(s.working_tree.staged, 1);
        assert_eq!(s.working_tree.unstaged, 0);
    }

    #[test]
    fn ignores_ignored_lines_and_empty_input() {
        let text = "# branch.head main\n! ignored.txt\n";
        let s = parse_porcelain_v2(text);
        assert!(s.working_tree.is_clean());

        let s2 = parse_porcelain_v2("");
        assert_eq!(s2, RepoState::default());
    }

    // ── in-progress detection ──────────────────────────────────────

    #[test]
    fn detects_no_in_progress_op_in_empty_git_dir() {
        let tmp = TempDir::new().unwrap();
        assert_eq!(detect_in_progress(tmp.path()), InProgressOp::None);
    }

    #[test]
    fn detects_merge_in_progress() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("MERGE_HEAD"), b"abc").unwrap();
        assert_eq!(detect_in_progress(tmp.path()), InProgressOp::Merge);
    }

    #[test]
    fn detects_rebase_takes_precedence_over_merge() {
        // A rebase-merge directory implies an in-progress rebase even
        // when MERGE_HEAD is also present (the rebase machinery can
        // create both).
        let tmp = TempDir::new().unwrap();
        fs::create_dir(tmp.path().join("rebase-merge")).unwrap();
        fs::write(tmp.path().join("MERGE_HEAD"), b"abc").unwrap();
        assert_eq!(detect_in_progress(tmp.path()), InProgressOp::Rebase);
    }

    #[test]
    fn detects_cherry_pick_revert_bisect() {
        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("CHERRY_PICK_HEAD"), b"abc").unwrap();
        assert_eq!(detect_in_progress(tmp.path()), InProgressOp::CherryPick);

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("REVERT_HEAD"), b"abc").unwrap();
        assert_eq!(detect_in_progress(tmp.path()), InProgressOp::Revert);

        let tmp = TempDir::new().unwrap();
        fs::write(tmp.path().join("BISECT_LOG"), b"").unwrap();
        assert_eq!(detect_in_progress(tmp.path()), InProgressOp::Bisect);
    }
}
