//! Renders the interface into an in-memory terminal and inspects the result.

use gitswitch::model::Account;
use gitswitch::secrets::memory::MemoryStore;
use gitswitch::service::Service;
use gitswitch::store::Store;
use gitswitch::testing::{MockRunner, gh_status_output};
use gitswitch::tui::app::{App, Screen};
use gitswitch::tui::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use tempfile::TempDir;

fn render(app: &App<'_, '_>) -> String {
    let mut terminal = Terminal::new(TestBackend::new(110, 34)).unwrap();
    terminal.draw(|frame| ui::draw(frame, app)).unwrap();
    let buffer = terminal.backend().buffer().clone();
    let width = buffer.area.width as usize;
    buffer
        .content()
        .chunks(width)
        .map(|row| row.iter().map(|cell| cell.symbol()).collect::<String>())
        .collect::<Vec<_>>()
        .join("\n")
}

fn runner() -> MockRunner {
    MockRunner::new()
        .ok("git --version", "git version 2.55.0")
        .ok("gh --version", "gh version 2.97.0")
        .ok("git config --get user.name", "Octo Cat")
        .ok("git config --get user.email", "octo@example.com")
        .ok("git config", "")
        .ok("gh auth setup-git", "")
        .ok(
            "gh auth status",
            &gh_status_output("github.com", &[("octocat", true)]),
        )
}

struct Env {
    _dir: TempDir,
    runner: MockRunner,
    secrets: MemoryStore,
    store: Option<Store>,
}

impl Env {
    fn new(accounts: &[(&str, &str)], active: Option<&str>) -> Self {
        let dir = TempDir::new().unwrap();
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
        if let Some(active) = active {
            store.set_active(active).unwrap();
        }
        Self {
            _dir: dir,
            runner: runner(),
            secrets: MemoryStore::default(),
            store: Some(store),
        }
    }

    fn service(&mut self) -> Service<'_> {
        Service::new(&self.runner, &self.secrets, self.store.take().unwrap())
    }
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

#[test]
fn the_main_screen_shows_identity_accounts_and_actions() {
    let mut env = Env::new(
        &[("personal", "octocat"), ("work", "work-bot")],
        Some("personal"),
    );
    let mut service = env.service();
    let app = App::new(&mut service);
    let screen = render(&app);

    assert!(screen.contains("gitswitch"));
    assert!(screen.contains("personal"));
    assert!(screen.contains("work"));
    assert!(screen.contains("octocat"));
    assert!(screen.contains("Octo Cat"));
    assert!(screen.contains("authenticated as octocat"));
    assert!(screen.contains("switch"));
    assert!(screen.contains("quit"));
}

#[test]
fn onboarding_explains_the_tool_and_lists_requirements() {
    let mut env = Env::new(&[], None);
    let mut service = env.service();
    let app = App::new(&mut service);
    let screen = render(&app);

    assert_eq!(app.screen, Screen::Onboarding);
    assert!(screen.contains("Welcome to gitswitch"));
    assert!(screen.contains("Requirements"));
    assert!(screen.contains("git version 2.55.0"));
    assert!(screen.contains("gh version 2.97.0"));
    assert!(screen.contains("to add your first account"));
}

#[test]
fn missing_dependencies_are_called_out_during_onboarding() {
    let dir = TempDir::new().unwrap();
    let runner = MockRunner::new().missing("git").missing("gh");
    let secrets = MemoryStore::default();
    let store = Store::open(dir.path().join("accounts.json")).unwrap();
    let mut service = Service::new(&runner, &secrets, store);
    let app = App::new(&mut service);

    let screen = render(&app);
    assert!(screen.contains("git-scm.com"));
    assert!(screen.contains("cli.github.com"));
}

#[test]
fn the_add_form_masks_the_token_field() {
    let mut env = Env::new(&[], None);
    let mut service = env.service();
    let mut app = App::new(&mut service);

    app.on_key(key(KeyCode::Char('a')));
    for _ in 0..5 {
        app.on_key(key(KeyCode::Tab));
    }
    for ch in "ghp_EXAMPLE0123456789abc".chars() {
        app.on_key(key(KeyCode::Char(ch)));
    }

    let screen = render(&app);
    assert!(screen.contains("Add account"));
    assert!(!screen.contains("ghp_"));
    assert!(screen.contains("\u{2022}\u{2022}\u{2022}"));
}

#[test]
fn removal_shows_a_destructive_confirmation() {
    let mut env = Env::new(&[("work", "work-bot")], None);
    let mut service = env.service();
    let mut app = App::new(&mut service);

    app.on_key(key(KeyCode::Char('d')));
    let screen = render(&app);

    assert!(screen.contains("Remove account"));
    assert!(screen.contains("cannot be undone"));
    assert!(screen.contains("gh auth logout"));
}

#[test]
fn a_successful_switch_is_confirmed_on_screen() {
    let mut env = Env::new(&[("personal", "octocat")], None);
    let mut service = env.service();
    let mut app = App::new(&mut service);

    app.on_key(key(KeyCode::Enter));
    app.run_pending();

    let screen = render(&app);
    assert!(screen.contains("Now using personal"));
}

#[test]
fn failures_are_shown_with_a_next_step() {
    let dir = TempDir::new().unwrap();
    let runner = MockRunner::new()
        .ok("git --version", "git version 2.55.0")
        .ok("gh --version", "gh version 2.97.0")
        .ok("git config", "")
        .status("gh auth status", 1, "", "not logged in");
    let secrets = MemoryStore::default();
    let mut store = Store::open(dir.path().join("accounts.json")).unwrap();
    store
        .add(Account::new(
            "personal",
            "octocat",
            "Octo Cat",
            "octo@example.com",
        ))
        .unwrap();
    let mut service = Service::new(&runner, &secrets, store);
    let mut app = App::new(&mut service);

    app.on_key(key(KeyCode::Enter));
    app.run_pending();

    let screen = render(&app);
    assert!(screen.contains("not authenticated"));
    assert!(screen.contains("gh auth login"));
}

#[test]
fn the_help_overlay_documents_every_shortcut() {
    let mut env = Env::new(&[("work", "work-bot")], None);
    let mut service = env.service();
    let mut app = App::new(&mut service);

    app.on_key(key(KeyCode::Char('?')));
    let screen = render(&app);

    assert!(screen.contains("Keyboard"));
    assert!(screen.contains("rename the selected account"));
    assert!(screen.contains("Ctrl-C"));
}

#[test]
fn the_loading_state_is_visible() {
    let mut env = Env::new(&[("work", "work-bot")], None);
    let mut service = env.service();
    let mut app = App::new(&mut service);

    app.on_key(key(KeyCode::Enter));
    app.busy = Some("Switching account".into());
    let screen = render(&app);
    assert!(screen.contains("Switching account"));
}

#[test]
fn a_small_terminal_still_renders_without_panicking() {
    let mut env = Env::new(&[("work", "work-bot")], None);
    let mut service = env.service();
    let app = App::new(&mut service);

    for (width, height) in [(20u16, 8u16), (40, 12), (200, 60)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::draw(frame, &app)).unwrap();
    }
}
