//! Rules for `git checkout` — the split into `git switch` + `git restore`
//! introduced in Git 2.23 (August 2019).
//!
//! `checkout` is the most overloaded command in Git: it moves HEAD between
//! branches, creates branches, detaches HEAD, restores files from the index
//! or from arbitrary commits, and more. The 2.23 split broke these concerns
//! apart into `switch` (branch motion) and `restore` (file content), and Git
//! itself recommends the new forms. Each Marshal rule here covers one of the
//! documented patterns.

use crate::git::parser::ParsedGitInvocation;
use crate::modernize::rule::{ModernizationRule, RuleOpinion, Suggestion};
use std::ffi::{OsStr, OsString};

const CHECKOUT_SWITCH_NOTE: &str =
    "`switch` was split out of `checkout` in Git 2.23 for branch-only operations.";
const CHECKOUT_RESTORE_NOTE: &str =
    "`restore` was split out of `checkout` in Git 2.23 for file-only operations.";

// ─── Rule 2: `git checkout -b <branch> [<start-point>]` → `git switch -c ...` ───

pub struct CheckoutCreateBranch;

impl ModernizationRule for CheckoutCreateBranch {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("-b")) {
            return None;
        }
        let rest = &args[1..];
        if rest.is_empty() {
            // `git checkout -b` alone is a user error — Git will reject it.
            return None;
        }
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-create-branch",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("switch", &prepend_flag("-c", rest)),
                note: Some(CHECKOUT_SWITCH_NOTE),
            },
            rewrite: Some(rewrite_subcommand(
                parsed,
                "switch",
                &prepend_flag("-c", rest),
            )),
        })
    }
}

// ─── Rule 3: `git checkout -B <branch> [<start>]` → `git switch -C ...` ───

pub struct CheckoutCreateBranchForce;

impl ModernizationRule for CheckoutCreateBranchForce {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("-B")) {
            return None;
        }
        let rest = &args[1..];
        if rest.is_empty() {
            return None;
        }
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-create-branch-force",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("switch", &prepend_flag("-C", rest)),
                note: Some(CHECKOUT_SWITCH_NOTE),
            },
            rewrite: Some(rewrite_subcommand(
                parsed,
                "switch",
                &prepend_flag("-C", rest),
            )),
        })
    }
}

// ─── Rule 4: `git checkout --orphan <branch>` → `git switch --orphan <branch>` ───

pub struct CheckoutOrphan;

impl ModernizationRule for CheckoutOrphan {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("--orphan")) {
            return None;
        }
        let rest = &args[1..];
        if rest.is_empty() {
            return None;
        }
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-orphan",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("switch", &prepend_flag("--orphan", rest)),
                note: Some(CHECKOUT_SWITCH_NOTE),
            },
            rewrite: Some(rewrite_subcommand(
                parsed,
                "switch",
                &prepend_flag("--orphan", rest),
            )),
        })
    }
}

// ─── Rule 5: `git checkout --detach [<ref>]` → `git switch --detach [<ref>]` ───

pub struct CheckoutDetach;

impl ModernizationRule for CheckoutDetach {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("--detach")) {
            return None;
        }
        let rest = &args[1..];
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-detach",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("switch", &prepend_flag("--detach", rest)),
                note: Some(CHECKOUT_SWITCH_NOTE),
            },
            rewrite: Some(rewrite_subcommand(
                parsed,
                "switch",
                &prepend_flag("--detach", rest),
            )),
        })
    }
}

// ─── Rule 7: `git checkout <commit> -- <files…>` → `git restore --source=<commit> <files>`

pub struct CheckoutRestoreFromCommit;

impl ModernizationRule for CheckoutRestoreFromCommit {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        // Pattern: `<non-flag commit> -- <file...>`.
        // Not `HEAD` (rule 6 covers that case without `--` requirement and
        // translates differently).
        if args.len() < 3 {
            return None;
        }
        let commit = &args[0];
        if is_flag(commit) {
            return None;
        }
        if commit == OsStr::new("HEAD") {
            // `checkout HEAD -- file` is semantically equivalent to
            // `checkout -- file` plus the HEAD source, but Rule 6 already
            // handles the HEAD case; keep it there to avoid overlap.
            return None;
        }
        if args[1] != OsStr::new("--") {
            return None;
        }
        let files = &args[2..];
        if files.is_empty() {
            return None;
        }

