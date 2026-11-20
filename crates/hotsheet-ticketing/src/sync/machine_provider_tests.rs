//! HS2-1M8E9D: provider configuration is per machine. Two "machines" are two clones of
//! one bare remote, each with its own machine home. Synthetic remotes, stores, checkouts,
//! and provider records only; no keychain is touched.

use std::path::Path;

use super::tests::{clean, ok, remote_with_two_clones};
use super::*;
use crate::checkouts::{CheckoutRegistry, TicketSource};
use crate::store::LEGACY_PROVIDER_CONFIG_FILE;
use crate::{
    FsStore, ProviderConfigRegistry, ProviderConfigSeed, ProviderConnection, StoreMetadata,
};

fn connection(id: &str) -> ProviderConnection {
    ProviderConnection {
        id: id.into(),
        provider: "github".into(),
        locator: format!("synthetic/{id}"),
        name: None,
        default: false,
        settings: serde_json::json!({}),
        disabled: false,
    }
}

fn ids(registry: &ProviderConfigRegistry) -> Vec<String> {
    registry
        .load()
        .unwrap()
        .into_iter()
        .map(|connection| connection.id)
        .collect()
}

fn tracked(store: &Path, path: &str) -> bool {
    git_stdout(store, &["ls-files", "--", path]).is_some_and(|out| !out.trim().is_empty())
}

fn remote_tracks(store: &Path, path: &str) -> bool {
    ok(store, &["fetch", "--quiet"]);
    git_stdout(store, &["ls-tree", "-r", "--name-only", "@{u}"])
        .unwrap()
        .lines()
        .any(|line| line == path)
}

/// A published store whose shared history carries a `providers.json` an older client
/// committed, pulled by the peer: the state every machine upgrades from.
fn legacy_shared_store(store: &Path, peer: &Path, connections: &[ProviderConnection]) {
    FsStore::init(store, &StoreMetadata::new("HS")).unwrap();
    ProviderConfigRegistry::new(store.join(LEGACY_PROVIDER_CONFIG_FILE))
        .save(connections)
        .unwrap();
    // An older client's ignore list did not name the file, so its `add -A` shared it.
    ok(store, &["add", "-A"]);
    ok(store, &["add", "-f", "--", LEGACY_PROVIDER_CONFIG_FILE]);
    ok(store, &["commit", "--quiet", "-m", "older client"]);
    ok(store, &["push", "--quiet"]);
    ok(peer, &["pull", "--quiet", "--rebase"]);
    assert!(tracked(peer, LEGACY_PROVIDER_CONFIG_FILE));
}

/// Register a checkout on one machine that links `connection_id`.
fn link(home: &Path, providers: &ProviderConfigRegistry, name: &str, connection_id: &str) {
    let root = home.join(name);
    std::fs::create_dir_all(&root).unwrap();
    CheckoutRegistry::new(home.join("checkouts.json"))
        .register_registered_sources(
            providers,
            &root,
            Some(name),
            None,
            vec![TicketSource {
                connection_id: connection_id.into(),
                provider: "github".into(),
                locator: format!("synthetic/{connection_id}"),
            }],
            None,
        )
        .unwrap();
}

#[test]
fn hs2_1m8e9d_one_machines_cleanup_never_removes_another_machines_providers() {
    let (_dir, store, peer) = remote_with_two_clones();
    legacy_shared_store(&store, &peer, &[connection("github-shared")]);
    let (home_a, home_b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());

    // Both machines migrate from the shared file.
    let a = ProviderConfigRegistry::for_store_in(home_a.path(), &store).unwrap();
    let b = ProviderConfigRegistry::for_store_in(home_b.path(), &peer).unwrap();
    assert_ne!(a.path(), b.path());
    assert!(a.path().starts_with(home_a.path()));
    assert_eq!(ids(&a), ["github-shared"]);
    assert_eq!(ids(&b), ["github-shared"]);
    link(home_a.path(), &a, "a-project", "github-shared");
    link(home_b.path(), &b, "b-project", "github-shared");

    // Machine A removes its only checkout that links the source: A's cleanup deletes the
    // now-orphaned record from A's configuration.
    let removal = crate::checkout_removal::remove_checkout(
        &a,
        &CheckoutRegistry::new(home_a.path().join("checkouts.json")),
        "a-project",
        false,
    )
    .unwrap();
    assert_eq!(removal.removed_connections, ["github-shared"]);
    assert!(ids(&a).is_empty());

    // Both machines sync. The shared file leaves the history; nothing A did reaches B.
    for clone in [&store, &peer, &store] {
        let report = sync_once(clone);
        assert!(
            matches!(report, SyncReport::Synced { .. } | SyncReport::UpToDate),
            "{report:?}"
        );
    }
    for clone in [&store, &peer] {
        assert!(!tracked(clone, LEGACY_PROVIDER_CONFIG_FILE));
        assert!(!remote_tracks(clone, LEGACY_PROVIDER_CONFIG_FILE));
        // The working file stays as the seed and rollback snapshot.
        assert!(clone.join(LEGACY_PROVIDER_CONFIG_FILE).is_file());
        assert!(
            clean(clone),
            "{:?}",
            git_stdout(clone, &["status", "--porcelain"])
        );
    }
    let b = ProviderConfigRegistry::for_store_in(home_b.path(), &peer).unwrap();
    assert_eq!(ids(&b), ["github-shared"]);
    assert!(ids(&ProviderConfigRegistry::for_store_in(home_a.path(), &store).unwrap()).is_empty());

    // A later write on B stays on B.
    b.save(&[connection("github-shared"), connection("github-b-only")])
        .unwrap();
    for clone in [&peer, &store] {
        sync_once(clone);
    }
    assert!(!remote_tracks(&store, LEGACY_PROVIDER_CONFIG_FILE));
    assert!(ids(&ProviderConfigRegistry::for_store_in(home_a.path(), &store).unwrap()).is_empty());
}

