use std::io::Write;
use std::process::{Command, Stdio};

use crate::error::{Error, Result};

/// Result of an external command invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub status: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.status == 0
    }

    /// Combined output with the most useful stream first, used for messages.
    pub fn message(&self) -> String {
        let stderr = self.stderr.trim();
        if !stderr.is_empty() {
            return redact(stderr);
        }
        redact(self.stdout.trim())
    }
}

/// Abstraction over process execution so Git and GitHub CLI interactions can be
/// exercised in tests without touching the real system.
pub trait Runner: Send + Sync {
    fn run(&self, program: &str, args: &[&str], stdin: Option<&str>) -> Result<Output>;

    /// Runs a command attached to the current terminal (interactive login).
    fn run_interactive(&self, program: &str, args: &[&str]) -> Result<i32>;

    fn is_available(&self, program: &str) -> bool;
}

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemRunner;

impl Runner for SystemRunner {
    fn run(&self, program: &str, args: &[&str], stdin: Option<&str>) -> Result<Output> {
        let mut child = Command::new(program)
            .args(args)
            .stdin(if stdin.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|source| map_spawn_error(program, source))?;

        if let Some(data) = stdin {
            let mut pipe = child.stdin.take().expect("stdin was requested");
            pipe.write_all(data.as_bytes())
                .map_err(|source| Error::Spawn {
                    program: program.to_string(),
                    source,
                })?;
            // Dropping the handle closes the pipe so the child can proceed.
            drop(pipe);
        }

        let output = child.wait_with_output().map_err(|source| Error::Spawn {
            program: program.to_string(),
            source,
        })?;

        Ok(Output {
            status: output.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        })
    }

    fn run_interactive(&self, program: &str, args: &[&str]) -> Result<i32> {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|source| map_spawn_error(program, source))?;
        Ok(status.code().unwrap_or(-1))
    }

    fn is_available(&self, program: &str) -> bool {
        which::which(program).is_ok()
    }
}

fn map_spawn_error(program: &str, source: std::io::Error) -> Error {
    if source.kind() == std::io::ErrorKind::NotFound {
        return match program {
            "git" => Error::MissingDependency("git"),
            "gh" => Error::MissingDependency("gh"),
            _ => Error::Spawn {
                program: program.to_string(),
                source,
            },
        };
    }
    Error::Spawn {
        program: program.to_string(),
        source,
    }
}

const TOKEN_PREFIXES: [&str; 6] = ["ghp_", "gho_", "ghu_", "ghs_", "ghr_", "github_pat_"];

/// Replaces anything that looks like a GitHub token so it can never reach the
/// terminal, a log line or an error message.
pub fn redact(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = String::with_capacity(input.len());
    let mut index = 0;

    while index < input.len() {
        let rest = &input[index..];
        if let Some(prefix) = TOKEN_PREFIXES.iter().find(|p| rest.starts_with(**p)) {
            let mut end = index + prefix.len();
            while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
                end += 1;
            }
            // Anything longer than the prefix itself is treated as a secret.
            if end - index > 8 {
                out.push_str("[redacted]");
                index = end;
                continue;
            }
        }
        let ch = rest.chars().next().expect("index is on a char boundary");
        out.push(ch);
        index += ch.len_utf8();
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_token_shapes() {
        let input = "Token: ghp_EXAMPLEabcdefghij and github_pat_EXAMPLExyz done";
        let out = redact(input);
        assert!(!out.contains("ghp_EXAMPLEabcdefghij"));
        assert!(!out.contains("github_pat_EXAMPLExyz"));
        assert!(out.contains("[redacted]"));
        assert!(out.contains("Token:"));
        assert!(out.ends_with("done"));
    }

    #[test]
    fn keeps_ordinary_text_intact() {
        let input = "failed to connect to github.com (network unreachable)";
        assert_eq!(redact(input), input);
    }

    #[test]
    fn redacts_quoted_tokens() {
        let out = redact("token=\"gho_EXAMPLE0123456789\",");
        assert!(!out.contains("gho_EXAMPLE0123456789"));
        assert!(out.ends_with("\","));
    }

    #[test]
    fn short_prefix_matches_are_left_alone() {
        assert_eq!(redact("ghp_x"), "ghp_x");
    }
}
