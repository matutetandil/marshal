//! Parse a Git invocation's argv into a structured view.
//!
//! This module takes `[OsString]` (argv with argv[0] already stripped) and
//! splits it into global options, subcommand, and subcommand arguments. It is
//! the substrate every modernization rule and command interceptor consults so
//! that argv is walked only once.
//!
//! **No validation, no rewriting.** If the user typed a nonsensical invocation,
//! the parser returns a best-effort structured view and leaves the decision of
//! "this is malformed" to Git itself downstream. Passthrough fidelity demands
//! that we never second-guess Git's own argument grammar.
//!
//! Cross-platform note: arguments stay as `OsString` throughout, preserving
//! non-UTF-8 paths on Unix and wide-char arguments on Windows. Byte comparisons
//! for flag detection use `OsStr::as_encoded_bytes`, whose leading ASCII bytes
//! are stable across OS encodings.
//!
use std::ffi::{OsStr, OsString};

/// A structured view over a Git invocation's argv (argv[0] already removed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedGitInvocation {
    /// Global options that come *before* the subcommand — things like `-C`,
    /// `-c key=value`, `--git-dir=...`, `-p`, `--no-pager`.
    pub global_flags: Vec<OsString>,

    /// The first non-flag argument, typically the subcommand (`checkout`,
    /// `log`, `commit`). `None` when the user only typed global flags (e.g.
    /// `git --version`) or passed nothing at all.
    pub subcommand: Option<OsString>,

    /// Everything after the subcommand, verbatim and ungrouped. Passed to
    /// rules for pattern matching and to the passthrough path unchanged.
    pub subcommand_args: Vec<OsString>,

    /// The original argv exactly as received. Preserved so rewriting rules
    /// can reconstruct the command and the passthrough path can forward a
    /// byte-exact copy when no rule fires.
    pub raw: Vec<OsString>,
}

impl ParsedGitInvocation {
    /// Convenience: `true` when the subcommand equals `name` exactly.
    pub fn subcommand_is(&self, name: &str) -> bool {
        self.subcommand
            .as_deref()
            .map(|s| s == OsStr::new(name))
            .unwrap_or(false)
    }
}

/// Parse `argv` (argv[0] must already be stripped) into a structured view.
pub fn parse(argv: &[OsString]) -> ParsedGitInvocation {
    let raw = argv.to_vec();
    let mut global_flags: Vec<OsString> = Vec::new();
    let mut subcommand: Option<OsString> = None;
    let mut subcommand_args: Vec<OsString> = Vec::new();

    let mut i = 0;
    while i < argv.len() {
        let arg = &argv[i];

        if !is_flag(arg) {
            // First positional: this is the subcommand. Everything after is
            // subcommand scope.
            subcommand = Some(arg.clone());
            subcommand_args = argv[i + 1..].to_vec();
            break;
        }

        if takes_next_arg_as_value(arg) {
            global_flags.push(arg.clone());
            if let Some(next) = argv.get(i + 1) {
                // Consume the value too, without inspecting it. It may itself
                // start with `-` (e.g. `-c -something=foo`), and that's fine —
                // Git accepts it.
                global_flags.push(next.clone());
                i += 2;
                continue;
            }
            // Dangling option with no value. The user is giving Git a broken
            // invocation; push what we have and exit the loop with no
            // subcommand. Git will error on its own terms.
            i += 1;
            continue;
        }

        // Regular global flag (value-less, or in `--opt=value` form).
        global_flags.push(arg.clone());
        i += 1;
    }

    ParsedGitInvocation {
        global_flags,
        subcommand,
        subcommand_args,
        raw,
    }
}

/// Global options that consume the *next* argv token as their value when
/// not written in `--opt=value` form.
///
/// The list comes from `git(1)` — OPTIONS section. Flags outside this list
/// that happen to take values always do so via the `=` form (verified against
/// Git docs). If a future Git version adds a new value-taking global option
/// in separated form, this list needs an entry for correct parsing of that
/// form; the `=`-form always parses correctly without updates here.
const GLOBAL_OPTIONS_WITH_VALUE: &[&str] = &[
    "-C",
    "-c",
    "--git-dir",
    "--work-tree",
    "--namespace",
    "--super-prefix",
    "--config-env",
    "--attr-source",
];

fn takes_next_arg_as_value(arg: &OsStr) -> bool {
    GLOBAL_OPTIONS_WITH_VALUE
        .iter()
        .any(|opt| arg == OsStr::new(opt))
}

