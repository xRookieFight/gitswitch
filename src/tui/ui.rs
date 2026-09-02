use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Flex, Layout, Rect};
use ratatui::style::{Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, ListState, Padding, Paragraph, Wrap,
};

use super::app::{App, Level, Screen};
use super::theme;
use crate::model::Account;

/// Full set of shortcuts, shown when the terminal is wide enough.
const KEY_HINTS: &[(&str, &str)] = &[
    ("\u{2191}\u{2193}", "move"),
    ("\u{21b5}", "switch"),
    ("a", "add"),
    ("r", "rename"),
    ("t", "token"),
    ("A", "gh login"),
    ("d", "remove"),
    ("L", "scope"),
    ("g", "refresh"),
    ("?", "help"),
    ("q", "quit"),
];

/// Fallback for narrow terminals; the rest stay discoverable through `?`.
const KEY_HINTS_COMPACT: &[(&str, &str)] = &[
    ("\u{2191}\u{2193}", "move"),
    ("\u{21b5}", "switch"),
    ("a", "add"),
    ("d", "remove"),
    ("?", "help"),
    ("q", "quit"),
];

pub fn draw(frame: &mut Frame, app: &App<'_, '_>) {
    let area = frame.area();
    let [header, body, toast, footer] = Layout::vertical([
        Constraint::Length(7),
        Constraint::Min(6),
        Constraint::Length(if app.toast.is_some() { 3 } else { 0 }),
        Constraint::Length(1),
    ])
    .areas(area);

    draw_header(frame, app, header);

    match &app.screen {
        Screen::Onboarding => draw_onboarding(frame, app, body),
        _ => draw_accounts(frame, app, body),
    }

    draw_toast(frame, app, toast);
    draw_footer(frame, app, footer);

    match &app.screen {
        Screen::Form(_) => draw_form(frame, app),
        Screen::Confirm(_) => draw_confirm(frame, app),
        Screen::Ask(_) => draw_ask(frame, app),
        Screen::Help => draw_help(frame),
        _ => {}
    }

    if let Some(busy) = &app.busy {
        draw_busy(frame, busy);
    }
}

fn panel<'a>(title: impl Into<String>) -> Block<'a> {
    let title = title.into();
    Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(theme::border())
        .title(Span::styled(format!(" {title} "), theme::title()))
        .padding(Padding::horizontal(1))
}

fn field(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<10}"), theme::label()),
        value,
    ])
}

