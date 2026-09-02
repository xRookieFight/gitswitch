use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;

use clap::{ColorChoice, Parser, Subcommand};

use crate::error::{Error, Result};
use crate::git::Scope;
use crate::model::{Account, DEFAULT_HOST};
use crate::process::{Runner, SystemRunner};
use crate::secrets::{self, SecretStore};
use crate::service::{GhOutcome, Service, SwitchOptions};
use crate::store::Store;

/// Manage and switch between multiple GitHub accounts.
///
/// Run without arguments to open the interactive interface.
#[derive(Debug, Parser)]
#[command(
    name = "gitswitch",
    version,
    about = "Switch between multiple GitHub accounts - git identity and gh auth in one step",
    long_about = None,
    color = ColorChoice::Auto,
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Command>,

    /// Directory holding accounts.json (defaults to the OS config directory).
    #[arg(long, global = true, value_name = "DIR")]
    pub config_dir: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// List every saved account.
    List {
        /// Print machine readable JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show the account that is currently active.
    Current {
        #[arg(long)]
        json: bool,
    },
    /// Switch git and the GitHub CLI to an account.
    Switch {
        /// Saved account name.
        account: String,
        /// Write the identity to the current repository instead of globally.
        #[arg(long)]
        local: bool,
        /// Skip the post-switch `gh auth status` check.
        #[arg(long)]
        no_verify: bool,
    },
    /// Save a new account.
    ///
    /// With no flags this opens the interactive wizard. Tokens are only ever
    /// read from stdin, never from an argument.
    Add {
        #[arg(long, value_name = "NAME")]
        name: Option<String>,
        /// GitHub login, e.g. xRookieFight.
        #[arg(long, value_name = "LOGIN")]
        username: Option<String>,
        /// Value for git's user.name.
        #[arg(long, value_name = "NAME")]
        git_name: Option<String>,
        /// Value for git's user.email.
        #[arg(long, value_name = "EMAIL")]
        git_email: Option<String>,
        /// GitHub host for GitHub Enterprise setups.
        #[arg(long, default_value = DEFAULT_HOST, value_name = "HOST")]
        host: String,
        /// Read a personal access token from stdin and authenticate with it.
        #[arg(long)]
        token_stdin: bool,
    },
    /// Delete a saved account.
    Remove {
        account: String,
        /// Do not ask for confirmation.
        #[arg(long, short)]
        yes: bool,
        /// Also run `gh auth logout` for the account.
        #[arg(long)]
        logout: bool,
    },
    /// Rename a saved account.
    Rename { from: String, to: String },
    /// Sign an account in to the GitHub CLI.
    ///
    /// Opens the browser based `gh auth login` flow unless a token is piped in.
    Auth {
        account: String,
        /// Read a personal access token from stdin instead of using the browser.
        #[arg(long)]
        token_stdin: bool,
    },
    /// Check git, the GitHub CLI and the credential store.
    Doctor,
    /// Print the version.
    Version,
}

pub fn run() -> Result<()> {
    let cli = Cli::parse();
    let runner = SystemRunner;
    let secrets = secrets::default_store();
    execute(cli, &runner, secrets.as_ref())
}

pub fn execute(cli: Cli, runner: &dyn Runner, secrets: &dyn SecretStore) -> Result<()> {
    let path = match &cli.config_dir {
        Some(dir) => dir.join("accounts.json"),
        None => Store::default_path()?,
    };
    let store = Store::open(path)?;
    let mut service = Service::new(runner, secrets, store);

    match cli.command {
        None => crate::tui::run(&mut service),
        Some(Command::Version) => {
            println!("gitswitch {}", crate::VERSION);
            Ok(())
        }
        Some(Command::List { json }) => list(&service, json),
        Some(Command::Current { json }) => current(&service, json),
        Some(Command::Switch {
            account,
            local,
            no_verify,
        }) => switch(&mut service, &account, local, no_verify),
        Some(Command::Add {
            name,
            username,
            git_name,
            git_email,
            host,
            token_stdin,
        }) => add(
            &mut service,
            name,
            username,
            git_name,
            git_email,
            host,
            token_stdin,
        ),
        Some(Command::Remove {
            account,
            yes,
            logout,
        }) => remove(&mut service, &account, yes, logout),
        Some(Command::Rename { from, to }) => {
            service.rename_account(&from, &to)?;
            println!("{} renamed `{from}` to `{to}`", ok_mark());
            Ok(())
        }
        Some(Command::Auth {
            account,
            token_stdin,
        }) => {
            if token_stdin {
                let token = read_token_from_stdin()?;
                service.reauthenticate(&account, &token)?;
                println!("{} `{account}` re-authenticated", ok_mark());
                return Ok(());
            }
            browser_login(&mut service, &account)
        }
        Some(Command::Doctor) => doctor(&service),
    }
}

