//! HS2-S3FKJ7: the orchestration lines of a target checkout's tickets.
//!
//! A read from the target side. It finds the orchestration projects whose reviewed
//! binding on this machine puts one of their targets on the checkout, reads each
//! one's timeline (HS2-J89JC7) from this machine's records, and keeps one line per
//! target ticket through the pure fold `target_lines`. Like the timeline it passes no
//! gate, contacts no target and writes nothing. With a version and `wait_ms` it is a
//! long poll on the records the timelines read, never a timer.
//!
//! HS2-3D8D6V: the same read answers for a checkout on a remote server this machine
//! binds (`Remote { registration_id, checkout_id }`), so the desktop that runs the
//! orchestration project shows the line in that remote project's tab. The plan stays
//! here; nothing is asked of the remote server.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hotsheet_ticketing::orchestration::{
    BindingRegistry, DecisionJournal, LineObservations, ObservedOwner, RunEventJournal,
    TargetLineSource, TargetLines, Timeline, target_lines_observed,
};

use super::scheduler::WAKE_DIRECTORY;
use super::tickets::identifier;
use super::timeline::MAX_TIMELINE_WAIT_MS;
use super::*;

/// At most this many orchestration projects are read for one checkout.
pub const MAX_TARGET_LINE_OWNERS: usize = 16;

/// Which checkout the lines are for: one on this machine, or one on a remote server
/// this machine binds (HS2-3D8D6V).
#[derive(Clone, Copy)]
pub(super) enum LinesOf<'a> {
    Here(&'a str),
    Remote {
        registration_id: &'a str,
        checkout_id: &'a str,
    },
}

impl LinesOf<'_> {
    fn bound(self, home: &std::path::Path) -> Result<Vec<TargetRef>, OrchestrationError> {
        let bindings = BindingRegistry::new(home);
        match self {
            Self::Here(checkout_id) => bindings.bound_here(checkout_id),
            Self::Remote {
                registration_id,
                checkout_id,
            } => bindings.bound_remote(registration_id, checkout_id),
        }
    }
}

impl OrchestrationRuntime {
    /// The lines as the records are now, and the ticket stores whose change could
    /// clear a pending decision.
    pub(super) fn target_lines_now(
        &self,
        of: LinesOf<'_>,
        tx: &mut Transaction<'_>,
    ) -> Result<(TargetLines, Vec<String>, Vec<PathBuf>), OrchestrationError> {
        // Owners in the bindings file's order, each with its targets bound here.
        let mut owners: Vec<(String, Vec<String>)> = Vec::new();
        for target in of.bound(&self.home)? {
            if let Some(at) = owners
                .iter()
                .position(|(owner, _)| *owner == target.owner_checkout_id)
            {
                owners[at].1.push(target.target_id);
            } else if owners.len() < MAX_TARGET_LINE_OWNERS {
                owners.push((target.owner_checkout_id, vec![target.target_id]));
            }
        }
        let mut read: Vec<(String, String, Vec<String>, Timeline)> = Vec::new();
        let mut stores: Vec<PathBuf> = Vec::new();
        for (owner_id, targets) in owners {
            // An owner whose checkout is gone, or no longer an orchestration project
            // here, shows nothing; the rest still answer.
            let Ok(owner) = self.owner(&owner_id) else {
                continue;
            };
            let Ok((view, watched)) = self.timeline_now(&owner.id, tx) else {
                continue;
            };
            let label = Some(owner.alias.clone())
                .filter(|alias| !alias.is_empty())
                .unwrap_or_else(|| owner.id.clone());
            stores.extend(watched);
            read.push((owner.id, label, targets, view));
        }
        let sources: Vec<TargetLineSource<'_>> = read
            .iter()
            .map(|(owner, label, targets, view)| TargetLineSource {
                owner_checkout_id: owner,
                owner_label: label,
                target_ids: targets,
                timeline: view,
            })
            .collect();
        // HS2-KB2Y51: a checkout hosted here also shows the lines other machines
        // pushed for it, merged after this machine's own; a remote checkout's lines
        // are this machine's alone.
        let pushed = match of {
            LinesOf::Here(checkout_id) => {
                LineObservations::new(&self.home).live(checkout_id, tx.clock.now_ms())
            }
            LinesOf::Remote { .. } => Vec::new(),
        };
        let observed: Vec<ObservedOwner<'_>> = pushed
            .iter()
            .map(|entry| ObservedOwner {
                owner_server_id: &entry.owner_server_id,
                lines: &entry.lines,
            })
            .collect();
        let lines = target_lines_observed(&sources, &observed);
        let owners = read.into_iter().map(|(owner, ..)| owner).collect();
        Ok((lines, owners, stores))
    }