fn draw_header(frame: &mut Frame, app: &App<'_, '_>, area: Rect) {
    let status = &app.status;

    let account = match &status.active_account {
        Some(account) => Span::styled(
            format!("{}  ({})", account.name, account.username),
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        ),
        None => Span::styled("no account selected", Style::default().fg(theme::WARN)),
    };

    let identity = Span::styled(
        format!(
            "{} <{}>",
            status.identity.name.as_deref().unwrap_or("unset"),
            status.identity.email.as_deref().unwrap_or("unset")
        ),
        theme::value(),
    );

    let gh = if !status.gh_installed {
        Span::styled("gh is not installed", Style::default().fg(theme::ERROR))
    } else {
        match &status.gh_login {
            Some(login) if status.is_consistent() => Span::styled(
                format!("\u{2713} authenticated as {login}"),
                Style::default().fg(theme::OK),
            ),
            Some(login) => Span::styled(
                format!("\u{26a0} authenticated as {login} - does not match the active profile"),
                Style::default().fg(theme::WARN),
            ),
            None => Span::styled(
                "\u{26a0} not authenticated",
                Style::default().fg(theme::WARN),
            ),
        }
    };

    let scope = Span::styled(
        format!(
            "{} config \u{2022} {} accounts \u{2022} keychain {}",
            app.scope.label(),
            app.accounts().len(),
            if app.keychain_available() {
                "on"
            } else {
                "off"
            }
        ),
        theme::label(),
    );

    let lines = vec![
        field("Account", account),
        field("Git", identity),
        field("GitHub", gh),
        field("Session", scope),
    ];

    let block = panel(format!("gitswitch {}", crate::VERSION));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_accounts(frame: &mut Frame, app: &App<'_, '_>, area: Rect) {
    let [list_area, detail_area] =
        Layout::horizontal([Constraint::Percentage(55), Constraint::Percentage(45)]).areas(area);

    let active = app.active_name();
    let items: Vec<ListItem> = app
        .accounts()
        .iter()
        .enumerate()
        .map(|(index, account)| {
            let is_active = Some(account.name.as_str()) == active;
            let marker = if is_active { "\u{25cf}" } else { "\u{25cb}" };
            let marker_style = if is_active {
                Style::default().fg(theme::OK)
            } else {
                Style::default().fg(theme::MUTED)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{} ", index + 1), theme::label()),
                Span::styled(format!("{marker} "), marker_style),
                Span::styled(format!("{:<16}", account.name), theme::value()),
                Span::styled(account.username.clone(), theme::label()),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(panel("Accounts"))
        .highlight_style(theme::selected())
        .highlight_symbol("\u{25b8} ");
    let mut state = ListState::default().with_selected(Some(app.selected));
    frame.render_stateful_widget(list, list_area, &mut state);

    frame.render_widget(details(app.selected_account(), app), detail_area);
}

fn details<'a>(account: Option<&Account>, app: &App<'_, '_>) -> Paragraph<'a> {
    let Some(account) = account else {
        return Paragraph::new("No account selected.")
            .style(theme::label())
            .block(panel("Details"));
    };

    let token = if account.has_stored_token {
        Span::styled("cached in the OS keychain", Style::default().fg(theme::OK))
    } else {
        Span::styled("not cached", theme::label())
    };

    let is_active = Some(account.name.as_str()) == app.active_name();
    let lines = vec![
        field(
            "Profile",
            Span::styled(account.name.clone(), theme::value()).bold(),
        ),
        field(
            "GitHub",
            Span::styled(account.username.clone(), theme::value()),
        ),
        field("Host", Span::styled(account.host.clone(), theme::value())),
        field(
            "Git name",
            Span::styled(account.git_name.clone(), theme::value()),
        ),
        field(
            "Git email",
            Span::styled(account.git_email.clone(), theme::value()),
        ),
        field("Token", token),
        Line::from(""),
        Line::from(if is_active {
            Span::styled("This profile is active.", Style::default().fg(theme::OK))
        } else {
            Span::styled("Press \u{21b5} to switch to it.", theme::label())
        }),
    ];

    Paragraph::new(lines).block(panel("Details"))
}

fn draw_onboarding(frame: &mut Frame, app: &App<'_, '_>, area: Rect) {
    let status = &app.status;
    let mut lines = vec![
        Line::from(Span::styled("Welcome to gitswitch", theme::title())),
        Line::from(""),
        Line::from(Span::styled(
            "Save your GitHub accounts once, then switch git and the GitHub CLI together in a single keystroke.",
            theme::value(),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Signing in happens in your browser through the GitHub CLI - there is no token to copy unless you want one.",
            theme::label(),
        )),
        Line::from(""),
        Line::from(Span::styled("Requirements", theme::title())),
        dependency_line("git", status.git_installed, status.git_version.as_deref()),
        dependency_line("gh", status.gh_installed, status.gh_version.as_deref()),
        dependency_line(
            "OS credential store",
            app.keychain_available(),
            Some("used to cache tokens"),
        ),
        Line::from(""),
    ];

    if !status.git_installed {
        lines.push(Line::from(Span::styled(
            "Install git from https://git-scm.com/downloads",
            Style::default().fg(theme::WARN),
        )));
    }
    if !status.gh_installed {
        lines.push(Line::from(Span::styled(
            "Install the GitHub CLI from https://cli.github.com",
            Style::default().fg(theme::WARN),
        )));
    }

    lines.push(Line::from(vec![
        Span::styled("Press ", theme::label()),
        Span::styled("a", theme::key()),
        Span::styled(" to add your first account, ", theme::label()),
        Span::styled("?", theme::key()),
        Span::styled(" for help, ", theme::label()),
        Span::styled("q", theme::key()),
        Span::styled(" to quit.", theme::label()),
    ]));

    frame.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(panel("Getting started")),
        area,
    );
}

fn dependency_line(name: &str, ok: bool, detail: Option<&str>) -> Line<'static> {
    let (mark, style) = if ok {
        ("\u{2713}", Style::default().fg(theme::OK))
    } else {
        ("\u{2717}", Style::default().fg(theme::ERROR))
    };
    Line::from(vec![
        Span::styled(format!("  {mark} "), style),
        Span::styled(format!("{name:<22}"), theme::value()),
        Span::styled(
            detail
                .unwrap_or(if ok { "found" } else { "missing" })
                .to_string(),
            theme::label(),
        ),
    ])
}

fn draw_toast(frame: &mut Frame, app: &App<'_, '_>, area: Rect) {
    let Some(toast) = &app.toast else {
        return;
    };
    if area.height == 0 {
        return;
    }

    let (color, prefix) = match toast.level {
        Level::Info => (theme::ACCENT, "\u{2022}"),
        Level::Success => (theme::OK, "\u{2713}"),
        Level::Warning => (theme::WARN, "\u{26a0}"),
        Level::Error => (theme::ERROR, "\u{2717}"),
    };

    let mut spans = vec![
        Span::styled(format!("{prefix} "), Style::default().fg(color)),
        Span::styled(toast.text.clone(), Style::default().fg(color)),
    ];
    if let Some(hint) = &toast.hint {
        spans.push(Span::styled(format!("  \u{2014} {hint}"), theme::label()));
    }

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(color))
        .padding(Padding::horizontal(1));
    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .wrap(Wrap { trim: true })
            .block(block),
        area,
    );
}

