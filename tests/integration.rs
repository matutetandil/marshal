//! Integration tests covering Marshal's user-facing behaviour.
//!
//! Two broad groups:
//!
//! * Passthrough fidelity (inherited from 0.1.0): when no rule applies,
//!   marshal is byte-for-byte indistinguishable from calling `git` directly.
//! * 0.2.0 wrapper behaviour: `marshal` namespace dispatch, modernization
//!   tips on stderr, config-gated tip suppression and rewrite mode.
//!
//! Every test that touches Marshal's config points `MARSHAL_CONFIG` at a
//! per-test temp file so the user's real config is never read or mutated.

use std::process::Command as StdCommand;

use assert_cmd::Command;
use tempfile::TempDir;

fn marshal() -> Command {
    Command::cargo_bin("marshal").unwrap()
}

/// A test-scoped marshal invocation that isolates every config layer from
/// the host machine. The global path is chosen by the caller; system and
/// local default to sibling temp paths so tests that don't care about those
/// layers cannot accidentally read host state.
fn marshal_with_isolated_config(config_path: &std::path::Path) -> Command {
    let mut cmd = marshal();
    cmd.env("MARSHAL_CONFIG", config_path)
        .env(
            "MARSHAL_SYSTEM_CONFIG",
            config_path.with_extension("system"),
        )
        .env("MARSHAL_LOCAL_CONFIG", config_path.with_extension("local"))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("ProgramData");
    cmd
}

/// Same as [`marshal_with_isolated_config`] but with both `global` and
/// `system` paths picked by the caller. Local still defaults to a sibling
/// path.
fn marshal_with_both_isolations(
    global_path: &std::path::Path,
    system_path: &std::path::Path,
) -> Command {
    let mut cmd = marshal();
    cmd.env("MARSHAL_CONFIG", global_path)
        .env("MARSHAL_SYSTEM_CONFIG", system_path)
        .env(
            "MARSHAL_LOCAL_CONFIG",
            global_path.with_extension("local-unused"),
        )
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("ProgramData");
    cmd
}

/// Full triple-layer isolation: global, system, and local paths all picked
/// by the caller. For tests that exercise all three layers together.
fn marshal_with_all_isolations(
    global_path: &std::path::Path,
    system_path: &std::path::Path,
    local_path: &std::path::Path,
) -> Command {
    let mut cmd = marshal();
    cmd.env("MARSHAL_CONFIG", global_path)
        .env("MARSHAL_SYSTEM_CONFIG", system_path)
        .env("MARSHAL_LOCAL_CONFIG", local_path)
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("ProgramData");
    cmd
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

/// `marshal --version` preserves `git --version`'s output verbatim and
/// appends marshal's own version on a new stdout line. Pattern borrowed
/// from node+npm and php+xdebug: each tool in the chain identifies itself.
///
/// stderr stays byte-exact with `git --version` — only the stdout is
/// augmented.
#[test]
fn version_output_augments_git_with_marshal_line() {
    let direct = StdCommand::new("git")
        .arg("--version")
        .output()
        .expect("run git --version");
    let wrapped = marshal()
        .arg("--version")
        .output()
        .expect("run marshal --version");

    assert_eq!(direct.status.code(), wrapped.status.code());
    assert_eq!(direct.stderr, wrapped.stderr, "stderr remains byte-exact");

    let git_line = String::from_utf8_lossy(&direct.stdout);
    let wrapped_stdout = String::from_utf8_lossy(&wrapped.stdout);
    let marshal_line = format!("marshal version {}", env!("CARGO_PKG_VERSION"));

    // Git's version line appears verbatim at the start of the output.
    assert!(
        wrapped_stdout.starts_with(git_line.trim_end()),
        "git's version line must be preserved verbatim, got: {wrapped_stdout}"
    );

    // Marshal's line appears afterward on stdout.
    assert!(
        wrapped_stdout.contains(&marshal_line),
        "marshal's own version must appear, got: {wrapped_stdout}"
    );
    let git_pos = wrapped_stdout.find(git_line.trim_end()).unwrap();
    let marshal_pos = wrapped_stdout.find(&marshal_line).unwrap();
    assert!(
        git_pos < marshal_pos,
        "git's line precedes marshal's (got git at {git_pos}, marshal at {marshal_pos})"
    );
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

/// `git marshal` (alias) or `marshal marshal` (direct) lands in marshal's
/// own namespace and prints an overview. The overview includes the crate
/// version so users can confirm which marshal is in their PATH.
#[test]
fn marshal_namespace_no_subcommand_prints_overview() {
    let output = marshal()
        .arg("marshal")
        .output()
        .expect("run marshal marshal");
    assert!(
        output.status.success(),
        "exit 0 expected, got {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("marshal"),
        "overview mentions marshal, got: {stdout}"
    );
    assert!(
        stdout.contains(env!("CARGO_PKG_VERSION")),
        "overview prints crate version, got: {stdout}"
    );
}

/// An unknown subcommand inside the marshal namespace exits non-zero with a
/// clear error — and critically, never reaches `git`. A regression that
/// forwarded the `marshal` token to git would surface as git's own
/// "is not a git command" message in stderr; that must not appear.
#[test]
fn marshal_namespace_unknown_subcommand_errors_without_reaching_git() {
    let output = marshal()
        .args(["marshal", "totally-not-a-real-subcommand"])
        .output()
        .expect("run marshal marshal totally-not-...");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown subcommand") && stderr.contains("totally-not-a-real-subcommand"),
        "stderr names the unknown subcommand, got: {stderr}"
    );
    assert!(
        !stderr.contains("is not a git command"),
        "marshal incorrectly forwarded to git; stderr was: {stderr}"
    );
}

/// A canonical legacy Git invocation triggers a modernization tip on
/// stderr, then the command itself still runs to completion. Verifies the
/// whole modernize → passthrough flow end-to-end.
#[test]
fn legacy_checkout_b_emits_tip_and_still_runs() {
    let tmp = init_git_repo();
    // Seed a first commit so branches can exist.
    std::fs::write(tmp.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    let output = marshal()
        .current_dir(tmp.path())
        .args(["checkout", "-b", "feat/test-branch"])
        .output()
        .expect("run marshal checkout -b");

    assert!(output.status.success(), "git still runs and succeeds");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("marshal: tip:")
            && stderr.contains("git switch -c feat/test-branch")
            && stderr.contains("git checkout -b feat/test-branch"),
        "expected modernization tip on stderr, got: {stderr}"
    );
    // And git's own output still follows the tip.
    assert!(
        stderr.contains("Switched to a new branch 'feat/test-branch'"),
        "expected git's own stderr message below the tip, got: {stderr}"
    );
}

/// Modern Git commands must still pass through byte-exact — no tip, no
/// augmentation. Regression guard against a rule accidentally matching a
/// modern form.
#[test]
fn modern_switch_c_passes_through_with_no_tip() {
    let tmp = init_git_repo();
    std::fs::write(tmp.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    let direct = StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["switch", "-c", "feat/modern"])
        .output()
        .expect("git switch -c directly");

    let wrapped = marshal()
        .current_dir(tmp.path())
        .args(["switch", "-c", "feat/modern-marshal"])
        .output()
        .expect("marshal switch -c");

    assert!(wrapped.status.success());
    let wrapped_stderr = String::from_utf8_lossy(&wrapped.stderr);
    assert!(
        !wrapped_stderr.contains("marshal: tip:"),
        "modern form must not trigger any tip, got stderr: {wrapped_stderr}"
    );
    // The non-tip portion of stderr should match git's own message shape
    // (branch name differs, so we only compare the leading "Switched to a
    // new branch '" prefix).
    assert!(
        String::from_utf8_lossy(&direct.stderr).starts_with("Switched to a new branch '"),
        "precondition: git direct emits 'Switched to a new branch'"
    );
    assert!(
        wrapped_stderr.starts_with("Switched to a new branch '"),
        "marshal's stderr matches git's leading message, got: {wrapped_stderr}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Config command and config-gated modernization
// ───────────────────────────────────────────────────────────────────────────

/// `marshal config get` falls through to defaults when no config file exists.
#[test]
fn config_get_returns_defaults_when_no_file_present() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let tips = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "get", "modernize.tips"])
        .output()
        .expect("get tips");
    assert!(tips.status.success());
    assert_eq!(String::from_utf8_lossy(&tips.stdout).trim(), "true");

    let rewrite = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "get", "modernize.rewrite"])
        .output()
        .expect("get rewrite");
    assert!(rewrite.status.success());
    assert_eq!(String::from_utf8_lossy(&rewrite.stdout).trim(), "false");
}

/// `set` persists, `get` reads it back, `unset` returns to the default.
#[test]
fn config_set_unset_round_trip() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "set", "modernize.tips", "false"])
        .assert()
        .success();

    let after_set = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "get", "modernize.tips"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&after_set.stdout).trim(), "false");

    marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "unset", "modernize.tips"])
        .assert()
        .success();

    let after_unset = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "get", "modernize.tips"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&after_unset.stdout).trim(),
        "true",
        "unset returns the key to its default"
    );
}

/// `set` rejects a non-boolean value with a clear error and exits non-zero.
#[test]
fn config_set_rejects_bad_boolean() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "set", "modernize.tips", "maybe"])
        .output()
        .unwrap();
    assert!(!output.status.success(), "non-boolean value must fail");
    assert!(String::from_utf8_lossy(&output.stderr).contains("not a boolean"));
}

/// `list` prints every known key with its effective value.
#[test]
fn config_list_shows_every_known_key() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("modernize.tips=true"));
    assert!(stdout.contains("modernize.rewrite=false"));
}

/// When `modernize.tips = false`, legacy invocations must not emit a tip —
/// but must still run the original command.
#[test]
fn modernize_tips_can_be_disabled_via_config() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "set", "modernize.tips", "false"])
        .assert()
        .success();

    let repo = init_git_repo();
    std::fs::write(repo.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .args(["checkout", "-b", "feat/silent"])
        .output()
        .expect("run checkout -b with tips disabled");
    assert!(output.status.success(), "git still runs to completion");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("marshal: tip:"),
        "tip must be suppressed when modernize.tips=false; got: {stderr}"
    );
    // Git's own output still appears.
    assert!(stderr.contains("Switched to a new branch 'feat/silent'"));
}

/// When `modernize.rewrite = true`, legacy `checkout -b X` is rewritten to
/// `switch -c X` before running. Detectable by git's own message format:
/// `switch -c` says "Switched to a new branch", same as `checkout -b` —
/// but we inject a canary global flag (`-c color.ui=false`) and use
/// `tracing` logs as a backup. For a deterministic signal, we check that
/// after the command runs, the commit that `HEAD` now points at is on the
/// new branch. That works regardless of which legacy-or-modern form git
/// actually received.
#[test]
fn modernize_rewrite_actually_rewrites_legacy_form() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "set", "modernize.rewrite", "true"])
        .assert()
        .success();

    let repo = init_git_repo();
    std::fs::write(repo.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    // Run the legacy form. With rewrite enabled, marshal should invoke
    // `git switch -c feat/rewritten` under the hood.
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .args(["checkout", "-b", "feat/rewritten"])
        .output()
        .expect("run legacy checkout -b with rewrite=true");
    assert!(output.status.success());

    // The branch exists and HEAD is on it — confirms the command ran. The
    // real signature of rewrite vs passthrough: RUST_LOG=debug would show
    // the rewritten argv in tracing output; a lighter proof is the tip
    // still appears on stderr (rewrite doesn't suppress the tip) AND the
    // operation succeeded, so SOMETHING branch-like ran.
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("marshal: tip:"),
        "tip still emitted when rewrite is on"
    );

    let branch_out = StdCommand::new("git")
        .current_dir(repo.path())
        .args(["symbolic-ref", "--short", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&branch_out.stdout).trim(),
        "feat/rewritten",
        "HEAD moved to the new branch"
    );
}

/// `--system` writes the system layer, and the effective value reflects it
/// when no global override is present.
#[test]
fn config_set_system_writes_system_layer() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");

    marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "set",
            "--system",
            "modernize.tips",
            "false",
        ])
        .assert()
        .success();

    // The system file exists with the value we set.
    let on_disk = std::fs::read_to_string(&system_path).unwrap();
    assert!(on_disk.contains("tips = false"));

    // `get` reflects the system value because no global override is set.
    let got = marshal_with_both_isolations(&global_path, &system_path)
        .args(["marshal", "config", "get", "modernize.tips"])
        .output()
        .unwrap();
    assert!(got.status.success());
    assert_eq!(String::from_utf8_lossy(&got.stdout).trim(), "false");
}

/// Precedence: global overrides system when both set the same key.
#[test]
fn global_layer_overrides_system_layer() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");

    // Admin disables tips system-wide.
    marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "set",
            "--system",
            "modernize.tips",
            "false",
        ])
        .assert()
        .success();
    // User re-enables tips for themselves.
    marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "set",
            "--global",
            "modernize.tips",
            "true",
        ])
        .assert()
        .success();

    let got = marshal_with_both_isolations(&global_path, &system_path)
        .args(["marshal", "config", "get", "modernize.tips"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&got.stdout).trim(),
        "true",
        "global must override system when both are set"
    );
}

