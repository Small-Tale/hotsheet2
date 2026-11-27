//! HS2-AWB7BX: wake the scheduler when a target's capacity changes.
//!
//! A pass that left work unplaced because a target's machine had no room (it refused
//! admission, or a capacity refusal is being waited out) follows that target's
//! capacity with the long poll of HS2-P5GTSW, from the version the pass read. A change
//! lifts the capacity refusal and wakes the loop. Nothing polls: each request parks on
//! the host until the capacity changes or the wait ends, and is then asked again. When
//! no event can arrive (an older host without the wait, an unreachable one, one that
//! cannot wait) the watch ends, the target is marked as having no stream, and the
//! refusal backoff's retry time applies again.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use hotsheet_resource_claims::MAX_WAIT;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    Blocker, CapacitySnapshot, Schedule, SchedulerStore, TargetAction, is_capacity_refusal,
    is_signed_out_detail,
};

use super::capacity::{MAX_CAPACITY_WAIT_MS, capacity_query};
use super::*;

/// Targets one process follows at a time, per machine home.
pub const MAX_CAPACITY_WATCHES: usize = 16;
/// A watch passes the gate again after this long.
const REAUTHORIZE_MS: u64 = 10 * 60 * 1000;
/// HS2-H1CSSN: why a follow ended when its owner stopped.
pub const STOPPING: &str = "stopping";

/// How a capacity follow ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapacityFollow {
    /// The capacity changed to this version.
    Changed(String),
    /// No event can arrive this way; a stable code says why.
    Ended(String),
}

/// What this process knows of one target's capacity stream.
#[derive(Debug, Clone, Default)]
struct Followed {
    /// The version and claim namespaces of the last read.
    version: Option<String>,
    namespaces: Vec<String>,
    /// The host offers no wait, or the last wait could not be made.
    failed: bool,
    watching: bool,
}

