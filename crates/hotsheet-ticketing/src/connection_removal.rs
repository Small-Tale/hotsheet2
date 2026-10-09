//! Removal of external ticket-provider connections (HS2-724S9N, HS2-SM9PM8).
//!
//! A connection is owned by the checkouts that link it (HS2-SM9PM8): there is no
//! machine-wide catalog of ticket sources. Two workflows are shared by the server and the
//! headless CLI:
//!
//! - [`detach_source`] removes a source from one project. When no other checkout still
//!   links the connection, its `providers.json` record goes too.
//! - [`remove_provider_connection`] removes a connection from every project at once.
//!
//! Neither deletes the credential the connection used: that is a machine-wide **account**
//! (see [`crate::accounts`]) that other projects may reuse and that is signed out
//! separately. Each step is idempotent, so a retry after a partial failure — or removing an
//! id that is already gone — converges on the same end state. Ticket data lives in the
//! provider itself (never mirrored locally), so nothing else holds the connection.

use serde::Serialize;
use thiserror::Error;

use crate::checkouts::{CheckoutError, CheckoutRegistry};
use crate::provider::{ProviderConfigRegistry, ProviderConnection, ProviderError};
use crate::secrets::SecretError;

/// Credential references Hot Sheet creates during GitHub device sign-in. They are listed as
/// accounts even when no connection uses them, so they can be signed out; a user-managed
/// key (a PAT registered with `hotsheet key set`) may serve other tools and is listed only
/// while a connection references it.
pub const MANAGED_CREDENTIAL_PREFIX: &str = "github-app-";

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ConnectionRemoval {
    pub connection_id: String,
    /// Whether `providers.json` still listed the connection.
    pub removed_connection: bool,
    /// Checkout ids whose linked sources (and default) no longer name the connection.
    pub unlinked_checkouts: Vec<String>,
    /// The account (credential reference) the connection used. It stays signed in: accounts
    /// are machine-wide and signed out separately (HS2-SM9PM8).
    pub kept_credential: Option<String>,
}

/// What [`detach_source`] did.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceDetach {
    pub checkout_id: String,
    pub connection_id: String,
    /// Whether the checkout still linked the source.
    pub unlinked: bool,
    /// Whether the connection record was deleted because no project uses it any more.
    pub removed_connection: bool,
    /// Other checkouts that still use the connection (it was kept for them).
    pub still_used_by: Vec<String>,
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

pub(crate) fn credential_of(connection: &ProviderConnection) -> Option<&str> {
    connection
        .settings
        .get("credential")?
        .get("secret")?
        .as_str()
}

/// Remove `connection_id` from the checkout `reference` (HS2-SM9PM8). An external
/// connection that no other checkout links afterwards is deleted from `providers.json`; its
/// account credential stays. A Hot Sheet git source is only unlinked: its store is never
/// deleted. Detaching an already-detached source still collects an orphaned record, and
/// fails with [`CheckoutError::NotFound`] only when there was nothing to do at all.
pub fn detach_source(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    reference: &str,
    connection_id: &str,
) -> Result<SourceDetach, ConnectionRemovalError> {
    detach_source_with_hook(providers, checkouts, reference, connection_id, || {})
}

fn detach_source_with_hook(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    reference: &str,
    connection_id: &str,
    before_provider_save: impl FnOnce(),
) -> Result<SourceDetach, ConnectionRemovalError> {
    let checkouts = checkouts.lock_source_links()?;
    let checkout = checkouts.resolve(reference)?;
    let linked_provider = checkout
        .source(connection_id)
        .map(|source| source.provider.clone());
    let unlinked = match checkouts.remove_source(&checkout.id, connection_id) {
        Ok(_) => true,
        Err(CheckoutError::NotFound(_)) => false,
        Err(error) => return Err(error.into()),
    };
    let still_used_by = checkouts
        .list()?
        .into_iter()
        .filter(|other| other.source(connection_id).is_some())
        .map(|other| other.id)
        .collect::<Vec<_>>();
    before_provider_save();
    let mut removed_connection = false;
    if linked_provider.as_deref() != Some("git") && still_used_by.is_empty() {
        let mut connections = providers.load()?;
        let before = connections.len();
        connections
            .retain(|connection| connection.id != connection_id || connection.provider == "git");
        if connections.len() != before {
            providers.save(&connections)?;
            removed_connection = true;
        }
    }
    if !unlinked && !removed_connection {
        return Err(CheckoutError::NotFound(connection_id.into()).into());
    }
    Ok(SourceDetach {
        checkout_id: checkout.id,
        connection_id: connection_id.into(),
        unlinked,
        removed_connection,
        still_used_by,
    })
}