fn list(service: &Service<'_>, json: bool) -> Result<()> {
    let accounts = service.store().accounts();
    if json {
        let active = service.store().active().map(|a| a.name.clone());
        let payload = serde_json::json!({ "active": active, "accounts": accounts });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return Ok(());
    }

    if accounts.is_empty() {
        println!("No accounts saved yet. Run `gitswitch add` to create one.");
        return Ok(());
    }

    let active = service.store().active().map(|a| a.name.clone());
    let width = accounts.iter().map(|a| a.name.len()).max().unwrap_or(4);
    for account in accounts {
        let marker = if Some(&account.name) == active.as_ref() {
            "*"
        } else {
            " "
        };
        println!(
            "{marker} {:width$}  {:<20} {}",
            account.name, account.username, account.git_email
        );
    }
    Ok(())
}

fn current(service: &Service<'_>, json: bool) -> Result<()> {
    let status = service.status();
    if json {
        let payload = serde_json::json!({
            "account": status.active_account,
            "git_name": status.identity.name,
            "git_email": status.identity.email,
            "gh_login": status.gh_login,
            "consistent": status.is_consistent(),
        });
        println!("{}", serde_json::to_string_pretty(&payload).unwrap());
        return Ok(());
    }

    let Some(account) = &status.active_account else {
        return Err(Error::NoActiveAccount);
    };

    println!("{}  ({})", account.name, account.username);
    println!(
        "  git       {} <{}>",
        status.identity.name.as_deref().unwrap_or("unset"),
        status.identity.email.as_deref().unwrap_or("unset")
    );
    println!(
        "  gh        {}",
        status.gh_login.as_deref().unwrap_or("not authenticated")
    );
    if !status.is_consistent() {
        println!(
            "  {} git and gh disagree - run `gitswitch switch {}`",
            warn_mark(),
            account.name
        );
    }
    Ok(())
}

