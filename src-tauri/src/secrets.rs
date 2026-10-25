//! OS keychain (Windows Credential Manager, macOS Keychain, Secret Service on Linux),
//! falling back to an owner-only file when no keychain is reachable.

use alloy_core::auth::{FileSecretStore, SecretStore};

const SERVICE: &str = "dev.elchi.alloy";

pub struct KeyringStore {
    fallback: FileSecretStore,
}

impl KeyringStore {
    pub fn new(fallback: FileSecretStore) -> Self {
        Self { fallback }
    }

    fn entry(key: &str) -> Option<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).ok()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>, String> {
        if let Some(entry) = Self::entry(key) {
            match entry.get_password() {
                Ok(v) => return Ok(Some(v)),
                Err(keyring::Error::NoEntry) => {}
                Err(e) => tracing::warn!("keychain read failed, trying file store: {e}"),
            }
        }
        self.fallback.get(key)
    }

    fn set(&self, key: &str, value: &str) -> Result<(), String> {
        if let Some(entry) = Self::entry(key) {
            match entry.set_password(value) {
                Ok(()) => {
                    // Don't leave an older copy behind in the file store.
                    let _ = self.fallback.delete(key);
                    return Ok(());
                }
                Err(e) => tracing::warn!("keychain write failed, using file store: {e}"),
            }
        }
        self.fallback.set(key, value)
    }

    fn delete(&self, key: &str) -> Result<(), String> {
        if let Some(entry) = Self::entry(key) {
            let _ = entry.delete_credential();
        }
        self.fallback.delete(key)
    }
}
