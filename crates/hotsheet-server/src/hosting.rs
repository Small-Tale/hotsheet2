//! Store hosting: index + watcher setup, project unhost sweeps, instance discovery
//! registration, and configured-store startup (HS2-YMG48C split from lib.rs).

use crate::api_error::ApiError;
use crate::multistore::StoreEntry;
use crate::routes::checkouts::prewarm_corrupt_tickets;
use crate::routes::store_scoped::run_unhost_sweeper;
use crate::watcher::{
    WatchTarget, registered_watcher_backend, same_path, spawn_repository_watcher, spawn_watcher_for,
};
use crate::{AppState, InstanceMeta, lifecycle, multistore, presence};
use axum::http::StatusCode;
use hotsheet_index::Index;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::FsStore;
use std::path::Path as FsPath;
use std::sync::{Arc, Mutex};
use std::time::Duration;

impl AppState {
    /// Host a store: build its index (file-backed when persisting, else in-memory),
    /// register it, and spawn its fs-watcher. Idempotent by store id — returns whether it
    /// was newly added. Shared by `POST /stores` and startup discovery.
    pub(crate) fn host_store(&self, store: FsStore) -> Result<bool, ApiError> {
        let store = store.with_deferred_push().with_background_autocommit();
        // Validate the store boundary before registering an index, watcher, or discovery
        // file. Otherwise an older server can appear to accept a newer store and fail only
        // when its first ticket mutation reaches the writer.
        store
            .metadata()
            .map_err(|error| ApiError::new(StatusCode::CONFLICT, error.to_string()))?;
        let id = multistore::store_url_id(&store);
        let initialization = self.host.initialization_lock(&id);
        let _initializing = initialization.lock_or_recover();
        if self.host.contains(&id) {
            return Ok(false);
        }
        let index = if self.persist_indexes {
            let path = multistore::index_path_for(&self.machine_home, &store)
                .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
            Index::open_reconciled(&path, &store)?
        } else {
            let ix = Index::open_in_memory(store.root().display().to_string())?;
            ix.rebuild_from_store(&store)?;
            ix
        };
        let entry = StoreEntry {
            instance_id: store
                .metadata()
                .ok()
                .and_then(|metadata| metadata.instance_id),
            store: store.clone(),
            index: Arc::new(Mutex::new(index)),
            corrupt: Arc::default(),
            freshness: Arc::default(),
        };
        self.host.register(entry.clone());
        prewarm_corrupt_tickets(entry.clone());
        let project = store.root().display().to_string();
        let mut paths = self.permission_rule_paths.lock_or_recover();
        if !paths.is_empty()
            && !paths.contains_key(&project)
            && let Some(directory) = paths.values().next().and_then(|path| path.parent())
        {
            let path = directory.join(format!("{id}.json"));
            let rules = hotsheet_aitools::load_permission_rules(&path)
                .into_iter()
                .map(|mut rule| {
                    rule.project = project.clone();
                    rule
                });
            self.permissions.add_rules(rules);
            paths.insert(project, path);
        }
        drop(paths);
        let store_root = store.root().to_path_buf();
        let watched_id = id.clone();
        match spawn_watcher_for(
            WatchTarget {
                entry,
                store_id: id,
                host: self.host.clone(),
                local_write_hashes: self.local_write_hashes.clone(),
                events: self.events.clone(),
                event_log: self.event_log.clone(),
                checkout_registry: self.checkout_registry.clone(),
                index_repairs: self.index_repairs.clone(),
                worklist_refresher: self.worklist_refresher.clone(),
            },
            registered_watcher_backend(),
        ) {
            Ok(handle) => {
                let mut w = self.watchers.lock_or_recover();
                w.insert(watched_id, handle);
            }
            Err(e) => {
                tracing::warn!(store = %store_root.display(), error = %e, "store watcher failed to start")
            }
        }
        // Advertise the newly-hosted store for discovery (real run only; a no-op in tests).
        self.register_store_instance(&store_root);
        Ok(true)
    }

    pub(crate) fn watch_checkout_repository(
        &self,
        checkout: &hotsheet_ticketing::checkouts::Checkout,
    ) {
        let mut watchers = self.repository_watchers.lock_or_recover();
        if watchers.contains_key(&checkout.id) {
            return;
        }
        let root = std::path::PathBuf::from(&checkout.root);
        if hotsheet_ticketing::repository_status::snapshot(&root).is_err() {
            return;
        }
        match spawn_repository_watcher(
            root.clone(),
            checkout.id.clone(),
            self.events.clone(),
            self.event_log.clone(),
        ) {
            Ok(handle) => {
                watchers.insert(checkout.id.clone(), handle);
            }
            Err(error) => tracing::warn!(
                checkout_root = %root.display(),
                %error,
                "repository watcher failed to start"
            ),
        }
    }

    /// The hosted entry for a checkout's git source, hosting it again when an earlier sweep
    /// unhosted it (HS2-ARJ9J1): a client that kept a project open across a long sleep keeps
    /// working without reopening it.
    pub(crate) fn hosted_source(
        &self,
        source: &hotsheet_ticketing::checkouts::TicketSource,
    ) -> Option<StoreEntry> {
        if let Some(entry) = self.host.get(&source.connection_id) {
            return Some(entry);
        }
        if source.provider != "git" {
            return None;
        }
        let store = FsStore::open(&source.locator).ok()?;
        self.host_project_store(store).ok()?;
        self.host.get(&source.connection_id)
    }

    /// Host a store for an open project, making it eligible to be unhosted once no open
    /// checkout references it (HS2-ARJ9J1).
    pub(crate) fn host_project_store(&self, store: FsStore) -> Result<bool, ApiError> {
        let id = multistore::store_url_id(&store);
        let pinned = self.host.contains(&id) && !self.is_project_hosted(&id);
        let added = self.host_store(store)?;
        if !pinned {
            self.project_hosted.lock_or_recover().insert(id);
        }
        self.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
        Ok(added)
    }