        // Build: `restore --source=<commit> <files>`.
        let mut modern_args: Vec<OsString> = Vec::with_capacity(1 + files.len());
        let mut source_flag = OsString::from("--source=");
        source_flag.push(commit);
        modern_args.push(source_flag);
        modern_args.extend(files.iter().cloned());

        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-restore-from-commit",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("restore", &modern_args),
                note: Some(CHECKOUT_RESTORE_NOTE),
            },
            rewrite: Some(rewrite_subcommand(parsed, "restore", &modern_args)),
        })
    }
}

// ─── Rule 8: `git checkout HEAD <file>…` → `git restore <file>` ───

pub struct CheckoutRestoreFromHead;

impl ModernizationRule for CheckoutRestoreFromHead {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        // Pattern: `HEAD [--] <file>...`. At least HEAD + one file.
        if args.len() < 2 {
            return None;
        }
        if args[0] != OsStr::new("HEAD") {
            return None;
        }
        // Accept optional `--` separator.
        let files_start = if args[1] == OsStr::new("--") { 2 } else { 1 };
        let files = &args[files_start..];
        if files.is_empty() {
            return None;
        }
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-restore-from-head",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("restore", files),
                note: Some(
                    "`restore` defaults to reading from HEAD — the explicit \
                     HEAD argument is no longer needed.",
                ),
            },
            rewrite: Some(rewrite_subcommand(parsed, "restore", files)),
        })
    }
}

// ─── Rule 6: `git checkout -- <file>…` → `git restore <file>` ───

pub struct CheckoutRestoreFile;

impl ModernizationRule for CheckoutRestoreFile {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("--")) {
            return None;
        }
        let files = &args[1..];
        if files.is_empty() {
            return None;
        }
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-restore-file",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("restore", files),
                note: Some(CHECKOUT_RESTORE_NOTE),
            },
            rewrite: Some(rewrite_subcommand(parsed, "restore", files)),
        })
    }
}

// ─── Rule 1: `git checkout <branch>` → `git switch <branch>` (catch-all) ───

pub struct CheckoutSwitchBranch;

impl ModernizationRule for CheckoutSwitchBranch {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("checkout") {
            return None;
        }
        let args = &parsed.subcommand_args;
        // Exactly one positional, non-flag argument, no `--`, no HEAD
        // (HEAD alone is a no-op, not a branch motion).
        if args.len() != 1 {
            return None;
        }
        let only = &args[0];
        if is_flag(only) || only == OsStr::new("HEAD") {
            return None;
        }
        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "checkout-switch-branch",
                legacy_form: render_cmd("checkout", args),
                modern_form: render_cmd("switch", args),
                note: Some(CHECKOUT_SWITCH_NOTE),
            },
            rewrite: Some(rewrite_subcommand(parsed, "switch", args)),
        })
    }
}

// ─── helpers ──────────────────────────────────────────────────────────────

fn is_flag(arg: &OsStr) -> bool {
    arg.as_encoded_bytes().first() == Some(&b'-')
}

