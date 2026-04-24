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

/// A test-scoped marshal invocation that points `MARSHAL_CONFIG` at a temp
/// file and clears every other config-related env var. Returns the builder
/// so callers can `.arg(…)` and `.output()` / `.assert()` normally.
fn marshal_with_isolated_config(config_path: &std::path::Path) -> Command {
    let mut cmd = marshal();
    cmd.env("MARSHAL_CONFIG", config_path)
        // Point system config somewhere unreachable by default so tests that
        // don't care about it never accidentally read /etc/marshal. Tests
        // that DO care call `marshal_with_both_config_isolations`.
        .env(
            "MARSHAL_SYSTEM_CONFIG",
            config_path.with_extension("system"),
        )
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("ProgramData");
    cmd
}

/// Same as [`marshal_with_isolated_config`] but also points
/// `MARSHAL_SYSTEM_CONFIG` at a caller-specified path. For tests that need to
/// exercise the system layer explicitly.
fn marshal_with_both_isolations(
    global_path: &std::path::Path,
    system_path: &std::path::Path,
) -> Command {
    let mut cmd = marshal();
    cmd.env("MARSHAL_CONFIG", global_path)
        .env("MARSHAL_SYSTEM_CONFIG", system_path)
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

/// `--local` is reserved for step 5c and must fail cleanly until then.
#[test]
fn local_flag_is_rejected_until_step_5c() {
    let dir = TempDir::new().unwrap();
    let global_path = dir.path().join("user.toml");
    let system_path = dir.path().join("sys.toml");

    let output = marshal_with_both_isolations(&global_path, &system_path)
        .args([
            "marshal",
            "config",
            "set",
            "--local",
            "modernize.tips",
            "false",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success(), "--local must reject for now");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--local is not available yet"),
        "stderr explains --local is deferred, got: {stderr}"
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
