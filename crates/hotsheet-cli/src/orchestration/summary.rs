//! HS2-S13719: the progress and cost summary of an owning checkout's runs.
//!
//! A read. It answers from this machine's records, contacts no target and writes
//! nothing, like the gate reads of HS2-5JBYPV. Cost comes from the usage records of
//! `docs/14` (HS2-38RJMK): the same per-ticket read the ticket timeline uses, taken
//! from the target ticket's Git store when the target's reviewed destination is a
//! checkout on this machine. Anything else is reported as unknown with its reason.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_ticketing::metrics;
use hotsheet_ticketing::orchestration::{
    BindingRegistry, CostReading, GateFilter, GateRegistry, MAX_AUDIT_PAGE, Metric, ProgressInput,
    ProgressSummary, Run, RunEvent, RunEventJournal, RunTicket, SessionRegistry, TargetDestination,
    UnknownReason, ValidationRegistry, WorkItemKey, summarize_progress,
};

use super::tickets::{identifier, native};
use super::*;

fn unknown(reason: UnknownReason) -> Metric<CostReading> {
    Metric::Unknown { reason }
}

impl OrchestrationRuntime {
    /// Every retained run event of the owner, and whether retention removed older ones.
    fn retained_run_events(
        &self,
        owner_id: &str,
    ) -> Result<(Vec<RunEvent>, bool), OrchestrationError> {
        let journal = RunEventJournal::new(&self.home, owner_id)?;
        let (mut events, mut after, mut trimmed) = (Vec::new(), 0, false);
        loop {
            let page = journal.read(after, MAX_AUDIT_PAGE, None)?;
            // Only the first page can show that the journal no longer starts at its
            // beginning; a later gap would be retention racing this read.
            trimmed |= page.gap;
            let done = !page.more || page.cursor == after;
            after = page.cursor;
            events.extend(page.entries.into_iter().map(|entry| entry.record));
            if done {
                return Ok((events, trimmed));
            }
        }
    }

    /// What the usage records say the run's target ticket cost, read through the run's
    /// target. Never contacts another machine.
    fn work_item_cost(&self, run: &Run) -> Metric<CostReading> {
        let Ok(target) = Self::target_ref(&run.owner_checkout_id, &run.target.target_id) else {
            return unknown(UnknownReason::TargetUnbound);
        };
        let checkout_id = match BindingRegistry::new(&self.home).get(&target) {
            Ok(binding) => match binding.destination {
                Some(TargetDestination::Local { checkout_id, .. }) => checkout_id,
                Some(TargetDestination::Remote { .. }) => {
                    return unknown(UnknownReason::TargetRemote);
                }
                None => return unknown(UnknownReason::TargetUnbound),
            },
            Err(_) => return unknown(UnknownReason::UsageStoreUnavailable),
        };
        let Ok(checkout) = self.checkouts.describe(&checkout_id) else {
            return unknown(UnknownReason::UsageStoreUnavailable);
        };
        match checkout.source(&run.target.connection_id) {
            Some(source) if source.provider == "git" => {}
            Some(_) => return unknown(UnknownReason::UsageSourceUnsupported),
            None => return unknown(UnknownReason::UsageStoreUnavailable),
        }
        let cost = checkout
            .verified_git_source(&run.target.connection_id)
            .ok()
            .and_then(|store| metrics::ticket_cost(&store, &run.target.native_id).ok());
        match cost {
            Some(Some(cost)) => Metric::Known {
                value: CostReading {
                    cost_usd: cost.cost_usd,
                    events: cost.events,
                    unpriced_events: cost.unpriced_events,
                },
            },
            Some(None) => unknown(UnknownReason::NoUsageRecorded),
            None => unknown(UnknownReason::UsageStoreUnavailable),
        }
    }

    /// HS2-S13719: lead time, blocked time, retries, gate outcomes and known AI cost of
    /// the owner's runs, per run, per work item, per target and in total. With
    /// `ticket`, only the runs of that orchestration ticket; an empty connection id
    /// matches any source.
    pub fn progress_summary(
        &self,
        owner_id: &str,
        ticket: Option<&RunTicket>,
        tx: &mut Transaction<'_>,
    ) -> Result<ProgressSummary, OrchestrationError> {
        if let Some(ticket) = ticket {
            native(&ticket.native_id)?;
            if !ticket.connection_id.is_empty() {
                identifier(&ticket.connection_id, "bounded connection id required")?;
            }
        }
        let now_ms = tx.clock.now_ms();
        let mut runs = self.run_list(owner_id, false)?;
        runs.retain(|run| {
            ticket.is_none_or(|ticket| {
                run.ticket.native_id == ticket.native_id
                    && (ticket.connection_id.is_empty()
                        || run.ticket.connection_id == ticket.connection_id)
            })
        });
        // One reading per work item. A second run of the same item is asked only when
        // an earlier one could not be read, so a readable target is preferred.
        let mut costs: BTreeMap<WorkItemKey, Metric<CostReading>> = BTreeMap::new();
        let mut asked: BTreeSet<(String, WorkItemKey)> = BTreeSet::new();
        for run in &runs {
            let key = WorkItemKey::of(run);
            if costs.get(&key).is_some_and(|cost| cost.known().is_some())
                || !asked.insert((run.target.target_id.clone(), key.clone()))
            {
                continue;
            }
            let cost = self.work_item_cost(run);
            if cost.known().is_some() || !costs.contains_key(&key) {
                costs.insert(key, cost);
            }
        }
        let (events, events_trimmed) = self.retained_run_events(owner_id)?;
        Ok(summarize_progress(&ProgressInput {
            owner_checkout_id: owner_id,
            now_ms,
            runs: &runs,
            gates: &GateRegistry::new(&self.home).list(owner_id, &GateFilter::default())?,
            validations: &ValidationRegistry::new(&self.home).steps(owner_id, None, now_ms)?,
            sessions: &SessionRegistry::new(&self.home).list(owner_id, false)?,
            events: &events,
            events_trimmed,
            costs: &costs,
        }))
    }
}