fn draw_footer(frame: &mut Frame, app: &App<'_, '_>, area: Rect) {
    let hints: Vec<Span> = match app.screen {
        Screen::Form(_) => [
            key_hint("\u{21b5}", "next / save"),
            key_hint("\u{21e5}", "field"),
            key_hint("^U", "clear"),
            key_hint("Esc", "cancel"),
        ]
        .concat(),
        Screen::Confirm(_) => [
            key_hint("y", "confirm"),
            key_hint("l", "toggle gh logout"),
            key_hint("n", "cancel"),
        ]
        .concat(),
        Screen::Ask(_) => [
            key_hint("y / \u{21b5}", "open the browser sign-in"),
            key_hint("n", "later"),
        ]
        .concat(),
        Screen::Help => [key_hint("any key", "back")].concat(),
        Screen::Onboarding => [
            key_hint("a", "add your first account"),
            key_hint("?", "help"),
            key_hint("q", "quit"),
        ]
        .concat(),
        _ => {
            let full = hint_row(KEY_HINTS);
            if row_width(&full) <= area.width as usize {
                full
            } else {
                hint_row(KEY_HINTS_COMPACT)
            }
        }
    };

    frame.render_widget(Paragraph::new(Line::from(hints)), area);
}

fn hint_row(hints: &[(&str, &str)]) -> Vec<Span<'static>> {
    hints
        .iter()
        .flat_map(|(key, label)| key_hint(key, label))
        .collect()
}

fn row_width(spans: &[Span<'_>]) -> usize {
    spans.iter().map(|span| span.content.chars().count()).sum()
}

fn key_hint(key: &str, label: &str) -> Vec<Span<'static>> {
    vec![
        Span::styled(format!(" {key} "), theme::key()),
        Span::styled(format!("{label}  "), theme::label()),
    ]
}

fn draw_form(frame: &mut Frame, app: &App<'_, '_>) {
    let Screen::Form(form) = &app.screen else {
        return;
    };

    let height = form.fields.len() as u16 * 2 + 5;
    let area = popup(frame.area(), 70, height);
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    for (index, field) in form.fields.iter().enumerate() {
        let focused = index == form.cursor;
        lines.push(Line::from(Span::styled(
            field.label.to_string(),
            if focused {
                theme::key()
            } else {
                theme::label()
            },
        )));
        let value = field.display();
        let shown = if focused {
            format!("{value}\u{2588}")
        } else if value.is_empty() {
            field.hint.to_string()
        } else {
            value
        };
        let style = if !focused && field.value.is_empty() {
            Style::default().fg(theme::MUTED).italic()
        } else {
            theme::value()
        };
        lines.push(Line::from(vec![
            Span::styled("  ", theme::label()),
            Span::styled(shown, style),
        ]));
    }

    frame.render_widget(Paragraph::new(lines).block(panel(form.title.clone())), area);
}

