//! Rule for `git reset` — the file-mode split into `git restore --staged`
//! introduced alongside `switch`/`restore` in Git 2.23.
//!
//! Scope narrowed carefully: `git reset` is an overloaded command; only the
//! file-mode usage (`reset [HEAD] <file>…`, which unstages files) has a
//! direct `restore --staged` replacement. `git reset`, `git reset --hard`,
//! `git reset <commit>`, etc. are NOT matched here — they remain idiomatic
//! uses of `reset`.

use crate::git::parser::ParsedGitInvocation;
use crate::modernize::rule::{ModernizationRule, RuleOpinion, Suggestion};
use std::ffi::{OsStr, OsString};

pub struct ResetRestoreStaged;

impl ModernizationRule for ResetRestoreStaged {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("reset") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.is_empty() {
            return None;
        }

        // File-mode reset has two forms:
        //   reset <file>…            (HEAD implicit)
        //   reset HEAD <file>…
        // Either way there is no flag and no commit-ish other than optional
        // HEAD before the files. We take a conservative approach: match only
        // these two specific shapes. Anything with a flag (`--soft`,
        // `--hard`, `--mixed`, `--keep`, `--merge`, `--patch`, `-p`) is a
        // commit-level reset, not a file unstage — skip.

        if args.iter().any(|a| is_flag(a)) {
            return None;
        }

        // Find where the files start.
        let files_start = if args[0] == OsStr::new("HEAD") { 1 } else { 0 };
        let files = &args[files_start..];
        if files.is_empty() {
            return None;
        }

        // Reject if the "file" position actually looks like a commit-ish
        // (e.g. `reset HEAD~1` with no files). A conservative heuristic:
        // `reset <non-HEAD single arg>` could be a commit reset, so skip
        // the single-non-HEAD-arg case when HEAD is absent.
        if files_start == 0 && files.len() == 1 {
            // Could be `reset <file>` or `reset <commit>`. Marshal cannot
            // tell without the filesystem. Be conservative: skip.
            return None;
        }

        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "reset-restore-staged",
                legacy_form: render_cmd("reset", args),
                modern_form: render_cmd("restore", &prepend_flag("--staged", files)),
                note: Some(
                    "`restore --staged` was split out of `reset` in Git 2.23 \
                     specifically for unstaging.",
                ),
            },
            rewrite: Some(rewrite_subcommand(
                parsed,
                "restore",
                &prepend_flag("--staged", files),
            )),
        })
    }
}

fn is_flag(arg: &OsStr) -> bool {
    arg.as_encoded_bytes().first() == Some(&b'-')
}

fn render_cmd(subcommand: &str, args: &[OsString]) -> String {
    let mut s = format!("git {subcommand}");
    for a in args {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

fn prepend_flag(flag: &str, args: &[OsString]) -> Vec<OsString> {
    let mut v = Vec::with_capacity(args.len() + 1);
    v.push(OsString::from(flag));
    v.extend_from_slice(args);
    v
}

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

    #[test]
    fn reset_head_file_rewrites_to_restore_staged() {
        let p = parsed(&["reset", "HEAD", "file.txt"]);
        let op = ResetRestoreStaged.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "reset-restore-staged");
        assert_eq!(op.suggestion.modern_form, "git restore --staged file.txt");
        assert_eq!(op.rewrite, Some(osv(&["restore", "--staged", "file.txt"])));
    }

    #[test]
    fn reset_head_multiple_files() {
        let p = parsed(&["reset", "HEAD", "a.txt", "b.txt"]);
        let op = ResetRestoreStaged.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["restore", "--staged", "a.txt", "b.txt"]))
        );
    }

    #[test]
    fn reset_multiple_files_without_head_is_handled() {
        // Two non-HEAD positionals → definitely not a commit-reset, must be
        // multiple files.
        let p = parsed(&["reset", "a.txt", "b.txt"]);
        let op = ResetRestoreStaged.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["restore", "--staged", "a.txt", "b.txt"]))
        );
    }

    #[test]
    fn single_non_head_arg_is_skipped_as_ambiguous() {
        // Could be `reset <commit>` or `reset <file>`. Marshal cannot tell
        // without filesystem access, so we leave it to Git.
        let p = parsed(&["reset", "abc123"]);
        assert!(ResetRestoreStaged.examine(&p).is_none());
    }

    #[test]
    fn reset_hard_is_not_matched() {
        let p = parsed(&["reset", "--hard", "HEAD~1"]);
        assert!(ResetRestoreStaged.examine(&p).is_none());
    }

    #[test]
    fn reset_soft_is_not_matched() {
        let p = parsed(&["reset", "--soft", "HEAD~2"]);
        assert!(ResetRestoreStaged.examine(&p).is_none());
    }

    #[test]
    fn bare_reset_is_not_matched() {
        let p = parsed(&["reset"]);
        assert!(ResetRestoreStaged.examine(&p).is_none());
    }

    #[test]
    fn non_reset_subcommand_is_ignored() {
        let p = parsed(&["checkout", "HEAD", "file.txt"]);
        assert!(ResetRestoreStaged.examine(&p).is_none());
    }
}
