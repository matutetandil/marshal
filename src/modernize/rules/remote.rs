//! Rule for `git remote rm` — deprecated in favour of `git remote remove`.
//!
//! The translation is a straight subcommand rename; all positional arguments
//! (the remote name) and options are forwarded unchanged.

use crate::git::parser::ParsedGitInvocation;
use crate::modernize::rule::{ModernizationRule, RuleOpinion, Suggestion};
use std::ffi::{OsStr, OsString};

pub struct RemoteRmToRemove;

impl ModernizationRule for RemoteRmToRemove {
    fn examine(&self, parsed: &ParsedGitInvocation) -> Option<RuleOpinion> {
        if !parsed.subcommand_is("remote") {
            return None;
        }
        let args = &parsed.subcommand_args;
        if args.first().map(OsString::as_os_str) != Some(OsStr::new("rm")) {
            return None;
        }
        let rest = &args[1..];
        if rest.is_empty() {
            // `git remote rm` alone is a user error — let Git complain.
            return None;
        }

        let mut modern_args: Vec<OsString> = Vec::with_capacity(rest.len() + 1);
        modern_args.push(OsString::from("remove"));
        modern_args.extend_from_slice(rest);

        Some(RuleOpinion {
            suggestion: Suggestion {
                rule_id: "remote-rm-to-remove",
                legacy_form: render_cmd("remote", args),
                modern_form: render_cmd("remote", &modern_args),
                note: Some("`git remote rm` has been deprecated in favour of `git remote remove`."),
            },
            rewrite: Some(rewrite_remote(parsed, &modern_args)),
        })
    }
}

fn render_cmd(subcommand: &str, args: &[OsString]) -> String {
    let mut s = format!("git {subcommand}");
    for a in args {
        s.push(' ');
        s.push_str(&a.to_string_lossy());
    }
    s
}

fn rewrite_remote(parsed: &ParsedGitInvocation, remote_args: &[OsString]) -> Vec<OsString> {
    let mut argv = parsed.global_flags.clone();
    argv.push(OsString::from("remote"));
    argv.extend_from_slice(remote_args);
    argv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modernize::rules::test_support::{osv, parsed};

    #[test]
    fn remote_rm_name_rewrites_to_remove() {
        let p = parsed(&["remote", "rm", "origin"]);
        let op = RemoteRmToRemove.examine(&p).unwrap();
        assert_eq!(op.suggestion.rule_id, "remote-rm-to-remove");
        assert_eq!(op.suggestion.modern_form, "git remote remove origin");
        assert_eq!(op.rewrite, Some(osv(&["remote", "remove", "origin"])));
    }

    #[test]
    fn remote_rm_without_name_does_not_match() {
        let p = parsed(&["remote", "rm"]);
        assert!(RemoteRmToRemove.examine(&p).is_none());
    }

    #[test]
    fn remote_remove_is_not_matched() {
        let p = parsed(&["remote", "remove", "origin"]);
        assert!(RemoteRmToRemove.examine(&p).is_none());
    }

    #[test]
    fn remote_add_is_not_matched() {
        let p = parsed(&["remote", "add", "origin", "git@github.com:x/y.git"]);
        assert!(RemoteRmToRemove.examine(&p).is_none());
    }

    #[test]
    fn bare_remote_is_not_matched() {
        let p = parsed(&["remote"]);
        assert!(RemoteRmToRemove.examine(&p).is_none());
    }

    #[test]
    fn non_remote_subcommand_is_ignored() {
        let p = parsed(&["branch", "rm"]);
        assert!(RemoteRmToRemove.examine(&p).is_none());
    }
}
