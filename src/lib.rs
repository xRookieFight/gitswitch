//! gitswitch - manage and switch between multiple GitHub accounts.
//!
//! The crate is split so that everything below the interface layer is testable
//! without a real GitHub account: [`process::Runner`] abstracts subprocesses and
//! [`secrets::SecretStore`] abstracts the OS credential store.

pub mod cli;
pub mod error;
pub mod gh;
pub mod git;
pub mod model;
pub mod process;
pub mod secrets;
pub mod service;
pub mod store;
pub mod testing;
pub mod tui;

pub use error::{Error, Result};
pub use model::Account;
pub use service::{GhOutcome, Service, Status, SwitchOptions, SwitchReport};
pub use store::Store;

pub const VERSION: &str = env!("CARGO_PKG_VERSION");
