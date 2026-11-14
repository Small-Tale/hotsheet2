//! HS2-S13719: the progress and cost summary of an owning checkout's runs.
//!
//! A read. It answers from this machine's records and writes nothing, like the gate
//! reads of HS2-5JBYPV. Cost comes from the usage records of `docs/14` (HS2-38RJMK): the
//! same per-ticket read the ticket timeline uses, taken from the target ticket's Git
//! store when the target's reviewed destination is a checkout on this machine, or
//! (HS2-B6236S) asked of a remote target's server through the reviewed transport, once
//! per remote work item, as an observation. Anything else is reported as unknown with
//! its reason.
use std::collections::{BTreeMap, BTreeSet};

use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::metrics;
use hotsheet_ticketing::orchestration::{
    BindingRegistry, CostReading, GateFilter, GateRegistry, MAX_AUDIT_PAGE, Metric,
    PlacementPolicy, ProgressInput, ProgressSummary, Run, RunEvent, RunEventJournal, RunTicket,
    SessionRegistry, TargetAction, TargetDestination, TargetOpError, UnknownReason,
    ValidationRegistry, WorkItemKey, summarize_progress,
};
use hotsheet_ticketing::orchestration::{
    DecisionJournal, DecisionKind, DecisionRecord, ExternalWaitInput, OrchestratedSession,
    Utilization, UtilizationInput, summarize_external_wait, summarize_utilization,
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
    /// target: from this machine's checkout, or (HS2-B6236S) from a remote target's
    /// server through the reviewed transport.
    fn work_item_cost(&self, run: &Run, tx: &mut Transaction<'_>) -> Metric<CostReading> {
        let Ok(target) = Self::target_ref(&run.owner_checkout_id, &run.target.target_id) else {
            return unknown(UnknownReason::TargetUnbound);
        };
        let checkout_id = match BindingRegistry::new(&self.home).get(&target) {
            Ok(binding) => match binding.destination {
                Some(TargetDestination::Local { checkout_id, .. }) => checkout_id,
                Some(TargetDestination::Remote { .. }) => {
                    return self.remote_ticket_cost(&target, run, tx);
                }
                None => return unknown(UnknownReason::TargetUnbound),
            },
            Err(_) => return unknown(UnknownReason::UsageStoreUnavailable),
        };
        let Ok(checkout) = self.checkouts.describe(&checkout_id) else {
            return unknown(UnknownReason::UsageStoreUnavailable);
        };
        Self::checkout_ticket_cost(&checkout, &run.target.connection_id, &run.target.native_id)
    }

    /// HS2-B6236S: a remote target's cost reading, asked of its server through the
    /// reviewed transport. It is an observation (`tickets_read`): adding the project
    /// consented to reading it, so it never asks, and it is audited as a read by the
    /// orchestration project's own orchestrator. The host's own unknowns pass through; a
    /// host without the route is `target_remote`, and anything else that fails is
    /// `target_unreachable`.
    fn remote_ticket_cost(
        &self,
        target: &TargetRef,
        run: &Run,
        tx: &mut Transaction<'_>,
    ) -> Metric<CostReading> {
        let Ok(actor) = MutationActor::parse(
            "system",
            Some(PlacementPolicy::default().acting_actor(&run.owner_checkout_id)),
        ) else {
            return unknown(UnknownReason::TargetUnreachable);
        };
        let Ok(authorized) = self.authorize(
            target,
            Some(&actor),
            ActionRequest::new(TargetAction::TicketsRead),
            tx,
        ) else {
            return unknown(UnknownReason::TargetUnreachable);
        };
        let query = format!(
            "?connection_id={}&native_id={}",
            super::tickets::encode(&run.target.connection_id),
            super::tickets::encode(&run.target.native_id)
        );
        let outcome = Self::destination(&authorized).and_then(|destination| {
            self.hosted_call_at(destination, "/target-tickets/cost", "GET", &query, None, tx)
        });
        let _ = self.finish(target, &authorized, &outcome, tx);
        match outcome {
            Ok(answer) => {
                serde_json::from_value(answer).unwrap_or(unknown(UnknownReason::TargetUnreachable))
            }
            // An older peer has no such route.
            Err(OrchestrationError::TargetOperation(TargetOpError::PeerUnsupported)) => {
                unknown(UnknownReason::TargetRemote)
            }
            Err(_) => unknown(UnknownReason::TargetUnreachable),
        }
    }

    /// HS2-B6236S hosted half: what a checkout of this machine's usage records say one of
    /// its tickets cost. Identifiers and metrics only; contacts nothing.
    pub fn hosted_ticket_cost(
        &self,
        checkout_id: &str,
        connection_id: &str,
        native_id: &str,
    ) -> Result<Metric<CostReading>, OrchestrationError> {
        identifier(connection_id, "bounded connection id required")?;
        native(native_id)?;
        let checkout = self.owner(checkout_id)?;
        Ok(Self::checkout_ticket_cost(
            &checkout,
            connection_id,
            native_id,
        ))
    }

    /// One ticket's cost from a checkout's Git source on this machine.
    fn checkout_ticket_cost(
        checkout: &hotsheet_ticketing::checkouts::Checkout,
        connection_id: &str,
        native_id: &str,
    ) -> Metric<CostReading> {
        match checkout.source(connection_id) {
            Some(source) if source.provider == "git" => {}
            Some(_) => return unknown(UnknownReason::UsageSourceUnsupported),
            None => return unknown(UnknownReason::UsageStoreUnavailable),
        }
        let cost = checkout
            .verified_git_source(connection_id)
            .ok()
            .and_then(|store| metrics::ticket_cost(&store, native_id).ok());
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
            let cost = self.work_item_cost(run, tx);
            if cost.known().is_some() || !costs.contains_key(&key) {
                costs.insert(key, cost);
            }
        }
        let (events, events_trimmed) = self.retained_run_events(owner_id)?;
        let sessions = SessionRegistry::new(&self.home).list(owner_id, false)?;
        let gates = GateRegistry::new(&self.home).list(owner_id, &GateFilter::default())?;
        let mut summary = summarize_progress(&ProgressInput {
            owner_checkout_id: owner_id,
            now_ms,
            runs: &runs,
            gates: &gates,
            validations: &ValidationRegistry::new(&self.home).steps(owner_id, None, now_ms)?,
            sessions: &sessions,
            events: &events,
            events_trimmed,
            costs: &costs,
        });
        let (decisions, decisions_trimmed) = self.summary_decisions(owner_id)?;
        summary.utilization =
            self.utilization(owner_id, &sessions, &decisions, decisions_trimmed, now_ms);
        // HS2-9FXASE: what the owner's work waited on outside the orchestrator.
        summary.external_wait = summarize_external_wait(&ExternalWaitInput {
            owner_checkout_id: owner_id,
            now_ms,
            gates: &gates,
            decisions: &decisions,
            decisions_trimmed,
            provisional: self.provisional_tally(owner_id)?,
        });
        Ok(summary)
    }

    /// The owner's retained scheduler decisions the summary's measures read (starts,
    /// keeps, wind-downs and unplaced work), and whether retention removed older ones.
    fn summary_decisions(
        &self,
        owner_id: &str,
    ) -> Result<(Vec<DecisionRecord>, bool), OrchestrationError> {
        let journal = DecisionJournal::new(&self.home, owner_id)?;
        let (mut decisions, mut after, mut trimmed) = (Vec::new(), 0, false);
        loop {
            // Only the decisions the measures read: starts, keeps, wind-downs and
            // unplaced work.
            let page = journal.read_matching(after, MAX_AUDIT_PAGE, |record| {
                matches!(
                    record.kind,
                    DecisionKind::Start
                        | DecisionKind::Keep
                        | DecisionKind::WindDown
                        | DecisionKind::Unplaced
                )
            })?;
            trimmed |= page.gap;
            let done = !page.more || page.cursor == after;
            after = page.cursor;
            decisions.extend(page.entries.into_iter().map(|entry| entry.record));
            if done {
                break;
            }
        }
        Ok((decisions, trimmed))
    }

    /// HS2-XK7TJS: throughput and utilization of the whole owner, from its session
    /// records, the scheduler's decision journal and the policy's caps.
    fn utilization(
        &self,
        owner_id: &str,
        sessions: &[OrchestratedSession],
        decisions: &[DecisionRecord],
        decisions_trimmed: bool,
        now_ms: u64,
    ) -> Utilization {
        let policy = self.policy(owner_id).ok().and_then(|view| view.policy);
        let tool_caps: BTreeMap<String, u32> = policy
            .iter()
            .flat_map(|policy| policy.tools.iter())
            .map(|tool| (tool.tool.clone(), tool.max_sessions))
            .collect();
        // A machine's cap is the strictest of the targets the sessions ran for there.
        let mut machine_caps: BTreeMap<String, u32> = BTreeMap::new();
        if let Some(policy) = &policy {
            for session in sessions {
                let Some(hosted) = &session.hosted else {
                    continue;
                };
                let cap = policy.machine_limits(&session.target_id).max_sessions;
                machine_caps
                    .entry(hosted.server_id.clone())
                    .and_modify(|known| *known = (*known).min(cap))
                    .or_insert(cap);
            }
        }
        summarize_utilization(&UtilizationInput {
            owner_checkout_id: owner_id,
            now_ms,
            sessions,
            decisions,
            decisions_trimmed,
            tool_caps: &tool_caps,
            machine_caps: &machine_caps,
        })
    }
}
