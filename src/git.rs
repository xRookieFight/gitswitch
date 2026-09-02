use crate::error::{Error, Result};
use crate::process::{Output, Runner};

/// Which git configuration file an operation targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Scope {
    /// `~/.gitconfig` - the identity used by every repository by default.
    #[default]
    Global,
    /// `.git/config` of the repository in the working directory.
    Local,
}

impl Scope {
    pub fn flag(self) -> &'static str {
        match self {
            Scope::Global => "--global",
            Scope::Local => "--local",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Scope::Global => "global",
            Scope::Local => "repository",
        }
    }
}

/// The `user.name` / `user.email` pair git would use right now.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Identity {
    pub name: Option<String>,
    pub email: Option<String>,
}

impl Identity {
    pub fn is_complete(&self) -> bool {
        self.name.is_some() && self.email.is_some()
    }
}

pub struct Git<'r> {
    runner: &'r dyn Runner,
}

impl<'r> Git<'r> {
    pub fn new(runner: &'r dyn Runner) -> Self {
        Self { runner }
    }

    pub fn is_installed(&self) -> bool {
        self.runner.is_available("git")
    }

    pub fn version(&self) -> Result<String> {
        let out = self.exec(&["--version"])?;
        Ok(out.stdout.trim().to_string())
    }

    /// Effective identity, honouring the local repository when there is one.
    pub fn identity(&self) -> Result<Identity> {
        Ok(Identity {
            name: self.config_get(None, "user.name")?,
            email: self.config_get(None, "user.email")?,
        })
    }

    pub fn identity_in(&self, scope: Scope) -> Result<Identity> {
        Ok(Identity {
            name: self.config_get(Some(scope), "user.name")?,
            email: self.config_get(Some(scope), "user.email")?,
        })
    }

    pub fn set_identity(&self, scope: Scope, name: &str, email: &str) -> Result<()> {
        self.config_set(scope, "user.name", name)?;
        self.config_set(scope, "user.email", email)
    }

    pub fn is_inside_repository(&self) -> bool {
        self.exec(&["rev-parse", "--is-inside-work-tree"])
            .map(|out| out.ok() && out.stdout.trim() == "true")
            .unwrap_or(false)
    }

    fn config_get(&self, scope: Option<Scope>, key: &str) -> Result<Option<String>> {
        let mut args = vec!["config"];
        if let Some(scope) = scope {
            args.push(scope.flag());
        }
        args.push("--get");
        args.push(key);

        let out = self.run(&args)?;
        // git exits with 1 when the key is simply not set, which is not an error.
        if out.status == 1 && out.stderr.trim().is_empty() {
            return Ok(None);
        }
        if !out.ok() {
            return Err(fail("git config --get", &out));
        }
        let value = out.stdout.trim();
        Ok((!value.is_empty()).then(|| value.to_string()))
    }

    fn config_set(&self, scope: Scope, key: &str, value: &str) -> Result<()> {
        self.exec(&["config", scope.flag(), key, value]).map(|_| ())
    }

    fn exec(&self, args: &[&str]) -> Result<Output> {
        let out = self.run(args)?;
        if !out.ok() {
            return Err(fail(&format!("git {}", args.join(" ")), &out));
        }
        Ok(out)
    }

    fn run(&self, args: &[&str]) -> Result<Output> {
        self.runner.run("git", args, None)
    }
}

fn fail(command: &str, out: &Output) -> Error {
    Error::CommandFailed {
        command: command.to_string(),
        status: out.status,
        message: out.message(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::MockRunner;

    #[test]
    fn reads_the_effective_identity() {
        let runner = MockRunner::new()
            .ok("git config --get user.name", "Octo Cat")
            .ok("git config --get user.email", "octo@example.com");
        let git = Git::new(&runner);

        let identity = git.identity().unwrap();
        assert_eq!(identity.name.as_deref(), Some("Octo Cat"));
        assert_eq!(identity.email.as_deref(), Some("octo@example.com"));
        assert!(identity.is_complete());
    }

    #[test]
    fn unset_keys_are_none_not_errors() {
        let runner = MockRunner::new()
            .status("git config --get user.name", 1, "", "")
            .status("git config --get user.email", 1, "", "");
        let identity = Git::new(&runner).identity().unwrap();
        assert_eq!(identity, Identity::default());
        assert!(!identity.is_complete());
    }

    #[test]
    fn config_errors_are_surfaced() {
        let runner = MockRunner::new().status(
            "git config --get user.name",
            128,
            "",
            "fatal: bad config line 3",
        );
        let err = Git::new(&runner).identity().unwrap_err();
        assert!(err.to_string().contains("bad config line 3"));
    }

    #[test]
    fn setting_the_identity_writes_both_keys() {
        let runner = MockRunner::new()
            .ok("git config --global user.name Octo Cat", "")
            .ok("git config --global user.email octo@example.com", "");
        Git::new(&runner)
            .set_identity(Scope::Global, "Octo Cat", "octo@example.com")
            .unwrap();
        assert_eq!(runner.calls().len(), 2);
    }

    #[test]
    fn local_scope_uses_the_local_flag() {
        let runner = MockRunner::new()
            .ok("git config --local user.name Octo Cat", "")
            .ok("git config --local user.email octo@example.com", "");
        Git::new(&runner)
            .set_identity(Scope::Local, "Octo Cat", "octo@example.com")
            .unwrap();
        assert!(runner.calls()[0].contains("--local"));
    }

    #[test]
    fn repository_detection_reads_rev_parse() {
        let inside = MockRunner::new().ok("git rev-parse --is-inside-work-tree", "true");
        assert!(Git::new(&inside).is_inside_repository());

        let outside = MockRunner::new().status(
            "git rev-parse --is-inside-work-tree",
            128,
            "",
            "fatal: not a git repository",
        );
        assert!(!Git::new(&outside).is_inside_repository());
    }

    #[test]
    fn missing_git_is_reported_as_a_dependency_problem() {
        let runner = MockRunner::new().missing("git");
        let err = Git::new(&runner).version().unwrap_err();
        assert!(matches!(err, Error::MissingDependency("git")));
        assert!(err.hint().unwrap().contains("git-scm.com"));
    }
}