/// Remove `connection_id` from every project and from `providers.json`. See the module docs.
pub fn remove_provider_connection(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    connection_id: &str,
) -> Result<ConnectionRemoval, ConnectionRemovalError> {
    let checkouts = checkouts.lock_source_links()?;
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
        report.kept_credential = credential_of(connection).map(str::to_owned);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::HashMap;
    use std::sync::mpsc;
    use std::time::Duration;

    use super::*;
    use crate::checkouts::TicketSource;
    use crate::secrets::{KeyRegistry, SecretStore};

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
    fn concurrent_link_cannot_resurrect_an_orphaned_provider() {
        let f = fixture();
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let (result_tx, result_rx) = mpsc::channel();
        let checkouts = f.checkouts.clone();
        let providers = f.providers.clone();
        let other = f.other_checkout_id.clone();
        let report = detach_source_with_hook(
            &f.providers,
            &f.checkouts,
            &f.checkout_id,
            "github-wiki",
            || {
                std::thread::spawn(move || {
                    attempt_tx.send(()).unwrap();
                    let linked = checkouts.add_registered_source(
                        &providers,
                        &other,
                        TicketSource {
                            connection_id: "github-wiki".into(),
                            provider: "github".into(),
                            locator: "acme/github-wiki".into(),
                        },
                        false,
                    );
                    result_tx.send(linked).unwrap();
                });
                attempt_rx.recv_timeout(Duration::from_secs(1)).unwrap();
                assert!(result_rx.recv_timeout(Duration::from_millis(100)).is_err());
            },
        )
        .unwrap();
        assert!(report.removed_connection);
        let result = result_rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert!(matches!(result, Err(CheckoutError::Invalid(_))));
        assert!(
            f.checkouts
                .list()
                .unwrap()
                .iter()
                .all(|checkout| checkout.source("github-wiki").is_none())
        );
        assert!(
            f.providers
                .load()
                .unwrap()
                .iter()
                .all(|connection| connection.id != "github-wiki")
        );
    }

    #[test]
    fn removes_the_connection_its_checkout_links_and_default_but_keeps_the_account() {
        let f = fixture();
        let report = remove_provider_connection(&f.providers, &f.checkouts, "github-main").unwrap();
        assert!(report.removed_connection);
        let mut unlinked = report.unlinked_checkouts.clone();
        unlinked.sort();
        let mut expected = vec![f.checkout_id.clone(), f.other_checkout_id.clone()];
        expected.sort();
        assert_eq!(unlinked, expected);
        assert_eq!(report.kept_credential.as_deref(), Some("github-app-01ABC"));
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
        // Accounts are machine-wide (HS2-SM9PM8): removing a source never signs out.
        assert!(f.keys.get("github-app-01ABC").is_ok());
    }

    #[test]
    fn keeps_user_managed_and_still_shared_credentials() {
        let f = fixture();
        let report = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
        assert_eq!(report.kept_credential.as_deref(), Some("shared-pat"));
        remove_provider_connection(&f.providers, &f.checkouts, "github-wiki").unwrap();
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
        let first = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
        assert!(!first.removed_connection);
        assert_eq!(first.unlinked_checkouts.len(), 2);
        let second = remove_provider_connection(&f.providers, &f.checkouts, "github-docs").unwrap();
        assert_eq!(
            second,
            ConnectionRemoval {
                connection_id: "github-docs".into(),
                ..ConnectionRemoval::default()
            }
        );
        let unknown =
            remove_provider_connection(&f.providers, &f.checkouts, "never-existed").unwrap();
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
            remove_provider_connection(&f.providers, &f.checkouts, "git-local"),
            Err(ConnectionRemovalError::GitSource(_))
        ));
        assert_eq!(f.providers.load().unwrap().len(), 4);
    }

    fn connection_ids(f: &Fixture) -> Vec<String> {
        f.providers
            .load()
            .unwrap()
            .into_iter()
            .map(|connection| connection.id)
            .collect()
    }

    /// Transition matrix for project-owned sources (HS2-SM9PM8): shared → detach one keeps
    /// the record; last detach removes it; a repeat is NotFound; re-attaching restores
    /// sharing; the account credential survives every step.
    #[test]
    fn detaching_keeps_a_shared_connection_and_removes_it_with_its_last_project() {
        let f = fixture();
        let first = detach_source(&f.providers, &f.checkouts, "one", "github-docs").unwrap();
        assert!(first.unlinked && !first.removed_connection);
        assert_eq!(first.still_used_by, vec![f.other_checkout_id.clone()]);
        assert!(connection_ids(&f).contains(&"github-docs".to_string()));

        // Re-attach headlessly (`checkout add-source`): links stay many-to-many.
        f.checkouts
            .add_source(
                "one",
                TicketSource {
                    connection_id: "github-docs".into(),
                    provider: "github".into(),
                    locator: "acme/github-docs".into(),
                },
                false,
            )
            .unwrap();
        detach_source(&f.providers, &f.checkouts, "two", "github-docs").unwrap();
        let last = detach_source(&f.providers, &f.checkouts, "one", "github-docs").unwrap();
        assert!(last.unlinked && last.removed_connection);
        assert!(last.still_used_by.is_empty());
        assert!(!connection_ids(&f).contains(&"github-docs".to_string()));
        // The account outlives its last source.
        assert_eq!(f.keys.get("shared-pat").unwrap(), "ghp_fixture");
        // Nothing left to do: an explicit NotFound, never a silent success.
        assert!(matches!(
            detach_source(&f.providers, &f.checkouts, "one", "github-docs"),
            Err(ConnectionRemovalError::Checkout(CheckoutError::NotFound(_)))
        ));
        // Other connections were untouched throughout.
        assert_eq!(connection_ids(&f), ["github-main", "github-wiki"]);
    }

    #[test]
    fn detaching_the_default_clears_it_and_collects_an_orphan_record() {
        let f = fixture();
        let report = detach_source(&f.providers, &f.checkouts, "one", "github-main").unwrap();
        assert!(!report.removed_connection);
        let one = f.checkouts.resolve("one").unwrap();
        assert_ne!(one.default_source.as_deref(), Some("github-main"));
        assert_eq!(
            f.checkouts
                .resolve("two")
                .unwrap()
                .default_source
                .as_deref(),
            Some("github-main")
        );
        // `github-wiki` is linked by no project (a pre-HS2-SM9PM8 catalog entry): detaching
        // it is a retry-safe cleanup that removes the orphaned record.
        let orphan = detach_source(&f.providers, &f.checkouts, "one", "github-wiki").unwrap();
        assert!(!orphan.unlinked && orphan.removed_connection);
        assert_eq!(connection_ids(&f), ["github-main", "github-docs"]);
    }

    #[test]
    fn detaching_a_git_source_never_touches_provider_records() {
        let f = fixture();
        let store = f._dir.path().join("one.hs2");
        std::fs::create_dir(&store).unwrap();
        let git = TicketSource::git(&store);
        f.checkouts.add_source("one", git.clone(), false).unwrap();
        let report = detach_source(&f.providers, &f.checkouts, "one", &git.connection_id).unwrap();
        assert!(report.unlinked && !report.removed_connection);
        assert_eq!(connection_ids(&f).len(), 3);
        assert!(store.is_dir());
    }
}
