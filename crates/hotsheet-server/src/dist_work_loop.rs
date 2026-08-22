//! The server-hosted **distributed driving loop** (`docs/13`, HS2-DTPX2V follow-up to
//! HS2-E7RXXR). The distributed self-claim *cycle* is done in the engine
//! ([`hotsheet_ticketing::distwork::work_once`]): self-claim the next ticket over the
//! shared remote, drive it, then keep or drop the lease by outcome. This module is the
//! **server-side orchestration** around it — the background thread that runs a work cycle
//! across every hosted store that opts in and has a remote, on a cadence, with a
//! worker-pool bound and a periodic expired-marker sweep.
//!
//! The **drive** (the actual AI turn) is *injected*: production supplies a
//! `live::run_trigger`-backed closure (spawn codex/claude per claimed ticket), while tests
//! supply a fake. That keeps the whole pass — participation filter, `NoRemote` skip,
//! concurrency bound, sweep, outcome aggregation — unit-testable against real bare-remote
//! clones with no AI tool in the loop. The live drive wiring itself (which spawns real
//! tools and can't be unit-tested) is the remaining live-only piece (HS2-1TY7GC).

use std::collections::HashSet;
use std::path::Path;
use std::sync::mpsc::{Receiver, RecvTimeoutError};
use std::time::Duration;

use hotsheet_model::Timestamp;
use hotsheet_ticketing::distwork::WorkOutcome;
use hotsheet_ticketing::{DistError, FsStore, distclaim, distwork};

use crate::AppState;

/// The base interval between driving passes.
pub const DEFAULT_INTERVAL: Duration = Duration::from_secs(15);

/// Which hosted stores the driving loop participates in.
#[derive(Debug, Clone, Default)]
pub enum Participation {
    /// Every hosted store that has a git remote (local-only stores are skipped). The
    /// default — the loop drives whatever the machine server hosts and can coordinate over.
    #[default]
    AllWithRemote,
    /// Only these store URL ids (still additionally requiring a remote). Opt-in by id.
    Only(HashSet<String>),
}

/// How the server-hosted driving loop behaves. **Off by default** (`enabled: false`) — a
/// server drives AI tools only when explicitly turned on, like the live-tool test tier.
#[derive(Debug, Clone)]
pub struct DistWorkConfig {
    /// Master switch. When false, [`spawn_dist_work_loop`] returns an inert handle.
    pub enabled: bool,
    /// The worker id recorded on claims (a git email identifies the machine/operator).
    pub worker: String,
    /// Lease length for each claim, in minutes.
    pub lease_minutes: i64,
    /// The most tickets this server drives concurrently across all hosted stores in one
    /// pass (a worker-pool bound). One claim is taken per store per pass, so this also caps
    /// how many stores advance per pass.
    pub max_in_flight: usize,
    /// Which stores participate.
    pub participation: Participation,
}

impl Default for DistWorkConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            worker: "server".to_string(),
            lease_minutes: 30,
            max_in_flight: 1,
            participation: Participation::AllWithRemote,
        }
    }
}

impl DistWorkConfig {
    /// Whether a store (by URL id) is in the participating set (the remote check is
    /// separate — it surfaces as [`StorePass::Skipped`] at claim time via `NoRemote`).
    pub fn participating(&self, store_id: &str) -> bool {
        match &self.participation {
            Participation::AllWithRemote => true,
            Participation::Only(ids) => ids.contains(store_id),
        }
    }
}

/// What one store did in a driving pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StorePass {
    /// A ticket was claimed and driven; carries its id (as a string) + the outcome.
    Worked(String, WorkOutcome),
    /// Nothing claimable this pass (queue drained, or every candidate lost the race).
    Idle,
    /// Skipped: not participating, no git remote (local-only), or the in-flight cap was hit.
    Skipped,
    /// A real error opening the store or talking to its remote (not a lost race).
    Errored(String),
}

