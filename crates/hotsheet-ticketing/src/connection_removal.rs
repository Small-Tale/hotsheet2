//! Permanent removal of an external ticket-provider connection (HS2-724S9N).
//!
//! One workflow shared by the server and the headless CLI: unlink the connection from
//! every registered checkout (clearing a default that pointed at it), drop it from
//! `providers.json`, and delete a keychain credential that Hot Sheet itself minted for
//! it. Each step is idempotent, so a retry after a partial failure — or removing an id
//! that is already gone — converges on the same end state. Ticket data lives in the
//! provider itself (never mirrored locally), so nothing else holds the connection.

use serde::Serialize;
use thiserror::Error;

use crate::checkouts::{CheckoutError, CheckoutRegistry};
use crate::provider::{ProviderConfigRegistry, ProviderConnection, ProviderError};
use crate::secrets::{KeyRegistry, SecretError, SecretStore};

/// Credential references Hot Sheet creates during GitHub device sign-in. Only these are
/// deleted with a connection; a user-managed key (a PAT registered with `hotsheet key set`)
/// may serve other tools and is left in place.
pub const MANAGED_CREDENTIAL_PREFIX: &str = "github-app-";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionRemoval {
    pub connection_id: String,
    /// Whether `providers.json` still listed the connection.
    pub removed_connection: bool,
    /// Checkout ids whose linked sources (and default) no longer name the connection.
    pub unlinked_checkouts: Vec<String>,
    /// The Hot Sheet–managed credential that was deleted, if any.
    pub deleted_credential: Option<String>,
    /// A credential kept because it is user-managed or still used by another connection.
    pub kept_credential: Option<String>,
}

