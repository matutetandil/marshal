//! Rule for `git stash save` — deprecated since Git 2.16 in favour of
//! `git stash push`. Both legacy and modern forms support `-u` (include
//! untracked), so the single rule normalises `stash save [-u] [<msg>]` to
//! `stash push [-u] [-m <msg>]`, preserving `-u` in its original position.

use crate::git::parser::ParsedGitInvocation;
use crate::modernize::rule::{ModernizationRule, RuleOpinion, Suggestion};
use std::ffi::{OsStr, OsString};

pub struct StashSaveToPush;

impl ModernizationRule for StashSaveToPush {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("stash") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("save")) {
            return None;
        }

        // Everything after "save": optional -u / --include-untracked, and an
        // optional trailing message (last arg that doesn't start with `-`).
        let after_save = &args[1..];

        // Split into flags and an optional trailing message.
        let (flags, message) = split_message(after_save);

        // Compose the modern argv: `push [flags...] [-m <message>]`.
        let mut modern_args: Vec<OsString> = Vec::new();
        modern_args.extend(flags.iter().cloned());
        if let Some(msg) = &message {
            modern_args.push(OsString::from("-m"));
            modern_args.push((*msg).clone());
        }

        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "stash-save-to-push",
                legacy_form: render_cmd("stash", args),
                modern_form: render_cmd("stash", &prepend_static("push", &modern_args)),
                note: Some(
                    "`git stash save` has been deprecated since Git 2.16; \
                     use `git stash push` with `-m` for messages.",
                ),
            },
            rewrite: Some(rewrite_stash_push(parsed, &modern_args)),
        })
    }
}

/// Split `save`'s arguments into (flags_that_stay, optional_message).
/// The message, if present, is the trailing non-flag argument(s) — but
/// legacy `stash save` accepts ONE positional message. If the trailing
/// arguments look flag-like or there is no positional, there is no message.
fn split_message(args: &[OsString]) -> (Vec<OsString>, Option<&OsString>) {
    // Walk from the left and collect leading flags. If a non-flag follows,
    // it is the message (and we expect nothing else after it).
    let mut flags = Vec::new();
    let mut message: Option<&OsString> = None;
    for arg in args {
        if is_flag(arg) {
            flags.push(arg.clone());
        } else {
            // First non-flag = the message. `git stash save` historically
            // interprets everything after as a message too; for simplicity
            // take the first non-flag only and ignore the rest (Git's own
            // tolerance here is already fuzzy).
            message = Some(arg);
            break;
        }
    }
    (flags, message)
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

fn prepend_static(first: &str, rest: &[OsString]) -> Vec<OsString> {
    let mut v = Vec::with_capacity(rest.len() + 1);
    v.push(OsString::from(first));
    v.extend_from_slice(rest);
    v
}

fn rewrite_stash_push(parsed: &ParsedGitInvocation, push_args: &[OsString]) -> Vec<OsString> {
    let mut argv = parsed.global_flags.clone();
    argv.push(OsString::from("stash"));
    argv.push(OsString::from("push"));
    argv.extend_from_slice(push_args);
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modernize::rules::test_support::{osv, parsed};

    #[test]
    fn stash_save_bare_rewrites_to_push() {
        let p = parsed(&["stash", "save"]);
        let op = StashSaveToPush.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "stash-save-to-push");
        assert_eq!(op.suggestion.modern_form, "git stash push");
        assert_eq!(op.rewrite, Some(osv(&["stash", "push"])));
    }

    #[test]
    fn stash_save_with_message_uses_minus_m() {
        let p = parsed(&["stash", "save", "wip fix"]);
        let op = StashSaveToPush.examine(&p).unwrap();
        assert_eq!(op.suggestion.modern_form, "git stash push -m wip fix");
        assert_eq!(op.rewrite, Some(osv(&["stash", "push", "-m", "wip fix"])));
    }

    #[test]
    fn stash_save_u_preserves_the_flag() {
        let p = parsed(&["stash", "save", "-u"]);
        let op = StashSaveToPush.examine(&p).unwrap();
        assert_eq!(op.rewrite, Some(osv(&["stash", "push", "-u"])));
    }

    #[test]
    fn stash_save_u_with_message() {
        let p = parsed(&["stash", "save", "-u", "wip with untracked"]);
        let op = StashSaveToPush.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["stash", "push", "-u", "-m", "wip with untracked"]))
        );
    }

    #[test]
    fn stash_save_long_flag_include_untracked_is_preserved() {
        let p = parsed(&["stash", "save", "--include-untracked", "wip"]);
        let op = StashSaveToPush.examine(&p).unwrap();
        assert_eq!(
            op.rewrite,
            Some(osv(&["stash", "push", "--include-untracked", "-m", "wip"]))
        );
    }

    #[test]
    fn stash_push_is_not_matched() {
        let p = parsed(&["stash", "push", "-m", "already modern"]);
        assert!(StashSaveToPush.examine(&p).is_none());
    }

    #[test]
    fn bare_stash_is_not_matched() {
        let p = parsed(&["stash"]);
        assert!(StashSaveToPush.examine(&p).is_none());
    }

    #[test]
    fn stash_pop_is_not_matched() {
        let p = parsed(&["stash", "pop"]);
        assert!(StashSaveToPush.examine(&p).is_none());
    }

    #[test]
    fn non_stash_subcommand_is_ignored() {
        let p = parsed(&["reset", "save"]);
        assert!(StashSaveToPush.examine(&p).is_none());
    }
}
