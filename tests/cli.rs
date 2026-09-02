//! End-to-end tests for the non-interactive commands.
//!
//! Commands that would touch GitHub are driven through [`gitswitch::cli::execute`]
//! with a mocked process runner and an in-memory credential store; the rest run
//! the real binary.

use std::process::Command;

use clap::Parser;
use gitswitch::cli::{Cli, execute};
use gitswitch::model::Account;
use gitswitch::secrets::memory::MemoryStore;
use gitswitch::store::Store;
use gitswitch::testing::{MockRunner, gh_status_output};
use tempfile::TempDir;

fn binary() -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_gitswitch"));
    command.env("NO_COLOR", "1");
    command
}

fn run(args: &[&str], dir: &TempDir, runner: &MockRunner) -> gitswitch::Result<()> {
    let secrets = MemoryStore::default();
    let mut full = vec!["gitswitch", "--config-dir", dir.path().to_str().unwrap()];
    full.extend_from_slice(args);
    execute(Cli::parse_from(full), runner, &secrets)
}

fn seed(dir: &TempDir, accounts: &[(&str, &str)]) {
    let mut store = Store::open(dir.path().join("accounts.json")).unwrap();
    for (name, username) in accounts {
        store
            .add(Account::new(
                *name,
                *username,
                "Octo Cat",
                "octo@example.com",
            ))
            .unwrap();
    }
}

fn ready_runner() -> MockRunner {
    MockRunner::new()
        .ok("git --version", "git version 2.55.0")
        .ok("gh --version", "gh version 2.97.0")
        .ok("git config", "")
        .ok("gh auth setup-git", "")
        .ok(
            "gh auth status",
            &gh_status_output("github.com", &[("octocat", true)]),
        )
}

#[test]
fn version_prints_the_crate_version() {
    let output = binary().arg("version").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn help_documents_the_commands() {
    let output = binary().arg("--help").output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    for command in ["list", "current", "switch", "add", "remove", "version"] {
        assert!(stdout.contains(command), "`{command}` missing from --help");
    }
}

#[test]
fn list_is_empty_on_a_fresh_install() {
    let dir = TempDir::new().unwrap();
    let output = binary()
        .args(["--config-dir", dir.path().to_str().unwrap(), "list"])
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("gitswitch add")
    );
}

#[test]
fn switching_an_unknown_account_fails_with_a_hint() {
    let dir = TempDir::new().unwrap();
    let output = binary()
        .args([
            "--config-dir",
            dir.path().to_str().unwrap(),
            "switch",
            "ghost",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("no account named `ghost`"));
    assert!(stderr.contains("gitswitch list"));
}

#[test]
fn removing_without_confirmation_is_refused_when_not_a_terminal() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("work", "work-bot")]);

    let output = binary()
        .args([
            "--config-dir",
            dir.path().to_str().unwrap(),
            "remove",
            "work",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8(output.stderr).unwrap().contains("--yes"));
}

#[test]
fn accounts_survive_a_restart() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("work", "work-bot"), ("personal", "octocat")]);

    let output = binary()
        .args(["--config-dir", dir.path().to_str().unwrap(), "list"])
        .output()
        .unwrap();
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("work"));
    assert!(stdout.contains("personal"));
}

#[test]
fn switch_updates_git_and_gh_then_records_the_active_account() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("personal", "octocat")]);
    let runner = ready_runner();

    run(&["switch", "personal"], &dir, &runner).unwrap();

    assert!(runner.was_called("git config --global user.name Octo Cat"));
    assert!(runner.was_called("git config --global user.email octo@example.com"));
    let store = Store::open(dir.path().join("accounts.json")).unwrap();
    assert_eq!(store.active().unwrap().name, "personal");
}