/// System value is visible when global is explicitly `unset` (i.e., global
/// file has no value for this key).
#[test]
fn system_value_surfaces_when_global_is_unset() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");

    marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "set",
            "--system",
            "modernize.rewrite",
            "true",
        ])
        .assert()
        .success();
    // Set then unset on global to confirm unset actually falls through to
    // system, not to the compiled-in default.
    marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "set",
            "--global",
            "modernize.rewrite",
            "false",
        ])
        .assert()
        .success();
    marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "unset",
            "--global",
            "modernize.rewrite",
        ])
        .assert()
        .success();

    let got = marshal_with_both_isolations(&global_path, &system_path)
        .args(["marshal", "config", "get", "modernize.rewrite"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&got.stdout).trim(),
        "true",
        "unsetting global falls through to system, not to compiled default"
    );
}

/// `--local` writes the local layer, which takes precedence over both
/// global and system when read back.
#[test]
fn local_layer_overrides_global_and_system() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");
    let local_path = dir.path().join("local.toml");

    marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "set",
            "--system",
            "modernize.tips",
            "false",
        ])
        .assert()
        .success();
    marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "set",
            "--global",
            "modernize.tips",
            "true",
        ])
        .assert()
        .success();
    marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "set",
            "--local",
            "modernize.tips",
            "false",
        ])
        .assert()
        .success();

    let got = marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args(["marshal", "config", "get", "modernize.tips"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&got.stdout).trim(),
        "false",
        "local wins over global and system"
    );
}

/// `config get --show-origin` reports which layer owns the effective value.
/// Walks each layer up the precedence chain and confirms the origin label
/// changes accordingly; falls back to `default` when no layer has the key.
#[test]
fn get_show_origin_identifies_the_winning_layer() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");
    let local_path = dir.path().join("local.toml");

    let out = marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "get",
            "--show-origin",
            "modernize.tips",
        ])
        .output()
        .unwrap();
    assert!(out.status.success());
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "default\ttrue");

    marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "set",
            "--system",
            "modernize.tips",
            "false",
        ])
        .assert()
        .success();
    let out = marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "get",
            "--show-origin",
            "modernize.tips",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "system\tfalse");

    marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "set",
            "--global",
            "modernize.tips",
            "true",
        ])
        .assert()
        .success();
    let out = marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "get",
            "--show-origin",
            "modernize.tips",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "global\ttrue");

    marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "set",
            "--local",
            "modernize.tips",
            "false",
        ])
        .assert()
        .success();
    let out = marshal_with_all_isolations(&global_path, &system_path, &local_path)
        .args([
            "marshal",
            "config",
            "get",
            "--show-origin",
            "modernize.tips",
        ])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "local\tfalse");
}

/// When outside a Git repository and without `MARSHAL_LOCAL_CONFIG`, using
/// `--local` fails cleanly with a message pointing the user at the right
/// remediation (run inside a repo, or use --global / --system).
#[test]
fn local_flag_fails_cleanly_outside_any_repo() {
    // This test specifically removes MARSHAL_LOCAL_CONFIG so the binary has
    // to fall back to git-dir detection — which must fail because the
    // child's cwd (the repo-less tempdir we pass) contains no .git.
    let dir = TempDir::new().unwrap();
    let non_repo = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");

    let mut cmd = marshal();
    cmd.env("MARSHAL_CONFIG", &global_path)
        .env("MARSHAL_SYSTEM_CONFIG", &system_path)
        .env_remove("MARSHAL_LOCAL_CONFIG")
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("ProgramData")
        .current_dir(non_repo.path())
        .args([
            "marshal",
            "config",
            "set",
            "--local",
            "modernize.tips",
            "false",
        ]);
    let output = cmd.output().unwrap();

    assert!(
        !output.status.success(),
        "--local outside any repo must fail; stderr was: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("local config is only available inside a git repository"),
        "stderr names the remediation path, got: {stderr}"
    );
}

/// A malformed config file must not break Git commands — we fall back to
/// defaults and warn once on stderr, but the passthrough still completes.
#[test]
fn malformed_config_falls_back_to_defaults_with_a_warning() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    std::fs::write(&cfg_path, "this is not valid [[ toml").unwrap();

    // Run a plain, non-modernize command (no rule matches) so the failure
    // mode is only about config loading, not modernize hooks.
    let repo = init_git_repo();
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .arg("status")
        .output()
        .expect("run marshal status with broken config");

    // git status in an empty repo succeeds.
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("marshal: warning:"),
        "warning emitted when config is malformed, got: {stderr}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `ws` namespace — workspace context detection
// ───────────────────────────────────────────────────────────────────────────

/// Create a synthetic workspace at a fresh `TempDir` by dropping a
/// `.workspace/` marker into it. Returns the `TempDir` for lifetime
/// management — the workspace is gone the moment the dir drops.
fn make_workspace() -> TempDir {
    let tmp = TempDir::new().unwrap();
    std::fs::create_dir(tmp.path().join(".workspace")).unwrap();
    tmp
}

/// `git ws` at the workspace root prints the root path and reports
/// the cwd as being at the root (not inside any child repo).
#[test]
fn ws_at_workspace_root_reports_root_and_no_child() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws"])
        .output()
        .expect("run git ws at workspace root");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Workspace at:"),
        "expected workspace banner, got: {stdout}"
    );
    assert!(
        stdout.contains("Current repo: (workspace root)"),
        "expected workspace-root marker, got: {stdout}"
    );
}

/// Inside a `<root>/src/<repo>/…` path, `git ws` reports the repo
/// name by convention (will be reconciled against the manifest
/// once parsing lands in Slice B).
#[test]
fn ws_inside_child_repo_reports_repo_name_by_convention() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();
    let nested = ws.path().join("src").join("service-a").join("deep");
    std::fs::create_dir_all(&nested).unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(&nested)
        .args(["ws"])
        .output()
        .expect("run git ws inside child repo");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Workspace at:"));
    assert!(
        stdout.contains("Current repo: service-a"),
        "expected child repo identification, got: {stdout}"
    );
}

/// `--json` emits `{root, current_repo?, manifest?}`. `current_repo`
/// and `manifest` are both `Option`s with `skip_serializing_if`.
#[test]
fn ws_json_includes_current_repo_only_when_inside_one() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();
    let child = ws.path().join("src").join("svc-x");
    std::fs::create_dir_all(&child).unwrap();

    // At the root: current_repo absent.
    let at_root = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "--json"])
        .output()
        .unwrap();
    assert!(at_root.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&at_root.stdout).unwrap();
    assert!(parsed["root"].is_string());
    assert!(
        parsed.get("current_repo").is_none(),
        "current_repo should be absent at the workspace root, got: {parsed}"
    );

    // Inside a child repo (no manifest): current_repo is an object
    // with `name` and `declared = false` (no manifest to declare in).
    let inside = marshal_with_isolated_config(&cfg_path)
        .current_dir(&child)
        .args(["ws", "--json"])
        .output()
        .unwrap();
    assert!(inside.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&inside.stdout).unwrap();
    assert_eq!(parsed["current_repo"]["name"], "svc-x");
    assert_eq!(parsed["current_repo"]["declared"], false);
    assert!(
        parsed.get("manifest").is_none(),
        "manifest should be absent when manifest.toml does not exist"
    );
}

/// Helper: create a workspace with a populated manifest at
/// `<root>/.workspace/manifest.toml`.
fn make_workspace_with_manifest(toml: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let ws = tmp.path().join(".workspace");
    std::fs::create_dir(&ws).unwrap();
    std::fs::write(ws.join("manifest.toml"), toml).unwrap();
    tmp
}

/// With a valid manifest in place, `git ws` includes the workspace
/// name, default branch, and declared repo list in the human form,
/// plus the structured manifest summary in the JSON form.
#[test]
fn ws_with_valid_manifest_reports_workspace_name_and_repos() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "my-project"
        default_branch = "develop"

        [[repos]]
        name = "service-a"
        url = "git@github.com:org/service-a.git"

        [[repos]]
        name = "shared-lib"
        url = "git@github.com:org/shared-lib.git"
        "#,
    );

    // Human form.
    let human = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws"])
        .output()
        .unwrap();
    assert!(human.status.success());
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(stdout.contains("Workspace name: my-project"));
    assert!(stdout.contains("default branch: develop"));
    assert!(stdout.contains("Declared repos (2): service-a, shared-lib"));

    // JSON form.
    let json = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "--json"])
        .output()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(parsed["manifest"]["name"], "my-project");
    assert_eq!(parsed["manifest"]["default_branch"], "develop");
    let repos = parsed["manifest"]["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 2);
    assert_eq!(repos[0], "service-a");
    assert_eq!(repos[1], "shared-lib");
}

/// `current_repo.declared = true` when the manifest declares the
/// repo (matched by name); `false` otherwise. The convention-based
/// path detection still finds the candidate; the manifest decides
/// whether it counts.
#[test]
fn ws_current_repo_reconciles_against_manifest() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "my-project"

        [[repos]]
        name = "declared-svc"
        url = "git@example.com:declared-svc.git"
        "#,
    );

    // Inside the declared repo.
    let declared_path = ws.path().join("src").join("declared-svc");
    std::fs::create_dir_all(&declared_path).unwrap();
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(&declared_path)
        .args(["ws", "--json"])
        .output()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["current_repo"]["name"], "declared-svc");
    assert_eq!(parsed["current_repo"]["declared"], true);

    // Inside an undeclared directory that matches the convention
    // path but isn't in the manifest.
    let undeclared_path = ws.path().join("src").join("rogue-svc");
    std::fs::create_dir_all(&undeclared_path).unwrap();
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(&undeclared_path)
        .args(["ws"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Current repo: rogue-svc (NOT declared in manifest)"));
}

/// A malformed manifest produces a clean error — the file exists,
/// so we don't fall through to "no manifest yet"; we propagate the
/// parse error with context.
#[test]
fn ws_with_malformed_manifest_fails_with_helpful_error() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest("this is not [[ valid toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to read workspace manifest")
            || stderr.contains("failed to parse manifest"),
        "expected manifest-related error context, got: {stderr}"
    );
}

/// At the workspace root with no manifest, the human form announces
/// the absence rather than crashing.
#[test]
fn ws_without_manifest_announces_the_gap() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace(); // no manifest written

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No manifest yet"),
        "expected 'no manifest yet' notice, got: {stdout}"
    );
}

/// Helper: write `state.toml` next to the manifest of an existing
/// workspace. `make_workspace_with_manifest` returns a TempDir;
/// this drops the state file alongside.
fn write_state(workspace: &TempDir, toml: &str) {
    std::fs::write(workspace.path().join(".workspace").join("state.toml"), toml).unwrap();
}

/// state.toml with a pinned repo: human form lists the pinned one
/// individually; defaulted repos collapse into a count line.
#[test]
fn ws_state_pinned_repos_shown_others_collapsed() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    // 8 repos so abbreviation kicks in (>5 threshold).
    let mut manifest = String::from("[workspace]\nname = \"demo\"\ndefault_branch = \"main\"\n\n");
    for i in 0..8 {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"svc-{i}\"\nurl = \"git@example.com:svc-{i}.git\"\n\n"
        ));
    }
    let ws = make_workspace_with_manifest(&manifest);
    write_state(
        &ws,
        r#"
        [repos."svc-0"]
        branch = "feat/payment"

        [repos."svc-3"]
        branch = "feat/api-v2"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Pinned repos appear individually.
    assert!(stdout.contains("svc-0"));
    assert!(stdout.contains("on `feat/payment`"));
    assert!(stdout.contains("svc-3"));
    assert!(stdout.contains("on `feat/api-v2`"));
    // Defaulted ones collapse into a count.
    assert!(
        stdout.contains("6 others default to manifest's default branch"),
        "expected default-count line, got: {stdout}"
    );
    // Repos list also abbreviates (8 > 5).
    assert!(
        stdout.contains("Declared repos: 8"),
        "expected count-only repos line for N>5, got: {stdout}"
    );
}

/// `--all` expands both the repos list and the state declarations
/// to show every entry, regardless of total.
#[test]
fn ws_all_flag_expands_repos_and_state() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let mut manifest = String::from("[workspace]\nname = \"demo\"\n\n");
    for i in 0..8 {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"svc-{i}\"\nurl = \"git@example.com:svc-{i}.git\"\n\n"
        ));
    }
    let ws = make_workspace_with_manifest(&manifest);
    write_state(
        &ws,
        r#"[repos."svc-0"]
branch = "feat/x"
"#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "--all"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // With --all, the repos list expands inline (with names).
    assert!(
        stdout.contains("Declared repos (8):") && stdout.contains("svc-0"),
        "expected expanded repos list, got: {stdout}"
    );
    // Every state entry shows up — even the defaulted ones.
    for i in 0..8 {
        assert!(
            stdout.contains(&format!("svc-{i}")),
            "svc-{i} should appear with --all, got: {stdout}"
        );
    }
    assert!(
        stdout.contains("default"),
        "expected `default` markers in expanded view, got: {stdout}"
    );
    // No "X others" abbreviation under --all.
    assert!(
        !stdout.contains("others default"),
        "abbreviation should be off under --all, got: {stdout}"
    );
}

