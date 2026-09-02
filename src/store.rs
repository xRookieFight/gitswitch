use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::model::Account;

/// Schema version written to disk. Bump whenever the on-disk shape changes and
/// add the corresponding step to [`migrate`].
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Config {
    pub version: u32,
    /// Name of the profile gitswitch last activated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default)]
    pub accounts: Vec<Account>,
}

impl Config {
    fn empty() -> Self {
        Self {
            version: SCHEMA_VERSION,
            active: None,
            accounts: Vec::new(),
        }
    }
}

/// Reads and writes the account list. The file is created with owner-only
/// permissions and replaced atomically so a crash cannot truncate it.
#[derive(Debug, Clone)]
pub struct Store {
    path: PathBuf,
    config: Config,
}

impl Store {
    /// Default location, e.g. `~/.config/gitswitch/accounts.json`.
    pub fn default_path() -> Result<PathBuf> {
        if let Some(custom) = std::env::var_os("GITSWITCH_CONFIG_DIR") {
            return Ok(PathBuf::from(custom).join("accounts.json"));
        }
        let dirs = directories::ProjectDirs::from("", "", "gitswitch").ok_or(Error::NoConfigDir)?;
        Ok(dirs.config_dir().join("accounts.json"))
    }

    pub fn open_default() -> Result<Self> {
        Self::open(Self::default_path()?)
    }

    pub fn open(path: impl Into<PathBuf>) -> Result<Self> {
        let path = path.into();
        let config = match fs::read_to_string(&path) {
            Ok(raw) => parse(&path, &raw)?,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Config::empty(),
            Err(source) => {
                return Err(Error::ConfigRead {
                    path: path.clone(),
                    source,
                });
            }
        };
        Ok(Self { path, config })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn accounts(&self) -> &[Account] {
        &self.config.accounts
    }

    pub fn is_empty(&self) -> bool {
        self.config.accounts.is_empty()
    }

    pub fn get(&self, name: &str) -> Option<&Account> {
        self.config
            .accounts
            .iter()
            .find(|a| a.name.eq_ignore_ascii_case(name))
    }

    pub fn require(&self, name: &str) -> Result<&Account> {
        self.get(name)
            .ok_or_else(|| Error::UnknownAccount(name.to_string()))
    }

    pub fn active(&self) -> Option<&Account> {
        self.config
            .active
            .as_deref()
            .and_then(|name| self.get(name))
    }

    pub fn add(&mut self, account: Account) -> Result<()> {
        account.validate()?;
        if self.get(&account.name).is_some() {
            return Err(Error::DuplicateAccount(account.name));
        }
        self.config.accounts.push(account);
        self.save()
    }

    /// Replaces the stored account with the same name.
    pub fn update(&mut self, account: Account) -> Result<()> {
        account.validate()?;
        let slot = self
            .config
            .accounts
            .iter_mut()
            .find(|a| a.name.eq_ignore_ascii_case(&account.name))
            .ok_or_else(|| Error::UnknownAccount(account.name.clone()))?;
        *slot = account;
        self.save()
    }

    pub fn remove(&mut self, name: &str) -> Result<Account> {
        let index = self
            .config
            .accounts
            .iter()
            .position(|a| a.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| Error::UnknownAccount(name.to_string()))?;
        let removed = self.config.accounts.remove(index);
        if self
            .config
            .active
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(name))
        {
            self.config.active = None;
        }
        self.save()?;
        Ok(removed)
    }

    pub fn rename(&mut self, from: &str, to: &str) -> Result<()> {
        crate::model::validate_name(to)?;
        if self.get(to).is_some() && !from.eq_ignore_ascii_case(to) {
            return Err(Error::DuplicateAccount(to.to_string()));
        }
        let account = self
            .config
            .accounts
            .iter_mut()
            .find(|a| a.name.eq_ignore_ascii_case(from))
            .ok_or_else(|| Error::UnknownAccount(from.to_string()))?;
        account.name = to.to_string();
        if self
            .config
            .active
            .as_deref()
            .is_some_and(|active| active.eq_ignore_ascii_case(from))
        {
            self.config.active = Some(to.to_string());
        }
        self.save()
    }