fn switch(service: &mut Service<'_>, name: &str, local: bool, no_verify: bool) -> Result<()> {
    let options = SwitchOptions {
        scope: if local { Scope::Local } else { Scope::Global },
        verify: !no_verify,
    };
    let report = service.switch(name, options)?;

    println!(
        "{} switched to {} ({})",
        ok_mark(),
        report.account.name,
        report.account.username
    );
    println!(
        "  git       {} <{}> [{}]",
        report.account.git_name,
        report.account.git_email,
        report.scope.label()
    );
    println!(
        "  gh        {}",
        match &report.gh {
            GhOutcome::AlreadyActive => "already authenticated".to_string(),
            GhOutcome::Switched => format!("switched to {}", report.account.username),
            GhOutcome::ReAuthenticated => "re-authenticated from the credential store".to_string(),
            GhOutcome::Skipped(reason) => format!("skipped - {reason}"),
        }
    );
    for warning in &report.warnings {
        println!("  {} {warning}", warn_mark());
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn add(
    service: &mut Service<'_>,
    name: Option<String>,
    username: Option<String>,
    git_name: Option<String>,
    git_email: Option<String>,
    host: String,
    token_stdin: bool,
) -> Result<()> {
    // No details on the command line means the user wants the wizard.
    if name.is_none() && username.is_none() && git_name.is_none() && git_email.is_none() {
        return crate::tui::run_add(service);
    }

    let account = Account::new(
        require_flag(name, "--name")?,
        require_flag(username, "--username")?,
        require_flag(git_name, "--git-name")?,
        require_flag(git_email, "--git-email")?,
    )
    .with_host(host);

    let token = if token_stdin {
        Some(read_token_from_stdin()?)
    } else {
        None
    };

    let saved = service.add_account(account, token.as_deref())?;
    println!("{} saved `{}` ({})", ok_mark(), saved.name, saved.username);
    if saved.has_stored_token {
        println!("  token stored in the OS credential store");
    } else if token.is_some() {
        println!(
            "  {} no OS credential store available - the token was not cached",
            warn_mark()
        );
    } else {
        println!(
            "  sign it in with `gitswitch auth {}` (opens your browser)",
            saved.name
        );
    }
    Ok(())
}

/// Hands the terminal to `gh auth login` so the user can sign in through the
/// browser, then activates the account.
fn browser_login(service: &mut Service<'_>, name: &str) -> Result<()> {
    let account = service.store().require(name)?.clone();
    let gh = service.gh();
    if !gh.is_installed() {
        return Err(Error::MissingDependency("gh"));
    }

    println!(
        "Opening the GitHub sign-in flow for `{}`...",
        account.username
    );
    gh.login_interactive(&account.host)?;

    let report = service.switch(&account.name, SwitchOptions::default())?;
    println!(
        "{} `{}` is signed in and active ({})",
        ok_mark(),
        report.account.name,
        report.account.username
    );
    Ok(())
}

fn remove(service: &mut Service<'_>, name: &str, yes: bool, logout: bool) -> Result<()> {
    let account = service.store().require(name)?.clone();
    if !yes {
        if !std::io::stdin().is_terminal() {
            return Err(Error::InvalidInput(
                "refusing to remove an account without confirmation; pass --yes".into(),
            ));
        }
        print!("Remove `{}` ({})? [y/N] ", account.name, account.username);
        std::io::stdout().flush()?;
        let mut answer = String::new();
        std::io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            println!("Cancelled.");
            return Ok(());
        }
    }

    service.remove_account(&account.name, logout)?;
    println!("{} removed `{}`", ok_mark(), account.name);
    Ok(())
}

fn doctor(service: &Service<'_>) -> Result<()> {
    let status = service.status();
    println!("gitswitch {}", crate::VERSION);
    println!(
        "  git       {}",
        status.git_version.as_deref().unwrap_or("NOT FOUND")
    );
    println!(
        "  gh        {}",
        status.gh_version.as_deref().unwrap_or("NOT FOUND")
    );
    println!(
        "  keychain  {}",
        if service.secrets().available() {
            "available"
        } else {
            "unavailable - tokens will not be cached"
        }
    );
    println!("  config    {}", service.store().path().display());
    println!("  accounts  {}", service.store().accounts().len());

    if status.problems.is_empty() {
        println!("\n{} everything looks good", ok_mark());
        return Ok(());
    }
    println!();
    for problem in &status.problems {
        println!("{} {problem}", warn_mark());
    }
    Ok(())
}

fn require_flag(value: Option<String>, flag: &str) -> Result<String> {
    value.ok_or_else(|| Error::InvalidInput(format!("{flag} is required in non-interactive mode")))
}

/// Reads a token from stdin. Using stdin keeps the secret out of the process
/// list and out of the shell history.
fn read_token_from_stdin() -> Result<String> {
    let mut buffer = String::new();
    std::io::stdin().read_to_string(&mut buffer)?;
    let token = buffer.trim().to_string();
    crate::model::validate_token(&token)?;
    Ok(token)
}

fn colors_enabled() -> bool {
    std::env::var_os("NO_COLOR").is_none() && std::io::stdout().is_terminal()
}

fn ok_mark() -> &'static str {
    if colors_enabled() {
        "\u{1b}[32m\u{2713}\u{1b}[0m"
    } else {
        "OK"
    }
}

fn warn_mark() -> &'static str {
    if colors_enabled() {
        "\u{1b}[33m!\u{1b}[0m"
    } else {
        "!"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    #[test]
    fn switch_parses_its_flags() {
        let cli = Cli::parse_from(["gitswitch", "switch", "work", "--local", "--no-verify"]);
        match cli.command {
            Some(Command::Switch {
                account,
                local,
                no_verify,
            }) => {
                assert_eq!(account, "work");
                assert!(local && no_verify);
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    #[test]
    fn no_subcommand_means_the_interactive_interface() {
        assert!(Cli::parse_from(["gitswitch"]).command.is_none());
    }

    #[test]
    fn tokens_cannot_be_passed_as_arguments() {
        let rendered = Cli::command().render_long_help().to_string();
        assert!(!rendered.contains("--token "));
        let add = Cli::command()
            .get_subcommands()
            .find(|c| c.get_name() == "add")
            .expect("add subcommand")
            .clone();
        assert!(add.get_arguments().all(|arg| arg.get_id() != "token"));
    }

    #[test]
    fn add_defaults_to_the_public_host() {
        let cli = Cli::parse_from(["gitswitch", "add", "--name", "work"]);
        match cli.command {
            Some(Command::Add { host, name, .. }) => {
                assert_eq!(host, DEFAULT_HOST);
                assert_eq!(name.as_deref(), Some("work"));
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }
}