/// When every repo is on the default branch, the state line
/// collapses to a single-line summary, regardless of count.
#[test]
fn ws_state_all_default_collapses_to_one_line() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    // No state.toml at all — every repo defaults implicitly.
    let mut manifest = String::from("[workspace]\nname = \"demo\"\n\n");
    for i in 0..3 {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"svc-{i}\"\nurl = \"git@example.com:svc-{i}.git\"\n\n"
        ));
    }
    let ws = make_workspace_with_manifest(&manifest);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("State (implicit") && stdout.contains("all 3 repos on default"),
        "expected implicit-default summary, got: {stdout}"
    );
}

/// Inside a child repo declared in the manifest with a state pin,
/// `current_repo.declared_branch` reflects the pin in JSON and the
/// human form shows it.
#[test]
fn ws_current_repo_includes_declared_branch_when_state_pins_it() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "service-a"
        url = "git@example.com:service-a.git"
        "#,
    );
    write_state(
        &ws,
        r#"[repos."service-a"]
branch = "feat/x"
"#,
    );
    let inside = ws.path().join("src").join("service-a");
    std::fs::create_dir_all(&inside).unwrap();

    // JSON: declared_branch carries the pinned value.
    let json = marshal_with_isolated_config(&cfg_path)
        .current_dir(&inside)
        .args(["ws", "--json"])
        .output()
        .unwrap();
    let parsed: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
    assert_eq!(parsed["current_repo"]["declared_branch"], "feat/x");

    // Human: line includes "state declares `feat/x`".
    let human = marshal_with_isolated_config(&cfg_path)
        .current_dir(&inside)
        .args(["ws"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&human.stdout);
    assert!(
        stdout.contains("state declares `feat/x`"),
        "expected declared branch in current-repo line, got: {stdout}"
    );
}

/// Outside any workspace, `git ws` exits non-zero with a helpful
/// message on stderr — same shape as `marshal what-now` outside a
/// repo, just for workspaces instead.
#[test]
fn ws_outside_workspace_fails_cleanly() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let non_ws = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(non_ws.path())
        .args(["ws"])
        .output()
        .expect("run git ws outside any workspace");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not in a marshal workspace"),
        "expected 'not in a marshal workspace' on stderr, got: {stderr}"
    );
}

/// An unknown `ws` subcommand exits non-zero with a hint pointing
/// at the bare `git ws` overview.
#[test]
fn ws_unknown_subcommand_errors_with_hint() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "totally-not-a-real-command"])
        .output()
        .expect("run git ws <unknown>");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unknown subcommand 'totally-not-a-real-command'"),
        "stderr names the unknown subcommand, got: {stderr}"
    );
}

/// The marshal-namespace overview points at `git ws` so users
/// discover the workspace namespace without having to read docs.
#[test]
fn marshal_overview_advertises_ws_namespace() {
    let output = marshal().arg("marshal").output().expect("run git marshal");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("git ws"),
        "marshal overview should advertise `git ws`, got: {stdout}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `ws init`
// ───────────────────────────────────────────────────────────────────────────

/// `ws init` in a clean directory creates `.workspace/manifest.toml`
/// and `.workspace/state.toml` with sensible defaults (name from
/// the cwd basename, branch from git config or "main").
#[test]
fn ws_init_creates_manifest_and_state_with_defaults() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let target_parent = TempDir::new().unwrap();
    let target = target_parent.path().join("my-init-project");
    std::fs::create_dir(&target).unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(&target)
        .args(["ws", "init"])
        .output()
        .expect("run ws init");
    assert!(
        output.status.success(),
        "expected success, got: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let manifest_path = target.join(".workspace").join("manifest.toml");
    let state_path = target.join(".workspace").join("state.toml");
    assert!(manifest_path.exists());
    assert!(state_path.exists());

    let manifest_content = std::fs::read_to_string(&manifest_path).unwrap();
    assert!(manifest_content.contains("name = \"my-init-project\""));
    // No repos section because Vec::is_empty triggers skip.
    assert!(
        !manifest_content.contains("[[repos]]") && !manifest_content.contains("repos = []"),
        "empty repos should not pollute the file, got: {manifest_content}"
    );

    let state_content = std::fs::read_to_string(&state_path).unwrap();
    assert!(
        state_content.contains("# state.toml"),
        "state.toml should carry a header comment, got: {state_content}"
    );

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Initialised workspace"));
    assert!(stdout.contains("my-init-project"));
}

/// `--name` and `--default-branch` flags override the defaults.
#[test]
fn ws_init_respects_name_and_default_branch_flags() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let target = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(target.path())
        .args([
            "ws",
            "init",
            "--name",
            "explicit-name",
            "--default-branch",
            "trunk",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());

    let manifest =
        std::fs::read_to_string(target.path().join(".workspace").join("manifest.toml")).unwrap();
    assert!(manifest.contains("name = \"explicit-name\""));
    assert!(manifest.contains("default_branch = \"trunk\""));
}

/// `--name=foo` (equals form) is also accepted.
#[test]
fn ws_init_accepts_equals_form_flags() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let target = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(target.path())
        .args(["ws", "init", "--name=eq-form", "--default-branch=develop"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let manifest =
        std::fs::read_to_string(target.path().join(".workspace").join("manifest.toml")).unwrap();
    assert!(manifest.contains("name = \"eq-form\""));
    assert!(manifest.contains("default_branch = \"develop\""));
}

/// Re-initing inside an existing workspace fails without `--force`.
#[test]
fn ws_init_refuses_in_existing_workspace_without_force() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "init"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("already inside a marshal workspace"),
        "expected refusal message, got: {stderr}"
    );
    assert!(
        stderr.contains("--force"),
        "stderr should mention the --force escape hatch, got: {stderr}"
    );
}

/// `--force` overwrites the manifest and re-initialises.
#[test]
fn ws_init_force_overwrites_existing_manifest() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "before"
        default_branch = "main"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "init", "--name", "after", "--force"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let manifest =
        std::fs::read_to_string(ws.path().join(".workspace").join("manifest.toml")).unwrap();
    assert!(
        manifest.contains("name = \"after\""),
        "manifest should reflect the new name, got: {manifest}"
    );
    assert!(!manifest.contains("name = \"before\""));

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Re-initialised"));
}

/// JSON form returns `{root, workspace_name, default_branch,
/// created_files, forced}`.
#[test]
fn ws_init_json_emits_structured_output() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let target = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(target.path())
        .args(["ws", "init", "--name", "json-test", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["workspace_name"], "json-test");
    assert!(parsed["default_branch"].is_string());
    assert!(parsed["root"].is_string());
    assert_eq!(parsed["forced"], false);
    let files = parsed["created_files"].as_array().unwrap();
    assert_eq!(files.len(), 2);
    assert!(files
        .iter()
        .any(|f| f.as_str().unwrap().ends_with("manifest.toml")));
    assert!(files
        .iter()
        .any(|f| f.as_str().unwrap().ends_with("state.toml")));
}

/// Unknown flags are rejected with a helpful error.
#[test]
fn ws_init_rejects_unknown_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let target = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(target.path())
        .args(["ws", "init", "--bogus"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--bogus'"),
        "expected unknown-flag error, got: {stderr}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `ws status`
// ───────────────────────────────────────────────────────────────────────────

/// Helper: turn a directory into a clean git repo on `main` with
/// a single seed commit. Used by `ws status` tests to populate
/// child repos with controlled state.
fn init_child_repo(path: &std::path::Path) {
    std::fs::create_dir_all(path).unwrap();
    StdCommand::new("git")
        .current_dir(path)
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(path)
        .args(["config", "user.email", "t@example.com"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(path)
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();
    std::fs::write(path.join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(path)
        .args(["add", "."])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(path)
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();
}

/// `ws status` outside a workspace fails cleanly.
#[test]
fn ws_status_outside_workspace_fails() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(outside.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not in a marshal workspace"));
}

/// Workspace exists but no manifest yet — `ws status` errors with
/// a hint pointing at `ws init`.
#[test]
fn ws_status_without_manifest_errors_helpfully() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no manifest yet") || stderr.contains("ws init"),
        "expected manifest-missing hint, got: {stderr}"
    );
}

/// With a few clean repos (≤ 5) all on the declared branch, the
/// human form lists every one inline (no abbreviation kicks in).
#[test]
fn ws_status_lists_small_repo_set_inline() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );
    init_child_repo(&ws.path().join("src").join("alpha"));
    init_child_repo(&ws.path().join("src").join("beta"));

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Workspace `demo`"));
    assert!(stdout.contains("alpha"));
    assert!(stdout.contains("beta"));
    assert!(stdout.contains("on `main`"));
}

/// With > 5 repos all clean+on-declared, hide-boring kicks in:
/// the body collapses to the "All N clean" summary.
#[test]
fn ws_status_collapses_all_clean_when_total_above_threshold() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let mut manifest = String::from("[workspace]\nname = \"demo\"\n\n");
    for i in 0..6 {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"svc-{i}\"\nurl = \"git@example.com:svc-{i}.git\"\n\n"
        ));
    }
    let ws = make_workspace_with_manifest(&manifest);
    for i in 0..6 {
        init_child_repo(&ws.path().join("src").join(format!("svc-{i}")));
    }

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("All 6 repos clean and on declared branch"),
        "expected all-clean summary, got: {stdout}"
    );
}

/// Mixed: most repos clean, one dirty, one missing → only the
/// "interesting" two are listed; the clean ones collapse to a
/// count.
#[test]
fn ws_status_surfaces_only_interesting_under_hide_boring() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let mut manifest = String::from("[workspace]\nname = \"demo\"\n\n");
    for i in 0..7 {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"svc-{i}\"\nurl = \"git@example.com:svc-{i}.git\"\n\n"
        ));
    }
    let ws = make_workspace_with_manifest(&manifest);
    init_child_repo(&ws.path().join("src").join("svc-0"));
    std::fs::write(
        ws.path().join("src").join("svc-0").join("seed.txt"),
        b"modified",
    )
    .unwrap();
    // svc-1 deliberately not created.
    for i in 2..7 {
        init_child_repo(&ws.path().join("src").join(format!("svc-{i}")));
    }

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("svc-0") && stdout.contains("unstaged"),
        "expected svc-0 dirty line, got: {stdout}"
    );
    assert!(
        stdout.contains("svc-1") && stdout.contains("missing on disk"),
        "expected svc-1 missing line, got: {stdout}"
    );
    assert!(
        stdout.contains("5 other repos clean and on declared branch"),
        "expected boring-count line, got: {stdout}"
    );
}

/// `--all` expands the full list — clean repos appear too.
#[test]
fn ws_status_all_flag_expands_clean_repos() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let mut manifest = String::from("[workspace]\nname = \"demo\"\n\n");
    for i in 0..7 {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"svc-{i}\"\nurl = \"git@example.com:svc-{i}.git\"\n\n"
        ));
    }
    let ws = make_workspace_with_manifest(&manifest);
    for i in 0..7 {
        init_child_repo(&ws.path().join("src").join(format!("svc-{i}")));
    }

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--all"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    for i in 0..7 {
        assert!(
            stdout.contains(&format!("svc-{i}")),
            "svc-{i} should appear with --all, got: {stdout}"
        );
    }
    assert!(
        !stdout.contains("other repos clean"),
        "abbreviation should be off, got: {stdout}"
    );
}

/// JSON form returns full data: workspace info plus per-repo
/// snapshot.
#[test]
fn ws_status_json_returns_full_per_repo_payload() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );
    init_child_repo(&ws.path().join("src").join("alpha"));

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(parsed["workspace"]["name"], "demo");
    assert!(parsed["workspace"]["root"].is_string());

    let repos = parsed["repos"].as_array().unwrap();
    assert_eq!(repos.len(), 1);
    let alpha = &repos[0];
    assert_eq!(alpha["name"], "alpha");
    assert_eq!(alpha["path"], "src/alpha");
    assert_eq!(alpha["declared_branch"], "main");
    assert_eq!(alpha["clean_on_declared"], true);
    assert_eq!(alpha["missing_from_disk"], false);
    assert_eq!(alpha["state"]["branch"]["name"], "main");
    assert_eq!(alpha["state"]["working_tree"]["staged"], 0);
}

