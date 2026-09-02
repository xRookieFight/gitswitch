use ratatui::style::{Color, Modifier, Style};

/// Colours are chosen to stay legible on both dark and light terminals and to
/// degrade gracefully where only 16 colours are available.
pub const ACCENT: Color = Color::Rgb(122, 162, 247);
pub const OK: Color = Color::Rgb(158, 206, 106);
pub const WARN: Color = Color::Rgb(224, 175, 104);
pub const ERROR: Color = Color::Rgb(247, 118, 142);
pub const MUTED: Color = Color::Rgb(126, 138, 166);
pub const BORDER: Color = Color::Rgb(68, 78, 104);
pub const TEXT: Color = Color::Rgb(202, 211, 245);

pub fn title() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}

pub fn border() -> Style {
    Style::default().fg(BORDER)
}

pub fn label() -> Style {
    Style::default().fg(MUTED)
}

pub fn value() -> Style {
    Style::default().fg(TEXT)
}

pub fn selected() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(ACCENT)
        .add_modifier(Modifier::BOLD)
}

pub fn key() -> Style {
    Style::default().fg(ACCENT).add_modifier(Modifier::BOLD)
}