/// Run **one** driving pass across the hosted stores: sweep expired markers, then for each
/// participating store with a remote drive one self-claimed ticket via `drive`, up to
/// `max_in_flight` driven tickets total. Pure over the injected `drive` and `now`, so the
/// orchestration is fully testable with a fake tool.
pub fn work_pass<D>(
    state: &AppState,
    cfg: &DistWorkConfig,
    now: &Timestamp,
    drive: &D,
) -> Vec<(String, StorePass)>
where
    D: Fn(&FsStore, &hotsheet_model::Ulid) -> WorkOutcome,
{
    let mut out = Vec::new();
    let mut in_flight = 0usize;
    for (id, root) in state.hosted_store_roots() {
        if !cfg.participating(&id) {
            out.push((id, StorePass::Skipped));
            continue;
        }
        let store = match FsStore::open(Path::new(&root)) {
            Ok(s) => s,
            Err(e) => {
                out.push((id, StorePass::Errored(e.to_string())));
                continue;
            }
        };
        // Reap any markers whose lease lapsed (best-effort — a NoRemote store just yields 0).
        let _ = distclaim::sweep_expired(store.root(), now);

        if in_flight >= cfg.max_in_flight {
            // At the worker-pool bound — don't claim more this pass.
            out.push((id, StorePass::Skipped));
            continue;
        }
        match distwork::work_once(&store, &cfg.worker, cfg.lease_minutes, now, |wid| {
            drive(&store, wid)
        }) {
            Ok(Some((wid, outcome))) => {
                in_flight += 1;
                out.push((id, StorePass::Worked(wid.to_string(), outcome)));
            }
            Ok(None) => out.push((id, StorePass::Idle)),
            // A store with no remote is a local-only project — harmless, just skipped.
            Err(DistError::NoRemote) => out.push((id, StorePass::Skipped)),
            Err(e) => out.push((id, StorePass::Errored(e.to_string()))),
        }
    }
    out
}

/// Keeps the driving-loop thread alive; **dropping it stops the loop** after its current
/// pass (the thread observes the disconnected channel). Inert when the loop is disabled.
pub struct DistWorkHandle {
    _stop: Option<std::sync::mpsc::Sender<()>>,
}

/// Spawn the server-hosted driving loop with an injected `drive`. Disabled config → an
/// inert handle and no thread (the default; a server drives tools only when opted in).
/// The thread runs [`work_pass`] every `interval`; the `drive` spawns the real tool in
/// production (a `live::run_trigger` closure) or a fake in tests. Dropping the returned
/// handle stops the loop.
pub fn spawn_dist_work_loop<D>(
    state: AppState,
    cfg: DistWorkConfig,
    interval: Duration,
    drive: D,
) -> DistWorkHandle
where
    D: Fn(&FsStore, &hotsheet_model::Ulid) -> WorkOutcome + Send + 'static,
{
    if !cfg.enabled {
        return DistWorkHandle { _stop: None };
    }
    // Drop-to-stop: the handle holds the sender; the thread's `recv_timeout` returns
    // `Disconnected` once it's dropped (mirrors the sync loop's shape).
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    std::thread::spawn(move || run(state, cfg, interval, rx, drive));
    DistWorkHandle { _stop: Some(tx) }
}