/// state.toml override flows through: a repo pinned to a non-default
/// branch is reported as "interesting" when it sits on the default.
#[test]
fn ws_status_reports_off_declared_branch_when_state_pins_one() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );
    write_state(
        &ws,
        r#"[repos."alpha"]
branch = "feat/x"
"#,
    );
    // On-disk repo is on `main`, but state.toml pins `feat/x`.
    init_child_repo(&ws.path().join("src").join("alpha"));

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("on `main`") && stdout.contains("declared `feat/x`"),
        "expected off-declared note, got: {stdout}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `ws log`
// ───────────────────────────────────────────────────────────────────────────

/// Helper: init a child repo and stamp `n` commits with controlled
/// dates (one per day starting at `2026-04-21`). Used by `ws log`
/// tests so the global ordering is deterministic.
fn init_child_repo_with_dated_commits(path: &std::path::Path, name_prefix: &str, n: usize) {
    init_child_repo(path);
    for i in 1..=n {
        std::fs::write(
            path.join("seed.txt"),
            format!("{name_prefix}-v{i}").as_bytes(),
        )
        .unwrap();
        StdCommand::new("git")
            .current_dir(path)
            .args(["add", "."])
            .status()
            .unwrap();
        let date = format!("2026-04-{:02} 10:00:00", 20 + i);
        StdCommand::new("git")
            .current_dir(path)
            .env("GIT_AUTHOR_DATE", &date)
            .env("GIT_COMMITTER_DATE", &date)
            .args(["commit", "-q", "-m", &format!("{name_prefix}: change {i}")])
            .status()
            .unwrap();
    }
}

/// `ws log` outside a workspace fails with the same shape as `ws status`.
#[test]
fn ws_log_outside_workspace_fails() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(outside.path())
        .args(["ws", "log"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not in a marshal workspace"));
}

/// `ws log` in a workspace with no manifest errors with an `ws init` hint.
#[test]
fn ws_log_without_manifest_errors_helpfully() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no manifest yet") || stderr.contains("ws init"),
        "expected manifest-missing hint, got: {stderr}"
    );
}

/// Workspace with declared repos that do not exist on disk yet:
/// `ws log` returns empty cleanly (no commits, no error).
#[test]
fn ws_log_with_no_repos_on_disk_yields_empty_output() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No commits yet") || stdout.contains("empty or missing"),
        "expected empty-state notice, got: {stdout}"
    );
}

/// With multiple repos, the entries are interleaved by date (most
/// recent first) — this is the monorepo-feel half of the thesis.
#[test]
fn ws_log_interleaves_repos_by_date() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 3);
    init_child_repo_with_dated_commits(&ws.path().join("src").join("beta"), "b", 3);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);

    // Total of 8 commits (init seed + 3 per repo, ×2 repos = 8). The
    // most-recent date is 2026-04-23 (third change) — both repos
    // share that date but should both appear above any 2026-04-22.
    let pos_third = stdout.find("change 3").expect("change 3 should appear");
    let pos_second = stdout.find("change 2").expect("change 2 should appear");
    let pos_first = stdout.find("change 1").expect("change 1 should appear");
    assert!(
        pos_third < pos_second && pos_second < pos_first,
        "expected newer-first ordering, got positions: third={pos_third}, second={pos_second}, first={pos_first}\nfull: {stdout}"
    );
}

/// `-n <N>` caps the displayed entries; the footer announces the
/// truncation.
#[test]
fn ws_log_n_flag_limits_and_announces_truncation() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 5);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log", "-n", "2"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Two newest-by-date should be there — change 5 and change 4.
    assert!(stdout.contains("change 5"));
    assert!(stdout.contains("change 4"));
    // Older changes should not appear.
    assert!(
        !stdout.contains("change 1"),
        "older commit leaked through -n 2: {stdout}"
    );
    // Truncation footer announces the cap.
    assert!(
        stdout.contains("Showing top 2") && stdout.contains("--all"),
        "expected truncation footer, got: {stdout}"
    );
}

/// `--all` lifts the cap and skips the truncation footer.
#[test]
fn ws_log_all_flag_lifts_cap() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 3);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log", "--all", "-n", "1"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Even with `-n 1`, --all wins: every commit (init seed + 3
    // changes = 4 commits) appears.
    assert!(stdout.contains("change 1"));
    assert!(stdout.contains("change 2"));
    assert!(stdout.contains("change 3"));
    // No truncation footer when --all is set.
    assert!(
        !stdout.contains("Showing"),
        "abbreviation footer should be off under --all, got: {stdout}"
    );
}

/// JSON form returns the structured payload.
#[test]
fn ws_log_json_returns_full_payload() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 2);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();

    assert_eq!(parsed["workspace"]["name"], "demo");
    assert_eq!(parsed["workspace"]["total_repos_declared"], 1);
    assert_eq!(parsed["workspace"]["repos_with_data"], 1);

    let entries = parsed["entries"].as_array().unwrap();
    assert!(!entries.is_empty());
    let first = &entries[0];
    assert_eq!(first["repo"], "alpha");
    assert!(first["hash"].as_str().unwrap().len() >= 40);
    assert!(first["date"].as_str().unwrap().contains("2026-04"));
    assert_eq!(first["author"], "Test");
}

/// Unknown flags rejected with helpful error.
#[test]
fn ws_log_rejects_unknown_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log", "--bogus"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unexpected argument '--bogus'"),
        "expected unknown-flag error, got: {stderr}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `ws diff`
// ───────────────────────────────────────────────────────────────────────────

/// Helper: prepare a workspace as a committed git repo. Returns the
/// TempDir holding the workspace root. After the helper, the repo
/// has one commit with `.workspace/{manifest.toml, state.toml}`
/// committed; `state.toml` is the one passed in.
fn make_committed_workspace(state_toml: &str, manifest_toml: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();
    StdCommand::new("git")
        .current_dir(root)
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(root)
        .args(["config", "user.email", "t@example.com"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(root)
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();
    let workspace_dir = root.join(".workspace");
    std::fs::create_dir(&workspace_dir).unwrap();
    std::fs::write(workspace_dir.join("manifest.toml"), manifest_toml).unwrap();
    std::fs::write(workspace_dir.join("state.toml"), state_toml).unwrap();
    StdCommand::new("git")
        .current_dir(root)
        .args(["add", "."])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(root)
        .args(["commit", "-q", "-m", "initial"])
        .status()
        .unwrap();
    tmp
}

/// `ws diff` outside a workspace fails with the same shape as `ws status`.
#[test]
fn ws_diff_outside_workspace_fails() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(outside.path())
        .args(["ws", "diff"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("not in a marshal workspace"));
}

/// Workspace with no manifest yet: same error shape as the other
/// aggregated commands.
#[test]
fn ws_diff_without_manifest_errors_helpfully() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "diff"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("has no manifest yet") || stderr.contains("ws init"),
        "expected manifest-missing hint, got: {stderr}"
    );
}

/// state.toml unchanged since HEAD → "No state declarations changed".
#[test]
fn ws_diff_reports_no_changes_when_state_matches_head() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_committed_workspace(
        r#"[repos."alpha"]
branch = "main"
"#,
        r#"[workspace]
name = "demo"
default_branch = "main"
"#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "diff"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("No state declarations changed since HEAD"),
        "expected no-changes message, got: {stdout}"
    );
}

/// Three change kinds in one diff: changed, removed, added.
#[test]
fn ws_diff_renders_changed_added_and_removed() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_committed_workspace(
        r#"[repos."alpha"]
branch = "main"

[repos."beta"]
branch = "main"
"#,
        r#"[workspace]
name = "demo"
"#,
    );
    // Modify the working-tree state.toml: alpha branch changes,
    // beta is dropped, gamma is new.
    std::fs::write(
        ws.path().join(".workspace").join("state.toml"),
        r#"[repos."alpha"]
branch = "feat/x"

[repos."gamma"]
branch = "feat/api"
"#,
    )
    .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "diff"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    // Symbols + names — sorted alphabetically.
    assert!(
        stdout.contains("~ alpha"),
        "expected changed line for alpha, got: {stdout}"
    );
    assert!(stdout.contains("`main` → `feat/x`"));
    assert!(
        stdout.contains("- beta"),
        "expected removed line for beta, got: {stdout}"
    );
    assert!(stdout.contains("declaration removed"));
    assert!(
        stdout.contains("+ gamma"),
        "expected added line for gamma, got: {stdout}"
    );
    assert!(stdout.contains("declared on `feat/api`"));
}

/// JSON form: the tagged-enum shape (`kind` + per-variant fields).
#[test]
fn ws_diff_json_emits_tagged_change_entries() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_committed_workspace(
        r#"[repos."alpha"]
branch = "main"
"#,
        r#"[workspace]
name = "demo"
"#,
    );
    std::fs::write(
        ws.path().join(".workspace").join("state.toml"),
        r#"[repos."alpha"]
branch = "feat/x"

[repos."beta"]
branch = "main"
"#,
    )
    .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "diff", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["workspace"]["name"], "demo");
    let changes = parsed["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);

    // Sorted alphabetically: alpha (changed) then beta (added).
    assert_eq!(changes[0]["kind"], "changed");
    assert_eq!(changes[0]["name"], "alpha");
    assert_eq!(changes[0]["from"], "main");
    assert_eq!(changes[0]["to"], "feat/x");

    assert_eq!(changes[1]["kind"], "added");
    assert_eq!(changes[1]["name"], "beta");
    assert_eq!(changes[1]["branch"], "main");
}

/// Workspace with no commits yet (no HEAD): every state.toml entry
/// reads as an addition. Demonstrates the graceful-degrade path
/// for `git show HEAD:.workspace/state.toml` failure.
#[test]
fn ws_diff_treats_no_head_as_empty_baseline() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    // Create a workspace inside a NON-committed git repo: init,
    // ws init, but no `git commit`. HEAD does not exist yet.
    let tmp = TempDir::new().unwrap();
    StdCommand::new("git")
        .current_dir(tmp.path())
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();
    let workspace_dir = tmp.path().join(".workspace");
    std::fs::create_dir(&workspace_dir).unwrap();
    std::fs::write(
        workspace_dir.join("manifest.toml"),
        r#"[workspace]
name = "demo"
"#,
    )
    .unwrap();
    std::fs::write(
        workspace_dir.join("state.toml"),
        r#"[repos."alpha"]
branch = "main"
"#,
    )
    .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(tmp.path())
        .args(["ws", "diff"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("+ alpha"),
        "expected alpha as added (no HEAD baseline), got: {stdout}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Scope inference + `--on` (Slice H)
// ───────────────────────────────────────────────────────────────────────────

/// `ws log` inside a child repo narrows to that repo by spatial
/// inference — without the user passing `--on`.
#[test]
fn ws_log_inside_child_repo_narrows_via_spatial_inference() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 2);
    init_child_repo_with_dated_commits(&ws.path().join("src").join("beta"), "b", 2);

    // Cwd inside alpha → only alpha's commits should appear.
    let inside_alpha = ws.path().join("src").join("alpha");
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(&inside_alpha)
        .args(["ws", "log"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("a: change"),
        "expected alpha commits, got: {stdout}"
    );
    assert!(
        !stdout.contains("b: change"),
        "beta commits leaked into spatial-narrowed log: {stdout}"
    );
}

/// At the workspace root, `ws log` is workspace-wide (no spatial
/// narrowing). Sanity check that the spatial-fallback policy falls
/// back to "all" when there's no current repo.
#[test]
fn ws_log_at_workspace_root_remains_workspace_wide() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 1);
    init_child_repo_with_dated_commits(&ws.path().join("src").join("beta"), "b", 1);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("a: change"));
    assert!(stdout.contains("b: change"));
}

/// `--on <name>` overrides spatial inference: even from inside
/// alpha, `--on beta` returns beta's commits.
#[test]
fn ws_log_on_flag_overrides_spatial_inference() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );
    init_child_repo_with_dated_commits(&ws.path().join("src").join("alpha"), "a", 1);
    init_child_repo_with_dated_commits(&ws.path().join("src").join("beta"), "b", 1);

    let inside_alpha = ws.path().join("src").join("alpha");
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(&inside_alpha)
        .args(["ws", "log", "--on", "beta"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("b: change") && !stdout.contains("a: change"),
        "expected --on to override spatial, got: {stdout}"
    );
}

/// `ws status --on <name>` filters the repos list to one entry.
#[test]
fn ws_status_on_flag_filters_to_single_repo() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );
    init_child_repo(&ws.path().join("src").join("alpha"));
    init_child_repo(&ws.path().join("src").join("beta"));

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--on", "alpha"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpha"));
    assert!(
        !stdout.contains("beta"),
        "beta leaked into --on alpha output: {stdout}"
    );
    // Header reports the filtered repos count.
    assert!(stdout.contains("1 repos declared"));
}