    pub fn set_active(&mut self, name: &str) -> Result<()> {
        let account = self.require(name)?;
        self.config.active = Some(account.name.clone());
        self.save()
    }

    pub fn save(&self) -> Result<()> {
        let mut config = self.config.clone();
        config.version = SCHEMA_VERSION;
        let body = serde_json::to_string_pretty(&config).map_err(|err| Error::ConfigCorrupt {
            path: self.path.clone(),
            message: err.to_string(),
        })?;
        write_private(&self.path, &body)
    }
}

fn parse(path: &Path, raw: &str) -> Result<Config> {
    if raw.trim().is_empty() {
        return Ok(Config::empty());
    }
    let value: Value = serde_json::from_str(raw).map_err(|err| Error::ConfigCorrupt {
        path: path.to_path_buf(),
        message: err.to_string(),
    })?;
    let migrated = migrate(path, value)?;
    serde_json::from_value(migrated).map_err(|err| Error::ConfigCorrupt {
        path: path.to_path_buf(),
        message: err.to_string(),
    })
}

/// Brings older documents up to [`SCHEMA_VERSION`].
fn migrate(path: &Path, mut value: Value) -> Result<Value> {
    let version = value.get("version").and_then(Value::as_u64).unwrap_or(0) as u32;

    if version > SCHEMA_VERSION {
        return Err(Error::ConfigTooNew {
            path: path.to_path_buf(),
            found: version,
            supported: SCHEMA_VERSION,
        });
    }

    if version == 0 {
        // v0 kept accounts in an object keyed by profile name and called the
        // selected profile `current`.
        let object = value.as_object_mut().ok_or_else(|| Error::ConfigCorrupt {
            path: path.to_path_buf(),
            message: "expected a JSON object at the top level".into(),
        })?;

        if let Some(current) = object.remove("current") {
            object.insert("active".into(), current);
        }
        if let Some(Value::Object(map)) = object.get("accounts").cloned() {
            let accounts = map
                .into_iter()
                .map(|(name, mut account)| {
                    if let Some(entry) = account.as_object_mut() {
                        entry.insert("name".into(), Value::String(name));
                    }
                    account
                })
                .collect::<Vec<_>>();
            object.insert("accounts".into(), Value::Array(accounts));
        }
        object.insert("version".into(), Value::from(SCHEMA_VERSION));
    }

    Ok(value)
}

/// Writes `body` to `path` via a temporary file in the same directory, with
/// owner-only permissions on Unix.
fn write_private(path: &Path, body: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|source| Error::ConfigWrite {
            path: parent.to_path_buf(),
            source,
        })?;
        restrict_dir(parent)?;
    }

    let temp = path.with_extension("json.tmp");
    write_file(&temp, body)?;
    fs::rename(&temp, path).map_err(|source| Error::ConfigWrite {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(())
}

#[cfg(unix)]
fn write_file(path: &Path, body: &str) -> Result<()> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;

    let mut file = fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .map_err(|source| Error::ConfigWrite {
            path: path.to_path_buf(),
            source,
        })?;
    file.write_all(body.as_bytes())
        .map_err(|source| Error::ConfigWrite {
            path: path.to_path_buf(),
            source,
        })
}

#[cfg(not(unix))]
fn write_file(path: &Path, body: &str) -> Result<()> {
    fs::write(path, body).map_err(|source| Error::ConfigWrite {
        path: path.to_path_buf(),
        source,
    })
}

#[cfg(unix)]
fn restrict_dir(path: &Path) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;

    let mut perms = fs::metadata(path)
        .map_err(|source| Error::ConfigWrite {
            path: path.to_path_buf(),
            source,
        })?
        .permissions();
    if perms.mode() & 0o077 != 0 {
        perms.set_mode(0o700);
        fs::set_permissions(path, perms).map_err(|source| Error::ConfigWrite {
            path: path.to_path_buf(),
            source,
        })?;
    }
    Ok(())
}