    fn is_project_hosted(&self, id: &str) -> bool {
        self.project_hosted.with_lock(|hosted| hosted.contains(id))
    }

    /// Stop hosting a project store: its index, watcher, discovery file and writer lock go,
    /// and the next open or checkout request hosts it again (HS2-ARJ9J1).
    pub(crate) fn unhost_store(&self, id: &str) {
        if !self.project_hosted.lock_or_recover().remove(id) {
            return;
        }
        let Some(entry) = self.host.unregister(id) else {
            return;
        };
        let root = entry.store.root().display().to_string();
        let watcher = self.watchers.with_lock(|w| w.remove(id));
        let guard = self.instance_guards.with_lock(|g| g.remove(&root));
        let lock = self.writer_locks.with_lock(|w| w.remove(&root));
        // Stop the watcher and release the files outside every lock.
        drop((watcher, guard, lock));
        tracing::info!(store = %root, "unhosted store: no open project references it");
    }

    /// Ask for an unhost sweep `delay` from now; an earlier pending request wins.
    pub(crate) fn request_unhost_sweep(&self, delay: Duration) {
        let Ok(runtime) = tokio::runtime::Handle::try_current() else {
            return;
        };
        let at = tokio::time::Instant::now() + delay;
        {
            let mut due = self.unhost_sweep.due.lock_or_recover();
            if due.is_some_and(|current| current <= at) {
                return;
            }
            *due = Some(at);
        }
        self.unhost_sweep.wake.notify_one();
        if !self
            .unhost_sweep
            .started
            .swap(true, std::sync::atomic::Ordering::SeqCst)
        {
            let state = self.clone();
            runtime.spawn(async move { run_unhost_sweeper(state).await });
        }
    }

    /// Record the machine server's coordinates (URL + start time; a real run, after bind)
    /// and register a discovery instance file for **every** already-hosted store, so a
    /// client asking "who serves project X?" finds this one machine server for each project
    /// it hosts (HS2-87 topology A). Runtime `POST /stores` additions register via
    /// [`Self::host_store`]. No-op'd in tests (they never call this).
    pub fn publish_instances(&self, url: String, started_at: String) {
        {
            let mut m = self.instance.lock_or_recover();
            *m = Some(InstanceMeta {
                url,
                secret: self.secret.clone(),
                started_at,
                build_revision: self.source_revision.status().build_revision,
                executable: self.executable(),
                launcher: self.launcher(),
            });
        }
        for (_, root) in self.host.locations() {
            self.register_store_instance(&root);
        }
    }

    /// Write the discovery instance file for one hosted store (if instance publishing is
    /// on), retaining its guard so the file is removed on shutdown.
    fn register_store_instance(&self, store_path: &FsPath) {
        let Some(meta) = self.instance.with_lock(|m| m.clone()) else {
            return; // not a published (real) run — nothing to register
        };
        let index_path = if self.persist_indexes {
            FsStore::open(store_path)
                .ok()
                .and_then(|s| multistore::index_path_for(&self.machine_home, &s).ok())
                .map(|p| p.display().to_string())
                .unwrap_or_default()
        } else {
            "(in-memory)".into()
        };
        let info = lifecycle::InstanceInfo {
            pid: std::process::id(),
            url: meta.url,
            secret: meta.secret,
            store_path: store_path.display().to_string(),
            index_path,
            started_at: meta.started_at,
            build_revision: meta.build_revision,
            executable: meta.executable,
            launcher: Some(meta.launcher),
        };
        // Hold this store's index-writer lock too, so a second machine server can't
        // double-write its index (HS2-AYCA1W). The primary store's lock is already held by
        // the server binary (main.rs), so skip it here. A lock held by another *live* server
        // is logged, not fatal — the discovery instance file (above) already steers clients
        // to a single server; this is belt-and-suspenders against a stray duplicate.
        let is_primary = same_path(store_path, self.store.root());
        let instances = self.instance_registry();
        if !is_primary {
            match instances.acquire_writer_lock(store_path) {
                Ok(lock) => {
                    let mut w = self.writer_locks.lock_or_recover();
                    w.insert(store_path.display().to_string(), lock);
                }
                Err(lifecycle::LockError::Held(pid)) => tracing::warn!(
                    "warning: store {} is also index-write-locked by live server pid {pid} \
                     — index writes may collide",
                    store_path.display()
                ),
                Err(e) => tracing::warn!("writer lock for {} failed: {e}", store_path.display()),
            }
        }
        match instances.register_instance(&info, store_path) {
            Ok(guard) => {
                let mut g = self.instance_guards.lock_or_recover();
                g.insert(store_path.display().to_string(), guard);
            }
            Err(e) => tracing::warn!(
                store = %store_path.display(),
                error = %e,
                "instance registration failed"
            ),
        }
    }

    /// Auto-host the stores listed in `${HOTSHEET_HOME}/stores.json` (HS2-87 startup
    /// discovery). A path that isn't a store is logged and skipped — one bad entry never
    /// stops the server. Returns how many were newly hosted.
    pub fn host_configured_stores(&self) -> usize {
        let mut hosted = 0;
        for path in multistore::configured_store_paths(&self.machine_home) {
            match FsStore::open(&path) {
                Ok(store) => match self.host_store(store) {
                    Ok(true) => hosted += 1,
                    Ok(false) => {}
                    Err(e) => tracing::warn!("could not host {}: {}", path.display(), e.message),
                },
                Err(e) => tracing::warn!("configured store {} skipped: {e}", path.display()),
            }
        }
        hosted
    }
}