fn is_flag(arg: &OsStr) -> bool {
    // Leading ASCII `-` byte is stable across OsStr encodings on all
    // platforms we support (Unix raw bytes, Windows WTF-8-ish).
    arg.as_encoded_bytes().first() == Some(&b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn osv(strs: &[&str]) -> Vec<OsString> {
        strs.iter().map(OsString::from).collect()
    }

    #[test]
    fn empty_argv_yields_no_subcommand() {
        let p = parse(&[]);
        assert!(p.global_flags.is_empty());
        assert!(p.subcommand.is_none());
        assert!(p.subcommand_args.is_empty());
        assert!(p.raw.is_empty());
    }

    #[test]
    fn just_a_subcommand() {
        let p = parse(&osv(&["status"]));
        assert!(p.global_flags.is_empty());
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("status")));
        assert!(p.subcommand_args.is_empty());
    }

    #[test]
    fn subcommand_with_args() {
        let p = parse(&osv(&["log", "--oneline", "-n", "5"]));
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("log")));
        assert_eq!(p.subcommand_args, osv(&["--oneline", "-n", "5"]));
        assert!(p.global_flags.is_empty());
    }

    #[test]
    fn global_paginate_flag_before_subcommand() {
        let p = parse(&osv(&["-p", "log"]));
        assert_eq!(p.global_flags, osv(&["-p"]));
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("log")));
    }

    #[test]
    fn dash_c_consumes_next_arg_as_value() {
        let p = parse(&osv(&["-c", "user.name=X", "commit", "-m", "msg"]));
        assert_eq!(p.global_flags, osv(&["-c", "user.name=X"]));
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("commit")));
        assert_eq!(p.subcommand_args, osv(&["-m", "msg"]));
    }

    #[test]
    fn capital_c_consumes_next_arg_as_value() {
        let p = parse(&osv(&["-C", "/tmp/foo", "status"]));
        assert_eq!(p.global_flags, osv(&["-C", "/tmp/foo"]));
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("status")));
    }

    #[test]
    fn git_dir_long_form_with_separate_value() {
        let p = parse(&osv(&["--git-dir", "/repo/.git", "log"]));
        assert_eq!(p.global_flags, osv(&["--git-dir", "/repo/.git"]));
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("log")));
    }

    #[test]
    fn equals_form_does_not_consume_the_next_arg() {
        let p = parse(&osv(&["--git-dir=/repo/.git", "log"]));
        assert_eq!(p.global_flags, osv(&["--git-dir=/repo/.git"]));
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("log")));
    }

    #[test]
    fn version_alone_yields_no_subcommand() {
        // `git --version` is a terminal action; there is no subcommand.
        let p = parse(&osv(&["--version"]));
        assert_eq!(p.global_flags, osv(&["--version"]));
        assert!(p.subcommand.is_none());
        assert!(p.subcommand_args.is_empty());
    }

    #[test]
    fn multiple_global_flags_before_subcommand() {
        let p = parse(&osv(&[
            "-p",
            "-c",
            "color.ui=always",
            "-C",
            "/tmp",
            "status",
            "-s",
        ]));
        assert_eq!(
            p.global_flags,
            osv(&["-p", "-c", "color.ui=always", "-C", "/tmp"])
        );
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("status")));
        assert_eq!(p.subcommand_args, osv(&["-s"]));
    }

    #[test]
    fn dangling_value_option_at_end_is_tolerated() {
        // `git -c` with no value is a user error. The parser must not panic
        // and should leave subcommand unset so Git gets the malformed
        // invocation and produces its own error message.
        let p = parse(&osv(&["-c"]));
        assert_eq!(p.global_flags, osv(&["-c"]));
        assert!(p.subcommand.is_none());
    }

    #[test]
    fn raw_argv_is_preserved_exactly() {
        let input = osv(&["-c", "user.email=x", "commit", "-m", "msg"]);
        let p = parse(&input);
        assert_eq!(p.raw, input);
    }

    #[test]
    fn subcommand_is_exact_match_only() {
        let p = parse(&osv(&["checkout", "-b", "foo"]));
        assert!(p.subcommand_is("checkout"));
        assert!(!p.subcommand_is("check"));
        assert!(!p.subcommand_is("checkouts"));
    }

    #[cfg(unix)]
    #[test]
    fn non_utf8_os_strings_survive_round_trip() {
        // On Unix, argv can contain arbitrary bytes. Paths with 0xFF are
        // valid filenames but not valid UTF-8. Marshal must forward them
        // intact.
        use std::os::unix::ffi::OsStringExt;
        let bad = OsString::from_vec(vec![b'f', b'i', b'l', b'e', 0xFF]);
        let input = vec![OsString::from("add"), bad.clone()];
        let p = parse(&input);
        assert_eq!(p.subcommand.as_deref(), Some(OsStr::new("add")));
        assert_eq!(p.subcommand_args, vec![bad.clone()]);
        assert_eq!(p.raw, vec![OsString::from("add"), bad]);
    }
}