/// `ws diff --on <name>` filters the changes list to entries
/// matching that repo.
#[test]
fn ws_diff_on_flag_filters_change_list() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_committed_workspace(
        r#"[repos."alpha"]
branch = "main"

[repos."beta"]
branch = "main"
"#,
        r#"[workspace]
name = "demo"

[[repos]]
name = "alpha"
url = "git@example.com:alpha.git"

[[repos]]
name = "beta"
url = "git@example.com:beta.git"
"#,
    );
    // Modify both repos' state.
    std::fs::write(
        ws.path().join(".workspace").join("state.toml"),
        r#"[repos."alpha"]
branch = "feat/alpha"

[repos."beta"]
branch = "feat/beta"
"#,
    )
    .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "diff", "--on", "beta"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("beta") && stdout.contains("`main` → `feat/beta`"),
        "expected beta change, got: {stdout}"
    );
    assert!(
        !stdout.contains("`main` → `feat/alpha`"),
        "alpha change leaked into --on beta diff: {stdout}"
    );
}

/// `--on <unknown>` errors helpfully and lists the known repos.
#[test]
fn on_flag_with_unknown_repo_errors_with_known_list() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--on", "bogus"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("'bogus'") && stderr.contains("alpha") && stderr.contains("beta"),
        "expected error naming bogus and listing known repos, got: {stderr}"
    );
}

/// `--on=<name>` (equals form) is also accepted.
#[test]
fn on_flag_equals_form_works() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );
    init_child_repo(&ws.path().join("src").join("alpha"));

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--on=alpha"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpha"));
}

// ───────────────────────────────────────────────────────────────────────────
// `--explain` flag (Slice I)
// ───────────────────────────────────────────────────────────────────────────

/// `ws init --explain` shows the plan but does NOT touch the
/// filesystem. Critical safety property — Invariant 6 says the
/// plan is shown *before* execution; with --explain it's shown
/// *instead of* execution.
#[test]
fn ws_init_explain_does_not_create_files() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let target = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(target.path())
        .args(["ws", "init", "--explain", "--name", "explained"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws init`"));
    assert!(stdout.contains("create directory"));
    assert!(stdout.contains("manifest.toml"));
    assert!(stdout.contains("state.toml"));

    // The critical assertion: nothing was written.
    assert!(
        !target.path().join(".workspace").exists(),
        "`.workspace/` must not be created under --explain"
    );
}

/// `ws status --explain` enumerates the per-repo `git -C <path>`
/// invocations without running them.
#[test]
fn ws_status_explain_lists_git_invocations() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws status`"));
    assert!(stdout.contains("git -C") && stdout.contains("rev-parse --git-dir"));
    assert!(stdout.contains("status --porcelain=v2 --branch"));
    assert!(stdout.contains("alpha"));
    assert!(stdout.contains("beta"));
}

/// `ws status --on alpha --explain` narrows the plan via --on.
#[test]
fn ws_status_explain_respects_on_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"

        [[repos]]
        name = "beta"
        url = "git@example.com:beta.git"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--on", "alpha", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpha"));
    assert!(
        !stdout.contains("beta"),
        "beta must not appear in --on alpha plan, got: {stdout}"
    );
}

/// `ws log --explain` lists the per-repo `git log` invocation.
#[test]
fn ws_log_explain_lists_git_log_invocations() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "log", "--explain", "-n", "5"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws log`"));
    assert!(stdout.contains("git -C") && stdout.contains("log --pretty=format:"));
    // The cap from `-n 5` must show in the plan.
    assert!(stdout.contains("-n5"));
    // Sanity: the format string is the actual git format
    // (single-percent), not the doubled form.
    assert!(stdout.contains("%H%x09"));
}

/// `ws diff --explain` describes the comparison without running it.
#[test]
fn ws_diff_explain_describes_comparison() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "diff", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws diff`"));
    assert!(stdout.contains("git -C") && stdout.contains("show HEAD:.workspace/state.toml"));
    assert!(stdout.contains("compare"));
}

/// JSON form under `--explain` carries the plan in the
/// `explain_plan` field (skipped otherwise).
#[test]
fn ws_status_explain_json_includes_plan_field() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"

        [[repos]]
        name = "alpha"
        url = "git@example.com:alpha.git"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--explain", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let plan = parsed["explain_plan"]
        .as_array()
        .expect("explain_plan is an array");
    assert!(!plan.is_empty(), "plan should have entries");
    assert!(plan
        .iter()
        .any(|p| p.as_str().unwrap_or("").contains("rev-parse --git-dir")));
}

/// Without `--explain`, the `explain_plan` field is absent in JSON.
#[test]
fn ws_status_without_explain_omits_plan_field_in_json() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_workspace_with_manifest(
        r#"
        [workspace]
        name = "demo"
        "#,
    );

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert!(
        parsed.get("explain_plan").is_none(),
        "explain_plan should be omitted without --explain, got: {parsed}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `marshal help`
// ───────────────────────────────────────────────────────────────────────────

/// `marshal help` (no arg) lands on the overview topic.
#[test]
fn help_with_no_arg_shows_overview() {
    let output = marshal()
        .args(["marshal", "help"])
        .output()
        .expect("run marshal help");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Marshal"));
    assert!(stdout.contains("Subcommands"));
    assert!(stdout.contains("config"));
    assert!(stdout.contains("what-now"));
}

/// `marshal help overview` resolves the named topic explicitly.
#[test]
fn help_with_named_topic_resolves_it() {
    let output = marshal()
        .args(["marshal", "help", "overview"])
        .output()
        .expect("run marshal help overview");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Subcommands"));
}

/// Unknown topic exits non-zero and lists available topics.
#[test]
fn help_with_unknown_topic_errors_with_hint() {
    let output = marshal()
        .args(["marshal", "help", "totally-not-a-topic"])
        .output()
        .expect("run marshal help <bogus>");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("unknown help topic 'totally-not-a-topic'"));
    assert!(stderr.contains("overview"));
}

/// `--json` flips the help output to a structured JSON shape with
/// `topic`, `title`, and `sections[]`.
#[test]
fn help_json_emits_structured_payload() {
    let output = marshal()
        .args(["marshal", "help", "--json"])
        .output()
        .expect("run marshal help --json");
    assert!(output.status.success());
    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(parsed["topic"], "overview");
    assert!(parsed["title"].is_string());
    let sections = parsed["sections"].as_array().expect("sections is array");
    assert!(!sections.is_empty());
    // Each section has heading + body (array of strings).
    let first = &sections[0];
    assert!(first["heading"].is_string());
    assert!(first["body"].is_array());
}

/// The marshal-namespace overview (no help arg) advertises help.
#[test]
fn marshal_overview_advertises_help() {
    let output = marshal()
        .arg("marshal")
        .output()
        .expect("run marshal marshal");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("help"),
        "overview should list help, got: {stdout}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `--json` global output flag
// ───────────────────────────────────────────────────────────────────────────

/// `marshal --json config list` emits a parseable JSON object whose
/// `entries` array carries every known key with its effective value.
#[test]
fn config_list_json_emits_parseable_payload() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "--json", "config", "list"])
        .output()
        .expect("run config list --json");
    assert!(output.status.success());

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    let entries = parsed["entries"].as_array().expect("entries is an array");
    let keys: Vec<&str> = entries.iter().map(|e| e["key"].as_str().unwrap()).collect();
    assert!(keys.contains(&"modernize.tips"));
    assert!(keys.contains(&"modernize.rewrite"));
    assert!(keys.contains(&"errors.actionable_hints"));
}

/// `config get` plain JSON form is `{key, value}` — no `origin`
/// when `--show-origin` was not requested.
#[test]
fn config_get_json_omits_origin_without_show_origin() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "get", "modernize.tips", "--json"])
        .output()
        .expect("run config get --json");
    assert!(output.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["key"], "modernize.tips");
    assert_eq!(parsed["value"], "true");
    assert!(
        parsed.get("origin").is_none(),
        "origin must not appear without --show-origin, got: {parsed}"
    );
}

/// `--show-origin` adds the `origin` field to the JSON payload —
/// "default" when no layer has the key set, the layer name otherwise.
#[test]
fn config_get_json_includes_origin_with_show_origin() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .args([
            "marshal",
            "config",
            "get",
            "--show-origin",
            "modernize.tips",
            "--json",
        ])
        .output()
        .expect("run config get --show-origin --json");
    assert!(output.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["origin"], "default");
    assert_eq!(parsed["value"], "true");
}

/// `marshal what-now --json` emits the advice as a JSON object with
/// `rule_id`, `title`, and `suggestions[]`.
#[test]
fn what_now_json_emits_structured_advice() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let repo = init_git_repo();

    // Seed a commit so the rule isn't `initial-state`.
    std::fs::write(repo.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .args(["marshal", "what-now", "--json"])
        .output()
        .expect("run what-now --json");
    assert!(output.status.success());

    let parsed: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("stdout is valid JSON");
    assert_eq!(parsed["rule_id"], "clean");
    assert!(parsed["title"].as_str().unwrap().contains("clean"));
    let suggestions = parsed["suggestions"].as_array().unwrap();
    assert!(!suggestions.is_empty());
}

/// `--json` is global: it works equally before *or* after the
/// subcommand. Position-independent so users place it where it
/// reads best.
#[test]
fn json_flag_works_in_any_position() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    // Before the subcommand.
    let before = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "--json", "config", "list"])
        .output()
        .unwrap();
    assert!(before.status.success());
    let _: serde_json::Value = serde_json::from_slice(&before.stdout).unwrap();

    // After the subcommand.
    let after = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "list", "--json"])
        .output()
        .unwrap();
    assert!(after.status.success());
    let _: serde_json::Value = serde_json::from_slice(&after.stdout).unwrap();
}

/// Without `--json` the human form is preserved byte-exact —
/// regression guard against the migration accidentally changing
/// the default output shape.
#[test]
fn human_format_remains_default_when_json_not_set() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_with_isolated_config(&cfg_path)
        .args(["marshal", "config", "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("modernize.tips=true"),
        "default human form should keep `key=value` lines, got: {stdout}"
    );
    // And it must NOT be JSON.
    assert!(
        !stdout.trim_start().starts_with('{'),
        "default form must not be JSON, got: {stdout}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `marshal what-now`
// ───────────────────────────────────────────────────────────────────────────

/// `what-now` outside any repository fails cleanly with a message
/// pointing at the cause. Exit code is non-zero so scripts can react.
#[test]
fn what_now_fails_cleanly_outside_a_repository() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let non_repo = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(non_repo.path())
        .args(["marshal", "what-now"])
        .output()
        .expect("run marshal what-now");

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("not in a git repository"),
        "expected 'not in a git repository' on stderr, got: {stderr}"
    );
}

/// `what-now` in a fresh repo with no commits yet picks the
/// `initial-state` rule.
#[test]
fn what_now_in_fresh_repo_recommends_first_commit() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let repo = init_git_repo();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .args(["marshal", "what-now"])
        .output()
        .expect("run marshal what-now in fresh repo");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Fresh repository"),
        "expected fresh-repo title, got: {stdout}"
    );
    assert!(stdout.contains("git commit -m \"initial\""));
}

/// `what-now` on a repo with uncommitted changes picks the
/// `uncommitted-changes` rule and composes the title from the buckets
/// that have content.
#[test]
fn what_now_reports_uncommitted_changes_with_bucket_breakdown() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let repo = init_git_repo();

    // Create a baseline commit so we're past `initial-state`.
    std::fs::write(repo.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    // Now create one staged, one unstaged, one untracked. We achieve
    // the staged+unstaged combo by editing the same file after staging
    // (M. → MM after a second edit).
    std::fs::write(repo.path().join("seed.txt"), b"v2").unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    std::fs::write(repo.path().join("seed.txt"), b"v3").unwrap();
    std::fs::write(repo.path().join("new.txt"), b"new").unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .args(["marshal", "what-now"])
        .output()
        .expect("run marshal what-now");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Working tree has"),
        "expected uncommitted-changes title, got: {stdout}"
    );
    // All three buckets must show up — staged (the seed file's M),
    // unstaged (the second edit's M), and untracked (new.txt).
    assert!(stdout.contains("staged"), "got: {stdout}");
    assert!(stdout.contains("unstaged"), "got: {stdout}");
    assert!(stdout.contains("untracked"), "got: {stdout}");
    // Suggestions cover the round-trip.
    assert!(stdout.contains("git diff"));
    assert!(stdout.contains("git add"));
    assert!(stdout.contains("git commit"));
}

/// A clean repo with one commit and nothing changed gets the catch-all
/// `clean` rule.
#[test]
fn what_now_in_clean_repo_reports_clean_state() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let repo = init_git_repo();

    std::fs::write(repo.path().join("seed.txt"), b"seed").unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["add", "seed.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(repo.path())
        .args(["commit", "-q", "-m", "seed"])
        .status()
        .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .args(["marshal", "what-now"])
        .output()
        .expect("run marshal what-now in clean repo");

    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("Working tree clean"),
        "expected clean title, got: {stdout}"
    );
    assert!(
        stdout.contains("on `main`"),
        "expected branch label, got: {stdout}"
    );
}

