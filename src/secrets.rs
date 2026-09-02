use crate::error::{Error, Result};

/// Service name used for every entry gitswitch creates in the OS keychain.
pub const SERVICE: &str = "gitswitch";

/// Storage for GitHub tokens. Implemented by the OS credential store in
/// production and by an in-memory double in tests.
///
/// Tokens are the only secret gitswitch handles; passwords are never accepted
/// or stored.
pub trait SecretStore: Send + Sync {
    fn set(&self, key: &str, token: &str) -> Result<()>;
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn delete(&self, key: &str) -> Result<()>;
    /// Whether the backend is usable on this machine.
    fn available(&self) -> bool;
}

/// Backed by the platform keychain: Keychain on macOS, Credential Manager on
/// Windows, Secret Service on Linux.
#[derive(Debug, Default, Clone, Copy)]
pub struct KeyringStore;

impl KeyringStore {
    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).map_err(map_err)
    }
}

impl SecretStore for KeyringStore {
    fn set(&self, key: &str, token: &str) -> Result<()> {
        Self::entry(key)?.set_password(token).map_err(map_err)
    }

    fn get(&self, key: &str) -> Result<Option<String>> {
        match Self::entry(key)?.get_password() {
            Ok(token) => Ok(Some(token)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(map_err(err)),
        }
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(map_err(err)),
        }
    }

    fn available(&self) -> bool {
        Self::entry("gitswitch-probe").is_ok()
    }
}

/// The keyring crate's messages never contain the secret itself, but they are
/// mapped through [`crate::process::redact`] as a second line of defence.
fn map_err(err: keyring::Error) -> Error {
    Error::Secret(crate::process::redact(&err.to_string()))
}

/// Discards every token. Used when the platform has no usable credential store
/// so that gitswitch keeps working without ever falling back to plaintext.
#[derive(Debug, Default, Clone, Copy)]
pub struct NullStore;

impl SecretStore for NullStore {
    fn set(&self, _key: &str, _token: &str) -> Result<()> {
        Err(Error::Secret(
            "no OS credential store is available on this system".into(),
        ))
    }

    fn get(&self, _key: &str) -> Result<Option<String>> {
        Ok(None)
    }

    fn delete(&self, _key: &str) -> Result<()> {
        Ok(())
    }

    fn available(&self) -> bool {
        false
    }
}

/// Returns the keyring when it works on this machine, otherwise a no-op store.
pub fn default_store() -> Box<dyn SecretStore> {
    let keyring = KeyringStore;
    if keyring.available() {
        Box::new(keyring)
    } else {
        Box::new(NullStore)
    }
}

/// In-memory credential store used by the test suite and by contributors who
/// want to exercise gitswitch without touching their real keychain.
pub mod memory {
    use std::collections::HashMap;
    use std::sync::Mutex;

    use super::*;

    #[derive(Debug, Default)]
    pub struct MemoryStore {
        entries: Mutex<HashMap<String, String>>,
    }

    impl SecretStore for MemoryStore {
        fn set(&self, key: &str, token: &str) -> Result<()> {
            self.entries
                .lock()
                .expect("secret store poisoned")
                .insert(key.to_string(), token.to_string());
            Ok(())
        }

        fn get(&self, key: &str) -> Result<Option<String>> {
            Ok(self
                .entries
                .lock()
                .expect("secret store poisoned")
                .get(key)
                .cloned())
        }

        fn delete(&self, key: &str) -> Result<()> {
            self.entries
                .lock()
                .expect("secret store poisoned")
                .remove(key);
            Ok(())
        }

        fn available(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::memory::MemoryStore;
    use super::*;

    #[test]
    fn memory_store_round_trips() {
        let store = MemoryStore::default();
        store.set("github.com:octocat", "ghp_EXAMPLEtoken").unwrap();
        assert_eq!(
            store.get("github.com:octocat").unwrap().as_deref(),
            Some("ghp_EXAMPLEtoken")
        );
        store.delete("github.com:octocat").unwrap();
        assert!(store.get("github.com:octocat").unwrap().is_none());
    }

    #[test]
    fn null_store_refuses_to_persist_tokens() {
        let store = NullStore;
        assert!(store.set("k", "ghp_EXAMPLEtoken").is_err());
        assert!(store.get("k").unwrap().is_none());
        assert!(!store.available());
    }
}
