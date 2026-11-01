//! Share managed GitHub sign-in bundles across requests in one machine server.
//! A Keychain approval belongs to the process, not to every provider read. The
//! non-secret stored generation enables caching only after metadata publication.
//! Old generation-less rows bypass values and denials (HS2-HWC0FS).

use hotsheet_sync::LockExt;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use hotsheet_ticketing::connection_removal::MANAGED_CREDENTIAL_PREFIX;
use hotsheet_ticketing::{
    KeyMetadataCacheObservation, SecretError, SecretStore, key_metadata_cache_observation,
};

#[derive(Default)]
struct CacheState {
    observation: Option<KeyMetadataCacheObservation>,
    values: HashMap<String, Result<String, String>>,
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
    fn state(
        &self,
        account: &str,
    ) -> Result<
        (
            std::sync::MutexGuard<'_, CacheState>,
            KeyMetadataCacheObservation,
        ),
        SecretError,
    > {
        let mut state = self.cache.0.lock_or_recover();
        let observation = key_metadata_cache_observation(&self.home, account)?;
        if state
            .observation
            .as_ref()
            .is_none_or(|prior| !prior.same_document(&observation))
        {
            state.values.clear();
        }
        state.observation = Some(observation.clone());
        Ok((state, observation))
    }
}

impl<S: SecretStore> SecretStore for CachedSecretStore<S> {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        if !account.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            return self.backend.set(account, secret);
        }
        let mut state = self.cache.0.lock_or_recover();
        // HS2-HWC0FS: this native phase precedes the owner's generation commit.
        // Evict even on failure; never seed a new value under the old generation.
        state.values.remove(account);
        self.backend.set(account, secret)
    }

    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        if !account.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            return self.backend.get(account);
        }
        // Hold one mutex across the OS read: parallel startup requests share one prompt.
        let (mut state, before) = self.state(account)?;
        if !before.is_cacheable() {
            state.values.remove(account);
            drop(state);
            return self.backend.get(account);
        }
        if let Some(secret) = state.values.get(account) {
            return secret.clone().map(Some).map_err(SecretError::Backend);
        }
        let value = self.backend.get(account);
        let after = key_metadata_cache_observation(&self.home, account)?;
        let same = before.same_document(&after);
        if !same {
            state.values.clear();
        }
        state.observation = Some(after.clone());
        if !before.is_cacheable() || !after.is_cacheable() || !same {
            state.values.remove(account);
            return value;
        }
        match &value {
            Ok(Some(secret)) => {
                state.values.insert(account.into(), Ok(secret.clone()));
            }
            Err(error) => {
                // A refusal is shared only for this eligible, unchanged document.
                state.values.insert(account.into(), Err(error.to_string()));
            }
            Ok(None) => {}
        }
        value
    }

    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        if !account.starts_with(MANAGED_CREDENTIAL_PREFIX) {
            return self.backend.delete(account);
        }
        let mut state = self.cache.0.lock_or_recover();
        state.values.remove(account);
        self.backend.delete(account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_ticketing::KeyRegistry;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[cfg(any(unix, windows))]
    mod generation_tests;

    #[derive(Clone, Default)]
    struct CountingStore {
        reads: Arc<AtomicUsize>,
        values: Arc<Mutex<HashMap<String, String>>>,
    }

    impl SecretStore for CountingStore {
        fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
            self.values
                .lock_or_recover()
                .insert(account.into(), secret.into());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            self.reads.fetch_add(1, Ordering::SeqCst);
            Ok(self.values.lock_or_recover().get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            Ok(self.values.lock_or_recover().remove(account).is_some())
        }
    }

    #[test]
    fn managed_reads_are_shared_across_parallel_registries_and_revision_changes() {
        let home = tempfile::tempdir().unwrap();
        let backend = CountingStore::default();
        KeyRegistry::new(home.path(), backend.clone())
            .set("github-app-one", "first")
            .unwrap();
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
        KeyRegistry::new(home.path(), backend.clone())
            .set("github-app-one", "second")
            .unwrap();
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

    #[test]
    fn refused_managed_read_is_attempted_only_once_per_revision() {
        #[derive(Clone)]
        struct Denied(Arc<AtomicUsize>);
        impl SecretStore for Denied {
            fn set(&self, _: &str, _: &str) -> Result<(), SecretError> {
                Ok(())
            }
            fn get(&self, _: &str) -> Result<Option<String>, SecretError> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Err(SecretError::Backend("user denied access".into()))
            }
            fn delete(&self, _: &str) -> Result<bool, SecretError> {
                Ok(false)
            }
        }

        let home = tempfile::tempdir().unwrap();
        let reads = Arc::new(AtomicUsize::new(0));
        let backend = Denied(reads.clone());
        KeyRegistry::new(home.path(), backend.clone())
            .set("github-app-one", "synthetic")
            .unwrap();
        let store = ManagedSecretCache::default().store(home.path().into(), backend.clone());
        for _ in 0..20 {
            assert!(store.get("github-app-one").is_err());
        }
        assert_eq!(reads.load(Ordering::SeqCst), 1);
        KeyRegistry::new(home.path(), backend)
            .set("github-app-one", "replacement")
            .unwrap();
        assert!(store.get("github-app-one").is_err());
        assert_eq!(reads.load(Ordering::SeqCst), 2);
    }
}
