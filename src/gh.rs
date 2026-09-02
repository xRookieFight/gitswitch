use crate::error::{Error, Result};
use crate::process::{Output, Runner};

/// One entry from `gh auth status`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GhAccount {
    pub host: String,
    pub login: String,
    pub active: bool,
}

/// Thin wrapper over the official GitHub CLI. gitswitch deliberately delegates
/// all authentication to `gh` instead of implementing its own OAuth flow.
pub struct Gh<'r> {
    runner: &'r dyn Runner,
}

impl<'r> Gh<'r> {
    pub fn new(runner: &'r dyn Runner) -> Self {
        Self { runner }
    }

    pub fn is_installed(&self) -> bool {
        self.runner.is_available("gh")
    }

    pub fn version(&self) -> Result<String> {
        let out = self.runner.run("gh", &["--version"], None)?;
        if !out.ok() {
            return Err(fail("gh --version", &out));
        }
        Ok(out
            .stdout
            .lines()
            .next()
            .unwrap_or_default()
            .trim()
            .to_string())
    }

    /// Accounts `gh` knows about. An unauthenticated CLI yields an empty list
    /// rather than an error, because that is a normal first-run state.
    pub fn accounts(&self) -> Result<Vec<GhAccount>> {
        let out = self.runner.run("gh", &["auth", "status"], None)?;
        let text = format!("{}\n{}", out.stdout, out.stderr);
        if !out.ok() && !text.contains("Logged in to") {
            if is_logged_out(&text) {
                return Ok(Vec::new());
            }
            return Err(fail("gh auth status", &out));
        }
        Ok(parse_status(&text))
    }

    pub fn accounts_for(&self, host: &str) -> Result<Vec<GhAccount>> {
        Ok(self
            .accounts()?
            .into_iter()
            .filter(|account| account.host == host)
            .collect())
    }

    pub fn active_login(&self, host: &str) -> Result<Option<String>> {
        Ok(self
            .accounts_for(host)?
            .into_iter()
            .find(|account| account.active)
            .map(|account| account.login))
    }

    pub fn is_known(&self, host: &str, login: &str) -> Result<bool> {
        Ok(self
            .accounts_for(host)?
            .iter()
            .any(|account| account.login.eq_ignore_ascii_case(login)))
    }

    /// Makes an already authenticated account the active one.
    pub fn switch(&self, host: &str, login: &str) -> Result<()> {
        let out = self.runner.run(
            "gh",
            &["auth", "switch", "--hostname", host, "--user", login],
            None,
        )?;
        if !out.ok() {
            return Err(fail(&format!("gh auth switch --user {login}"), &out));
        }
        Ok(())
    }

    /// Authenticates using a token supplied by the user. The token is passed on
    /// stdin so it never appears in the process list.
    pub fn login_with_token(&self, host: &str, token: &str) -> Result<()> {
        let out = self.runner.run(
            "gh",
            &["auth", "login", "--hostname", host, "--with-token"],
            Some(token.trim()),
        )?;
        if !out.ok() {
            return Err(Error::CommandFailed {
                command: "gh auth login --with-token".into(),
                status: out.status,
                message: out.message(),
            });
        }
        Ok(())
    }

    /// Hands the terminal to `gh` for its interactive browser/device flow.
    pub fn login_interactive(&self, host: &str) -> Result<()> {
        let status = self.runner.run_interactive(
            "gh",
            &[
                "auth",
                "login",
                "--hostname",
                host,
                "--git-protocol",
                "https",
            ],
        )?;
        if status != 0 {
            return Err(Error::CommandFailed {
                command: "gh auth login".into(),
                status,
                message: "interactive login did not complete".into(),
            });
        }
        Ok(())
    }

    pub fn logout(&self, host: &str, login: &str) -> Result<()> {
        let out = self.runner.run(
            "gh",
            &["auth", "logout", "--hostname", host, "--user", login],
            None,
        )?;
        if !out.ok() && !out.message().contains("not logged in") {
            return Err(fail(&format!("gh auth logout --user {login}"), &out));
        }
        Ok(())
    }

