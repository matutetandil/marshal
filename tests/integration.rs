//! Integration tests for Marshal 0.1.0 — pure passthrough.
//!
//! These invoke the compiled binary and verify that, for any invocation, it
//! is indistinguishable from calling `git` directly. Workspace commands and
//! context detection arrive in later releases and will get their own test
//! files when they are wired up.

use std::process::Command as StdCommand;

use assert_cmd::Command;
use tempfile::TempDir;

fn marshal() -> Command {
    Command::cargo_bin("marshal").unwrap()
}

fn init_git_repo() -> TempDir {
    let tmp = TempDir::new().unwrap();
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .expect("git init");
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["config", "user.email", "test@example.com"])
        .status()
        .expect("git config user.email");
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["config", "user.name", "Test"])
        .status()
        .expect("git config user.name");
    tmp
}

/// `marshal --version` must produce exactly what `git --version` produces.
/// This is the most basic fidelity check: when aliased to `git`, users see
/// Git's own version, not Marshal's. (In 0.1.0 Marshal has no voice of its
/// own; its whole job is to be transparent.)
#[test]
fn version_output_matches_git() {
    let direct = StdCommand::new("git")
        .arg("--version")
        .output()
        .expect("run git --version");
    let wrapped = marshal()
        .arg("--version")
        .output()
        .expect("run marshal --version");

    assert_eq!(direct.status.code(), wrapped.status.code());
    assert_eq!(direct.stdout, wrapped.stdout);
    assert_eq!(direct.stderr, wrapped.stderr);
}

/// `marshal status` inside a fresh git repo must match `git status` byte-for-byte.
#[test]
fn status_in_fresh_repo_matches_git() {
    let tmp = init_git_repo();

    let direct = StdCommand::new("git")
        .current_dir(tmp.path())
        .arg("status")
        .output()
        .expect("run git status");
    let wrapped = marshal()
        .current_dir(tmp.path())
        .arg("status")
        .output()
        .expect("run marshal status");

    assert_eq!(direct.status.code(), wrapped.status.code());
    assert_eq!(direct.stdout, wrapped.stdout);
    assert_eq!(direct.stderr, wrapped.stderr);
}

/// Non-zero exit codes from git must reach the caller unchanged.
#[test]
fn nonzero_exit_codes_propagate() {
    let tmp = TempDir::new().unwrap();

    let direct = StdCommand::new("git")
        .current_dir(tmp.path())
        .arg("status")
        .output()
        .expect("run git status outside a repo");
    let wrapped = marshal()
        .current_dir(tmp.path())
        .arg("status")
        .output()
        .expect("run marshal status outside a repo");

    assert!(
        !direct.status.success(),
        "precondition: git status outside a repo should fail"
    );
    assert_eq!(direct.status.code(), wrapped.status.code());
}

/// An unknown git subcommand passes through unchanged. Marshal never intercepts
/// or "corrects" commands in 0.1.0.
#[test]
fn unknown_subcommand_is_forwarded() {
    let direct = StdCommand::new("git")
        .arg("definitely-not-a-git-subcommand-xyz")
        .output()
        .expect("run git <unknown>");
    let wrapped = marshal()
        .arg("definitely-not-a-git-subcommand-xyz")
        .output()
        .expect("run marshal <unknown>");

    assert_eq!(direct.status.code(), wrapped.status.code());
    assert_eq!(direct.stderr, wrapped.stderr);
}

/// A successful commit round-trip: init, add, commit, log. Exercises several
/// commands in sequence and confirms marshal threads through.
#[test]
fn commit_round_trip_works_through_marshal() {
    let tmp = init_git_repo();

    std::fs::write(tmp.path().join("file.txt"), b"hello").unwrap();

    marshal()
        .current_dir(tmp.path())
        .args(["add", "file.txt"])
        .assert()
        .success();

    marshal()
        .current_dir(tmp.path())
        .args(["commit", "-m", "initial"])
        .assert()
        .success();

    let log = marshal()
        .current_dir(tmp.path())
        .args(["log", "--oneline"])
        .output()
        .expect("marshal log");
    assert!(log.status.success());
    assert!(
        String::from_utf8_lossy(&log.stdout).contains("initial"),
        "expected commit subject to appear in log output"
    );
}

/// Arguments with spaces and unicode survive the passthrough. Ensures we never
/// reinterpret or re-quote argv on the way to git.
#[test]
fn args_with_spaces_and_unicode_are_preserved() {
    let tmp = init_git_repo();

    std::fs::write(tmp.path().join("file.txt"), b"hi").unwrap();
    marshal()
        .current_dir(tmp.path())
        .args(["add", "file.txt"])
        .assert()
        .success();

    let subject = "mensaje con espacios y unicode: café 🚀";
    marshal()
        .current_dir(tmp.path())
        .args(["commit", "-m", subject])
        .assert()
        .success();

    let log = marshal()
        .current_dir(tmp.path())
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .expect("marshal log");
    assert!(log.status.success());
    let logged = String::from_utf8_lossy(&log.stdout);
    assert_eq!(logged.trim_end(), subject);
}
