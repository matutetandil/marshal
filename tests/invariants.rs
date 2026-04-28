//! Architectural invariant guards.
//!
//! Each test in this file enforces one of the invariants from
//! `docs/PRINCIPLES.md` by running the binary against fixtures and
//! asserting externally-visible properties. A failure here means a
//! documented invariant was violated; the next commit must either
//! fix the violation or amend the invariant.
//!
//! Test names are prefixed with `invariant_<N>_` so a CI failure
//! says exactly which principle was broken. Companion to the
//! unit-level sync guards under `src/` (e.g. error_hints rule_id ↔
//! help topic body) which enforce code-adjacent invariants closer
//! to the source.
//!
//! Coverage matrix (from `docs/PRINCIPLES.md`):
//!
//! * Invariant 1 (Repo Independence) — too semantic to test from
//!   outside; relies on code review of every git operation we
//!   issue against child repos.
//! * Invariant 2/4 (Manifest as Source of Truth) — semantic,
//!   covered by integration tests on the manifest loader.
//! * Invariant 5 (Partial Failure is Acceptable) — exercised by
//!   `ws_clone_partial_failure_…` in tests/integration.rs.
//! * Invariant 6 (Explainable Operations) — covered here.
//! * Invariant 8 (Conservative Defaults) — covered here for the
//!   `ws init` refusal; will extend as more write-side commands
//!   land (Phase 3 Slices C, E).
//! * Invariant 9 (Developer Flow Preserved) — exercised by the
//!   passthrough fidelity tests in tests/integration.rs.
//! * Invariant 10 (Open/Closed via Strategy) — code-architecture
//!   rule. The "doc parity" tests here enforce one corollary: the
//!   dispatcher's unknown-subcommand error must list every known
//!   subcommand. Adding a new ws subcommand without updating the
//!   error message is detected.

use assert_cmd::Command;
use std::process::Command as StdCommand;
use tempfile::TempDir;

// ── Local helpers ─────────────────────────────────────────────────
//
// Duplicated from `tests/integration.rs` deliberately. Promoting to
// `tests/common/mod.rs` is a refactor that touches many call sites;
// for the small surface this file needs, duplication is cheaper and
// keeps the invariant tests self-contained.

fn marshal_isolated(cfg_path: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("marshal").unwrap();
    cmd.env("MARSHAL_CONFIG", cfg_path)
        .env("MARSHAL_SYSTEM_CONFIG", cfg_path.with_extension("system"))
        .env("MARSHAL_LOCAL_CONFIG", cfg_path.with_extension("local"))
        .env_remove("XDG_CONFIG_HOME")
        .env_remove("APPDATA")
        .env_remove("ProgramData");
    cmd
}

fn seed_child_repo(path: &std::path::Path) {
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

/// Build a workspace with one declared child repo. Suitable fixture
/// for the read-side ws operations and the staging commands.
fn fixture_with_one_child(name: &str) -> TempDir {
    let tmp = TempDir::new().unwrap();
    let workspace_dir = tmp.path().join(".workspace");
    std::fs::create_dir(&workspace_dir).unwrap();
    let manifest = format!(
        "[workspace]\n\
         name = \"invtest\"\n\
         default_branch = \"main\"\n\n\
         [[repos]]\n\
         name = \"{name}\"\n\
         url = \"git@example.com:{name}.git\"\n"
    );
    std::fs::write(workspace_dir.join("manifest.toml"), manifest).unwrap();
    seed_child_repo(&tmp.path().join("src").join(name));
    tmp
}

// ── Invariant 6: Explainable Operations ───────────────────────────
//
// Every workspace operation must support `--explain` and `--explain`
// must be a dry run: zero side effects, plan announced on stdout.

/// `ws init --explain` does not create the `.workspace/` directory.
/// Mirrors the safety property described in `CHANGELOG.md` for the
/// `--explain` slice.
#[test]
fn invariant_6_init_explain_creates_no_files() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    marshal_isolated(&cfg_path)
        .current_dir(outside.path())
        .args(["ws", "init", "--explain"])
        .assert()
        .success();

    assert!(
        !outside.path().join(".workspace").exists(),
        "ws init --explain must not create the .workspace/ directory"
    );
}

/// `ws clone --explain <url> <dest>` does not create the destination
/// directory. The plan is described, no clone is performed.
#[test]
fn invariant_6_clone_explain_creates_no_dest_dir() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    marshal_isolated(&cfg_path)
        .current_dir(outside.path())
        .args([
            "ws",
            "clone",
            "--explain",
            "https://example.com/foo.git",
            "my-dest",
        ])
        .assert()
        .success();

    assert!(
        !outside.path().join("my-dest").exists(),
        "ws clone --explain must not create the destination directory"
    );
}

/// `ws stage <repo> --explain` does not create
/// `.workspace/local/staged.toml` or its sibling `.gitignore`.
#[test]
fn invariant_6_stage_explain_creates_no_staged_toml() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = fixture_with_one_child("alpha");

    marshal_isolated(&cfg_path)
        .current_dir(ws.path())
        .args(["ws", "stage", "alpha", "--explain"])
        .assert()
        .success();

    let local = ws.path().join(".workspace").join("local");
    assert!(
        !local.join("staged.toml").exists(),
        "ws stage --explain must not create staged.toml"
    );
    assert!(
        !local.join(".gitignore").exists(),
        "ws stage --explain must not seed local/.gitignore"
    );
}

