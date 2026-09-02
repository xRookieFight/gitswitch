use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

/// Every failure surfaced to the user. Messages are written to be read by a
/// human in a terminal, and never contain credentials.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("`{0}` was not found on your PATH")]
    MissingDependency(&'static str),

    #[error("failed to run `{program}`: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },

    #[error("`{command}` exited with status {status}: {message}")]
    CommandFailed {
        command: String,
        status: i32,
        message: String,
    },

    #[error("could not determine a configuration directory for the current user")]
    NoConfigDir,

    #[error("could not read configuration at {path}: {source}")]
    ConfigRead {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not write configuration to {path}: {source}")]
    ConfigWrite {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("configuration at {path} is corrupted: {message}")]
    ConfigCorrupt { path: PathBuf, message: String },

    #[error(
        "configuration at {path} was written by a newer version of gitswitch (schema {found}, supported {supported})"
    )]
    ConfigTooNew {
        path: PathBuf,
        found: u32,
        supported: u32,
    },

    #[error("no account named `{0}` is saved")]
    UnknownAccount(String),

    #[error("an account named `{0}` already exists")]
    DuplicateAccount(String),

    #[error("{0}")]
    InvalidInput(String),

    #[error("no account is currently active")]
    NoActiveAccount,

    #[error("GitHub CLI is not authenticated for {0}")]
    NotAuthenticated(String),

    #[error(
        "account switch could not be verified: gh reports `{found}` as active, expected `{expected}`"
    )]
    VerificationFailed { expected: String, found: String },

    #[error("credential store error: {0}")]
    Secret(String),

    #[error("this terminal cannot host the interactive interface: {0}")]
    UnsupportedTerminal(String),

    #[error("{0}")]
    Io(#[from] std::io::Error),
}

impl Error {
    /// A short, actionable next step shown underneath the error message.
    pub fn hint(&self) -> Option<String> {
        match self {
            Error::MissingDependency("git") => Some(
                "Install Git from https://git-scm.com/downloads and make sure `git` is on your PATH."
                    .into(),
            ),
            Error::MissingDependency("gh") => Some(
                "Install the GitHub CLI from https://cli.github.com and make sure `gh` is on your PATH."
                    .into(),
            ),
            Error::MissingDependency(other) => Some(format!("Install `{other}` and retry.")),
            Error::NotAuthenticated(host) => {
                Some(format!("Run `gitswitch add` or `gh auth login --hostname {host}`."))
            }
            Error::UnknownAccount(_) => {
                Some("Run `gitswitch list` to see the accounts you have saved.".into())
            }
            Error::DuplicateAccount(name) => Some(format!(
                "Pick another name, or update the existing one with `gitswitch remove {name}`."
            )),
            Error::ConfigCorrupt { path, .. } => Some(format!(
                "Fix the file by hand or delete it to start over: {}",
                path.display()
            )),
            Error::ConfigTooNew { .. } => {
                Some("Upgrade gitswitch to the latest release.".into())
            }
            Error::VerificationFailed { expected, .. } => Some(format!(
                "Re-authenticate the account with `gitswitch add` or `gh auth login --user {expected}`."
            )),
            Error::NoActiveAccount => {
                Some("Select an account with `gitswitch switch <account>`.".into())
            }
            Error::Secret(_) => Some(
                "gitswitch could not reach your OS credential store; tokens will not be cached."
                    .into(),
            ),
            _ => None,
        }
    }
}