    /// Points git's credential helper at `gh` so pushes use the active account.
    pub fn setup_git(&self, host: &str) -> Result<()> {
        let out = self
            .runner
            .run("gh", &["auth", "setup-git", "--hostname", host], None)?;
        if !out.ok() {
            return Err(fail("gh auth setup-git", &out));
        }
        Ok(())
    }
}

fn is_logged_out(text: &str) -> bool {
    text.contains("not logged in") || text.contains("You are not logged into any GitHub hosts")
}

/// Parses the human readable `gh auth status` report.
///
/// Token lines are ignored on purpose - gitswitch never reads or displays them.
pub fn parse_status(text: &str) -> Vec<GhAccount> {
    let mut accounts: Vec<GhAccount> = Vec::new();
    let mut host = String::new();

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Host headers are the only unindented, non-bulleted lines.
        if !line.starts_with(char::is_whitespace)
            && !trimmed.starts_with('-')
            && !trimmed.contains(' ')
        {
            host = trimmed.trim_end_matches(':').to_string();
            continue;
        }

        if let Some(login) = login_from(trimmed) {
            let entry_host = host_from(trimmed).unwrap_or_else(|| host.clone());
            accounts.push(GhAccount {
                host: entry_host,
                login,
                active: false,
            });
            continue;
        }

        if let Some(rest) = trimmed.strip_prefix("- Active account:")
            && let Some(last) = accounts.last_mut()
        {
            last.active = rest.trim().eq_ignore_ascii_case("true");
        }
    }

    // Older gh versions omit the "Active account" line when only one account
    // exists on a host; that single account is the active one.
    for host in accounts
        .iter()
        .map(|a| a.host.clone())
        .collect::<Vec<_>>()
        .into_iter()
    {
        let mut per_host = accounts.iter_mut().filter(|a| a.host == host);
        if let (Some(only), None) = (per_host.next(), per_host.next()) {
            only.active = true;
        }
    }

    accounts
}

fn login_from(line: &str) -> Option<String> {
    let index = line.find("account ")?;
    if !line.contains("Logged in to") {
        return None;
    }
    let rest = &line[index + "account ".len()..];
    let login = rest.split_whitespace().next()?;
    (!login.is_empty()).then(|| login.to_string())
}

fn host_from(line: &str) -> Option<String> {
    let start = line.find("Logged in to ")? + "Logged in to ".len();
    let rest = &line[start..];
    let host = rest.split_whitespace().next()?;
    (!host.is_empty()).then(|| host.to_string())
}

