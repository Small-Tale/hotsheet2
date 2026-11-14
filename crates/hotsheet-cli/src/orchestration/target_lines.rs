//! HS2-S3FKJ7: the orchestration lines of a target checkout's tickets.
//!
//! A read from the target side. It finds the orchestration projects whose reviewed
//! binding on this machine puts one of their targets on the checkout, reads each
//! one's timeline (HS2-J89JC7) from this machine's records, and keeps one line per
//! target ticket through the pure fold `target_lines`. Like the timeline it passes no
//! gate, contacts no target and writes nothing. With a version and `wait_ms` it is a
//! long poll on the records the timelines read, never a timer.

use std::collections::BTreeMap;
use std::path::PathBuf;

use hotsheet_ticketing::orchestration::{
    BindingRegistry, DecisionJournal, RunEventJournal, TargetLineSource, TargetLines, Timeline,
    target_lines,
};

use super::scheduler::WAKE_DIRECTORY;
use super::timeline::MAX_TIMELINE_WAIT_MS;
use super::*;

/// At most this many orchestration projects are read for one checkout.
pub const MAX_TARGET_LINE_OWNERS: usize = 16;

impl OrchestrationRuntime {
    /// The lines as the records are now, and the ticket stores whose change could
    /// clear a pending decision.
    fn target_lines_now(
        &self,
        checkout_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<(TargetLines, Vec<String>, Vec<PathBuf>), OrchestrationError> {
        // Owners in the bindings file's order, each with its targets bound here.
        let mut owners: Vec<(String, Vec<String>)> = Vec::new();
        for target in BindingRegistry::new(&self.home).bound_here(checkout_id)? {
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
        let lines = target_lines(&sources);
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
        if wait_ms == 0 || after.is_none() {
            return Ok(self.target_lines_now(&checkout.id, tx)?.0);
        }
        // Watch before reading, so a change in between is not missed. The journals
        // keep one file per owner in shared directories, so one watch covers them all.
        let any = &checkout.id;
        let runs = RunEventJournal::new(&self.home, any)?;
        let decisions = DecisionJournal::new(&self.home, any)?;
        let wake = self.home.join(WAKE_DIRECTORY);
        // A checkout no project orchestrates still waits to the deadline, so a reader
        // that keeps asking never spins.
        let (first, _, stores) = self.target_lines_now(&checkout.id, tx)?;
        if after != Some(first.version.as_str()) {
            return Ok(first);
        }
        let mut dirs = vec![runs.directory(), decisions.directory(), wake.as_path()];
        let unique: BTreeMap<&PathBuf, ()> = stores.iter().map(|store| (store, ())).collect();
        dirs.extend(unique.keys().map(|store| store.as_path()));
        let Ok(waiter) = hotsheet_jobs::ChangeWaiter::watching(&dirs) else {
            return Ok(first);
        };
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(wait_ms.min(MAX_TIMELINE_WAIT_MS));
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return self
                    .target_lines_now(&checkout.id, tx)
                    .map(|(lines, ..)| lines);
            }
            if !waiter.wait(remaining) {
                continue;
            }
            let (lines, ..) = self.target_lines_now(&checkout.id, tx)?;
            if lines.version != first.version {
                return Ok(lines);
            }
        }
    }
}