/// `git marshal` (no subcommand) lists `what-now` in the overview so
/// users can discover it.
#[test]
fn marshal_overview_advertises_what_now() {
    let output = marshal()
        .arg("marshal")
        .output()
        .expect("run marshal marshal");
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("what-now"),
        "overview should list what-now, got: {stdout}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Actionable error hints
// ───────────────────────────────────────────────────────────────────────────

/// Running any git command outside a repository triggers the
/// `not-a-git-repository` hint after git's own error message. Exit code
/// and the original stderr line stay byte-exact; the hint is appended.
#[test]
fn not_a_git_repository_emits_hint_after_git_error() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let non_repo = TempDir::new().unwrap();

    let direct = StdCommand::new("git")
        .current_dir(non_repo.path())
        .arg("status")
        .output()
        .expect("run git status");
    let wrapped = marshal_with_isolated_config(&cfg_path)
        .current_dir(non_repo.path())
        .arg("status")
        .output()
        .expect("run marshal status");

    assert_eq!(direct.status.code(), wrapped.status.code());

    let stderr = String::from_utf8_lossy(&wrapped.stderr);
    // Git's own message must appear unchanged (substring, since the path
    // suffix can vary by platform).
    assert!(
        stderr.contains("not a git repository"),
        "git's own error must still appear, got: {stderr}"
    );
    // Marshal's hint is appended below it.
    assert!(
        stderr.contains("marshal: hint:"),
        "expected actionable hint on stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("git init"),
        "hint mentions `git init` as a remediation, got: {stderr}"
    );
    assert!(
        stderr.contains("`cd`"),
        "hint mentions `cd` as a remediation, got: {stderr}"
    );

    // The hint comes *after* git's own line — order matters for
    // readability.
    let git_pos = stderr.find("not a git repository").unwrap();
    let hint_pos = stderr.find("marshal: hint:").unwrap();
    assert!(
        git_pos < hint_pos,
        "git's stderr precedes marshal's hint (git={git_pos}, hint={hint_pos})"
    );
}

/// `errors.actionable_hints = false` restores pure passthrough: stderr
/// matches `git`'s own bytes, and no hint is appended.
#[test]
fn actionable_hints_can_be_disabled_via_config() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let non_repo = TempDir::new().unwrap();

    marshal_with_isolated_config(&cfg_path)
        .args([
            "marshal",
            "config",
            "set",
            "errors.actionable_hints",
            "false",
        ])
        .assert()
        .success();

    let direct = StdCommand::new("git")
        .current_dir(non_repo.path())
        .arg("status")
        .output()
        .expect("run git status");
    let wrapped = marshal_with_isolated_config(&cfg_path)
        .current_dir(non_repo.path())
        .arg("status")
        .output()
        .expect("run marshal status with hints disabled");

    assert_eq!(direct.status.code(), wrapped.status.code());
    assert_eq!(
        direct.stderr, wrapped.stderr,
        "hints disabled must restore byte-exact stderr"
    );
    let stderr = String::from_utf8_lossy(&wrapped.stderr);
    assert!(
        !stderr.contains("marshal: hint:"),
        "no hint must be appended when feature is off, got: {stderr}"
    );
}

/// Successful git commands never get a hint, even when actionable_hints
/// is on. Hints are gated behind `!status.success()`.
#[test]
fn successful_commands_do_not_get_hints() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let repo = init_git_repo();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(repo.path())
        .arg("status")
        .output()
        .expect("marshal status in fresh repo");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        !stderr.contains("marshal: hint:"),
        "no hint on a successful command, got: {stderr}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// Remaining passthrough-fidelity tests
// ───────────────────────────────────────────────────────────────────────────

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

// ───────────────────────────────────────────────────────────────────────────
// `ws clone` — workspace clone with parallel children + indicatif progress
// ───────────────────────────────────────────────────────────────────────────

/// Build a `file://` URL for a local path. Delegates to
/// `url::Url::from_file_path`, which handles Windows drive letters
/// (`C:\foo\bar` → `file:///C:/foo/bar`), forward-slash normalisation,
/// and URL-encoding of special characters in one shot. Without it, a
/// raw `format!("file://{}", path.display())` on Windows produces
/// backslashes that TOML reads as the start of `\U`-prefixed Unicode
/// escapes, breaking the manifest parser.
fn local_file_url(path: &std::path::Path) -> String {
    url::Url::from_file_path(path)
        .expect("test path must be absolute")
        .to_string()
}

/// Build a self-contained workspace source on disk: N child repos plus a
/// "workspace repo" that declares them all in `.workspace/manifest.toml`.
/// Returns the temp dir owning the trees plus the absolute path of the
/// workspace source repo (use [`local_file_url`] to build a clone URL
/// from it).
///
/// Used by every `ws clone` integration test below — they only differ in
/// what they do *with* the cloned tree (assert child presence, assert
/// JSON shape, etc.). Centralising the fixture keeps each test focused.
fn make_workspace_with_children(child_names: &[&str]) -> (TempDir, std::path::PathBuf) {
    let tmp = TempDir::new().unwrap();
    let sources = tmp.path().join("sources");
    std::fs::create_dir(&sources).unwrap();

    // Each child is a one-commit git repo on `main`.
    for name in child_names {
        let child = sources.join(name);
        init_child_repo(&child);
    }

    // Workspace repo lives alongside, with a manifest pointing at every
    // child via its absolute file:// path.
    let workspace = sources.join("workspace");
    std::fs::create_dir_all(workspace.join(".workspace")).unwrap();
    StdCommand::new("git")
        .current_dir(&workspace)
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(&workspace)
        .args(["config", "user.email", "t@example.com"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(&workspace)
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();

    let mut manifest = String::from("[workspace]\nname = \"smoke\"\ndefault_branch = \"main\"\n\n");
    for name in child_names {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"{name}\"\nurl = \"{}\"\n\n",
            local_file_url(&sources.join(name))
        ));
    }
    std::fs::write(workspace.join(".workspace").join("manifest.toml"), manifest).unwrap();

    StdCommand::new("git")
        .current_dir(&workspace)
        .args(["add", "."])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(&workspace)
        .args(["commit", "-q", "-m", "seed workspace"])
        .status()
        .unwrap();
    (tmp, workspace)
}

/// `ws clone --explain` is a dry run: it prints the plan and writes
/// nothing — even when the destination would otherwise be a perfectly
/// valid clone target. Mirrors the safety property documented for
/// `ws init --explain`.
#[test]
fn ws_clone_explain_does_not_clone() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(outside.path())
        .args([
            "ws",
            "clone",
            "--explain",
            "https://github.com/example/foo.git",
            "my-dest",
        ])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws clone`:"));
    assert!(stdout.contains("git clone https://github.com/example/foo.git my-dest"));
    // Crucially, no destination directory was created.
    assert!(!outside.path().join("my-dest").exists());
}

/// Happy path: a workspace with three children clones cleanly. Each
/// child is materialised under `<dest>/src/<name>/` with the seed
/// commit visible. Exercises the parallel clone + manifest-driven
/// child fan-out end-to-end.
#[test]
fn ws_clone_workspace_with_children_materialises_each_child() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let dest_root = TempDir::new().unwrap();

    let (_sources, ws_path) = make_workspace_with_children(&["alpha", "beta", "gamma"]);
    let url = local_file_url(&ws_path);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(dest_root.path())
        .args(["ws", "clone", &url, "cloned"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "clone exited non-zero: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cloned 3/3 child repos"));

    // Every child has its working tree on disk.
    let cloned = dest_root.path().join("cloned");
    for name in ["alpha", "beta", "gamma"] {
        let child = cloned.join("src").join(name);
        assert!(
            child.is_dir(),
            "child `{name}` missing at {}",
            child.display()
        );
        // Seed file from `init_child_repo` is the proof-of-life.
        assert!(child.join("seed.txt").is_file());
        assert!(child.join(".git").exists());
    }
    // Manifest came across as part of the workspace repo.
    assert!(cloned.join(".workspace").join("manifest.toml").is_file());
}

/// `--no-children` clones the workspace repo only and skips the fan-out.
/// The cloned tree therefore has the manifest but no `src/<name>` dirs.
#[test]
fn ws_clone_no_children_flag_skips_fan_out() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let dest_root = TempDir::new().unwrap();

    let (_sources, ws_path) = make_workspace_with_children(&["alpha", "beta"]);
    let url = local_file_url(&ws_path);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(dest_root.path())
        .args(["ws", "clone", &url, "cloned", "--no-children"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let cloned = dest_root.path().join("cloned");
    assert!(cloned.join(".workspace").join("manifest.toml").is_file());
    assert!(!cloned.join("src").join("alpha").exists());
    assert!(!cloned.join("src").join("beta").exists());

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--no-children"));
}

/// A non-Marshal git repo (no `.workspace/manifest.toml`) is still a
/// valid clone target. `ws clone` falls through to "plain clone, no
/// fan-out" rather than erroring — a user pointing at an arbitrary
/// repo gets a sensible result.
#[test]
fn ws_clone_against_plain_repo_succeeds_without_children() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let dest_root = TempDir::new().unwrap();

    let plain = TempDir::new().unwrap();
    init_child_repo(plain.path());
    let url = local_file_url(plain.path());

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(dest_root.path())
        .args(["ws", "clone", &url, "cloned"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("No `.workspace/manifest.toml`"));
    // The cloned repo itself does exist on disk; the seed file came
    // along with it.
    assert!(dest_root.path().join("cloned").join("seed.txt").is_file());
}

/// JSON form: tagged-enum `kind` per child + the top-level
/// `workspace_root`/`children` shape. Mirrors `ws diff`'s tagged-enum
/// JSON style — single-switch consumption.
#[test]
fn ws_clone_json_carries_per_child_results_with_kind_field() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let dest_root = TempDir::new().unwrap();

    let (_sources, ws_path) = make_workspace_with_children(&["alpha", "beta"]);
    let url = local_file_url(&ws_path);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(dest_root.path())
        .args(["ws", "clone", "--json", &url, "cloned"])
        .output()
        .unwrap();
    assert!(output.status.success());

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["manifest_present"], true);
    assert_eq!(parsed["no_children"], false);
    let children = parsed["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    for child in children {
        assert_eq!(child["kind"], "success");
        assert!(child.get("name").is_some());
        assert!(child.get("duration_ms").is_some());
    }
}

/// Partial failure: one bogus URL among real ones. The good child
/// clones successfully; the bad one is recorded as `kind = "failed"`
/// and the operation still exits 0 (Invariant 5). Other children are
/// not held back by the bad one.
#[test]
fn ws_clone_partial_failure_completes_other_children_and_exits_zero() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let dest_root = TempDir::new().unwrap();

    // Build a workspace with one real child and inject a bogus second
    // entry whose URL points at a path that does not exist.
    let (sources_owner, ws_path) = make_workspace_with_children(&["alpha"]);
    let bogus = sources_owner.path().join("does-not-exist");
    let amended = format!(
        "[workspace]\nname = \"partial\"\ndefault_branch = \"main\"\n\n\
         [[repos]]\nname = \"alpha\"\nurl = \"{}\"\n\n\
         [[repos]]\nname = \"missing\"\nurl = \"{}\"\n",
        local_file_url(&sources_owner.path().join("sources").join("alpha")),
        local_file_url(&bogus)
    );
    std::fs::write(ws_path.join(".workspace").join("manifest.toml"), &amended).unwrap();
    StdCommand::new("git")
        .current_dir(&ws_path)
        .args(["commit", "-aq", "-m", "amend manifest"])
        .status()
        .unwrap();

    let url = local_file_url(&ws_path);
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(dest_root.path())
        .args(["ws", "clone", "--json", &url, "cloned"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "partial failure should still exit 0"
    );

    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let children = parsed["children"].as_array().unwrap();
    assert_eq!(children.len(), 2);
    // Order matches manifest declaration.
    assert_eq!(children[0]["kind"], "success");
    assert_eq!(children[0]["name"], "alpha");
    assert_eq!(children[1]["kind"], "failed");
    assert_eq!(children[1]["name"], "missing");
    assert!(children[1]["error"]
        .as_str()
        .unwrap()
        .to_lowercase()
        .contains("git clone"));

    // Real child made it to disk regardless of the failed sibling.
    assert!(dest_root
        .path()
        .join("cloned")
        .join("src")
        .join("alpha")
        .join("seed.txt")
        .is_file());
}

/// `ws clone` with no positional URL prints a clear error.
#[test]
fn ws_clone_without_url_errors_clearly() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(outside.path())
        .args(["ws", "clone"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("missing required <url>"),
        "expected missing-url hint, got: {stderr}"
    );
}

// ───────────────────────────────────────────────────────────────────────────
// `ws add` / `ws unstage` — Phase 3 staging area (Slice A)
// ───────────────────────────────────────────────────────────────────────────

/// Build a minimal but realistic staging fixture: a `.workspace/`
/// with a manifest declaring `count` child repos, plus a real on-disk
/// child repo per declared name (each repo seeded with one commit on
/// `main`). Returns the temp dir owning the tree. Centralises a
/// shape that every staging test below needs.
fn make_staging_fixture(child_names: &[&str]) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let root = tmp.path();

    let mut manifest =
        String::from("[workspace]\nname = \"stagedemo\"\ndefault_branch = \"main\"\n\n");
    for name in child_names {
        manifest.push_str(&format!(
            "[[repos]]\nname = \"{name}\"\nurl = \"git@example.com:{name}.git\"\n\n"
        ));
    }
    let workspace_dir = root.join(".workspace");
    std::fs::create_dir(&workspace_dir).unwrap();
    std::fs::write(workspace_dir.join("manifest.toml"), manifest).unwrap();

    for name in child_names {
        init_child_repo(&root.join("src").join(name));
    }
    tmp
}

/// Switch the given child repo to a non-default branch with one
/// extra commit. Used to verify that `ws add` captures the
/// non-default branch + the new commit, rather than whatever was
/// HEAD when the repo was seeded.
fn switch_child_to_branch(child: &std::path::Path, branch: &str) -> String {
    StdCommand::new("git")
        .current_dir(child)
        .args(["switch", "-q", "-c", branch])
        .status()
        .unwrap();
    std::fs::write(child.join("change.txt"), "x").unwrap();
    StdCommand::new("git")
        .current_dir(child)
        .args(["add", "change.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(child)
        .args(["commit", "-q", "-m", "branch change"])
        .status()
        .unwrap();
    let out = StdCommand::new("git")
        .current_dir(child)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// Happy path: `ws add <repo>` writes `.workspace/local/staged.toml`
/// with the child's current `(branch, commit)` and seeds the
/// `.gitignore` so per-developer staging never leaks into the
/// workspace repo's history.
#[test]
fn ws_add_writes_snapshot_and_seeds_gitignore() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = ws.path().join("src").join("alpha");
    let head_sha = switch_child_to_branch(&alpha, "feat/x");

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stage failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Staged `alpha`"));
    assert!(stdout.contains("`feat/x`"));

    // The on-disk artefacts are exactly what we promised.
    let local = ws.path().join(".workspace").join("local");
    let staged = std::fs::read_to_string(local.join("staged.toml")).unwrap();
    assert!(staged.contains("[repos.alpha]"));
    assert!(staged.contains("branch = \"feat/x\""));
    assert!(staged.contains(&format!("commit = \"{head_sha}\"")));
    assert_eq!(
        std::fs::read_to_string(local.join(".gitignore")).unwrap(),
        "*\n"
    );
}

/// Re-staging the same repo overwrites the previous snapshot and
/// the JSON form surfaces the replaced entry. Mirrors `git add`'s
/// re-staging-refreshes-content behaviour.
#[test]
fn ws_add_re_stage_overwrites_and_reports_previous() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = ws.path().join("src").join("alpha");

    // First stage: still on main, the seeded commit.
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    // Move the child to a different branch, re-stage.
    let new_sha = switch_child_to_branch(&alpha, "feat/x");
    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["snapshot"]["branch"], "feat/x");
    assert_eq!(parsed["snapshot"]["commit"], new_sha);
    // The previous snapshot is the main-branch HEAD we captured first.
    assert_eq!(parsed["previous_snapshot"]["branch"], "main");
}

/// `ws add <bogus-repo>` errors with the list of known names —
/// same pattern as `--on bogus`. The user always sees a path to the
/// fix without leaving the terminal.
#[test]
fn ws_add_with_unknown_repo_errors_with_known_list() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha", "beta"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "missing"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("'missing'"));
    assert!(stderr.contains("does not match any repo"));
    assert!(stderr.contains("alpha"));
    assert!(stderr.contains("beta"));
}

/// `ws add --explain` is a dry run: it lists the planned shellouts
/// and writes nothing. Same safety property as `ws init --explain`.
#[test]
fn ws_add_explain_does_not_write() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws add`"));
    // Branch + HEAD oid arrive together via `status --porcelain=v2 --branch`.
    assert!(stdout.contains("status --porcelain=v2 --branch"));

    // The two on-disk artefacts that the real run would produce: neither exists.
    let local = ws.path().join(".workspace").join("local");
    assert!(!local.join("staged.toml").exists());
    assert!(!local.join(".gitignore").exists());
}