/// Build a human-readable `git <sub> <args...>` string from structured args.
/// Lossy conversion to String is intentional — the tip is for the human's
/// terminal, where UTF-8 is overwhelmingly dominant.
fn render_cmd(subcommand: &str, args: &[OsString]) -> String {
    let mut s = format!("git {subcommand}");
    for a in args {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

/// Return `[flag, args...]` as a new Vec.
fn prepend_flag(flag: &str, args: &[OsString]) -> Vec<OsString> {
    let mut v = Vec::with_capacity(args.len() + 1);
    v.push(OsString::from(flag));
    v.extend_from_slice(args);
    v
}

/// Rebuild the full argv with `parsed.global_flags` preserved and the
/// subcommand replaced by `new_sub`, with `new_args` afterwards.
fn rewrite_subcommand(
    parsed: &ParsedGitInvocation,
    new_sub: &str,
    new_args: &[OsString],
) -> Vec<OsString> {
    let mut argv = parsed.global_flags.clone();
    argv.push(OsString::from(new_sub));
    argv.extend_from_slice(new_args);
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modernize::rules::test_support::{osv, parsed};

    // ── CheckoutCreateBranch ────────────────────────────────────────

    #[test]
    fn checkout_b_matches_and_rewrites() {
        let p = parsed(&["checkout", "-b", "feat/auth"]);
        let op = CheckoutCreateBranch.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-create-branch");
        assert_eq!(op.suggestion.legacy_form, "git checkout -b feat/auth");
        assert_eq!(op.suggestion.modern_form, "git switch -c feat/auth");
        assert_eq!(op.rewrite, Some(osv(&["switch", "-c", "feat/auth"])));
    }

    #[test]
    fn checkout_b_with_start_point_preserves_it() {
        let p = parsed(&["checkout", "-b", "feat/auth", "origin/main"]);
        let op = CheckoutCreateBranch.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["switch", "-c", "feat/auth", "origin/main"]))
        );
    }

    #[test]
    fn checkout_b_with_global_flags_are_preserved_in_rewrite() {
        let p = parsed(&["-C", "/tmp/foo", "checkout", "-b", "feat"]);
        let op = CheckoutCreateBranch.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["-C", "/tmp/foo", "switch", "-c", "feat"]))
        );
    }

    #[test]
    fn checkout_without_b_is_not_matched_by_create_branch_rule() {
        let p = parsed(&["checkout", "main"]);
        assert!(CheckoutCreateBranch.examine(&p).is_none());
    }

    #[test]
    fn non_checkout_subcommand_is_ignored() {
        let p = parsed(&["branch", "-b", "x"]);
        assert!(CheckoutCreateBranch.examine(&p).is_none());
    }

    #[test]
    fn dangling_checkout_b_with_no_branch_does_not_match() {
        let p = parsed(&["checkout", "-b"]);
        assert!(CheckoutCreateBranch.examine(&p).is_none());
    }

    // ── CheckoutCreateBranchForce ───────────────────────────────────

    #[test]
    fn checkout_capital_b_rewrites_to_switch_capital_c() {
        let p = parsed(&["checkout", "-B", "feat"]);
        let op = CheckoutCreateBranchForce.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-create-branch-force");
        assert_eq!(op.rewrite, Some(osv(&["switch", "-C", "feat"])));
    }

    #[test]
    fn lowercase_b_is_not_matched_by_force_rule() {
        let p = parsed(&["checkout", "-b", "feat"]);
        assert!(CheckoutCreateBranchForce.examine(&p).is_none());
    }

    // ── CheckoutOrphan ──────────────────────────────────────────────

    #[test]
    fn checkout_orphan_rewrites_to_switch_orphan() {
        let p = parsed(&["checkout", "--orphan", "gh-pages"]);
        let op = CheckoutOrphan.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-orphan");
        assert_eq!(op.rewrite, Some(osv(&["switch", "--orphan", "gh-pages"])));
    }

    // ── CheckoutDetach ──────────────────────────────────────────────

    #[test]
    fn checkout_detach_with_ref() {
        let p = parsed(&["checkout", "--detach", "abc123"]);
        let op = CheckoutDetach.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-detach");
        assert_eq!(op.rewrite, Some(osv(&["switch", "--detach", "abc123"])));
    }

    #[test]
    fn checkout_detach_without_ref() {
        let p = parsed(&["checkout", "--detach"]);
        let op = CheckoutDetach.examine(&p).unwrap();
        assert_eq!(op.rewrite, Some(osv(&["switch", "--detach"])));
    }

    // ── CheckoutRestoreFromCommit ───────────────────────────────────

    #[test]
    fn checkout_commit_double_dash_files_rewrites_with_source() {
        let p = parsed(&["checkout", "abc123", "--", "file.txt"]);
        let op = CheckoutRestoreFromCommit.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-restore-from-commit");
        assert_eq!(
            op.suggestion.modern_form,
            "git restore --source=abc123 file.txt"
        );
        assert_eq!(
            op.rewrite,
            Some(osv(&["restore", "--source=abc123", "file.txt"]))
        );
    }

    #[test]
    fn checkout_commit_double_dash_multiple_files() {
        let p = parsed(&["checkout", "v1.0", "--", "a.txt", "b.txt"]);
        let op = CheckoutRestoreFromCommit.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["restore", "--source=v1.0", "a.txt", "b.txt"]))
        );
    }

    #[test]
    fn head_commit_form_is_not_matched_here_but_by_head_rule() {
        let p = parsed(&["checkout", "HEAD", "--", "file.txt"]);
        assert!(CheckoutRestoreFromCommit.examine(&p).is_none());
    }

    // ── CheckoutRestoreFromHead ─────────────────────────────────────

    #[test]
    fn checkout_head_file_rewrites_to_bare_restore() {
        let p = parsed(&["checkout", "HEAD", "file.txt"]);
        let op = CheckoutRestoreFromHead.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-restore-from-head");
        assert_eq!(op.rewrite, Some(osv(&["restore", "file.txt"])));
    }

    #[test]
    fn checkout_head_double_dash_file_also_works() {
        let p = parsed(&["checkout", "HEAD", "--", "file.txt"]);
        let op = CheckoutRestoreFromHead.examine(&p).unwrap();
        assert_eq!(op.rewrite, Some(osv(&["restore", "file.txt"])));
    }

    #[test]
    fn checkout_head_alone_is_not_matched() {
        // `checkout HEAD` is a no-op and not a restore target.
        let p = parsed(&["checkout", "HEAD"]);
        assert!(CheckoutRestoreFromHead.examine(&p).is_none());
    }

    // ── CheckoutRestoreFile ─────────────────────────────────────────

    #[test]
    fn checkout_double_dash_file_rewrites_to_restore() {
        let p = parsed(&["checkout", "--", "file.txt"]);
        let op = CheckoutRestoreFile.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-restore-file");
        assert_eq!(op.rewrite, Some(osv(&["restore", "file.txt"])));
    }

    #[test]
    fn checkout_double_dash_multiple_files() {
        let p = parsed(&["checkout", "--", "a.txt", "b.txt"]);
        let op = CheckoutRestoreFile.examine(&p).unwrap();
        assert_eq!(op.rewrite, Some(osv(&["restore", "a.txt", "b.txt"])));
    }

    #[test]
    fn dangling_double_dash_with_no_files_does_not_match() {
        let p = parsed(&["checkout", "--"]);
        assert!(CheckoutRestoreFile.examine(&p).is_none());
    }

    // ── CheckoutSwitchBranch (catch-all) ────────────────────────────

    #[test]
    fn bare_checkout_branch_rewrites_to_switch() {
        let p = parsed(&["checkout", "main"]);
        let op = CheckoutSwitchBranch.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "checkout-switch-branch");
        assert_eq!(op.rewrite, Some(osv(&["switch", "main"])));
    }

    #[test]
    fn checkout_alone_does_not_match() {
        let p = parsed(&["checkout"]);
        assert!(CheckoutSwitchBranch.examine(&p).is_none());
    }

    #[test]
    fn checkout_head_alone_does_not_match_switch_branch() {
        let p = parsed(&["checkout", "HEAD"]);
        assert!(CheckoutSwitchBranch.examine(&p).is_none());
    }

    #[test]
    fn checkout_with_multiple_args_does_not_match_switch_branch() {
        // `checkout main foo` is not a simple branch change.
        let p = parsed(&["checkout", "main", "foo"]);
        assert!(CheckoutSwitchBranch.examine(&p).is_none());
    }
}
