use serde::{Deserialize, Serialize};

use crate::error::{Error, Result};

pub const DEFAULT_HOST: &str = "github.com";

/// A saved GitHub profile. Never holds a token - those live in the OS
/// credential store, keyed by [`Account::secret_key`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    /// Profile label chosen by the user, e.g. `work`.
    pub name: String,
    /// GitHub login the profile authenticates as.
    pub username: String,
    /// Value written to `user.name`.
    pub git_name: String,
    /// Value written to `user.email`.
    pub git_email: String,
    #[serde(default = "default_host")]
    pub host: String,
    /// True when a token for this account is cached in the OS credential store.
    #[serde(default)]
    pub has_stored_token: bool,
}

fn default_host() -> String {
    DEFAULT_HOST.to_string()
}

impl Account {
    pub fn new(
        name: impl Into<String>,
        username: impl Into<String>,
        git_name: impl Into<String>,
        git_email: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            username: username.into(),
            git_name: git_name.into(),
            git_email: git_email.into(),
            host: default_host(),
            has_stored_token: false,
        }
    }

    pub fn with_host(mut self, host: impl Into<String>) -> Self {
        self.host = host.into();
        self
    }

    /// Identifier used as the credential store entry name.
    pub fn secret_key(&self) -> String {
        format!("{}:{}", self.host, self.username)
    }

    pub fn validate(&self) -> Result<()> {
        validate_name(&self.name)?;
        validate_username(&self.username)?;
        if self.git_name.trim().is_empty() {
            return Err(Error::InvalidInput("Git name must not be empty".into()));
        }
        validate_email(&self.git_email)?;
        if self.host.trim().is_empty() {
            return Err(Error::InvalidInput("host must not be empty".into()));
        }
        Ok(())
    }
}

/// Profile labels double as CLI arguments, so keep them shell friendly.
pub fn validate_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::InvalidInput("account name must not be empty".into()));
    }
    if name.len() > 64 {
        return Err(Error::InvalidInput(
            "account name must be 64 characters or fewer".into(),
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
    {
        return Err(Error::InvalidInput(
            "account name may only contain letters, digits, '-', '_' and '.'".into(),
        ));
    }
    Ok(())
}

pub fn validate_username(username: &str) -> Result<()> {
    if username.is_empty() {
        return Err(Error::InvalidInput(
            "GitHub username must not be empty".into(),
        ));
    }
    if username.len() > 39 {
        return Err(Error::InvalidInput(
            "GitHub usernames are at most 39 characters".into(),
        ));
    }
    if !username
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-')
    {
        return Err(Error::InvalidInput(
            "GitHub usernames may only contain letters, digits and '-'".into(),
        ));
    }
    Ok(())
}

pub fn validate_email(email: &str) -> Result<()> {
    let trimmed = email.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput("Git email must not be empty".into()));
    }
    let mut parts = trimmed.split('@');
    let local = parts.next().unwrap_or_default();
    let domain = parts.next().unwrap_or_default();
    if parts.next().is_some() || local.is_empty() || domain.is_empty() || !domain.contains('.') {
        return Err(Error::InvalidInput(format!(
            "`{trimmed}` is not a valid email address"
        )));
    }
    Ok(())
}

/// Basic sanity check for a pasted personal access token. The value itself is
/// never echoed back.
pub fn validate_token(token: &str) -> Result<()> {
    let trimmed = token.trim();
    if trimmed.is_empty() {
        return Err(Error::InvalidInput("token must not be empty".into()));
    }
    if trimmed.len() < 20 || trimmed.contains(char::is_whitespace) {
        return Err(Error::InvalidInput(
            "that does not look like a GitHub token".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn account_names_are_restricted() {
        assert!(validate_name("work").is_ok());
        assert!(validate_name("open-source.2").is_ok());
        assert!(validate_name("").is_err());
        assert!(validate_name("has space").is_err());
        assert!(validate_name("semi;colon").is_err());
    }

    #[test]
    fn usernames_follow_github_rules() {
        assert!(validate_username("xRookieFight").is_ok());
        assert!(validate_username("bad_user").is_err());
        assert!(validate_username(&"a".repeat(40)).is_err());
    }

    #[test]
    fn emails_need_a_domain() {
        assert!(validate_email("dev@example.com").is_ok());
        assert!(validate_email("dev@example").is_err());
        assert!(validate_email("dev.example.com").is_err());
        assert!(validate_email("a@b@c.com").is_err());
    }

    #[test]
    fn tokens_are_checked_without_being_echoed() {
        assert!(validate_token("ghp_EXAMPLE0123456789abc").is_ok());
        assert!(validate_token("short").is_err());
        let err = validate_token("has space in it here").unwrap_err();
        assert!(!err.to_string().contains("has space"));
    }

    #[test]
    fn secret_key_is_host_scoped() {
        let account = Account::new("work", "octocat", "Octo Cat", "octo@example.com")
            .with_host("github.example.com");
        assert_eq!(account.secret_key(), "github.example.com:octocat");
    }
}