fn run<D>(state: AppState, cfg: DistWorkConfig, interval: Duration, rx: Receiver<()>, drive: D)
where
    D: Fn(&FsStore, &hotsheet_model::Ulid) -> WorkOutcome,
{
    loop {
        let now = crate::now();
        let _ = work_pass(&state, &cfg, &now, &drive);
        match rx.recv_timeout(interval) {
            Ok(()) => {}
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::Ulid;
    use hotsheet_ticketing::ops::{NewTicket, create};
    use hotsheet_ticketing::{FsStore, StoreMetadata};
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SECRET: &str = "test-secret";

    fn ts(s: &str) -> Timestamp {
        Timestamp::new(format!("2026-08-22T00:00:{s}Z"))
    }

    fn git(args: &[&str]) {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .status()
                .unwrap()
                .success(),
            "git {args:?}"
        );
    }

    /// A store that is a clone of a fresh bare remote, seeded with `n` Up Next tickets.
    /// Returns the temp dir (kept alive) + the clone's path.
    fn store_with_remote(root: &Path, n: usize) -> std::path::PathBuf {
        let bare = root.join("remote.git");
        let base = root.join("base");
        git(&["init", "-q", "--bare", "-b", "main", bare.to_str().unwrap()]);
        git(&["init", "-q", "-b", "main", base.to_str().unwrap()]);
        let base_store = FsStore::init(&base, &StoreMetadata::new("HS")).unwrap();
        for i in 0..n {
            create(
                &base_store,
                Ulid::new(),
                "HS",
                ts("00"),
                NewTicket {
                    title: format!("t-{i}"),
                    up_next: true,
                    ..Default::default()
                },
            )
            .unwrap();
        }
        git(&[
            "-C",
            base.to_str().unwrap(),
            "remote",
            "add",
            "origin",
            bare.to_str().unwrap(),
        ]);
        git(&[
            "-C",
            base.to_str().unwrap(),
            "push",
            "-q",
            "-u",
            "origin",
            "main",
        ]);
        let clone = root.join("clone");
        git(&[
            "clone",
            "-q",
            bare.to_str().unwrap(),
            clone.to_str().unwrap(),
        ]);
        clone
    }

    /// A local-only store (no remote) with an Up Next ticket.
    fn local_store(root: &Path) -> std::path::PathBuf {
        let s = FsStore::init(root, &StoreMetadata::new("LO")).unwrap();
        create(
            &s,
            Ulid::new(),
            "LO",
            ts("00"),
            NewTicket {
                title: "local".into(),
                up_next: true,
                ..Default::default()
            },
        )
        .unwrap();
        root.to_path_buf()
    }

    #[test]
    fn pass_drives_remote_stores_and_skips_local_only() {
        let dir = tempfile::tempdir().unwrap();
        // Primary store: a clone with a remote + two Up Next tickets.
        let remote_clone = store_with_remote(&dir.path().join("r"), 2);
        let state = AppState::new(FsStore::open(&remote_clone).unwrap(), SECRET.into()).unwrap();
        // A second hosted store: local-only (no remote).
        let local = local_store(&dir.path().join("l"));
        state.host_store(FsStore::open(&local).unwrap()).unwrap();

        let calls = AtomicUsize::new(0);
        let drive = |_s: &FsStore, _id: &Ulid| {
            calls.fetch_add(1, Ordering::SeqCst);
            WorkOutcome::Completed
        };

        let cfg = DistWorkConfig {
            enabled: true,
            max_in_flight: 8,
            ..Default::default()
        };
        let now = ts("10");
        let passes = work_pass(&state, &cfg, &now, &drive);

        // Exactly one ticket driven (the remote store's top); the local-only store skipped.
        assert_eq!(calls.load(Ordering::SeqCst), 1, "one ticket driven");
        let worked = passes
            .iter()
            .filter(|(_, p)| matches!(p, StorePass::Worked(_, _)))
            .count();
        let skipped = passes
            .iter()
            .filter(|(_, p)| *p == StorePass::Skipped)
            .count();
        assert_eq!(worked, 1, "the remote store drove one ticket");
        assert_eq!(skipped, 1, "the local-only store was skipped (NoRemote)");
    }

    #[test]
    fn max_in_flight_caps_driven_tickets_across_stores() {
        let dir = tempfile::tempdir().unwrap();
        // Two independent remote stores, each with a queue.
        let c1 = store_with_remote(&dir.path().join("a"), 2);
        let c2 = store_with_remote(&dir.path().join("b"), 2);
        let state = AppState::new(FsStore::open(&c1).unwrap(), SECRET.into()).unwrap();
        state.host_store(FsStore::open(&c2).unwrap()).unwrap();

        let calls = AtomicUsize::new(0);
        let drive = |_s: &FsStore, _id: &Ulid| {
            calls.fetch_add(1, Ordering::SeqCst);
            WorkOutcome::Completed
        };
        // Bound of 1: even with two ready stores, only one ticket is driven this pass.
        let cfg = DistWorkConfig {
            enabled: true,
            max_in_flight: 1,
            ..Default::default()
        };
        let now = ts("10");
        let passes = work_pass(&state, &cfg, &now, &drive);
        assert_eq!(calls.load(Ordering::SeqCst), 1, "in-flight bound honored");
        assert_eq!(
            passes
                .iter()
                .filter(|(_, p)| matches!(p, StorePass::Worked(_, _)))
                .count(),
            1
        );
    }

    #[test]
    fn participation_only_filters_by_store_id() {
        let dir = tempfile::tempdir().unwrap();
        let clone = store_with_remote(&dir.path().join("r"), 1);
        let store = FsStore::open(&clone).unwrap();
        let state = AppState::new(store.clone(), SECRET.into()).unwrap();
        let primary_id = crate::multistore::store_url_id(&store);

        // Participation set that does NOT include the primary → it's skipped, never driven.
        let cfg = DistWorkConfig {
            enabled: true,
            participation: Participation::Only(HashSet::from(["nonexistent".to_string()])),
            ..Default::default()
        };
        let calls = AtomicUsize::new(0);
        let drive = |_s: &FsStore, _id: &Ulid| {
            calls.fetch_add(1, Ordering::SeqCst);
            WorkOutcome::Completed
        };
        let passes = work_pass(&state, &cfg, &ts("10"), &drive);
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "non-participating → no drive"
        );
        assert_eq!(passes, vec![(primary_id, StorePass::Skipped)]);
    }

    #[test]
    fn disabled_config_spawns_no_thread() {
        let dir = tempfile::tempdir().unwrap();
        let clone = store_with_remote(&dir.path().join("r"), 1);
        let state = AppState::new(FsStore::open(&clone).unwrap(), SECRET.into()).unwrap();
        // Default is disabled → an inert handle, and the drive is never invoked.
        let _h = spawn_dist_work_loop(
            state,
            DistWorkConfig::default(),
            DEFAULT_INTERVAL,
            |_s: &FsStore, _id: &Ulid| WorkOutcome::Completed,
        );
        // Nothing to assert beyond "it returned without spawning work"; the type-level
        // guarantee is that a disabled loop takes the early return before std::thread::spawn.
    }
}