type Key = (PathBuf, String, String);
/// Global because runtimes are built per request and per command: the follows one
/// process holds, and its cap, span all of them (HS2-H1CSSN).
static FOLLOWED: OnceLock<Mutex<BTreeMap<Key, Followed>>> = OnceLock::new();
fn followed() -> std::sync::MutexGuard<'static, BTreeMap<Key, Followed>> {
    FOLLOWED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl OrchestrationRuntime {
    fn followed_key(&self, owner_id: &str, target_id: &str) -> Key {
        (self.home.clone(), owner_id.to_owned(), target_id.to_owned())
    }

    /// A pass read a target's capacity: remember the version a watch waits on. A
    /// snapshot without one comes from a host that offers no wait.
    pub(super) fn capacity_seen(
        &self,
        owner_id: &str,
        target_id: &str,
        namespaces: &[String],
        version: Option<&str>,
    ) {
        let mut known = followed();
        let entry = known
            .entry(self.followed_key(owner_id, target_id))
            .or_default();
        entry.version = version.map(str::to_owned);
        entry.namespaces = namespaces.to_vec();
        entry.failed = version.is_none();
    }

    /// Targets of the owner whose capacity changes arrive as events: those followed
    /// now. A one-shot pass, or a target whose host offers no wait, keeps the
    /// backoff's retry time.
    pub(super) fn capacity_streams(&self, owner_id: &str) -> Vec<String> {
        self.capacity_watches(owner_id)
    }

    /// Targets whose capacity is being followed now.
    pub fn capacity_watches(&self, owner_id: &str) -> Vec<String> {
        followed()
            .iter()
            .filter(|((home, owner, _), entry)| {
                home == &self.home && owner == owner_id && entry.watching
            })
            .map(|((_, _, target), _)| target.clone())
            .collect()
    }

    /// Follow every target where this schedule left work for lack of room: a refused
    /// admission, or a capacity refusal being waited out. Once per target.
    pub(super) fn watch_capacity(
        &self,
        owner_id: &str,
        actor: &MutationActor,
        schedule: &Schedule,
    ) {
        let roomless: std::collections::BTreeSet<&str> = schedule
            .unplaced
            .iter()
            .filter(|item| match item.reason {
                Blocker::MachineRefused => true,
                Blocker::RefusalBackoff => item.detail.as_deref().is_some_and(is_capacity_refusal),
                // HS2-ZRPZJJ: a sign-in there changes the machine's capacity version.
                Blocker::NoTool => item.detail.as_deref().is_some_and(is_signed_out_detail),
                _ => false,
            })
            .map(|item| item.target_id.as_str())
            .collect();
        for target_id in roomless {
            let key = self.followed_key(owner_id, target_id);
            let (after, namespaces) = {
                let mut known = followed();
                let here = known
                    .iter()
                    .filter(|((home, ..), entry)| home == &self.home && entry.watching)
                    .count();
                let Some(entry) = known.get_mut(&key) else {
                    continue;
                };
                let Some(version) = entry.version.clone() else {
                    continue;
                };
                if entry.failed || entry.watching || here >= MAX_CAPACITY_WATCHES {
                    continue;
                }
                entry.watching = true;
                (version, entry.namespaces.clone())
            };
            let Ok(target) = Self::target_ref(owner_id, target_id) else {
                continue;
            };
            let runtime = self.clone();
            let owner = owner_id.to_owned();
            let actor = actor.clone();
            let thread_key = key.clone();
            // HS2-H1CSSN: owned by the runtime's tasks, and ended by its stop token.
            let spawned = self.tasks.spawn("orchestration-capacity-wait", move || {
                let mut rng = super::request::Rng;
                let outcome = runtime.follow_capacity_change(
                    &target,
                    &actor,
                    &namespaces,
                    &after,
                    Instant::now() + MAX_WAIT,
                    &mut Transaction {
                        clock: &super::request::Clock,
                        rng: &mut rng,
                    },
                );
                runtime.capacity_followed(&owner, &target.target_id, &thread_key, &outcome);
            });
            if spawned.is_err()
                && let Some(entry) = followed().get_mut(&key)
            {
                entry.watching = false;
            }
        }
    }

    /// Fold how a follow ended into what this process knows, then wake the loop: a
    /// change lifts the target's capacity refusal; an end restores its retry time.
    fn capacity_followed(
        &self,
        owner_id: &str,
        target_id: &str,
        key: &Key,
        outcome: &CapacityFollow,
    ) {
        if let Some(entry) = followed().get_mut(key) {
            entry.watching = false;
            match outcome {
                CapacityFollow::Changed(version) => entry.version = Some(version.clone()),
                // A stop says nothing about the host's stream.
                CapacityFollow::Ended(code) if code == STOPPING => {}
                CapacityFollow::Ended(_) => entry.failed = true,
            }
        }
        // HS2-H1CSSN: a stopping owner wakes nothing more.
        if self.tasks.is_stopping() {
            return;
        }
        if matches!(outcome, CapacityFollow::Changed(_)) {
            let _ = SchedulerStore::new(&self.home).lift_capacity_refusal(owner_id, target_id);
        }
        self.scheduler_wake(owner_id);
    }

    /// Follow one target's capacity from `after` until it changes, the deadline passes
    /// or no event can arrive. Passes the gate as `gates_observe` (it never asks) and
    /// again after ten minutes; each request is a long poll on the target's host.
    pub fn follow_capacity_change(
        &self,
        target: &TargetRef,
        actor: &MutationActor,
        namespaces: &[String],
        after: &str,
        deadline: Instant,
        tx: &mut Transaction<'_>,
    ) -> CapacityFollow {
        let query = capacity_query(namespaces, Some(after), MAX_CAPACITY_WAIT_MS);
        let mut authorized: Option<(Authorized, u64)> = None;
        loop {
            if Instant::now() >= deadline {
                return CapacityFollow::Ended("deadline".into());
            }
            // HS2-H1CSSN: checked between requests; one in flight ends within its wait.
            if self.tasks.is_stopping() {
                return CapacityFollow::Ended(STOPPING.into());
            }
            let now_ms = tx.clock.now_ms();
            if authorized
                .as_ref()
                .is_none_or(|(_, at)| now_ms.saturating_sub(*at) >= REAUTHORIZE_MS)
            {
                match self.authorize(
                    target,
                    Some(actor),
                    ActionRequest::new(TargetAction::GatesObserve),
                    tx,
                ) {
                    Ok(fresh) => authorized = Some((fresh, now_ms)),
                    Err(error) => return CapacityFollow::Ended(error.code().into()),
                }
            }
            let Some((current, _)) = &authorized else {
                return CapacityFollow::Ended("gate_refused".into());
            };
            let asked = Instant::now();
            let reply = match Self::destination(current) {
                Ok(destination) => {
                    match self.hosted_long_poll(destination, "/target-capacity", &query, tx) {
                        Some(reply) => reply,
                        None => return CapacityFollow::Ended(STOPPING.into()),
                    }
                }
                Err(error) => Err(error),
            };
            // HS2-K0RSCQ: an answer the stop cut short says nothing about the host.
            if self.tasks.is_stopping() {
                return CapacityFollow::Ended(STOPPING.into());
            }
            let reply = reply.and_then(|reply| {
                serde_json::from_value::<CapacitySnapshot>(reply)
                    .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))
            });
            match reply {
                Err(error) => return CapacityFollow::Ended(error.code().into()),
                // An older host answers without a version and never waits.
                Ok(CapacitySnapshot { version: None, .. }) => {
                    return CapacityFollow::Ended("capacity_stream_unsupported".into());
                }
                Ok(CapacitySnapshot {
                    version: Some(version),
                    ..
                }) if version != after => return CapacityFollow::Changed(version),
                // A host that answers the same long before the wait it was asked for is
                // not waiting: asking again at once would spin.
                Ok(_) if asked.elapsed() < Duration::from_millis(MAX_CAPACITY_WAIT_MS / 2) => {
                    return CapacityFollow::Ended("peer_did_not_wait".into());
                }
                Ok(_) => {}
            }
        }
    }
}