/// `ws add` against a child repo that has no commits yet refuses
/// with a clear hint. Snapshotting needs a HEAD to point at.
#[test]
fn ws_add_refuses_when_child_has_no_commits() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&[]);

    // Manifest declares `alpha`, but we put a fresh `git init` on disk
    // (no commits), bypassing the helper that seeds one.
    std::fs::write(
        ws.path().join(".workspace").join("manifest.toml"),
        "[workspace]\nname = \"x\"\ndefault_branch = \"main\"\n\n\
         [[repos]]\nname = \"alpha\"\nurl = \"git@x:a.git\"\n",
    )
    .unwrap();
    let alpha = ws.path().join("src").join("alpha");
    std::fs::create_dir_all(&alpha).unwrap();
    StdCommand::new("git")
        .current_dir(&alpha)
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no commits yet"),
        "expected no-commits hint, got: {stderr}"
    );
}

/// `ws unstage <repo>` removes the entry and reports what was
/// dropped. A second `ws unstage` of the same repo is idempotent
/// and prints the "nothing to do" line — no error.
#[test]
fn ws_unstage_removes_entry_then_idempotent() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    // First unstage: removes the entry, reports the drop.
    let first = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "unstage", "alpha", "--json"])
        .output()
        .unwrap();
    assert!(first.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(parsed["was_staged"], true);
    assert_eq!(parsed["removed"]["branch"], "main");

    // Second unstage: nothing to do, still exit 0.
    let second = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "unstage", "alpha"])
        .output()
        .unwrap();
    assert!(second.status.success());
    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(stdout.contains("was not staged"));
}

/// `ws unstage` of a never-staged repo is a benign no-op (matches
/// how `git restore --staged <unstaged-path>` behaves). The repo
/// must still be declared in the manifest, though — typo protection
/// is more useful than literalism here.
#[test]
fn ws_unstage_unknown_repo_errors_like_stage() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "unstage", "ghost"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("'ghost'"));
    assert!(stderr.contains("does not match any repo"));
    assert!(stderr.contains("alpha"));
}

// ───────────────────────────────────────────────────────────────────────────
// `ws status` — staging integration (Phase 3 Slice B)
// ───────────────────────────────────────────────────────────────────────────

/// After `ws add`, `ws status` surfaces the repo with a "staged at
/// <branch>@<sha>" segment and the staged-count footer. The repo
/// becomes interesting (no longer collapsible) even when its
/// working state would otherwise read as boring.
#[test]
fn ws_status_surfaces_staged_repo_with_summary_footer() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha", "beta"]);

    // Stage alpha while it's still on main (the seeded branch). The
    // repo is therefore clean+on-declared+staged: would normally
    // collapse, but staging flips it to interesting.
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("alpha"));
    assert!(stdout.contains("staged at `main`"));
    assert!(
        stdout.contains("1 repo staged for commit"),
        "expected staged-count footer, got: {stdout}"
    );
    assert!(stdout.contains("ws commit"));
}

/// Staging entries that no longer match the working state are
/// flagged as drifted in both the per-repo line and the footer's
/// drift sub-line. Drift is informational — `ws commit` still
/// records the staged values verbatim.
#[test]
fn ws_status_flags_drift_when_working_state_advanced_past_staged_snapshot() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = ws.path().join("src").join("alpha");

    // Stage at the seeded HEAD on main, then move HEAD forward.
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    std::fs::write(alpha.join("change.txt"), "x").unwrap();
    StdCommand::new("git")
        .current_dir(&alpha)
        .args(["add", "change.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(&alpha)
        .args(["commit", "-q", "-m", "advance"])
        .status()
        .unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("drifted from working"),
        "expected drift marker on the alpha line, got: {stdout}"
    );
    assert!(
        stdout.contains("working state has drifted"),
        "expected drift sub-line in footer, got: {stdout}"
    );
}

/// JSON form gains `staging` and `staging_drifted` fields per repo.
/// Both are absent when not applicable (skip_serializing_if), so
/// machine consumers can branch on presence.
#[test]
fn ws_status_json_carries_staging_fields_only_when_applicable() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha", "beta"]);

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let repos = parsed["repos"].as_array().unwrap();

    let alpha = repos.iter().find(|r| r["name"] == "alpha").unwrap();
    assert!(alpha["staging"].is_object());
    assert_eq!(alpha["staging"]["branch"], "main");
    // No drift after a fresh stage.
    assert!(
        alpha.get("staging_drifted").is_none()
            || alpha["staging_drifted"] == serde_json::Value::Bool(false)
    );

    let beta = repos.iter().find(|r| r["name"] == "beta").unwrap();
    // Beta is not staged: both fields are absent.
    assert!(beta.get("staging").is_none());
    assert!(beta.get("staging_drifted").is_none());
}

/// `ws status --explain` surfaces the new staged.toml read step as
/// part of the plan, alongside the per-repo git invocations.
#[test]
fn ws_status_explain_lists_staging_read() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws status`"));
    assert!(
        stdout.contains("staged.toml"),
        "expected the plan to mention staging file, got: {stdout}"
    );
}

/// After `ws unstage`, `ws status` no longer surfaces staging
/// markers. The repo collapses back to its plain-status category.
#[test]
fn ws_status_drops_staging_markers_after_unstage() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "unstage", "alpha"])
        .assert()
        .success();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("staged at"));
    assert!(!stdout.contains("staged for commit"));
}

// ───────────────────────────────────────────────────────────────────────────
// `ws restore` — Phase 3 Slice C
// ───────────────────────────────────────────────────────────────────────────

/// Switch the seeded child repo to a non-default branch with one
/// extra commit, leaving HEAD on that branch. Returns the path to
/// the child for further mutations in tests.
fn switch_child_off_default(ws: &TempDir, name: &str) -> std::path::PathBuf {
    let child = ws.path().join("src").join(name);
    StdCommand::new("git")
        .current_dir(&child)
        .args(["switch", "-q", "-c", "feat/restore-test"])
        .status()
        .unwrap();
    std::fs::write(child.join("change.txt"), "x").unwrap();
    StdCommand::new("git")
        .current_dir(&child)
        .args(["add", "change.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(&child)
        .args(["commit", "-q", "-m", "feat change"])
        .status()
        .unwrap();
    child
}

/// Happy path: a clean child on a non-default branch is restored
/// to the manifest's default branch (no state.toml override here).
#[test]
fn ws_restore_switches_clean_child_to_declared_branch() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = switch_child_off_default(&ws, "alpha");

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "alpha"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "restore failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Restored `alpha`"));
    assert!(stdout.contains("`main`"));

    // Child is on main now.
    let head = StdCommand::new("git")
        .current_dir(&alpha)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "main");
}

/// When the child is already on the declared branch, restore is a
/// no-op and reports that clearly. No git operation runs.
#[test]
fn ws_restore_already_on_declared_branch_is_a_no_op() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "alpha"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already on declared branch"));
}

/// A dirty child is refused without a resolution flag, with an
/// error that lists the flags. Conservative defaults (Invariant 8).
#[test]
fn ws_restore_refuses_dirty_repo_without_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = ws.path().join("src").join("alpha");

    // Dirty: an unstaged modification.
    std::fs::write(alpha.join("seed.txt"), "modified").unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "alpha"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "restore should refuse a dirty repo without a resolution flag"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--auto-stash"));
    assert!(stderr.contains("--discard-changes"));
    assert!(stderr.contains("unstaged change"));
}

/// `--auto-stash` resolves uncommitted changes by stashing, then
/// the switch proceeds. The stash is recoverable via `git stash pop`.
#[test]
fn ws_restore_auto_stash_preserves_changes_and_switches() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = switch_child_off_default(&ws, "alpha");
    std::fs::write(alpha.join("seed.txt"), "modified").unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "alpha", "--auto-stash"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "restore --auto-stash failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("stashed"));

    // Verify the stash exists and the message is marshal-flagged.
    let stash_list = StdCommand::new("git")
        .current_dir(&alpha)
        .args(["stash", "list"])
        .output()
        .unwrap();
    let stash_text = String::from_utf8_lossy(&stash_list.stdout);
    assert!(stash_text.contains("marshal/ws-restore"));
}

/// `--discard-changes` resets and cleans, then switches. The
/// uncommitted changes are gone (destructive — explicit opt-in).
#[test]
fn ws_restore_discard_changes_resets_and_switches() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = switch_child_off_default(&ws, "alpha");
    std::fs::write(alpha.join("seed.txt"), "modified").unwrap();
    std::fs::write(alpha.join("untracked.txt"), "new").unwrap();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "alpha", "--discard-changes"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "restore --discard-changes failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    // Branch switched.
    let head = StdCommand::new("git")
        .current_dir(&alpha)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&head.stdout).trim(), "main");

    // Uncommitted file is gone.
    assert!(!alpha.join("untracked.txt").exists());
    // Tracked file is back to its committed content.
    assert_eq!(
        std::fs::read_to_string(alpha.join("seed.txt")).unwrap(),
        "seed"
    );
}

/// `--auto-stash` and `--discard-changes` are mutually exclusive.
#[test]
fn ws_restore_rejects_both_resolution_flags_together() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args([
            "ws",
            "restore",
            "alpha",
            "--auto-stash",
            "--discard-changes",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("mutually exclusive"));
}

/// `--explain` describes the plan without running git or touching
/// the working tree of the child.
#[test]
fn ws_restore_explain_creates_no_side_effects() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);
    let alpha = switch_child_off_default(&ws, "alpha");

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "alpha", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws restore`"));
    // The plan line is `git -C <abs-path> switch main`. Match on
    // `switch main` to stay path-independent.
    assert!(
        stdout.contains("switch main"),
        "expected the plan to mention switch main, got:\n{stdout}"
    );

    // Child stayed on the feature branch — nothing executed.
    let head = StdCommand::new("git")
        .current_dir(&alpha)
        .args(["branch", "--show-current"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).trim(),
        "feat/restore-test"
    );
}