#[cfg(not(unix))]
fn restrict_dir(_path: &Path) -> Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn store(dir: &TempDir) -> Store {
        Store::open(dir.path().join("accounts.json")).unwrap()
    }

    fn sample(name: &str) -> Account {
        Account::new(name, "octocat", "Octo Cat", "octo@example.com")
    }

    #[test]
    fn missing_file_starts_empty() {
        let dir = TempDir::new().unwrap();
        assert!(store(&dir).is_empty());
    }

    #[test]
    fn accounts_persist_between_sessions() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.json");
        let mut first = Store::open(&path).unwrap();
        first.add(sample("work")).unwrap();
        first.set_active("work").unwrap();

        let second = Store::open(&path).unwrap();
        assert_eq!(second.accounts().len(), 1);
        assert_eq!(second.active().unwrap().name, "work");
    }

    #[test]
    fn duplicate_names_are_rejected_case_insensitively() {
        let dir = TempDir::new().unwrap();
        let mut store = store(&dir);
        store.add(sample("work")).unwrap();
        let err = store.add(sample("WORK")).unwrap_err();
        assert!(matches!(err, Error::DuplicateAccount(_)));
    }

    #[test]
    fn removing_the_active_account_clears_it() {
        let dir = TempDir::new().unwrap();
        let mut store = store(&dir);
        store.add(sample("work")).unwrap();
        store.set_active("work").unwrap();
        store.remove("work").unwrap();
        assert!(store.active().is_none());
        assert!(store.is_empty());
    }

    #[test]
    fn removing_an_unknown_account_errors() {
        let dir = TempDir::new().unwrap();
        let mut store = store(&dir);
        assert!(matches!(
            store.remove("ghost").unwrap_err(),
            Error::UnknownAccount(_)
        ));
    }

    #[test]
    fn rename_moves_the_active_marker() {
        let dir = TempDir::new().unwrap();
        let mut store = store(&dir);
        store.add(sample("work")).unwrap();
        store.set_active("work").unwrap();
        store.rename("work", "job").unwrap();
        assert_eq!(store.active().unwrap().name, "job");
    }

    #[test]
    fn rename_onto_an_existing_name_is_rejected() {
        let dir = TempDir::new().unwrap();
        let mut store = store(&dir);
        store.add(sample("work")).unwrap();
        store.add(sample("personal")).unwrap();
        assert!(matches!(
            store.rename("work", "personal").unwrap_err(),
            Error::DuplicateAccount(_)
        ));
    }

    #[test]
    fn corrupted_json_is_reported_with_the_path() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.json");
        fs::write(&path, "{not json").unwrap();
        let err = Store::open(&path).unwrap_err();
        match err {
            Error::ConfigCorrupt { path: p, .. } => assert_eq!(p, path),
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn newer_schema_versions_are_refused() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.json");
        fs::write(&path, r#"{"version": 999, "accounts": []}"#).unwrap();
        assert!(matches!(
            Store::open(&path).unwrap_err(),
            Error::ConfigTooNew { .. }
        ));
    }

    #[test]
    fn legacy_documents_are_migrated() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.json");
        let legacy = r#"{
            "current": "work",
            "accounts": {
                "work": {
                    "username": "octocat",
                    "git_name": "Octo Cat",
                    "git_email": "octo@example.com"
                }
            }
        }"#;
        fs::write(&path, legacy).unwrap();

        let store = Store::open(&path).unwrap();
        assert_eq!(store.accounts().len(), 1);
        let account = store.get("work").unwrap();
        assert_eq!(account.username, "octocat");
        assert_eq!(account.host, crate::model::DEFAULT_HOST);
        assert_eq!(store.active().unwrap().name, "work");
    }

    #[test]
    fn empty_file_is_treated_as_no_accounts() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("accounts.json");
        fs::write(&path, "   \n").unwrap();
        assert!(Store::open(&path).unwrap().is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn config_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("nested").join("accounts.json");
        let mut store = Store::open(&path).unwrap();
        store.add(sample("work")).unwrap();

        let mode = fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
        let dir_mode = fs::metadata(path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(dir_mode & 0o077, 0);
    }
}
