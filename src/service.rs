use crate::error::{Error, Result};
use crate::gh::Gh;
use crate::git::{Git, Identity, Scope};
use crate::model::Account;
use crate::process::Runner;
use crate::secrets::SecretStore;
use crate::store::Store;

/// What the tool did to the GitHub CLI while switching.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhOutcome {
    /// `gh` already had the account selected.
    AlreadyActive,
    /// `gh auth switch` was used.
    Switched,
    /// The account was re-authenticated from the token in the OS credential store.
    ReAuthenticated,
    /// `gh` is unusable; only the git identity was updated.
    Skipped(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwitchReport {
    pub account: Account,
    pub scope: Scope,
    pub gh: GhOutcome,
    pub verified: bool,
    pub warnings: Vec<String>,
}

/// A snapshot of everything the main screen shows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Status {
    pub git_installed: bool,
    pub gh_installed: bool,
    pub git_version: Option<String>,
    pub gh_version: Option<String>,
    pub identity: Identity,
    pub active_account: Option<Account>,
    pub gh_login: Option<String>,
    pub problems: Vec<String>,
}

impl Status {
    /// True when git and gh disagree about who the user is.
    pub fn is_consistent(&self) -> bool {
        match (&self.active_account, &self.gh_login) {
            (Some(account), Some(login)) => account.username.eq_ignore_ascii_case(login),
            (Some(_), None) => false,
            _ => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SwitchOptions {
    pub scope: Scope,
    /// Re-read `gh auth status` afterwards to confirm the switch took effect.
    pub verify: bool,
}

impl Default for SwitchOptions {
    fn default() -> Self {
        Self {
            scope: Scope::Global,
            verify: true,
        }
    }
}

/// Coordinates the store, git and the GitHub CLI. All behaviour that the CLI
/// and the TUI share lives here.
pub struct Service<'a> {
    runner: &'a dyn Runner,
    secrets: &'a dyn SecretStore,
    store: Store,
}

impl<'a> Service<'a> {
    pub fn new(runner: &'a dyn Runner, secrets: &'a dyn SecretStore, store: Store) -> Self {
        Self {
            runner,
            secrets,
            store,
        }
    }

    pub fn store(&self) -> &Store {
        &self.store
    }

    pub fn store_mut(&mut self) -> &mut Store {
        &mut self.store
    }

    pub fn git(&self) -> Git<'a> {
        Git::new(self.runner)
    }

    pub fn gh(&self) -> Gh<'a> {
        Gh::new(self.runner)
    }

