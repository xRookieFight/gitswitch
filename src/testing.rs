//! Test doubles for external processes.
//!
//! Kept in the library (rather than behind `#[cfg(test)]`) so integration tests
//! and downstream contributors can drive gitswitch without a real GitHub
//! account, a real `gh` binary or network access.

use std::collections::HashSet;
use std::sync::Mutex;

use crate::error::{Error, Result};
use crate::process::{Output, Runner};

#[derive(Debug, Clone)]
enum Reply {
    Out(Output),
    Missing(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    pub command: String,
    pub stdin: Option<String>,
    pub interactive: bool,
}

/// Records invocations and replays canned replies.
///
/// Replies are matched against the full command line (`"gh auth status"`), with
/// a longest-prefix fallback so tests only spell out the interesting part.
/// Registering the same command twice queues the replies in order.
#[derive(Debug, Default)]
pub struct MockRunner {
    replies: Mutex<Vec<(String, Vec<Reply>)>>,
    calls: Mutex<Vec<Call>>,
    missing: Mutex<HashSet<String>>,
    cursor: Mutex<Vec<usize>>,
}

impl MockRunner {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a successful reply.
    pub fn ok(self, command: &str, stdout: &str) -> Self {
        self.status(command, 0, stdout, "")
    }

    /// Registers a reply with an explicit exit status.
    pub fn status(self, command: &str, status: i32, stdout: &str, stderr: &str) -> Self {
        self.push(
            command,
            Reply::Out(Output {
                status,
                stdout: stdout.to_string(),
                stderr: stderr.to_string(),
            }),
        )
    }

    /// Marks a program as absent from PATH.
    pub fn missing(self, program: &'static str) -> Self {
        self.missing
            .lock()
            .expect("mock poisoned")
            .insert(program.to_string());
        self.push(program, Reply::Missing(program))
    }

    fn push(self, command: &str, reply: Reply) -> Self {
        {
            let mut replies = self.replies.lock().expect("mock poisoned");
            if let Some(slot) = replies.iter_mut().find(|(key, _)| key == command) {
                slot.1.push(reply);
            } else {
                replies.push((command.to_string(), vec![reply]));
                self.cursor.lock().expect("mock poisoned").push(0);
            }
        }
        self
    }

    /// Every command line seen so far, in order.
    pub fn calls(&self) -> Vec<String> {
        self.calls
            .lock()
            .expect("mock poisoned")
            .iter()
            .map(|call| call.command.clone())
            .collect()
    }

    pub fn recorded(&self) -> Vec<Call> {
        self.calls.lock().expect("mock poisoned").clone()
    }

    pub fn was_called(&self, command: &str) -> bool {
        self.calls().iter().any(|call| call.starts_with(command))
    }

    /// Data piped to the first invocation of `command`, if any.
    pub fn stdin_for(&self, command: &str) -> Option<String> {
        self.calls
            .lock()
            .expect("mock poisoned")
            .iter()
            .find(|call| call.command.starts_with(command))
            .and_then(|call| call.stdin.clone())
    }

    fn lookup(&self, command: &str) -> Option<Reply> {
        let replies = self.replies.lock().expect("mock poisoned");
        let mut cursor = self.cursor.lock().expect("mock poisoned");

        let index = replies
            .iter()
            .enumerate()
            .filter(|(_, (key, _))| command == key || command.starts_with(key.as_str()))
            .max_by_key(|(_, (key, _))| key.len())
            .map(|(index, _)| index)?;

        let queue = &replies[index].1;
        let position = cursor[index].min(queue.len() - 1);
        cursor[index] = position + 1;
        Some(queue[position].clone())
    }

    fn record(&self, command: String, stdin: Option<&str>, interactive: bool) {
        self.calls.lock().expect("mock poisoned").push(Call {
            command,
            stdin: stdin.map(str::to_string),
            interactive,
        });
    }
}

fn join(program: &str, args: &[&str]) -> String {
    if args.is_empty() {
        return program.to_string();
    }
    format!("{program} {}", args.join(" "))
}

impl Runner for MockRunner {
    fn run(&self, program: &str, args: &[&str], stdin: Option<&str>) -> Result<Output> {
        let command = join(program, args);
        self.record(command.clone(), stdin, false);
        match self.lookup(&command) {
            Some(Reply::Out(out)) => Ok(out),
            Some(Reply::Missing(name)) => Err(Error::MissingDependency(name)),
            None => panic!("MockRunner received an unregistered command: `{command}`"),
        }
    }

    fn run_interactive(&self, program: &str, args: &[&str]) -> Result<i32> {
        let command = join(program, args);
        self.record(command.clone(), None, true);
        match self.lookup(&command) {
            Some(Reply::Out(out)) => Ok(out.status),
            Some(Reply::Missing(name)) => Err(Error::MissingDependency(name)),
            None => panic!("MockRunner received an unregistered command: `{command}`"),
        }
    }

    fn is_available(&self, program: &str) -> bool {
        !self
            .missing
            .lock()
            .expect("mock poisoned")
            .contains(program)
    }
}

/// `gh auth status` output for a single logged-in account.
pub fn gh_status_output(host: &str, accounts: &[(&str, bool)]) -> String {
    let mut out = format!("{host}\n");
    for (login, active) in accounts {
        out.push_str(&format!(
            "  \u{2713} Logged in to {host} account {login} (keyring)\n"
        ));
        out.push_str(&format!("  - Active account: {active}\n"));
        out.push_str("  - Git operations protocol: https\n");
        out.push_str("  - Token: gho_************************\n");
        out.push_str("  - Token scopes: 'gist', 'read:org', 'repo'\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_matched_by_longest_prefix() {
        let runner = MockRunner::new()
            .ok("git config", "generic")
            .ok("git config --global user.name", "Octo Cat");
        let out = runner
            .run("git", &["config", "--global", "user.name"], None)
            .unwrap();
        assert_eq!(out.stdout, "Octo Cat");
    }

    #[test]
    fn repeated_registrations_are_replayed_in_order() {
        let runner = MockRunner::new()
            .ok("gh auth status", "first")
            .ok("gh auth status", "second");
        assert_eq!(
            runner.run("gh", &["auth", "status"], None).unwrap().stdout,
            "first"
        );
        assert_eq!(
            runner.run("gh", &["auth", "status"], None).unwrap().stdout,
            "second"
        );
        // The last reply keeps being returned once the queue is exhausted.
        assert_eq!(
            runner.run("gh", &["auth", "status"], None).unwrap().stdout,
            "second"
        );
    }

    #[test]
    fn stdin_is_captured() {
        let runner = MockRunner::new().ok("gh auth login", "");
        runner
            .run("gh", &["auth", "login"], Some("ghp_EXAMPLEsecret"))
            .unwrap();
        assert_eq!(
            runner.stdin_for("gh auth login").as_deref(),
            Some("ghp_EXAMPLEsecret")
        );
    }
}
