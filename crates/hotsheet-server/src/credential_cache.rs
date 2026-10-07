//! Share managed GitHub sign-in bundles across requests in one machine server.
//! A Keychain approval belongs to the process, not to every provider read. The
//! registry file is non-secret and changes whenever Hot Sheet replaces/deletes a key.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hotsheet_ticketing::connection_removal::MANAGED_CREDENTIAL_PREFIX;
use hotsheet_ticketing::{SecretError, SecretStore};
use sha2::{Digest, Sha256};

type Revision = [u8; 32];

#[derive(Default)]
struct CacheState {
    revision: Option<Revision>,
    values: HashMap<String, String>,
}

#[derive(Clone, Default)]
pub(crate) struct ManagedSecretCache(Arc<Mutex<CacheState>>);

impl ManagedSecretCache {
    pub(crate) fn store<S: SecretStore>(&self, home: PathBuf, backend: S) -> CachedSecretStore<S> {
        CachedSecretStore {
            home,
            backend,
            cache: self.clone(),
        }
    }
}

pub(crate) struct CachedSecretStore<S> {
    home: PathBuf,
    backend: S,
    cache: ManagedSecretCache,
}

impl<S: SecretStore> CachedSecretStore<S> {
    fn revision(&self) -> Result<Option<Revision>, SecretError> {
        match fs::read(self.home.join("keys.json")) {
            Ok(bytes) => Ok(Some(Sha256::digest(bytes).into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, CacheState>, SecretError> {
        let mut state =
            self.cache.0.lock().map_err(|_| {
                SecretError::Backend("managed credential cache lock poisoned".into())
            })?;
        let revision = self.revision()?;
        if state.revision != revision {
            state.values.clear();
            state.revision = revision;
        }
        Ok(state)
    }
}

impl<S: SecretStore> SecretStore for CachedSecretStore<S> {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        if !account.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            return self.backend.set(account, secret);
        }
        let mut state = self.state()?;
        self.backend.set(account, secret)?;
        state.values.insert(account.into(), secret.into());
        Ok(())
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        if !account.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            return self.backend.get(account);
        }
        // Hold one mutex across the OS read: parallel startup requests share one prompt.
        let mut state = self.state()?;
        if let Some(secret) = state.values.get(account) {
            return Ok(Some(secret.clone()));
        }
        let value = self.backend.get(account)?;
        if let Some(secret) = &value {
            state.values.insert(account.into(), secret.clone());
        }
        Ok(value)
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        if !account.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            return self.backend.delete(account);
        }
        let mut state = self.state()?;
        let deleted = self.backend.delete(account)?;
        state.values.remove(account);
        Ok(deleted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::KeyRegistry;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[derive(Clone, Default)]
    struct CountingStore {
        reads: Arc<AtomicUsize>,
        values: Arc<Mutex<HashMap<String, String>>>,
    }

    impl SecretStore for CountingStore {
        fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
            self.values
                .lock()
                .unwrap()
                .insert(account.into(), secret.into());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.values.lock().unwrap().get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            Ok(self.values.lock().unwrap().remove(account).is_some())
        }
    }

    #[test]
    fn managed_reads_are_shared_across_parallel_registries_and_revision_changes() {
        let home = tempfile::tempdir().unwrap();
        let backend = CountingStore::default();
        backend.set("github-app-one", "first").unwrap();
        let cache = ManagedSecretCache::default();
        std::thread::scope(|threads| {
            for _ in 0..20 {
                let cache = cache.clone();
                let backend = backend.clone();
                let home_path = home.path().to_path_buf();
                threads.spawn(move || {
                    let keys =
                        KeyRegistry::new(&home_path, cache.store(home_path.clone(), backend));
                    assert_eq!(keys.get("github-app-one").unwrap(), "first");
                });
            }
        });
        assert_eq!(backend.reads.load(Ordering::SeqCst), 1);
        // A CLI process may replace the credential and update keys.json outside this server.
        backend.set("github-app-one", "second").unwrap();
        fs::write(home.path().join("keys.json"), "{}\n").unwrap();
        let keys = KeyRegistry::new(
            home.path(),
            cache.store(home.path().into(), backend.clone()),
        );
        assert_eq!(keys.get("github-app-one").unwrap(), "second");
        assert_eq!(backend.reads.load(Ordering::SeqCst), 2);
        keys.set("github-app-one", "third").unwrap();
        assert_eq!(keys.get("github-app-one").unwrap(), "third");
        keys.delete("github-app-one").unwrap();
        assert!(matches!(
            keys.get("github-app-one"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn other_credentials_bypass_the_managed_cache() {
        let home = tempfile::tempdir().unwrap();
        let backend = CountingStore::default();
        backend.set("other", "first").unwrap();
        let store = ManagedSecretCache::default().store(home.path().into(), backend.clone());
        assert_eq!(store.get("other").unwrap().as_deref(), Some("first"));
        backend.set("other", "second").unwrap();
        assert_eq!(store.get("other").unwrap().as_deref(), Some("second"));
        assert_eq!(backend.reads.load(Ordering::SeqCst), 2);
    }
}
