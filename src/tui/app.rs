use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::error::Error;
use crate::git::Scope;
use crate::model::{Account, DEFAULT_HOST};
use crate::service::{GhOutcome, Service, Status, SwitchOptions};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Success,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Toast {
    pub level: Level,
    pub text: String,
    /// Optional second line with a suggested next step.
    pub hint: Option<String>,
}

impl Toast {
    fn new(level: Level, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
            hint: None,
        }
    }

    fn from_error(err: &Error) -> Self {
        Self {
            level: Level::Error,
            text: err.to_string(),
            hint: err.hint(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FormKind {
    Add,
    Rename,
    Token,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Field {
    pub label: &'static str,
    pub value: String,
    pub masked: bool,
    pub hint: &'static str,
}

impl Field {
    fn new(label: &'static str, hint: &'static str) -> Self {
        Self {
            label,
            value: String::new(),
            masked: false,
            hint,
        }
    }

    fn with_value(mut self, value: &str) -> Self {
        self.value = value.to_string();
        self
    }

    fn masked(mut self) -> Self {
        self.masked = true;
        self
    }

    /// What the widget prints: secrets are never echoed back.
    pub fn display(&self) -> String {
        if self.masked {
            "\u{2022}".repeat(self.value.chars().count())
        } else {
            self.value.clone()
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Form {
    pub kind: FormKind,
    pub title: String,
    pub fields: Vec<Field>,
    pub cursor: usize,
    /// Account the form acts on, for rename and re-authentication.
    pub target: Option<String>,
}

impl Form {
    fn add() -> Self {
        Self {
            kind: FormKind::Add,
            title: "Add account".into(),
            fields: vec![
                Field::new("Profile name", "personal, work, open-source"),
                Field::new("GitHub username", "your GitHub login"),
                Field::new("Git name", "shown on your commits"),
                Field::new("Git email", "used for commit authorship"),
                Field::new("Host", "github.com or a GitHub Enterprise host")
                    .with_value(DEFAULT_HOST),
                Field::new("Token (optional)", "paste a PAT, or leave empty").masked(),
            ],
            cursor: 0,
            target: None,
        }
    }

    fn rename(current: &str) -> Self {
        Self {
            kind: FormKind::Rename,
            title: format!("Rename `{current}`"),
            fields: vec![Field::new("New name", "letters, digits, - _ .").with_value(current)],
            cursor: 0,
            target: Some(current.to_string()),
        }
    }

    fn token(account: &str) -> Self {
        Self {
            kind: FormKind::Token,
            title: format!("Re-authenticate `{account}`"),
            fields: vec![Field::new("Token", "a personal access token").masked()],
            cursor: 0,
            target: Some(account.to_string()),
        }
    }

    fn value(&self, index: usize) -> &str {
        self.fields
            .get(index)
            .map(|field| field.value.trim())
            .unwrap_or_default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Confirm {
    pub title: String,
    pub message: String,
    pub account: String,
    /// Whether `gh auth logout` runs as part of the removal.
    pub logout: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Onboarding,
    Accounts,
    Form(Form),
    Confirm(Confirm),
    Help,
}

/// Work that is slow enough to deserve a visible "working" frame. The event
/// loop paints first and executes afterwards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Pending {
    Switch(String),
    Refresh,
    Submit,
    Remove,
}

impl Pending {
    pub fn label(&self) -> &'static str {
        match self {
            Pending::Switch(_) => "Switching account",
            Pending::Refresh => "Refreshing",
            Pending::Submit => "Talking to GitHub CLI",
            Pending::Remove => "Removing account",
        }
    }
}

/// Side effects the event loop must perform outside the alternate screen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    None,
    Quit,
    /// Hand the terminal to `gh auth login` for the given host.
    InteractiveLogin(String),
}

pub struct App<'s, 'r> {
    service: &'s mut Service<'r>,
    pub screen: Screen,
    pub status: Status,
    pub selected: usize,
    pub scope: Scope,
    pub toast: Option<Toast>,
    pub pending: Option<Pending>,
    pub busy: Option<String>,
}

impl<'s, 'r> App<'s, 'r> {
    pub fn new(service: &'s mut Service<'r>) -> Self {
        let status = service.status();
        let screen = if service.store().is_empty() {
            Screen::Onboarding
        } else {
            Screen::Accounts
        };
        let mut app = Self {
            service,
            screen,
            status,
            selected: 0,
            scope: Scope::Global,
            toast: None,
            pending: None,
            busy: None,
        };
        app.select_active();
        app
    }

    /// Starts directly on the add form, used by `gitswitch add`.
    pub fn new_with_add_form(service: &'s mut Service<'r>) -> Self {
        let mut app = Self::new(service);
        app.screen = Screen::Form(Form::add());
        app
    }

    pub fn accounts(&self) -> &[Account] {
        self.service.store().accounts()
    }

    pub fn selected_account(&self) -> Option<&Account> {
        self.accounts().get(self.selected)
    }

    pub fn active_name(&self) -> Option<&str> {
        self.status.active_account.as_ref().map(|a| a.name.as_str())
    }

    pub fn config_path(&self) -> String {
        self.service.store().path().display().to_string()
    }

    pub fn keychain_available(&self) -> bool {
        self.service.secrets().available()
    }

    fn select_active(&mut self) {
        if let Some(active) = self.status.active_account.as_ref()
            && let Some(index) = self.accounts().iter().position(|a| a.name == active.name)
        {
            self.selected = index;
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.accounts().len();
        if len == 0 {
            self.selected = 0;
        } else if self.selected >= len {
            self.selected = len - 1;
        }
    }

    pub fn refresh(&mut self) {
        self.status = self.service.status();
        self.clamp_selection();
        if self.service.store().is_empty() && matches!(self.screen, Screen::Accounts) {
            self.screen = Screen::Onboarding;
        }
    }

    /// Routes a key press. Slow work is queued in [`App::pending`] so the event
    /// loop can render a loading frame first.
    pub fn on_key(&mut self, key: KeyEvent) -> Effect {
        if key.kind == KeyEventKind::Release {
            return Effect::None;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
            return Effect::Quit;
        }

        self.toast = None;

        match &self.screen {
            Screen::Help => {
                self.screen = if self.service.store().is_empty() {
                    Screen::Onboarding
                } else {
                    Screen::Accounts
                };
                Effect::None
            }
            Screen::Onboarding => self.on_onboarding_key(key),
            Screen::Accounts => self.on_accounts_key(key),
            Screen::Form(_) => self.on_form_key(key),
            Screen::Confirm(_) => self.on_confirm_key(key),
        }
    }

    fn on_onboarding_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => Effect::Quit,
            KeyCode::Char('?') => {
                self.screen = Screen::Help;
                Effect::None
            }
            KeyCode::Char('a') | KeyCode::Enter => {
                self.screen = Screen::Form(Form::add());
                Effect::None
            }
            _ => Effect::None,
        }
    }

    fn on_accounts_key(&mut self, key: KeyEvent) -> Effect {
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => return Effect::Quit,
            KeyCode::Up | KeyCode::Char('k') => {
                self.selected = self.selected.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let len = self.accounts().len();
                if len > 0 {
                    self.selected = (self.selected + 1).min(len - 1);
                }
            }
            KeyCode::Home => self.selected = 0,
            KeyCode::End => self.clamp_selection_to_end(),
            KeyCode::Enter => {
                if let Some(account) = self.selected_account() {
                    self.pending = Some(Pending::Switch(account.name.clone()));
                }
            }
            KeyCode::Char(digit @ '1'..='9') => {
                let index = digit as usize - '1' as usize;
                if index < self.accounts().len() {
                    self.selected = index;
                    let name = self.accounts()[index].name.clone();
                    self.pending = Some(Pending::Switch(name));
                }
            }
            KeyCode::Char('a') => self.screen = Screen::Form(Form::add()),
            KeyCode::Char('r') => {
                if let Some(account) = self.selected_account() {
                    self.screen = Screen::Form(Form::rename(&account.name));
                }
            }
            KeyCode::Char('t') => {
                if let Some(account) = self.selected_account() {
                    self.screen = Screen::Form(Form::token(&account.name));
                }
            }
            KeyCode::Char('A') => {
                if let Some(account) = self.selected_account() {
                    return Effect::InteractiveLogin(account.host.clone());
                }
            }
            KeyCode::Char('d') | KeyCode::Delete | KeyCode::Backspace => {
                if let Some(account) = self.selected_account() {
                    self.screen = Screen::Confirm(Confirm {
                        title: "Remove account".into(),
                        message: format!(
                            "Remove `{}` ({})? This deletes the saved profile and its cached token.",
                            account.name, account.username
                        ),
                        account: account.name.clone(),
                        logout: false,
                    });
                }
            }
            KeyCode::Char('L') => {
                self.scope = match self.scope {
                    Scope::Global => Scope::Local,
                    Scope::Local => Scope::Global,
                };
                self.toast = Some(Toast::new(
                    Level::Info,
                    format!(
                        "Git identity will be written to the {} config",
                        self.scope.label()
                    ),
                ));
            }
            KeyCode::Char('g') | KeyCode::F(5) => self.pending = Some(Pending::Refresh),
            KeyCode::Char('?') => self.screen = Screen::Help,
            _ => {}
        }
        Effect::None
    }

    fn clamp_selection_to_end(&mut self) {
        let len = self.accounts().len();
        self.selected = len.saturating_sub(1);
    }

    fn on_form_key(&mut self, key: KeyEvent) -> Effect {
        let Screen::Form(form) = &mut self.screen else {
            return Effect::None;
        };

        match key.code {
            KeyCode::Esc => {
                self.screen = self.default_screen();
            }
            KeyCode::Tab | KeyCode::Down => {
                form.cursor = (form.cursor + 1) % form.fields.len();
            }
            KeyCode::BackTab | KeyCode::Up => {
                form.cursor = (form.cursor + form.fields.len() - 1) % form.fields.len();
            }
            KeyCode::Backspace => {
                form.fields[form.cursor].value.pop();
            }
            KeyCode::Char(c) if key.modifiers.contains(KeyModifiers::CONTROL) => {
                // Ctrl-U clears the focused field, the usual terminal shortcut.
                if c == 'u' {
                    form.fields[form.cursor].value.clear();
                }
            }
            KeyCode::Char(c) => {
                form.fields[form.cursor].value.push(c);
            }
            KeyCode::Enter => {
                if form.cursor + 1 < form.fields.len() {
                    form.cursor += 1;
                } else {
                    self.pending = Some(Pending::Submit);
                }
            }
            _ => {}
        }
        Effect::None
    }

    fn on_confirm_key(&mut self, key: KeyEvent) -> Effect {
        let Screen::Confirm(confirm) = &mut self.screen else {
            return Effect::None;
        };
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => self.pending = Some(Pending::Remove),
            KeyCode::Char('l') | KeyCode::Char('L') => confirm.logout = !confirm.logout,
            KeyCode::Esc | KeyCode::Char('n') | KeyCode::Char('N') => {
                self.screen = self.default_screen();
                self.toast = Some(Toast::new(Level::Info, "Cancelled"));
            }
            _ => {}
        }
        Effect::None
    }

    fn default_screen(&self) -> Screen {
        if self.service.store().is_empty() {
            Screen::Onboarding
        } else {
            Screen::Accounts
        }
    }

    /// Executes the queued operation. Called after a loading frame is drawn.
    pub fn run_pending(&mut self) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        self.busy = None;

        match pending {
            Pending::Refresh => {
                self.refresh();
                self.toast = Some(Toast::new(Level::Info, "Refreshed"));
            }
            Pending::Switch(name) => self.do_switch(&name),
            Pending::Submit => self.submit_form(),
            Pending::Remove => self.do_remove(),
        }
    }

    fn do_switch(&mut self, name: &str) {
        let options = SwitchOptions {
            scope: self.scope,
            verify: true,
        };
        match self.service.switch(name, options) {
            Ok(report) => {
                let detail = match &report.gh {
                    GhOutcome::AlreadyActive => "gh was already on this account".to_string(),
                    GhOutcome::Switched => "gh switched too".to_string(),
                    GhOutcome::ReAuthenticated => "gh re-authenticated".to_string(),
                    GhOutcome::Skipped(reason) => reason.clone(),
                };
                let level = if report.warnings.is_empty() {
                    Level::Success
                } else {
                    Level::Warning
                };
                let mut toast = Toast::new(
                    level,
                    format!(
                        "Now using {} ({}) - {detail}",
                        report.account.name, report.account.username
                    ),
                );
                toast.hint = report.warnings.first().cloned();
                self.toast = Some(toast);
            }
            Err(err) => self.toast = Some(Toast::from_error(&err)),
        }
        self.refresh();
    }

    fn submit_form(&mut self) {
        let Screen::Form(form) = self.screen.clone() else {
            return;
        };

        let result = match form.kind {
            FormKind::Add => self.submit_add(&form),
            FormKind::Rename => self.submit_rename(&form),
            FormKind::Token => self.submit_token(&form),
        };

        match result {
            Ok(toast) => {
                self.screen = self.default_screen();
                self.refresh();
                if let Some(index) = form
                    .target
                    .as_deref()
                    .or(Some(form.value(0)))
                    .and_then(|name| self.accounts().iter().position(|a| a.name == name))
                {
                    self.selected = index;
                }
                self.toast = Some(toast);
            }
            Err(err) => self.toast = Some(Toast::from_error(&err)),
        }
    }

    fn submit_add(&mut self, form: &Form) -> crate::Result<Toast> {
        let host = if form.value(4).is_empty() {
            DEFAULT_HOST.to_string()
        } else {
            form.value(4).to_string()
        };
        let account = Account::new(form.value(0), form.value(1), form.value(2), form.value(3))
            .with_host(host);
        let token = form.value(5);
        let token = (!token.is_empty()).then_some(token);

        let saved = self.service.add_account(account, token)?;
        Ok(if saved.has_stored_token {
            Toast::new(
                Level::Success,
                format!("Saved `{}` and authenticated gh", saved.name),
            )
        } else if token.is_some() {
            let mut toast = Toast::new(
                Level::Warning,
                format!("Saved `{}` - gh is authenticated", saved.name),
            );
            toast.hint = Some("No OS credential store found, so the token was not cached".into());
            toast
        } else {
            let mut toast = Toast::new(Level::Success, format!("Saved `{}`", saved.name));
            toast.hint = Some("Press A to authenticate it with gh, or t to paste a token".into());
            toast
        })
    }

    fn submit_rename(&mut self, form: &Form) -> crate::Result<Toast> {
        let from = form.target.clone().unwrap_or_default();
        let to = form.value(0).to_string();
        self.service.rename_account(&from, &to)?;
        Ok(Toast::new(
            Level::Success,
            format!("Renamed `{from}` to `{to}`"),
        ))
    }

    fn submit_token(&mut self, form: &Form) -> crate::Result<Toast> {
        let name = form.target.clone().unwrap_or_default();
        self.service.reauthenticate(&name, form.value(0))?;
        Ok(Toast::new(
            Level::Success,
            format!("`{name}` re-authenticated"),
        ))
    }

    fn do_remove(&mut self) {
        let Screen::Confirm(confirm) = self.screen.clone() else {
            return;
        };
        match self
            .service
            .remove_account(&confirm.account, confirm.logout)
        {
            Ok(account) => {
                self.toast = Some(Toast::new(
                    Level::Success,
                    format!("Removed `{}`", account.name),
                ));
            }
            Err(err) => self.toast = Some(Toast::from_error(&err)),
        }
        self.screen = self.default_screen();
        self.refresh();
    }

    /// Called after an interactive `gh auth login` returns.
    pub fn after_interactive_login(&mut self, result: crate::Result<()>) {
        match result {
            Ok(()) => {
                self.refresh();
                self.toast = Some(Toast::new(
                    Level::Success,
                    "GitHub CLI authentication finished",
                ));
            }
            Err(err) => self.toast = Some(Toast::from_error(&err)),
        }
    }

    pub fn login_interactively(&self, host: &str) -> crate::Result<()> {
        self.service.gh().login_interactive(host)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::secrets::memory::MemoryStore;
    use crate::store::Store;
    use crate::testing::{MockRunner, gh_status_output};
    use tempfile::TempDir;

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn type_text(app: &mut App<'_, '_>, text: &str) {
        for ch in text.chars() {
            app.on_key(key(KeyCode::Char(ch)));
        }
    }

    struct Env {
        _dir: TempDir,
        runner: MockRunner,
        secrets: MemoryStore,
        store: Option<Store>,
    }

    impl Env {
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

    fn account(name: &str, username: &str) -> Account {
        Account::new(name, username, "Octo Cat", "octo@example.com")
    }

    #[test]
    fn an_empty_store_opens_the_onboarding_screen() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        let app = App::new(&mut service);
        assert_eq!(app.screen, Screen::Onboarding);
    }

    #[test]
    fn onboarding_leads_into_the_add_form() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        let mut app = App::new(&mut service);
        app.on_key(key(KeyCode::Char('a')));
        assert!(matches!(&app.screen, Screen::Form(form) if form.kind == FormKind::Add));
    }

    #[test]
    fn the_list_opens_on_the_active_account() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        service
            .store_mut()
            .add(account("work", "work-bot"))
            .unwrap();
        service.store_mut().set_active("work").unwrap();

        let app = App::new(&mut service);
        assert_eq!(app.screen, Screen::Accounts);
        assert_eq!(app.selected_account().unwrap().name, "work");
    }

    #[test]
    fn arrow_keys_move_the_selection_without_wrapping() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        service
            .store_mut()
            .add(account("work", "work-bot"))
            .unwrap();

        let mut app = App::new(&mut service);
        app.on_key(key(KeyCode::Up));
        assert_eq!(app.selected, 0);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.selected, 1);
        app.on_key(key(KeyCode::Down));
        assert_eq!(app.selected, 1);
    }

    #[test]
    fn enter_switches_the_selected_account() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let mut app = App::new(&mut service);
        app.on_key(key(KeyCode::Enter));
        assert_eq!(app.pending, Some(Pending::Switch("personal".into())));
        app.run_pending();

        assert_eq!(app.toast.as_ref().unwrap().level, Level::Success);
        assert_eq!(app.active_name(), Some("personal"));
    }

    #[test]
    fn a_failed_switch_becomes_an_error_toast_with_a_hint() {
        let runner = MockRunner::new()
            .ok("git --version", "git version 2.55.0")
            .ok("gh --version", "gh version 2.97.0")
            .ok("git config", "")
            .status("gh auth status", 1, "", "not logged in");
        let mut env = Env::new(runner);
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();

        let mut app = App::new(&mut service);
        app.on_key(key(KeyCode::Enter));
        app.run_pending();

        let toast = app.toast.as_ref().unwrap();
        assert_eq!(toast.level, Level::Error);
        assert!(toast.hint.is_some());
        assert!(app.active_name().is_none());
    }

    #[test]
    fn number_keys_switch_directly() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        service.store_mut().add(account("work", "octocat")).unwrap();

        let mut app = App::new(&mut service);
        app.on_key(key(KeyCode::Char('2')));
        assert_eq!(app.pending, Some(Pending::Switch("work".into())));
    }

    #[test]
    fn the_add_form_saves_an_account() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        let mut app = App::new(&mut service);

        app.on_key(key(KeyCode::Char('a')));
        type_text(&mut app, "work");
        app.on_key(key(KeyCode::Tab));
        type_text(&mut app, "work-bot");
        app.on_key(key(KeyCode::Tab));
        type_text(&mut app, "Work Bot");
        app.on_key(key(KeyCode::Tab));
        type_text(&mut app, "bot@example.com");
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Enter));
        app.run_pending();

        assert_eq!(app.screen, Screen::Accounts);
        assert_eq!(app.accounts().len(), 1);
        assert_eq!(app.accounts()[0].host, DEFAULT_HOST);
        assert_eq!(app.toast.as_ref().unwrap().level, Level::Success);
    }

    #[test]
    fn invalid_form_input_keeps_the_form_open() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        let mut app = App::new(&mut service);

        app.on_key(key(KeyCode::Char('a')));
        type_text(&mut app, "work");
        app.on_key(key(KeyCode::Tab));
        type_text(&mut app, "work-bot");
        app.on_key(key(KeyCode::Tab));
        type_text(&mut app, "Work Bot");
        app.on_key(key(KeyCode::Tab));
        type_text(&mut app, "not-an-email");
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Tab));
        app.on_key(key(KeyCode::Enter));
        app.run_pending();

        assert!(matches!(app.screen, Screen::Form(_)));
        assert_eq!(app.toast.as_ref().unwrap().level, Level::Error);
        assert!(app.accounts().is_empty());
    }

    #[test]
    fn token_fields_are_never_echoed() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        let mut app = App::new(&mut service);
        app.on_key(key(KeyCode::Char('a')));

        let Screen::Form(form) = &mut app.screen else {
            panic!("expected the add form");
        };
        form.cursor = 5;
        type_text(&mut app, "ghp_EXAMPLE0123456789abc");

        let Screen::Form(form) = &app.screen else {
            unreachable!()
        };
        let field = &form.fields[5];
        assert!(field.masked);
        assert!(!field.display().contains("ghp_"));
        assert_eq!(field.display().chars().count(), field.value.chars().count());
    }

    #[test]
    fn removal_asks_for_confirmation_first() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        app.on_key(key(KeyCode::Char('d')));
        assert!(matches!(app.screen, Screen::Confirm(_)));

        app.on_key(key(KeyCode::Char('n')));
        assert_eq!(app.accounts().len(), 1);

        app.on_key(key(KeyCode::Char('d')));
        app.on_key(key(KeyCode::Char('y')));
        app.run_pending();
        assert!(app.accounts().is_empty());
        assert_eq!(app.screen, Screen::Onboarding);
    }

    #[test]
    fn renaming_updates_the_list() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        app.on_key(key(KeyCode::Char('r')));
        for _ in 0.."personal".len() {
            app.on_key(key(KeyCode::Backspace));
        }
        type_text(&mut app, "home");
        app.on_key(key(KeyCode::Enter));
        app.run_pending();

        assert_eq!(app.accounts()[0].name, "home");
    }

    #[test]
    fn the_scope_toggle_switches_between_global_and_local() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        assert_eq!(app.scope, Scope::Global);
        app.on_key(key(KeyCode::Char('L')));
        assert_eq!(app.scope, Scope::Local);
        assert_eq!(app.toast.as_ref().unwrap().level, Level::Info);
    }

    #[test]
    fn ctrl_c_and_q_quit() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        assert_eq!(
            app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Effect::Quit
        );
        assert_eq!(app.on_key(key(KeyCode::Char('q'))), Effect::Quit);
    }

    #[test]
    fn escape_closes_a_form_instead_of_quitting() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        app.on_key(key(KeyCode::Char('a')));
        assert_eq!(app.on_key(key(KeyCode::Esc)), Effect::None);
        assert_eq!(app.screen, Screen::Accounts);
    }

    #[test]
    fn help_is_reachable_and_dismissable() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        app.on_key(key(KeyCode::Char('?')));
        assert_eq!(app.screen, Screen::Help);
        app.on_key(key(KeyCode::Esc));
        assert_eq!(app.screen, Screen::Accounts);
    }

    #[test]
    fn capital_a_asks_the_loop_for_an_interactive_login() {
        let mut env = Env::new(ready_runner());
        let mut service = env.service();
        service
            .store_mut()
            .add(account("personal", "octocat"))
            .unwrap();
        let mut app = App::new(&mut service);

        assert_eq!(
            app.on_key(key(KeyCode::Char('A'))),
            Effect::InteractiveLogin("github.com".into())
        );
    }
}