#[test]
fn switch_local_writes_to_the_repository_config() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("personal", "octocat")]);
    let runner = ready_runner().ok("git rev-parse --is-inside-work-tree", "true");

    run(&["switch", "personal", "--local"], &dir, &runner).unwrap();
    assert!(runner.was_called("git config --local user.name Octo Cat"));
}

#[test]
fn remove_deletes_the_account_from_disk() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("work", "work-bot")]);
    let runner = ready_runner();

    run(&["remove", "work", "--yes"], &dir, &runner).unwrap();

    let store = Store::open(dir.path().join("accounts.json")).unwrap();
    assert!(store.is_empty());
}

#[test]
fn rename_keeps_the_account_data() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("work", "work-bot")]);
    let runner = ready_runner();

    run(&["rename", "work", "job"], &dir, &runner).unwrap();

    let store = Store::open(dir.path().join("accounts.json")).unwrap();
    assert_eq!(store.get("job").unwrap().username, "work-bot");
}

#[test]
fn add_requires_every_field_in_non_interactive_mode() {
    let dir = TempDir::new().unwrap();
    let runner = ready_runner();
    let err = run(&["add", "--name", "work"], &dir, &runner).unwrap_err();
    assert!(err.to_string().contains("--username"));
}

#[test]
fn add_saves_an_account_without_a_token() {
    let dir = TempDir::new().unwrap();
    let runner = ready_runner();

    run(
        &[
            "add",
            "--name",
            "work",
            "--username",
            "work-bot",
            "--git-name",
            "Work Bot",
            "--git-email",
            "bot@example.com",
        ],
        &dir,
        &runner,
    )
    .unwrap();

    let store = Store::open(dir.path().join("accounts.json")).unwrap();
    let account = store.get("work").unwrap();
    assert_eq!(account.username, "work-bot");
    assert!(!account.has_stored_token);
}

#[test]
fn the_config_file_never_contains_a_token() {
    let dir = TempDir::new().unwrap();
    let runner = ready_runner().ok("gh auth login", "");
    let secrets = MemoryStore::default();

    let mut store = Store::open(dir.path().join("accounts.json")).unwrap();
    store
        .add(Account::new(
            "work",
            "work-bot",
            "Work Bot",
            "bot@example.com",
        ))
        .unwrap();
    drop(store);

    let mut service = gitswitch::Service::new(
        &runner,
        &secrets,
        Store::open(dir.path().join("accounts.json")).unwrap(),
    );
    service
        .reauthenticate("work", "ghp_EXAMPLE0123456789abc")
        .unwrap();

    let raw = std::fs::read_to_string(dir.path().join("accounts.json")).unwrap();
    assert!(!raw.contains("ghp_"));
    assert!(raw.contains("has_stored_token"));
}

#[test]
fn corrupted_configuration_is_reported_clearly() {
    let dir = TempDir::new().unwrap();
    std::fs::write(dir.path().join("accounts.json"), "}}broken").unwrap();

    let output = binary()
        .args(["--config-dir", dir.path().to_str().unwrap(), "list"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("corrupted"));
    assert!(stderr.contains("accounts.json"));
}

#[test]
fn list_json_is_machine_readable() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("work", "work-bot")]);

    let output = binary()
        .args([
            "--config-dir",
            dir.path().to_str().unwrap(),
            "list",
            "--json",
        ])
        .output()
        .unwrap();
    let value: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("valid JSON on stdout");
    assert_eq!(value["accounts"][0]["name"], "work");
}

#[test]
fn current_fails_when_nothing_is_active() {
    let dir = TempDir::new().unwrap();
    seed(&dir, &[("work", "work-bot")]);
    let runner = ready_runner();

    let err = run(&["current"], &dir, &runner).unwrap_err();
    assert!(err.to_string().contains("no account is currently active"));
}

#[test]
fn the_interactive_interface_refuses_to_run_in_a_pipe() {
    let dir = TempDir::new().unwrap();
    let output = binary()
        .args(["--config-dir", dir.path().to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("terminal")
    );
}
