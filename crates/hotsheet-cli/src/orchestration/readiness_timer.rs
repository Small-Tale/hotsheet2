//! HS2-J7N3PQ: publish readiness changes that only time causes.
//!
//! Readiness is recomputed where runs, gates and validation results change. One level
//! changes by time alone: a running validation attempt that outlives its deadline
//! becomes `unverified`, so a step that waits on it becomes `stale`. Whenever the plan
//! is evaluated, the first such deadline of the owner's running attempts arms one
//! timer for that instant (a one-shot wake-up at a known time, never an interval).
//! When it fires the plan is evaluated again at that instant, which publishes the
//! changed level as a `readiness` event and arms the next deadline, if any. An attempt
//! that finishes first makes the wake-up publish nothing. A timer lives as long as its
//! process; after a restart the next evaluation of the plan arms it again.
//!
//! HS2-H1CSSN: the timer is a thread of the runtime's [`RuntimeTasks`] and waits on its
//! stop token. A stopping owner wakes it at once; it then forgets its instant and
//! evaluates nothing, so a closed server's home is not touched again.
use std::collections::BTreeMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use hotsheet_ticketing::orchestration::next_lapse_ms;

use super::*;

/// The instant each owner's timer waits for, per machine home. Global because runtimes
/// are built per request and per command: one process arms one timer per owner.
type Armed = BTreeMap<(PathBuf, String), u64>;
static ARMED: OnceLock<Mutex<Armed>> = OnceLock::new();
fn armed() -> std::sync::MutexGuard<'static, Armed> {
    ARMED
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl OrchestrationRuntime {
    /// Replace how a deadline timer waits for its instant (tests skip the wait).
    #[must_use]
    pub fn with_timer_sleep(mut self, sleep: Arc<dyn Fn(Duration) + Send + Sync>) -> Self {
        self.timer_sleep = Some(sleep);
        self
    }

    /// The instant the owner's deadline timer waits for, if one is armed.
    pub fn lapse_timer(&self, owner_id: &str) -> Option<u64> {
        armed()
            .get(&(self.home.clone(), owner_id.to_owned()))
            .copied()
    }

    /// Arm one timer for the first deadline of the owner's running validation
    /// attempts after `now_ms`. An earlier deadline replaces a later armed one; the
    /// same or a later one changes nothing. Best effort.
    pub(super) fn arm_lapse_timer(&self, owner_id: &str, now_ms: u64) {
        let Ok(evidence) = self.validations().steps(owner_id, None, now_ms) else {
            return;
        };
        let Some(at) = next_lapse_ms(&evidence, now_ms) else {
            return;
        };
        let key = (self.home.clone(), owner_id.to_owned());
        {
            let mut timers = armed();
            if timers.get(&key).is_some_and(|armed_at| *armed_at <= at) {
                return;
            }
            timers.insert(key.clone(), at);
        }
        let runtime = self.clone();
        let owner = owner_id.to_owned();
        let tasks = self.tasks.clone();
        let spawned = tasks.spawn("orchestration-readiness-lapse", move || {
            let wait = Duration::from_millis(at.saturating_sub(now_ms));
            let stopping = match &runtime.timer_sleep {
                Some(sleep) => {
                    sleep(wait);
                    runtime.tasks.is_stopping()
                }
                None => runtime.tasks.wait_stop(wait),
            };
            {
                let mut timers = armed();
                if timers.get(&key) == Some(&at) {
                    timers.remove(&key);
                }
            }
            if stopping {
                return;
            }
            // Evaluated at the instant the level changes; this also arms the next.
            runtime.readiness_reevaluate(&owner, at);
        });
        if spawned.is_err() {
            let mut timers = armed();
            if timers.get(&(self.home.clone(), owner_id.to_owned())) == Some(&at) {
                timers.remove(&(self.home.clone(), owner_id.to_owned()));
            }
        }
    }
}