    pub fn secrets(&self) -> &'a dyn SecretStore {
        self.secrets
    }

    /// Collects the current state without changing anything.
    pub fn status(&self) -> Status {
        let git = self.git();
        let gh = self.gh();
        let mut problems = Vec::new();

        let git_installed = git.is_installed();
        let gh_installed = gh.is_installed();
        if !git_installed {
            problems.push("git is not installed or not on PATH".into());
        }
        if !gh_installed {
            problems.push("GitHub CLI (gh) is not installed or not on PATH".into());
        }

        let git_version = git_installed.then(|| git.version().ok()).flatten();
        let gh_version = gh_installed.then(|| gh.version().ok()).flatten();

        let identity = if git_installed {
            git.identity().unwrap_or_else(|err| {
                problems.push(err.to_string());
                Identity::default()
            })
        } else {
            Identity::default()
        };

        let active_account = self.store.active().cloned();
        let host = active_account
            .as_ref()
            .map(|a| a.host.clone())
            .unwrap_or_else(|| crate::model::DEFAULT_HOST.to_string());

        let gh_login = if gh_installed {
            match gh.active_login(&host) {
                Ok(login) => login,
                Err(err) => {
                    problems.push(err.to_string());
                    None
                }
            }
        } else {
            None
        };

        Status {
            git_installed,
            gh_installed,
            git_version,
            gh_version,
            identity,
            active_account,
            gh_login,
            problems,
        }
    }

    /// Saves a profile and, when a token is supplied, authenticates `gh` with it.
    ///
    /// The token is written straight to the OS credential store and piped to
    /// `gh` on stdin; it is never persisted in gitswitch's own config file.
    pub fn add_account(&mut self, account: Account, token: Option<&str>) -> Result<Account> {
        account.validate()?;
        if self.store.get(&account.name).is_some() {
            return Err(Error::DuplicateAccount(account.name));
        }

        let mut account = account;
        if let Some(token) = token {
            crate::model::validate_token(token)?;
            let gh = self.gh();
            if !gh.is_installed() {
                return Err(Error::MissingDependency("gh"));
            }
            gh.login_with_token(&account.host, token)?;
            match self.secrets.set(&account.secret_key(), token.trim()) {
                Ok(()) => account.has_stored_token = true,
                // Losing the cache is not fatal: gh already holds the session.
                Err(Error::Secret(_)) => account.has_stored_token = false,
                Err(err) => return Err(err),
            }
        }

        self.store.add(account.clone())?;
        Ok(account)
    }

    /// Forgets a profile, its cached token and optionally its `gh` session.
    pub fn remove_account(&mut self, name: &str, logout: bool) -> Result<Account> {
        let account = self.store.require(name)?.clone();
        let removed = self.store.remove(&account.name)?;
        let _ = self.secrets.delete(&account.secret_key());
        if logout && self.gh().is_installed() {
            self.gh().logout(&account.host, &account.username)?;
        }
        Ok(removed)
    }

    pub fn rename_account(&mut self, from: &str, to: &str) -> Result<()> {
        self.store.rename(from, to)
    }

    /// Stores a fresh token for an existing profile and logs `gh` back in.
    pub fn reauthenticate(&mut self, name: &str, token: &str) -> Result<()> {
        crate::model::validate_token(token)?;
        let mut account = self.store.require(name)?.clone();
        let gh = self.gh();
        if !gh.is_installed() {
            return Err(Error::MissingDependency("gh"));
        }
        gh.login_with_token(&account.host, token)?;
        account.has_stored_token = self
            .secrets
            .set(&account.secret_key(), token.trim())
            .is_ok();
        self.store.update(account)
    }

    /// Points git and `gh` at the named profile.
    pub fn switch(&mut self, name: &str, options: SwitchOptions) -> Result<SwitchReport> {
        let account = self.store.require(name)?.clone();
        let mut warnings = Vec::new();

        let git = self.git();
        if !git.is_installed() {
            return Err(Error::MissingDependency("git"));
        }
        if options.scope == Scope::Local && !git.is_inside_repository() {
            return Err(Error::InvalidInput(
                "the current directory is not inside a git repository".into(),
            ));
        }
        git.set_identity(options.scope, &account.git_name, &account.git_email)?;

        let gh = self.gh();
        let outcome = if !gh.is_installed() {
            GhOutcome::Skipped("GitHub CLI (gh) is not installed".into())
        } else {
            self.align_gh(&account)?
        };

        if matches!(
            outcome,
            GhOutcome::Switched | GhOutcome::ReAuthenticated | GhOutcome::AlreadyActive
        ) && let Err(err) = gh.setup_git(&account.host)
        {
            warnings.push(format!("git credential helper was not updated: {err}"));
        }

        let verified = if options.verify && !matches!(outcome, GhOutcome::Skipped(_)) {
            let found = gh.active_login(&account.host)?.unwrap_or_default();
            if !found.eq_ignore_ascii_case(&account.username) {
                return Err(Error::VerificationFailed {
                    expected: account.username.clone(),
                    found: if found.is_empty() {
                        "nobody".into()
                    } else {
                        found
                    },
                });
            }
            true
        } else {
            false
        };

        if let GhOutcome::Skipped(reason) = &outcome {
            warnings.push(format!("{reason}; only the git identity was updated"));
        }

        self.store.set_active(&account.name)?;

        Ok(SwitchReport {
            account,
            scope: options.scope,
            gh: outcome,
            verified,
            warnings,
        })
    }

    fn align_gh(&self, account: &Account) -> Result<GhOutcome> {
        let gh = self.gh();
        let accounts = gh.accounts_for(&account.host)?;

        if accounts
            .iter()
            .any(|entry| entry.active && entry.login.eq_ignore_ascii_case(&account.username))
        {
            return Ok(GhOutcome::AlreadyActive);
        }

        if accounts
            .iter()
            .any(|entry| entry.login.eq_ignore_ascii_case(&account.username))
        {
            gh.switch(&account.host, &account.username)?;
            return Ok(GhOutcome::Switched);
        }

        if let Some(token) = self.secrets.get(&account.secret_key())? {
            gh.login_with_token(&account.host, &token)?;
            return Ok(GhOutcome::ReAuthenticated);
        }

        Err(Error::NotAuthenticated(account.host.clone()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemoryStore;
    use crate::testing::{MockRunner, gh_status_output};
    use tempfile::TempDir;

    struct Harness {
        _dir: TempDir,
        runner: MockRunner,
        secrets: MemoryStore,
        store: Option<Store>,
    }

    impl Harness {
        fn new(runner: MockRunner) -> Self {
            let dir = TempDir::new().unwrap();
            let store = Store::open(dir.path().join("accounts.json")).unwrap();
            Self {
                _dir: dir,
                runner,
                secrets: MemoryStore::default(),
                store: Some(store),
            }
        }

        fn service(&mut self) -> Service<'_> {
            Service::new(&self.runner, &self.secrets, self.store.take().unwrap())
        }
    }

    fn account(name: &str, username: &str) -> Account {
        Account::new(
            name,
            username,
            "Octo Cat",
            format!("{username}@users.noreply.github.com"),
        )
    }

    fn base_runner() -> MockRunner {
        MockRunner::new()
            .ok("git config", "")
            .ok("gh auth setup-git", "")
    }

    #[test]
    fn switching_uses_gh_auth_switch_when_the_account_is_known() {
        let statuses = gh_status_output("github.com", &[("octocat", false), ("work-bot", true)]);
        let after = gh_status_output("github.com", &[("octocat", true), ("work-bot", false)]);
        let runner = base_runner()
            .ok("gh auth status", &statuses)
            .ok("gh auth status", &after)
            .ok("gh auth switch", "");

        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let report = service
            .switch("personal", SwitchOptions::default())
            .unwrap();
        assert_eq!(report.gh, GhOutcome::Switched);
        assert!(report.verified);
        assert!(report.warnings.is_empty());
        assert!(
            harness
                .runner
                .was_called("gh auth switch --hostname github.com --user octocat")
        );
        assert!(
            harness
                .runner
                .was_called("git config --global user.name Octo Cat")
        );
    }

    #[test]
    fn switching_to_the_active_account_is_a_no_op_for_gh() {
        let status = gh_status_output("github.com", &[("octocat", true)]);
        let runner = base_runner().ok("gh auth status", &status);

        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let report = service
            .switch("personal", SwitchOptions::default())
            .unwrap();
        assert_eq!(report.gh, GhOutcome::AlreadyActive);
        assert!(!harness.runner.was_called("gh auth switch"));
    }

    #[test]
    fn an_unknown_account_is_reauthenticated_from_the_credential_store() {
        let empty = "You are not logged into any GitHub hosts";
        let after = gh_status_output("github.com", &[("octocat", true)]);
        let runner = base_runner()
            .status("gh auth status", 1, "", empty)
            .ok("gh auth status", &after)
            .ok("gh auth login", "");

        let mut harness = Harness::new(runner);
        harness
            .secrets
            .set("github.com:octocat", "ghp_EXAMPLE0123456789abc")
            .unwrap();
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let report = service
            .switch("personal", SwitchOptions::default())
            .unwrap();
        assert_eq!(report.gh, GhOutcome::ReAuthenticated);
        assert_eq!(
            harness.runner.stdin_for("gh auth login").as_deref(),
            Some("ghp_EXAMPLE0123456789abc")
        );
    }

    #[test]
    fn switching_without_any_credentials_asks_the_user_to_authenticate() {
        let runner = base_runner().status("gh auth status", 1, "", "not logged in");

        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let err = service
            .switch("personal", SwitchOptions::default())
            .unwrap_err();
        assert!(matches!(err, Error::NotAuthenticated(_)));
    }

    #[test]
    fn a_switch_that_does_not_take_effect_is_reported() {
        let before = gh_status_output("github.com", &[("octocat", false), ("work-bot", true)]);
        let runner = base_runner()
            .ok("gh auth status", &before)
            .ok("gh auth status", &before)
            .ok("gh auth switch", "");

        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let err = service
            .switch("personal", SwitchOptions::default())
            .unwrap_err();
        match err {
            Error::VerificationFailed { expected, found } => {
                assert_eq!(expected, "octocat");
                assert_eq!(found, "work-bot");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn missing_gh_still_updates_the_git_identity() {
        let runner = MockRunner::new().ok("git config", "").missing("gh");

        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let report = service
            .switch("personal", SwitchOptions::default())
            .unwrap();
        assert!(matches!(report.gh, GhOutcome::Skipped(_)));
        assert_eq!(report.warnings.len(), 1);
        assert_eq!(
            service.store().active().map(|a| a.name.clone()),
            Some("personal".to_string())
        );
        drop(service);
        assert!(harness.runner.was_called("git config --global user.email"));
    }

    #[test]
    fn switching_an_unknown_profile_fails_before_touching_git() {
        let runner = MockRunner::new();
        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        let err = service
            .switch("ghost", SwitchOptions::default())
            .unwrap_err();
        assert!(matches!(err, Error::UnknownAccount(_)));
        assert!(harness.runner.calls().is_empty());
    }

    #[test]
    fn local_scope_requires_a_repository() {
        let runner = MockRunner::new().status(
            "git rev-parse --is-inside-work-tree",
            128,
            "",
            "fatal: not a git repository",
        );
        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let err = service
            .switch(
                "personal",
                SwitchOptions {
                    scope: Scope::Local,
                    verify: false,
                },
            )
            .unwrap_err();
        assert!(err.to_string().contains("not inside a git repository"));
    }

    #[test]
    fn adding_with_a_token_authenticates_and_caches_it() {
        let runner = MockRunner::new().ok("gh auth login", "");
        let mut harness = Harness::new(runner);
        let mut service = harness.service();

        let saved = service
            .add_account(
                account("work", "work-bot"),
                Some("ghp_EXAMPLE0123456789abc"),
            )
            .unwrap();
        assert!(saved.has_stored_token);
        assert_eq!(service.store().accounts().len(), 1);
        assert_eq!(
            harness
                .secrets
                .get("github.com:work-bot")
                .unwrap()
                .as_deref(),
            Some("ghp_EXAMPLE0123456789abc")
        );
    }

    #[test]
    fn a_rejected_token_does_not_save_the_account() {
        let runner = MockRunner::new().status("gh auth login", 1, "", "bad credentials");
        let mut harness = Harness::new(runner);
        let mut service = harness.service();

        assert!(
            service
                .add_account(
                    account("work", "work-bot"),
                    Some("ghp_EXAMPLE0123456789abc")
                )
                .is_err()
        );
        assert!(service.store().is_empty());
        assert!(
            harness
                .secrets
                .get("github.com:work-bot")
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn removing_an_account_clears_its_cached_token() {
        let runner = MockRunner::new().ok("gh auth logout", "");
        let mut harness = Harness::new(runner);
        harness.secrets.set("github.com:octocat", "ghp_x").unwrap();
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        service.remove_account("personal", true).unwrap();
        assert!(service.store().is_empty());
        assert!(harness.secrets.get("github.com:octocat").unwrap().is_none());
        assert!(harness.runner.was_called("gh auth logout"));
    }

    #[test]
    fn status_flags_a_mismatch_between_git_and_gh() {
        let runner = MockRunner::new()
            .ok("git --version", "git version 2.55.0")
            .ok("gh --version", "gh version 2.97.0")
            .ok("git config --get user.name", "Octo Cat")
            .ok("git config --get user.email", "octo@example.com")
            .ok(
                "gh auth status",
                &gh_status_output("github.com", &[("work-bot", true)]),
            );

        let mut harness = Harness::new(runner);
        let mut service = harness.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        service.store_mut().set_active("personal").unwrap();

        let status = service.status();
        assert!(status.git_installed && status.gh_installed);
        assert_eq!(status.gh_login.as_deref(), Some("work-bot"));
        assert!(!status.is_consistent());
    }

    #[test]
    fn status_reports_missing_dependencies_as_problems() {
        let runner = MockRunner::new().missing("git").missing("gh");
        let mut harness = Harness::new(runner);
        let service = harness.service();
        let status = service.status();
        assert_eq!(status.problems.len(), 2);
        assert!(status.problems.iter().any(|p| p.contains("git")));
        assert!(status.problems.iter().any(|p| p.contains("gh")));
    }
}
