//! Renders the interface into JSON frames used to generate the images in the
//! README.
//!
//! Run with `cargo run --example screenshots`. Everything is driven by the
//! mocked runner, so no GitHub account, network access or `gh` binary is needed
//! and the output is reproducible.

use std::fs;
use std::path::Path;

use gitswitch::model::Account;
use gitswitch::secrets::memory::MemoryStore;
use gitswitch::service::Service;
use gitswitch::store::Store;
use gitswitch::testing::{MockRunner, gh_status_output};
use gitswitch::tui::app::App;
use gitswitch::tui::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::style::{Color, Modifier};

const WIDTH: u16 = 100;
const HEIGHT: u16 = 26;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let out = Path::new("docs/frames");
    fs::create_dir_all(out)?;

    let dir = tempfile::TempDir::new()?;
    let runner = runner();
    let secrets = MemoryStore::default();

    // Onboarding, on a machine with everything installed but no profiles yet.
    let empty = Store::open(dir.path().join("empty.json"))?;
    let mut service = Service::new(&runner, &secrets, empty);
    let mut app = App::new(&mut service);
    save(out, "01-onboarding", &app)?;

    app.on_key(key(KeyCode::Char('a')));
    type_text(&mut app, "personal");
    app.on_key(key(KeyCode::Tab));
    type_text(&mut app, "xRookieFight");
    app.on_key(key(KeyCode::Tab));
    type_text(&mut app, "xRookieFight");
    app.on_key(key(KeyCode::Tab));
    type_text(&mut app, "xrookiefight@users.noreply.github.com");
    save(out, "02-add", &app)?;
    drop(service);

    // A populated store for the remaining screens.
    let mut store = Store::open(dir.path().join("full.json"))?;
    store.add(profile(
        "personal",
        "xRookieFight",
        "xRookieFight",
        "xrookiefight@users.noreply.github.com",
    ))?;
    store.add(profile("work", "acme-dev", "Acme Dev", "dev@acme.example"))?;
    store.add(profile(
        "open-source",
        "octocat",
        "Octo Cat",
        "octo@users.noreply.github.com",
    ))?;
    store.set_active("personal")?;

    // A fresh runner so the canned `gh auth status` replies start from the top.
    let runner = self::runner();
    let mut service = Service::new(&runner, &secrets, store);
    let mut app = App::new(&mut service);
    save(out, "03-accounts", &app)?;

    app.on_key(key(KeyCode::Char('?')));
    save(out, "04-help", &app)?;
    app.on_key(key(KeyCode::Esc));

    app.on_key(key(KeyCode::Down));
    save(out, "05-selected", &app)?;

    app.on_key(key(KeyCode::Enter));
    app.busy = Some("Switching account".into());
    save(out, "06-switching", &app)?;
    app.busy = None;
    app.run_pending();
    save(out, "07-switched", &app)?;

    app.on_key(key(KeyCode::Char('d')));
    save(out, "08-confirm", &app)?;
    app.on_key(key(KeyCode::Char('n')));

    // The browser sign-in prompt shown after saving an account without a token.
    app.ask_to_sign_in_for_screenshot("open-source");
    save(out, "09-signin", &app)?;

    println!("wrote frames to {}", out.display());
    Ok(())
}

fn profile(name: &str, username: &str, git_name: &str, email: &str) -> Account {
    let mut account = Account::new(name, username, git_name, email);
    account.has_stored_token = true;
    account
}

fn runner() -> MockRunner {
    MockRunner::new()
        .ok("git --version", "git version 2.55.0")
        .ok("gh --version", "gh version 2.97.0")
        .ok("git config --get user.name", "xRookieFight")
        .ok(
            "git config --get user.email",
            "xrookiefight@users.noreply.github.com",
        )
        .ok("git config --get user.email", "dev@acme.example")
        .ok("git config", "")
        .ok("gh auth setup-git", "")
        .ok("gh auth switch", "")
        // Replayed in order: the opening screen, the pre-switch check and the
        // verification that follows `gh auth switch`.
        .ok(
            "gh auth status",
            &gh_status_output("github.com", &[("xRookieFight", true)]),
        )
        .ok(
            "gh auth status",
            &gh_status_output("github.com", &[("xRookieFight", true), ("acme-dev", false)]),
        )
        .ok(
            "gh auth status",
            &gh_status_output("github.com", &[("xRookieFight", false), ("acme-dev", true)]),
        )
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn type_text(app: &mut App<'_, '_>, text: &str) {
    for ch in text.chars() {
        app.on_key(key(KeyCode::Char(ch)));
    }
}

/// Serialises the rendered buffer as `{width, height, cells: [{c, fg, bg, bold}]}`.
fn save(dir: &Path, name: &str, app: &App<'_, '_>) -> Result<(), Box<dyn std::error::Error>> {
    let mut terminal = Terminal::new(TestBackend::new(WIDTH, HEIGHT))?;
    terminal.draw(|frame| ui::draw(frame, app))?;
    let buffer = terminal.backend().buffer();

    let cells: Vec<serde_json::Value> = buffer
        .content()
        .iter()
        .map(|cell| {
            serde_json::json!({
                "c": cell.symbol(),
                "fg": rgb(cell.fg),
                "bg": rgb(cell.bg),
                "bold": cell.modifier.contains(Modifier::BOLD),
            })
        })
        .collect();

    let payload = serde_json::json!({
        "width": WIDTH,
        "height": HEIGHT,
        "cells": cells,
    });
    fs::write(
        dir.join(format!("{name}.json")),
        serde_json::to_string(&payload)?,
    )?;
    Ok(())
}

fn rgb(color: Color) -> Option<[u8; 3]> {
    match color {
        Color::Rgb(r, g, b) => Some([r, g, b]),
        Color::Black => Some([0x1a, 0x1b, 0x26]),
        Color::White => Some([0xff, 0xff, 0xff]),
        Color::Red => Some([0xf7, 0x76, 0x8e]),
        Color::Green => Some([0x9e, 0xce, 0x6a]),
        Color::Yellow => Some([0xe0, 0xaf, 0x68]),
        Color::Blue => Some([0x7a, 0xa2, 0xf7]),
        Color::Reset => None,
        _ => None,
    }
}