/// Meta-test: every read-side ws operation responds to `--explain`
/// with "Plan for `ws X`:" on stdout. A future op added without
/// threading `--explain` breaks this loudly.
///
/// Read-side ops share a single fixture, so the loop is cheap.
/// Write-side ops (init, clone, stage) have dedicated tests above
/// because their fixture requirements differ.
#[test]
fn invariant_6_read_side_ops_announce_plan_under_explain() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let ws = fixture_with_one_child("alpha");

    // (subcommand, extra args before --explain). `restore` is a
    // write-side op but its --explain path is read-only (it does
    // not even read the working tree), so it shares this fixture
    // safely. We include it here so adding a new write-side op
    // that forgets --explain breaks loudly.
    let cases: &[(&str, &[&str])] = &[
        ("status", &[]),
        ("log", &[]),
        ("diff", &[]),
        ("unstage", &["alpha"]),
        ("restore", &["alpha"]),
        ("reset", &[]),
    ];

    for (subcmd, args) in cases {
        let mut argv: Vec<&str> = vec!["ws", subcmd];
        argv.extend_from_slice(args);
        argv.push("--explain");

        let output = marshal_isolated(&cfg_path)
            .current_dir(ws.path())
            .args(&argv)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "ws {subcmd} --explain exited non-zero: {}",
            String::from_utf8_lossy(&output.stderr),
        );
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains(&format!("Plan for `ws {subcmd}`")),
            "ws {subcmd} --explain did not announce a plan. stdout:\n{stdout}"
        );
    }
}

// ── Invariant 8: Conservative Defaults ────────────────────────────
//
// Actions that modify state beyond what the user explicitly
// requested are forbidden unless they come with an opt-in flag.

/// `ws init` against an existing workspace must refuse without
/// `--force`. The marshal binary should never silently overwrite a
/// manifest the user did not ask to overwrite.
#[test]
fn invariant_8_init_refuses_existing_workspace_without_force() {
    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let tmp = TempDir::new().unwrap();

    // First init succeeds.
    marshal_isolated(&cfg_path)
        .current_dir(tmp.path())
        .args(["ws", "init"])
        .assert()
        .success();

    // Second init refuses with a hint pointing at --force.
    let output = marshal_isolated(&cfg_path)
        .current_dir(tmp.path())
        .args(["ws", "init"])
        .output()
        .unwrap();
    assert!(
        !output.status.success(),
        "second `ws init` against an existing workspace should refuse without --force"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("--force"),
        "refusal must point the user at --force as the explicit override; got: {stderr}"
    );
}

// ── Invariant 10 / doc parity: every dispatched name is announced ─
//
// Adding a Strategy member (here: a ws or marshal subcommand) is
// supposed to be `impl Trait` plus one registration line. A
// corollary: the user-facing surface that lists available members
// must stay in sync. The dispatcher's unknown-subcommand error
// is the canonical "list of names" — these tests fail loudly when
// a new subcommand ships without being added to it.

/// The `ws bogus` error message names every known ws subcommand.
/// Hardcoded list intentionally: adding a new subcommand should
/// require updating this test, surfacing the corresponding
/// documentation update at the same time.
#[test]
fn invariant_10_unknown_ws_subcommand_error_lists_every_known_subcommand() {
    const KNOWN_WS_SUBCOMMANDS: &[&str] = &[
        "init", "status", "log", "diff", "clone", "stage", "unstage", "restore", "reset",
    ];

    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");
    let outside = TempDir::new().unwrap();

    let output = marshal_isolated(&cfg_path)
        .current_dir(outside.path())
        .args(["ws", "definitely-not-a-real-subcommand"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);

    for name in KNOWN_WS_SUBCOMMANDS {
        let with_git_prefix = format!("git ws {name}");
        let with_ws_prefix = format!("ws {name}");
        assert!(
            stderr.contains(&with_git_prefix) || stderr.contains(&with_ws_prefix),
            "unknown-ws-subcommand error must mention `ws {name}` so users discover it. \
             stderr:\n{stderr}"
        );
    }
}

/// `marshal` with no args (or with an unknown subcommand) is the
/// authoritative user-facing list of marshal-namespace commands.
/// Verifies that every known subcommand appears so a new one does
/// not slip in unannounced.
#[test]
fn invariant_10_marshal_overview_lists_every_known_subcommand() {
    const KNOWN_MARSHAL_SUBCOMMANDS: &[&str] = &["config", "what-now", "help"];

    let cfg_dir = TempDir::new().unwrap();
    let cfg_path = cfg_dir.path().join("config.toml");

    let output = marshal_isolated(&cfg_path)
        .args(["marshal"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "`marshal` (no args) should print overview and exit 0"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);

    for name in KNOWN_MARSHAL_SUBCOMMANDS {
        assert!(
            stdout.contains(name),
            "`marshal` overview must list the `{name}` subcommand so users discover it. \
             stdout:\n{stdout}"
        );
    }

    // Plus the `ws` namespace pointer — workspace ops are advertised
    // in the same overview so users never have to read docs to find
    // them.
    assert!(
        stdout.contains("ws"),
        "`marshal` overview must advertise the `ws` namespace. stdout:\n{stdout}"
    );
}
