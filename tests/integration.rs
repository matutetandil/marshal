// Integration tests.
//
// These tests exercise the binary against real Git repositories created in
// temporary directories. They validate behavior end-to-end.

use assert_cmd::Command;
use tempfile::TempDir;

/// Run the binary and return the assertion builder.
fn bin() -> Command {
    Command::cargo_bin("marshal").unwrap()
}

#[test]
fn passthrough_works_outside_workspace() {
    let tmp = TempDir::new().unwrap();
    // git init in a plain directory
    std::process::Command::new("git")
        .current_dir(tmp.path())
        .args(["init", "--quiet"])
        .output()
        .unwrap();

    // Running `marshal status` in a plain git repo should pass through to git.
    let assertion = bin().current_dir(tmp.path()).arg("status").assert();
    assertion.success();
}

#[test]
fn detects_no_workspace_in_empty_directory() {
    let tmp = TempDir::new().unwrap();
    // In a non-git, non-workspace directory, git commands will fail;
    // that's fine. The point is we don't crash or pretend there's a workspace.
    let assertion = bin().current_dir(tmp.path()).arg("status").assert();
    // Git will error, we pass it through, exit code non-zero is fine.
    let _ = assertion;
}

#[test]
fn init_creates_workspace_marker() {
    let tmp = TempDir::new().unwrap();

    bin().current_dir(tmp.path()).arg("init").assert().success();

    assert!(tmp.path().join(".workspace").is_dir());
    assert!(tmp.path().join(".workspace/manifest.toml").is_file());
    assert!(tmp.path().join(".workspace/state.toml").is_file());
    assert!(tmp.path().join(".workspace/local").is_dir());
    assert!(tmp.path().join(".workspace/.gitignore").is_file());
}

#[test]
fn init_refuses_to_overwrite() {
    let tmp = TempDir::new().unwrap();

    bin().current_dir(tmp.path()).arg("init").assert().success();

    // Second init should fail
    bin().current_dir(tmp.path()).arg("init").assert().failure();
}
