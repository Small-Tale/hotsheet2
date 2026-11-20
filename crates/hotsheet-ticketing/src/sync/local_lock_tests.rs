//! HS2-457FGS: the lasting machine-local locks at a store root never reach the store's
//! Git history through sync, and a copy an older client already committed is removed
//! from the index on every clone without replacing the working file a holder locks.
//! Synthetic bare remotes, stores, and provider records only.

use std::path::Path;

use super::tests::{clean, ok, remote_with_two_clones};
use super::*;
use crate::store::{LOCAL_ONLY_ROOT_FILES, PROVIDER_CONFIG_LOCK_FILE};
use crate::{FsStore, ProviderConfigRegistry, ProviderConnection, StoreMetadata};

fn connection() -> ProviderConnection {
    ProviderConnection {
        id: "synthetic-source".into(),
        provider: "github".into(),
        locator: "synthetic/project".into(),
        name: None,
        default: false,
        settings: serde_json::json!({}),
        disabled: false,
    }
}

/// Paths the remote's branch tracks.
fn remote_paths(store: &Path) -> Vec<String> {
    ok(store, &["fetch", "--quiet"]);
    git_stdout(store, &["ls-tree", "-r", "--name-only", "@{u}"])
        .unwrap()
        .lines()
        .map(str::to_owned)
        .collect()
}

fn tracked(store: &Path, path: &str) -> bool {
    git_stdout(store, &["ls-files", "--", path]).is_some_and(|out| !out.trim().is_empty())
}

/// Turn the clone at `root` into a store and publish its metadata, as `init` would.
fn published_store(root: &Path) -> FsStore {
    let store = FsStore::init(root, &StoreMetadata::new("HS")).unwrap();
    assert!(matches!(
        sync_once(root),
        SyncReport::Synced { pushed: true, .. }
    ));
    store
}

#[test]
fn hs2_457fgs_a_provider_configuration_write_then_sync_never_commits_its_lock() {
    let (_dir, store, _peer) = remote_with_two_clones();
    published_store(&store);
    let registry = ProviderConfigRegistry::new(store.join("providers.json"));
    registry.save(&[connection()]).unwrap();
    assert!(store.join(PROVIDER_CONFIG_LOCK_FILE).is_file());

    assert!(matches!(
        sync_once(&store),
        SyncReport::Synced { pushed: true, .. }
    ));
    let published = remote_paths(&store);
    assert!(
        !published
            .iter()
            .any(|path| path == PROVIDER_CONFIG_LOCK_FILE),
        "{published:?}"
    );
    assert!(!tracked(&store, PROVIDER_CONFIG_LOCK_FILE));
    // The ignored lock leaves nothing pending, and a further write and pass agree.
    assert!(
        clean(&store),
        "{:?}",
        git_stdout(&store, &["status", "--porcelain"])
    );
    registry.set_disabled("synthetic-source", true).unwrap();
    sync_once(&store);
    assert!(
        !remote_paths(&store)
            .iter()
            .any(|path| path == PROVIDER_CONFIG_LOCK_FILE)
    );
    assert_eq!(sync_once(&store), SyncReport::UpToDate);
}

#[test]
fn hs2_457fgs_a_lock_an_older_client_committed_is_untracked_on_every_clone() {
    let (_dir, store, peer) = remote_with_two_clones();
    published_store(&store);
    // An older client committed both lasting locks and published them.
    for name in LOCAL_ONLY_ROOT_FILES {
        std::fs::write(store.join(name), "").unwrap();
        ok(&store, &["add", "-f", "--", name]);
    }
    ok(&store, &["commit", "--quiet", "-m", "older client"]);
    ok(&store, &["push", "--quiet"]);
    ok(&peer, &["pull", "--quiet", "--rebase"]);
    for name in LOCAL_ONLY_ROOT_FILES {
        assert!(tracked(&peer, name), "{name}");
    }

    // The peer holds its provider lock open across its own sync pass.
    let held = std::fs::File::open(peer.join(PROVIDER_CONFIG_LOCK_FILE)).unwrap();
    #[cfg(unix)]
    let held_inode = {
        use std::os::unix::fs::MetadataExt;
        held.metadata().unwrap().ino()
    };

    assert!(matches!(
        sync_once(&store),
        SyncReport::Synced { pushed: true, .. }
    ));
    assert!(matches!(sync_once(&peer), SyncReport::Synced { .. }));
    for clone in [&store, &peer] {
        let published = remote_paths(clone);
        for name in LOCAL_ONLY_ROOT_FILES {
            assert!(!tracked(clone, name), "{name} in {}", clone.display());
            assert!(!published.iter().any(|path| path == name), "{published:?}");
            // The working file stays: removal touches only the index.
            assert!(clone.join(name).is_file(), "{name} in {}", clone.display());
        }
        assert!(published.iter().any(|path| path == "hotsheet-store.json"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let now = std::fs::metadata(peer.join(PROVIDER_CONFIG_LOCK_FILE)).unwrap();
        assert_eq!(now.ino(), held_inode, "the held lock's inode was replaced");
    }
    drop(held);

    // Idempotent: further passes find nothing to untrack or publish.
    for clone in [&store, &peer] {
        let report = sync_once(clone);
        assert!(
            matches!(
                report,
                SyncReport::UpToDate | SyncReport::Synced { pushed: false, .. }
            ),
            "{report:?}"
        );
        assert_eq!(sync_once(clone), SyncReport::UpToDate);
        assert!(
            clean(clone),
            "{:?}",
            git_stdout(clone, &["status", "--porcelain"])
        );
    }
}

#[test]
fn hs2_457fgs_sync_brings_an_older_stores_ignore_list_up_to_date_before_staging() {
    let (_dir, store, _peer) = remote_with_two_clones();
    published_store(&store);
    // A store last maintained by an older client: its ignore list lacks the provider lock.
    let ignore = store.join(".gitignore");
    let older: String = std::fs::read_to_string(&ignore)
        .unwrap()
        .lines()
        .filter(|line| *line != PROVIDER_CONFIG_LOCK_FILE)
        .map(|line| format!("{line}\n"))
        .collect();
    std::fs::write(&ignore, older).unwrap();
    ok(&store, &["add", "--", ".gitignore"]);
    ok(&store, &["commit", "--quiet", "-m", "older ignore list"]);
    std::fs::write(store.join(PROVIDER_CONFIG_LOCK_FILE), "").unwrap();

    assert!(matches!(
        sync_once(&store),
        SyncReport::Synced { pushed: true, .. }
    ));
    assert!(!tracked(&store, PROVIDER_CONFIG_LOCK_FILE));
    assert!(
        std::fs::read_to_string(&ignore)
            .unwrap()
            .lines()
            .any(|line| line == PROVIDER_CONFIG_LOCK_FILE)
    );
}
