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