fn draw_confirm(frame: &mut Frame, app: &App<'_, '_>) {
    let Screen::Confirm(confirm) = &app.screen else {
        return;
    };

    let area = popup(frame.area(), 60, 9);
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::from(Span::styled(
            confirm.message.clone(),
            Style::default().fg(theme::TEXT),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("Also run `gh auth logout`: ", theme::label()),
            Span::styled(
                if confirm.logout { "yes" } else { "no" },
                Style::default().fg(if confirm.logout {
                    theme::WARN
                } else {
                    theme::MUTED
                }),
            ),
            Span::styled("  (press l to toggle)", theme::label()),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "This cannot be undone.",
            Style::default().fg(theme::ERROR),
        )),
    ];

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ERROR))
        .title(Span::styled(
            format!(" {} ", confirm.title),
            Style::default().fg(theme::ERROR).bold(),
        ))
        .padding(Padding::horizontal(1));

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        area,
    );
}

fn draw_ask(frame: &mut Frame, app: &App<'_, '_>) {
    let Screen::Ask(ask) = &app.screen else {
        return;
    };

    let area = popup(frame.area(), 64, 11);
    frame.render_widget(Clear, area);

    let mut lines: Vec<Line> = ask
        .message
        .split('\n')
        .map(|line| Line::from(Span::styled(line.to_string(), theme::value())))
        .collect();
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("y", theme::key()),
        Span::styled(" sign in now    ", theme::label()),
        Span::styled("n", theme::key()),
        Span::styled(" later", theme::label()),
    ]));

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(theme::ACCENT))
        .title(Span::styled(
            format!(" {} ", ask.title),
            Style::default().fg(theme::ACCENT).bold(),
        ))
        .padding(Padding::horizontal(1));

    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(block),
        area,
    );
}

fn draw_help(frame: &mut Frame) {
    let area = popup(frame.area(), 64, 20);
    frame.render_widget(Clear, area);

    let rows: &[(&str, &str)] = &[
        ("\u{2191} \u{2193} / k j", "move through the account list"),
        ("1 - 9", "switch to an account by number"),
        ("Enter", "switch to the selected account"),
        ("a", "add a new account"),
        ("r", "rename the selected account"),
        ("t", "store a new token for the account"),
        ("A", "sign in with your browser (gh auth login)"),
        ("d / Del", "remove the selected account"),
        ("L", "write the git identity globally or per repository"),
        ("g / F5", "re-read git and gh state"),
        ("?", "this help"),
        ("q / Esc / Ctrl-C", "quit"),
    ];

    let mut lines = vec![Line::from(Span::styled(
        "Switching updates git user.name, user.email and the active GitHub CLI account.",
        theme::label(),
    ))];
    lines.push(Line::from(""));
    lines.extend(rows.iter().map(|(key, description)| {
        Line::from(vec![
            Span::styled(format!("  {key:<18}"), theme::key()),
            Span::styled((*description).to_string(), theme::value()),
        ])
    }));

    frame.render_widget(Paragraph::new(lines).block(panel("Keyboard")), area);
}

fn draw_busy(frame: &mut Frame, message: &str) {
    let area = popup(frame.area(), 40, 5);
    frame.render_widget(Clear, area);
    let lines = vec![
        Line::from(Span::styled(
            format!("{message}\u{2026}"),
            Style::default().fg(theme::ACCENT).bold(),
        )),
        Line::from(Span::styled("please wait", theme::label())),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .block(panel("Working")),
        area,
    );
}

/// Centres a popup of the given width percentage and height in rows.
fn popup(area: Rect, width_percent: u16, height: u16) -> Rect {
    let [horizontal] = Layout::horizontal([Constraint::Percentage(width_percent)])
        .flex(Flex::Center)
        .areas(area);
    let [vertical] = Layout::vertical([Constraint::Length(height.min(area.height))])
        .flex(Flex::Center)
        .areas(horizontal);
    vertical
}
