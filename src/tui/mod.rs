//! The interactive terminal interface.
//!
//! [`app`] holds all state and behaviour and is driven by synthetic key events
//! in tests; [`ui`] is a pure function from state to frame. This module owns
//! only the terminal lifecycle and the event loop.

pub mod app;
pub mod theme;
pub mod ui;

use std::io::IsTerminal;
use std::time::Duration;

use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event};

use crate::error::{Error, Result};
use crate::service::Service;
use app::{App, Effect};

/// Opens the interface on the account list.
pub fn run(service: &mut Service<'_>) -> Result<()> {
    start(service, false)
}

/// Opens the interface directly on the "add account" form.
pub fn run_add(service: &mut Service<'_>) -> Result<()> {
    start(service, true)
}

fn start(service: &mut Service<'_>, add: bool) -> Result<()> {
    ensure_interactive()?;

    let mut terminal =
        ratatui::try_init().map_err(|err| Error::UnsupportedTerminal(err.to_string()))?;

    let mut app = if add {
        App::new_with_add_form(service)
    } else {
        App::new(service)
    };

    let result = event_loop(&mut app, &mut terminal);
    ratatui::restore();
    result
}

fn ensure_interactive() -> Result<()> {
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(Error::UnsupportedTerminal(
            "stdin and stdout must be a terminal; use `gitswitch list`, `switch` or `current` in scripts"
                .into(),
        ));
    }
    Ok(())
}

fn event_loop(app: &mut App<'_, '_>, terminal: &mut DefaultTerminal) -> Result<()> {
    loop {
        terminal.draw(|frame| ui::draw(frame, app))?;

        // Queued work gets a visible frame before it blocks the loop.
        if let Some(pending) = &app.pending {
            app.busy = Some(pending.label().to_string());
            terminal.draw(|frame| ui::draw(frame, app))?;
            app.run_pending();
            app.busy = None;
            continue;
        }

        // Polling keeps the loop responsive to resizes and to Ctrl-C.
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }

        match event::read()? {
            Event::Key(key) => match app.on_key(key) {
                Effect::Quit => return Ok(()),
                Effect::InteractiveLogin(host) => {
                    let result = suspended(terminal, || app.login_interactively(&host));
                    app.after_interactive_login(result);
                }
                Effect::None => {}
            },
            Event::Resize(_, _) => {}
            _ => {}
        }
    }
}

/// Leaves the alternate screen so a child process can own the terminal, then
/// restores it. Used for `gh auth login`, which is interactive by design.
fn suspended<T>(terminal: &mut DefaultTerminal, action: impl FnOnce() -> Result<T>) -> Result<T> {
    ratatui::restore();
    let result = action();
    *terminal = ratatui::try_init().map_err(|err| Error::UnsupportedTerminal(err.to_string()))?;
    terminal.clear()?;
    result
}