#[derive(Debug, Error)]
pub enum ConnectionRemovalError {
    #[error("'{0}' names a git ticket store, not an external provider connection")]
    GitSource(String),
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Checkout(#[from] CheckoutError),
    #[error(transparent)]
    Secret(#[from] SecretError),
}

fn credential_of(connection: &ProviderConnection) -> Option<&str> {
    connection
        .settings
        .get("credential")?
        .get("secret")?
        .as_str()
}

/// Remove `connection_id` and every local reference to it. See the module docs.
pub fn remove_provider_connection<S: SecretStore>(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    keys: &KeyRegistry<S>,
    connection_id: &str,
) -> Result<ConnectionRemoval, ConnectionRemovalError> {
    let mut connections = providers.load()?;
    let removed = connections
        .iter()
        .position(|connection| connection.id == connection_id)
        .map(|index| connections.remove(index));
    if removed
        .as_ref()
        .is_some_and(|connection| connection.provider == "git")
    {
        return Err(ConnectionRemovalError::GitSource(connection_id.into()));
    }
    let mut report = ConnectionRemoval {
        connection_id: connection_id.into(),
        removed_connection: removed.is_some(),
        ..ConnectionRemoval::default()
    };
    // Unlink first: a checkout never names a connection that providers.json has dropped.
    for checkout in checkouts.list()? {
        let linked = checkout
            .sources
            .iter()
            .any(|source| source.connection_id == connection_id && source.provider != "git");
        if !linked {
            continue;
        }
        match checkouts.remove_source(&checkout.id, connection_id) {
            Ok(_) | Err(CheckoutError::NotFound(_)) => {
                report.unlinked_checkouts.push(checkout.id);
            }
            Err(error) => return Err(error.into()),
        }
    }
    if let Some(connection) = &removed {
        providers.save(&connections)?;
        if let Some(credential) = credential_of(connection) {
            let shared = connections
                .iter()
                .any(|other| credential_of(other) == Some(credential));
            if credential.starts_with(MANAGED_CREDENTIAL_PREFIX) && !shared {
                keys.delete(credential)?;
                report.deleted_credential = Some(credential.into());
            } else {
                report.kept_credential = Some(credential.into());
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::checkouts::TicketSource;

    #[derive(Default)]
    struct Memory(RefCell<HashMap<String, String>>);

    impl SecretStore for Memory {
        fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
            self.0.borrow_mut().insert(account.into(), secret.into());
            Ok(())
        }
        fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
            Ok(self.0.borrow().get(account).cloned())
        }
        fn delete(&self, account: &str) -> Result<bool, SecretError> {
            Ok(self.0.borrow_mut().remove(account).is_some())
        }
    }

    fn connection(id: &str, credential: &str, default: bool) -> ProviderConnection {
        ProviderConnection {
            id: id.into(),
            provider: "github".into(),
            locator: format!("acme/{id}"),
            name: None,
            default,
            settings: serde_json::json!({"credential": {"secret": credential}}),
            disabled: false,
        }
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        providers: ProviderConfigRegistry,
        checkouts: CheckoutRegistry,
        keys: KeyRegistry<Memory>,
        checkout_id: String,
        other_checkout_id: String,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let providers = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        providers
            .save(&[
                connection("github-main", "github-app-01ABC", true),
                connection("github-docs", "shared-pat", false),
                connection("github-wiki", "shared-pat", false),
            ])
            .unwrap();
        let checkouts = CheckoutRegistry::new(dir.path().join("checkouts.json"));
        let source = |id: &str| TicketSource {
            connection_id: id.into(),
            provider: "github".into(),
            locator: format!("acme/{id}"),
        };
        let mut ids = Vec::new();
        for name in ["one", "two"] {
            let root = dir.path().join(name);
            std::fs::create_dir(&root).unwrap();
            ids.push(
                checkouts
                    .register_sources(
                        &root,
                        Some(name),
                        None,
                        vec![source("github-main"), source("github-docs")],
                        Some("github-main".into()),
                    )
                    .unwrap()
                    .id,
            );
        }
        let keys = KeyRegistry::new(dir.path(), Memory::default());
        keys.set("github-app-01ABC", "{\"kind\":\"github_app\"}")
            .unwrap();
        keys.set("shared-pat", "ghp_fixture").unwrap();
        Fixture {
            _dir: dir,
            providers,
            checkouts,
            keys,
            checkout_id: ids.remove(0),
            other_checkout_id: ids.remove(0),
        }
    }

    #[test]
    fn removes_the_connection_its_checkout_links_default_and_managed_credential() {
        let f = fixture();
        let report =
            remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "github-main").unwrap();
        assert!(report.removed_connection);
        let mut unlinked = report.unlinked_checkouts.clone();
        unlinked.sort();
        let mut expected = vec![f.checkout_id.clone(), f.other_checkout_id.clone()];
        expected.sort();
        assert_eq!(unlinked, expected);
        assert_eq!(
            report.deleted_credential.as_deref(),
            Some("github-app-01ABC")
        );
        let ids: Vec<_> = f
            .providers
            .load()
            .unwrap()
            .into_iter()
            .map(|connection| connection.id)
            .collect();
        assert_eq!(ids, ["github-docs", "github-wiki"]);
        for checkout in f.checkouts.list().unwrap() {
            assert!(checkout.source("github-main").is_none());
            assert!(checkout.source("github-docs").is_some());
            // The registry falls back to a remaining source; never the removed one.
            assert_ne!(checkout.default_source.as_deref(), Some("github-main"));
        }
        assert!(matches!(
            f.keys.get("github-app-01ABC"),
            Err(SecretError::NotFound(_))
        ));
    }

    #[test]
    fn keeps_user_managed_and_still_shared_credentials() {
        let f = fixture();
        let report =
            remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "github-docs").unwrap();
        assert_eq!(report.deleted_credential, None);
        assert_eq!(report.kept_credential.as_deref(), Some("shared-pat"));
        remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "github-wiki").unwrap();
        // Even unshared, a user-registered key may serve other tools.
        assert_eq!(f.keys.get("shared-pat").unwrap(), "ghp_fixture");
        for checkout in f.checkouts.list().unwrap() {
            assert_eq!(checkout.default_source.as_deref(), Some("github-main"));
        }
    }

    #[test]
    fn repeating_a_removal_converges_and_cleans_dangling_checkout_links() {
        let f = fixture();
        // A connection dropped from providers.json by an older client left checkout links behind.
        let mut connections = f.providers.load().unwrap();
        connections.retain(|connection| connection.id != "github-docs");
        f.providers.save(&connections).unwrap();
        let first =
            remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "github-docs").unwrap();
        assert!(!first.removed_connection);
        assert_eq!(first.unlinked_checkouts.len(), 2);
        let second =
            remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "github-docs").unwrap();
        assert_eq!(
            second,
            ConnectionRemoval {
                connection_id: "github-docs".into(),
                ..ConnectionRemoval::default()
            }
        );
        let unknown =
            remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "never-existed")
                .unwrap();
        assert!(!unknown.removed_connection && unknown.unlinked_checkouts.is_empty());
    }

    #[test]
    fn refuses_to_treat_a_git_connection_as_removable() {
        let f = fixture();
        let mut connections = f.providers.load().unwrap();
        connections.push(ProviderConnection {
            id: "git-local".into(),
            provider: "git".into(),
            locator: "/work/demo.hs2".into(),
            name: None,
            default: false,
            settings: serde_json::Value::Null,
            disabled: false,
        });
        f.providers.save(&connections).unwrap();
        assert!(matches!(
            remove_provider_connection(&f.providers, &f.checkouts, &f.keys, "git-local"),
            Err(ConnectionRemovalError::GitSource(_))
        ));
        assert_eq!(f.providers.load().unwrap().len(), 4);
    }
}
