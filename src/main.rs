use std::io::{IsTerminal, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    match gitswitch::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            report(&err);
            ExitCode::FAILURE
        }
    }
}

fn report(err: &gitswitch::Error) {
    let mut stderr = std::io::stderr();
    let colored = std::env::var_os("NO_COLOR").is_none() && stderr.is_terminal();
    let (red, dim, reset) = if colored {
        ("\u{1b}[31m", "\u{1b}[2m", "\u{1b}[0m")
    } else {
        ("", "", "")
    };

    // Messages are already redacted upstream; nothing here can leak a token.
    let _ = writeln!(stderr, "{red}error:{reset} {err}");
    if let Some(hint) = err.hint() {
        let _ = writeln!(stderr, "{dim}hint:{reset} {hint}");
    }
}