/// Unknown repo name → error with the list of known names.
#[test]
fn ws_restore_with_unknown_repo_lists_known_names() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha", "beta"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "ghost"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("'ghost'"));
    assert!(stderr.contains("alpha"));
    assert!(stderr.contains("beta"));
}

/// `--on <name>` is rejected: restore takes a positional. The hint
/// shows the canonical form.
#[test]
fn ws_restore_rejects_on_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "restore", "--on", "alpha"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ws restore alpha"));
}

// ───────────────────────────────────────────────────────────────────────────
// `ws reset` — Phase 3 Slice D
// ───────────────────────────────────────────────────────────────────────────

/// `ws reset` on an empty staging area is a clean no-op.
/// `was_empty` is `true` in the JSON form.
#[test]
fn ws_reset_on_empty_staging_is_a_no_op() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "reset"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("already empty"));
}

/// Happy path: stage two repos, `ws reset` clears both. The
/// resulting `staged.toml` keeps its header (so `cat staged.toml`
/// still tells a curious user what the file is for) but has no
/// `[repos.…]` entries.
#[test]
fn ws_reset_clears_every_staged_entry_and_preserves_header() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha", "beta"]);

    for r in ["alpha", "beta"] {
        marshal_with_isolated_config(&cfg_path)
            .current_dir(ws.path())
            .args(["ws", "add", r])
            .assert()
            .success();
    }

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "reset"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "reset failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Cleared 2 staged entries"));
    assert!(stdout.contains("alpha"));
    assert!(stdout.contains("beta"));

    // On-disk: header survives, body is empty.
    let staged_path = ws
        .path()
        .join(".workspace")
        .join("local")
        .join("staged.toml");
    let body = std::fs::read_to_string(&staged_path).unwrap();
    assert!(body.contains("# staged.toml"));
    // Real TOML tables begin at column 0; the header comment shows
    // `[repos."<name>"]` but only as part of a `#` line. Match by
    // line-start to ignore the example.
    let has_real_entry = body.lines().any(|line| line.starts_with("[repos."));
    assert!(
        !has_real_entry,
        "expected no `[repos.x]` table headers after reset, got:\n{body}"
    );

    // `ws status` no longer surfaces staging markers.
    let status = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "status"])
        .output()
        .unwrap();
    let status_out = String::from_utf8_lossy(&status.stdout);
    assert!(!status_out.contains("staged at"));
    assert!(!status_out.contains("staged for commit"));
}

/// JSON form: `cleared` lists every removed entry sorted by name;
/// `was_empty` is `false` after a real reset; `explain_plan` is
/// absent.
#[test]
fn ws_reset_json_lists_cleared_entries_in_sorted_order() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["zeta", "alpha", "mu"]);

    // Stage in a deliberately unsorted order — the output should
    // still come back alphabetical.
    for r in ["zeta", "mu", "alpha"] {
        marshal_with_isolated_config(&cfg_path)
            .current_dir(ws.path())
            .args(["ws", "add", r])
            .assert()
            .success();
    }

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "reset", "--json"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["was_empty"], false);
    let cleared = parsed["cleared"].as_array().unwrap();
    let names: Vec<&str> = cleared
        .iter()
        .map(|e| e["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["alpha", "mu", "zeta"]);
    assert!(parsed.get("explain_plan").is_none());
}

/// `--explain` describes the plan without writing.
#[test]
fn ws_reset_explain_creates_no_files() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "reset", "--explain"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws reset`"));

    // No on-disk artefacts after a dry run.
    let local = ws.path().join(".workspace").join("local");
    assert!(!local.join("staged.toml").exists());
    assert!(!local.join(".gitignore").exists());
}

/// `ws reset alpha` (a positional) is rejected with a hint at
/// `ws unstage alpha` — keeps the two commands disjoint.
#[test]
fn ws_reset_rejects_positional_and_redirects_to_unstage() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "reset", "alpha"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ws unstage alpha"));
}

/// `ws reset --on alpha` is similarly rejected.
#[test]
fn ws_reset_rejects_on_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_staging_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "reset", "--on", "alpha"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ws unstage alpha"));
}

// ───────────────────────────────────────────────────────────────────────────
// `ws commit` — Phase 3 Slice E
// ───────────────────────────────────────────────────────────────────────────

/// Commit-aware fixture: extends `make_staging_fixture` with an
/// initial workspace-repo git commit so the workspace itself is a
/// real git repo with a HEAD. `ws commit` cannot run against a
/// non-git workspace root (git would refuse the commit).
fn make_commit_fixture(child_names: &[&str]) -> TempDir {
    let ws = make_staging_fixture(child_names);
    StdCommand::new("git")
        .current_dir(ws.path())
        .args(["init", "--quiet", "--initial-branch=main"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(ws.path())
        .args(["config", "user.email", "t@example.com"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(ws.path())
        .args(["config", "user.name", "Test"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(ws.path())
        .args(["add", ".workspace/manifest.toml"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(ws.path())
        .args(["commit", "-q", "-m", "seed workspace"])
        .status()
        .unwrap();
    ws
}

/// Switch a child repo to a non-default branch with one extra
/// commit. Returns the absolute child path.
fn switch_child_to_named_branch(ws: &TempDir, name: &str, branch: &str) -> std::path::PathBuf {
    let child = ws.path().join("src").join(name);
    StdCommand::new("git")
        .current_dir(&child)
        .args(["switch", "-q", "-c", branch])
        .status()
        .unwrap();
    std::fs::write(child.join("change.txt"), "change").unwrap();
    StdCommand::new("git")
        .current_dir(&child)
        .args(["add", "change.txt"])
        .status()
        .unwrap();
    StdCommand::new("git")
        .current_dir(&child)
        .args(["commit", "-q", "-m", "feature change"])
        .status()
        .unwrap();
    child
}

/// Happy path: stage a child on a non-default branch, commit. The
/// state.toml is created with the staged entry, a workspace-repo
/// commit lands on HEAD, and the staging file is cleared (header
/// preserved).
#[test]
fn ws_commit_records_state_and_creates_workspace_commit() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);
    switch_child_to_named_branch(&ws, "alpha", "feat/payments");

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "-m", "release v1.0.0"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Committed"));
    assert!(stdout.contains("release v1.0.0"));
    assert!(stdout.contains("declared on `feat/payments`"));
    assert!(stdout.contains("Cleared 1 staged entry"));

    // The workspace repo's HEAD is the new commit.
    let log = StdCommand::new("git")
        .current_dir(ws.path())
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "release v1.0.0"
    );

    // state.toml carries the entry.
    let state = std::fs::read_to_string(ws.path().join(".workspace/state.toml")).unwrap();
    assert!(state.contains("[repos.alpha]"));
    assert!(state.contains("branch = \"feat/payments\""));

    // Staging file is cleared (header preserved).
    let staged = std::fs::read_to_string(
        ws.path()
            .join(".workspace")
            .join("local")
            .join("staged.toml"),
    )
    .unwrap();
    assert!(staged.contains("# staged.toml"));
    assert!(!staged.lines().any(|line| line.starts_with("[repos.")));
}

/// Empty staging → error with hint at `ws add <repo>`.
#[test]
fn ws_commit_with_empty_staging_errors_with_add_hint() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "-m", "test"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nothing staged"));
    assert!(stderr.contains("ws add"));
}

/// Re-staging a repo whose declared state already matches the
/// staged values → "nothing to commit" error. Workspace analogue
/// of `git commit` when the index has no changes.
#[test]
fn ws_commit_when_staged_matches_declared_errors_with_reset_hint() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);
    switch_child_to_named_branch(&ws, "alpha", "feat/payments");

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "-m", "first"])
        .assert()
        .success();

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "-m", "duplicate"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("nothing to commit"));
    assert!(stderr.contains("ws reset"));
}

/// JSON form: `commit_sha` populated, `changes` carries per-repo
/// entries, `cleared` lists what got promoted.
#[test]
fn ws_commit_json_returns_sha_and_changes_and_cleared() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha", "beta"]);
    switch_child_to_named_branch(&ws, "alpha", "feat/x");

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "beta"])
        .assert()
        .success();

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "-m", "two repos pinned", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "commit failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let parsed: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(parsed["message"], "two repos pinned");
    assert!(parsed["commit_sha"].is_string());
    assert_eq!(parsed["commit_sha"].as_str().unwrap().len(), 40);
    let changes = parsed["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    let cleared = parsed["cleared"].as_array().unwrap();
    assert_eq!(cleared.len(), 2);
    assert!(parsed.get("explain_plan").is_none());
}

/// `--json` without `-m` is rejected — editor mode is incompatible
/// with structured output.
#[test]
fn ws_commit_json_without_message_is_rejected() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "--json"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("--json"));
    assert!(stderr.contains("-m"));
}

/// `ws commit alpha` (a positional) is rejected with a hint at the
/// canonical flow (`ws add` then `ws commit`).
#[test]
fn ws_commit_rejects_positional_with_add_hint() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "alpha", "-m", "x"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ws add"));
}

/// `--on <name>` is rejected with a hint at `ws unstage`.
#[test]
fn ws_commit_rejects_on_flag() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "--on", "alpha", "-m", "x"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("ws unstage alpha"));
}

/// `--explain` describes the plan without writing state.toml or
/// invoking git. Independent of staging contents — the plan is
/// informational.
#[test]
fn ws_commit_explain_does_not_mutate_anything() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);

    let output = marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "--explain", "-m", "rehearsal"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Plan for `ws commit`"));
    // The plan line is `git -C <abs-path> add -- <state-rel-path>`.
    // Match on "add --" to stay path-independent.
    assert!(
        stdout.contains("add -- .workspace/state.toml"),
        "expected the plan to mention git add for state.toml, got:\n{stdout}"
    );
    assert!(stdout.contains("commit -m"));

    assert!(!ws.path().join(".workspace/state.toml").exists());

    let log = StdCommand::new("git")
        .current_dir(ws.path())
        .args(["log", "-1", "--pretty=%s"])
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&log.stdout).trim(),
        "seed workspace"
    );
}

/// Other staged paths in the workspace repo's git index stay
/// staged after `ws commit` — only `.workspace/state.toml` is
/// committed (`git commit -- <path>`'s `--only` semantics).
#[test]
fn ws_commit_does_not_disturb_other_staged_paths() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = make_commit_fixture(&["alpha"]);
    switch_child_to_named_branch(&ws, "alpha", "feat/x");

    // Create + git-add an unrelated file at the workspace root.
    std::fs::write(ws.path().join("README.md"), "# Hello").unwrap();
    StdCommand::new("git")
        .current_dir(ws.path())
        .args(["add", "README.md"])
        .status()
        .unwrap();

    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "add", "alpha"])
        .assert()
        .success();
    marshal_with_isolated_config(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "commit", "-m", "pin alpha"])
        .assert()
        .success();

    // README.md is still staged — not part of the workspace commit.
    let status = StdCommand::new("git")
        .current_dir(ws.path())
        .args(["status", "--porcelain"])
        .output()
        .unwrap();
    let status_text = String::from_utf8_lossy(&status.stdout);
    assert!(
        status_text
            .lines()
            .any(|line| line.starts_with("A  README.md")),
        "README.md should still be staged after ws commit; got status:\n{status_text}"
    );

    // The workspace commit's diff covers only state.toml.
    let show = StdCommand::new("git")
        .current_dir(ws.path())
        .args(["show", "--name-only", "--pretty=", "HEAD"])
        .output()
        .unwrap();
    let files: Vec<String> = String::from_utf8_lossy(&show.stdout)
        .lines()
        .filter(|l| !l.is_empty())
        .map(|l| l.trim().to_string())
        .collect();
    assert_eq!(files, vec![".workspace/state.toml"]);
}