    /// HS2-S3FKJ7: the orchestration line of each of `checkout_id`'s tickets that an
    /// orchestration project on this machine plans. With `after` equal to the current
    /// version and a `wait_ms`, waits for the lines to change, woken by the records the
    /// timelines read changing, and answers the same version at the deadline.
    pub fn orchestration_target_lines(
        &self,
        checkout_id: &str,
        after: Option<&str>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetLines, OrchestrationError> {
        let checkout = self.owner(checkout_id)?;
        self.target_lines_wait(LinesOf::Here(&checkout.id), after, wait_ms, tx)
    }

    /// HS2-3D8D6V: the same read for checkout `checkout_id` on the remote server
    /// registered here as `registration_id`: the line of each of its tickets that an
    /// orchestration project on this machine plans through a reviewed remote binding.
    /// Answers from this machine's records alone and never contacts the remote server;
    /// an unknown registration or checkout reads as not orchestrated.
    pub fn orchestration_remote_target_lines(
        &self,
        registration_id: &str,
        checkout_id: &str,
        after: Option<&str>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetLines, OrchestrationError> {
        identifier(registration_id, "bounded registration id required")?;
        identifier(checkout_id, "bounded checkout id required")?;
        self.target_lines_wait(
            LinesOf::Remote {
                registration_id,
                checkout_id,
            },
            after,
            wait_ms,
            tx,
        )
    }

    fn target_lines_wait(
        &self,
        of: LinesOf<'_>,
        after: Option<&str>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetLines, OrchestrationError> {
        if wait_ms == 0 || after.is_none() {
            return Ok(self.target_lines_now(of, tx)?.0);
        }
        // HS2-WD475E: the deadline counts from the request, whatever the reads and the
        // watcher's start take, so a reader's `wait_ms` is the longest it waits.
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(wait_ms.min(MAX_TIMELINE_WAIT_MS));
        // The journals
        // keep one file per owner in shared directories, so one watch covers them all;
        // the directory name does not depend on the id given.
        let any = "target-lines";
        let runs = RunEventJournal::new(&self.home, any)?;
        let decisions = DecisionJournal::new(&self.home, any)?;
        let wake = self.home.join(WAKE_DIRECTORY);
        // A checkout no project orchestrates still waits to the deadline, so a reader
        // that keeps asking never spins.
        let (first, _, stores) = self.target_lines_now(of, tx)?;
        if after != Some(first.version.as_str()) {
            return Ok(first);
        }
        // HS2-KB2Y51: a push from another machine ends the wait too.
        let pushed = LineObservations::new(&self.home).directory();
        let mut dirs = vec![
            runs.directory(),
            decisions.directory(),
            wake.as_path(),
            pushed.as_path(),
        ];
        let unique: BTreeMap<&PathBuf, ()> = stores.iter().map(|store| (store, ())).collect();
        dirs.extend(unique.keys().map(|store| store.as_path()));
        // HS2-F8174X: the stores to watch are known only after the first read, so the
        // watcher starts after it. HS2-WD475E: it starts in the background; until it
        // runs, each wait ends at the one-second safety re-check and the lines are read
        // again, so a change made meanwhile is found within that interval instead of
        // after a start that took seconds on a loaded machine.
        let Ok(waiter) = hotsheet_jobs::ChangeWaiter::starting(&dirs) else {
            return Ok(first);
        };
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return self.target_lines_now(of, tx).map(|(lines, ..)| lines);
            }
            // HS2-F8174X: read again at the waiter's one-second safety re-check too, so a
            // change no watched directory reported ends the wait within a second rather
            // than at the deadline (HS2-1C77XH). Each read is local files only.
            let _ = waiter.wait(remaining);
            let (lines, ..) = self.target_lines_now(of, tx)?;
            if lines.version != first.version {
                return Ok(lines);
            }
        }
    }
}