fn fail(command: &str, out: &Output) -> Error {
    Error::CommandFailed {
        command: command.to_string(),
        status: out.status,
        message: out.message(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::DEFAULT_HOST;
    use crate::testing::{MockRunner, gh_status_output};

    const TWO_ACCOUNTS: &str = "github.com\n  \u{2713} Logged in to github.com account xRookieFight (keyring)\n  - Active account: true\n  - Git operations protocol: https\n  - Token: gho_************************\n  \u{2713} Logged in to github.com account work-bot (keyring)\n  - Active account: false\n  - Token: gho_************************\n";

    #[test]
    fn parses_multiple_accounts_and_the_active_flag() {
        let accounts = parse_status(TWO_ACCOUNTS);
        assert_eq!(accounts.len(), 2);
        assert_eq!(accounts[0].login, "xRookieFight");
        assert!(accounts[0].active);
        assert_eq!(accounts[1].login, "work-bot");
        assert!(!accounts[1].active);
        assert!(accounts.iter().all(|a| a.host == DEFAULT_HOST));
    }

    #[test]
    fn parses_enterprise_hosts() {
        let text = gh_status_output("github.example.com", &[("octocat", true)]);
        let accounts = parse_status(&text);
        assert_eq!(accounts[0].host, "github.example.com");
    }

    #[test]
    fn a_lone_account_is_treated_as_active() {
        let text = "github.com\n  \u{2713} Logged in to github.com account solo (keyring)\n  - Git operations protocol: https\n";
        assert!(parse_status(text)[0].active);
    }

    #[test]
    fn tokens_are_never_returned_by_the_parser() {
        let accounts = parse_status(TWO_ACCOUNTS);
        assert!(!format!("{accounts:?}").contains("gho_"));
    }

    #[test]
    fn logged_out_cli_reports_no_accounts() {
        let runner = MockRunner::new().status(
            "gh auth status",
            1,
            "",
            "You are not logged into any GitHub hosts. To log in, run: gh auth login",
        );
        assert!(Gh::new(&runner).accounts().unwrap().is_empty());
    }

    #[test]
    fn unexpected_status_failures_are_errors() {
        let runner = MockRunner::new().status("gh auth status", 1, "", "connection refused");
        let err = Gh::new(&runner).accounts().unwrap_err();
        assert!(err.to_string().contains("connection refused"));
    }

    #[test]
    fn active_login_is_reported_per_host() {
        let runner = MockRunner::new().ok("gh auth status", TWO_ACCOUNTS);
        assert_eq!(
            Gh::new(&runner)
                .active_login(DEFAULT_HOST)
                .unwrap()
                .as_deref(),
            Some("xRookieFight")
        );
    }

    #[test]
    fn switch_passes_host_and_user() {
        let runner = MockRunner::new().ok("gh auth switch", "");
        Gh::new(&runner).switch(DEFAULT_HOST, "work-bot").unwrap();
        assert!(runner.was_called("gh auth switch --hostname github.com --user work-bot"));
    }

    #[test]
    fn switch_failures_are_actionable() {
        let runner = MockRunner::new().status(
            "gh auth switch",
            1,
            "",
            "no account named work-bot is logged in",
        );
        let err = Gh::new(&runner)
            .switch(DEFAULT_HOST, "work-bot")
            .unwrap_err();
        assert!(err.to_string().contains("no account named work-bot"));
    }

    #[test]
    fn token_login_uses_stdin_and_never_argv() {
        let runner = MockRunner::new().ok("gh auth login", "");
        Gh::new(&runner)
            .login_with_token(DEFAULT_HOST, "ghp_EXAMPLE0123456789abc")
            .unwrap();
        let command = &runner.calls()[0];
        assert!(!command.contains("ghp_"));
        assert!(command.contains("--with-token"));
        assert_eq!(
            runner.stdin_for("gh auth login").as_deref(),
            Some("ghp_EXAMPLE0123456789abc")
        );
    }

    #[test]
    fn token_login_errors_are_redacted() {
        let runner = MockRunner::new().status(
            "gh auth login",
            1,
            "",
            "error validating token ghp_EXAMPLE0123456789abc: bad credentials",
        );
        let err = Gh::new(&runner)
            .login_with_token(DEFAULT_HOST, "ghp_EXAMPLE0123456789abc")
            .unwrap_err();
        let message = err.to_string();
        assert!(!message.contains("ghp_EXAMPLE0123456789abc"));
        assert!(message.contains("[redacted]"));
        assert!(message.contains("bad credentials"));
    }

    #[test]
    fn missing_cli_is_reported_as_a_dependency_problem() {
        let runner = MockRunner::new().missing("gh");
        let err = Gh::new(&runner).accounts().unwrap_err();
        assert!(matches!(err, Error::MissingDependency("gh")));
        assert!(err.hint().unwrap().contains("cli.github.com"));
    }

    #[test]
    fn logout_tolerates_an_already_logged_out_account() {
        let runner =
            MockRunner::new().status("gh auth logout", 1, "", "not logged in to any hosts");
        assert!(Gh::new(&runner).logout(DEFAULT_HOST, "work-bot").is_ok());
    }
}