#[test]
fn hs2_1m8e9d_migration_is_idempotent_and_never_replaces_a_machine_copy() {
    let (_dir, store, peer) = remote_with_two_clones();
    legacy_shared_store(&store, &peer, &[connection("github-one")]);
    let home = tempfile::tempdir().unwrap();
    let key = crate::store_config_key(&store).unwrap();
    let local = crate::machine_provider_config_path(home.path(), &key);
    assert_eq!(
        local,
        home.path()
            .join(crate::MACHINE_STORE_CONFIG_DIR)
            .join(&key)
            .join(crate::PROVIDER_CONFIG_FILE)
    );

    let registry = ProviderConfigRegistry::new(&local);
    assert_eq!(
        registry.seed_from_store(&store).unwrap(),
        ProviderConfigSeed::StoreFile
    );
    assert_eq!(
        std::fs::read(&local).unwrap(),
        std::fs::read(store.join(LEGACY_PROVIDER_CONFIG_FILE)).unwrap()
    );
    // Re-running, and a later change to the legacy file, leave the machine copy alone.
    std::fs::write(
        store.join(LEGACY_PROVIDER_CONFIG_FILE),
        "{\"connections\":[]}\n",
    )
    .unwrap();
    for _ in 0..2 {
        assert_eq!(
            registry.seed_from_store(&store).unwrap(),
            ProviderConfigSeed::Existing
        );
        let resolved = ProviderConfigRegistry::for_store_in(home.path(), &store).unwrap();
        assert_eq!(resolved.path(), local);
        assert_eq!(ids(&resolved), ["github-one"]);
    }
    // The other clone of the same store on the same machine shares the copy.
    assert_eq!(
        ProviderConfigRegistry::for_store_in(home.path(), &peer)
            .unwrap()
            .path(),
        local
    );
}

#[test]
fn hs2_1m8e9d_a_clone_that_lost_its_working_file_restores_from_history() {
    let (_dir, store, peer) = remote_with_two_clones();
    legacy_shared_store(&store, &peer, &[connection("github-peer")]);
    // The upgraded machine untracks and publishes.
    assert!(matches!(
        sync_once(&store),
        SyncReport::Synced { pushed: true, .. }
    ));
    // The peer still runs an older client: its pull deletes the working file.
    ok(&peer, &["pull", "--quiet", "--rebase"]);
    assert!(!peer.join(LEGACY_PROVIDER_CONFIG_FILE).exists());

    // Upgrading the peer restores its configuration from the last committed version.
    let home = tempfile::tempdir().unwrap();
    let key = crate::store_config_key(&peer).unwrap();
    let registry =
        ProviderConfigRegistry::new(crate::machine_provider_config_path(home.path(), &key));
    assert_eq!(
        registry.seed_from_store(&peer).unwrap(),
        ProviderConfigSeed::StoreHistory
    );
    assert_eq!(ids(&registry), ["github-peer"]);
    assert_eq!(
        registry.seed_from_store(&peer).unwrap(),
        ProviderConfigSeed::Existing
    );
}

#[test]
fn hs2_1m8e9d_a_store_without_provider_configuration_seeds_an_empty_list() {
    let (_dir, store, _peer) = remote_with_two_clones();
    FsStore::init(&store, &StoreMetadata::new("HS")).unwrap();
    let home = tempfile::tempdir().unwrap();
    let registry = ProviderConfigRegistry::for_store_in(home.path(), &store).unwrap();
    assert!(registry.path().is_file());
    assert!(ids(&registry).is_empty());
    assert!(!store.join(LEGACY_PROVIDER_CONFIG_FILE).exists());
    assert_eq!(
        registry.seed_from_store(&store).unwrap(),
        ProviderConfigSeed::Existing
    );
}

#[test]
fn hs2_1m8e9d_an_older_clients_change_to_the_untracked_file_never_stops_sync() {
    for mode in [ReplayMode::Auto, ReplayMode::LiveRebase] {
        let (_dir, store, peer) = remote_with_two_clones();
        legacy_shared_store(&store, &peer, &[connection("github-one")]);
        // The upgraded machine untracks the file locally but has not published yet.
        assert!(commit_pending_checked(&store).is_ok());
        assert!(!tracked(&store, LEGACY_PROVIDER_CONFIG_FILE));
        // Meanwhile an older client changes the shared file and publishes first.
        ProviderConfigRegistry::new(peer.join(LEGACY_PROVIDER_CONFIG_FILE))
            .save(&[connection("github-one"), connection("github-two")])
            .unwrap();
        ok(&peer, &["add", "-f", "--", LEGACY_PROVIDER_CONFIG_FILE]);
        ok(&peer, &["commit", "--quiet", "-m", "older client edit"]);
        ok(&peer, &["push", "--quiet"]);

        let report = sync_once_with(
            &store,
            SyncLimits {
                replay: mode,
                ..SyncLimits::default()
            },
        );
        assert!(
            matches!(report, SyncReport::Synced { pushed: true, .. }),
            "{mode:?}: {report:?}"
        );
        assert!(!tracked(&store, LEGACY_PROVIDER_CONFIG_FILE), "{mode:?}");
        assert!(
            !remote_tracks(&store, LEGACY_PROVIDER_CONFIG_FILE),
            "{mode:?}"
        );
        assert!(
            store.join(LEGACY_PROVIDER_CONFIG_FILE).is_file(),
            "{mode:?}"
        );
        assert!(
            clean(&store),
            "{mode:?}: {:?}",
            git_stdout(&store, &["status", "--porcelain"])
        );
    }
}
