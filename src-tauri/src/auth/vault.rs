//! Credential storage (docs/spec/03 "Secret storage").
//!
//! Refresh credentials live only in the operating-system vault: Windows Credential
//! Manager, macOS Keychain or the Linux Secret Service. When no usable vault exists
//! the launcher offers an explicit session-only mode that keeps the secret in memory.
//! There is no plaintext or "encrypted file" fallback.

use std::collections::HashMap;
use std::sync::{Mutex, PoisonError};

use crate::error::{ErrorCode, LauncherError, LauncherResult};
use crate::secrets::SecretString;

/// Vault service name; one entry per account under this service.
pub const SERVICE: &str = "app.launcher";

/// The owned secret store abstraction (doc 02 own interfaces).
pub trait SecretStore: Send + Sync {
    /// Stores or replaces the secret for `key`.
    fn put(&self, key: &str, secret: &SecretString) -> LauncherResult<()>;
    /// Returns the secret for `key`, `Ok(None)` when absent.
    fn get(&self, key: &str) -> LauncherResult<Option<SecretString>>;
    /// Deletes the secret; deleting an absent entry succeeds.
    fn delete(&self, key: &str) -> LauncherResult<()>;
    /// True for the OS vault; false for the session-only memory store.
    fn persistent(&self) -> bool;
}

fn unavailable(error: &keyring::Error) -> LauncherError {
    LauncherError::new(
        ErrorCode::SecretStoreUnavailable,
        "The system credential store is not available. You can stay signed in for this \
         session only.",
    )
    .with_detail("vaultError", vault_error_kind(error))
}

fn vault_error_kind(error: &keyring::Error) -> &'static str {
    match error {
        keyring::Error::PlatformFailure(_) => "platform_failure",
        keyring::Error::NoStorageAccess(_) => "no_storage_access",
        keyring::Error::NoEntry => "no_entry",
        keyring::Error::BadEncoding(_) => "bad_encoding",
        keyring::Error::TooLong(_, _) => "too_long",
        keyring::Error::Invalid(_, _) => "invalid",
        keyring::Error::Ambiguous(_) => "ambiguous",
        _ => "other",
    }
}

/// The operating-system credential vault.
pub struct KeyringStore;

impl KeyringStore {
    fn entry(key: &str) -> LauncherResult<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).map_err(|e| unavailable(&e))
    }

    /// Checks that the vault answers at all, without storing anything.
    pub fn probe() -> LauncherResult<()> {
        match Self::entry("availability-probe")?.get_password() {
            Ok(_) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(unavailable(&error)),
        }
    }
}

impl SecretStore for KeyringStore {
    fn put(&self, key: &str, secret: &SecretString) -> LauncherResult<()> {
        Self::entry(key)?
            .set_password(secret.expose())
            .map_err(|e| unavailable(&e))
    }

    fn get(&self, key: &str) -> LauncherResult<Option<SecretString>> {
        match Self::entry(key)?.get_password() {
            Ok(value) => Ok(Some(SecretString::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(unavailable(&error)),
        }
    }

    fn delete(&self, key: &str) -> LauncherResult<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(LauncherError::new(
                ErrorCode::SecretStoreDeleteFailed,
                "Saved sign-in credentials could not be removed from the system credential \
                 store. Try signing out again.",
            )
            .with_detail("vaultError", vault_error_kind(&error))),
        }
    }

    fn persistent(&self) -> bool {
        true
    }
}

/// Session-only storage: secrets are lost when the launcher exits.
#[derive(Default)]
pub struct MemoryStore {
    entries: Mutex<HashMap<String, SecretString>>,
}

impl SecretStore for MemoryStore {
    fn put(&self, key: &str, secret: &SecretString) -> LauncherResult<()> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(key.to_owned(), SecretString::new(secret.expose()));
        Ok(())
    }

    fn get(&self, key: &str) -> LauncherResult<Option<SecretString>> {
        Ok(self
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(key)
            .map(|s| SecretString::new(s.expose())))
    }

    fn delete(&self, key: &str) -> LauncherResult<()> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(key);
        Ok(())
    }

    fn persistent(&self) -> bool {
        false
    }
}

#[cfg(test)]
pub mod test_support {
    use super::*;

    /// A store whose operations can be made to fail, for deletion-failure tests.
    #[derive(Default)]
    pub struct FlakyStore {
        pub inner: MemoryStore,
        pub fail_delete: std::sync::atomic::AtomicBool,
        pub fail_put: std::sync::atomic::AtomicBool,
    }

    impl SecretStore for FlakyStore {
        fn put(&self, key: &str, secret: &SecretString) -> LauncherResult<()> {
            if self.fail_put.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(LauncherError::new(ErrorCode::SecretStoreUnavailable, "x"));
            }
            self.inner.put(key, secret)
        }
        fn get(&self, key: &str) -> LauncherResult<Option<SecretString>> {
            self.inner.get(key)
        }
        fn delete(&self, key: &str) -> LauncherResult<()> {
            if self.fail_delete.load(std::sync::atomic::Ordering::SeqCst) {
                return Err(LauncherError::new(ErrorCode::SecretStoreDeleteFailed, "x"));
            }
            self.inner.delete(key)
        }
        fn persistent(&self) -> bool {
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_store_round_trip() {
        let store = MemoryStore::default();
        assert!(store.get("k").unwrap().is_none());
        store.put("k", &SecretString::new("v1")).unwrap();
        store.put("k", &SecretString::new("v2")).unwrap();
        assert_eq!(store.get("k").unwrap().unwrap().expose(), "v2");
        store.delete("k").unwrap();
        store.delete("k").unwrap();
        assert!(store.get("k").unwrap().is_none());
        assert!(!store.persistent());
    }

    /// Exercises the real OS vault when one is available (desktop sessions). On
    /// headless CI runners the vault is typically unavailable, which is reported as
    /// SECRET_STORE_UNAVAILABLE rather than silently falling back.
    #[test]
    fn os_vault_round_trip_or_explicit_unavailability() {
        match KeyringStore::probe() {
            Ok(()) => {
                let store = KeyringStore;
                let key = format!("test-{}", uuid::Uuid::new_v4());
                store.put(&key, &SecretString::new("secret-value")).unwrap();
                assert_eq!(store.get(&key).unwrap().unwrap().expose(), "secret-value");
                store.delete(&key).unwrap();
                assert!(store.get(&key).unwrap().is_none());
            }
            Err(error) => assert_eq!(error.code, ErrorCode::SecretStoreUnavailable),
        }
    }
}
