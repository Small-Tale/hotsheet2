//! HS2-J89JC7: the orchestration timeline of an owning checkout.
//!
//! A read. It gathers this machine's records (the plan and its readiness, runs,
//! sessions, the scheduler's decision log, pending permission requests, bindings and the
//! policy), labels tickets from stores on this machine, and hands everything to the
//! pure fold `hotsheet_ticketing::orchestration::timeline`. It contacts no target,
//! passes no gate and writes nothing. With a version and `wait_ms` it is a long poll:
//! it waits on the run event journal, the decision log and the scheduler's wake
//! marker, never on a timer, and answers as soon as what it shows changed.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use hotsheet_model::{NoteKind, Ticket};
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    BindingRegistry, DecisionJournal, DecisionKind, DecisionRecord, DecisionWait, DeclarationState,
    Gate, GateFilter, MAX_AUDIT_PAGE, PermissionState, PermissionWait, PlacementHint,
    ReadinessLevel, RunEventJournal, RunGateVerdicts, SessionRegistry, TargetDestination,
    TargetLocation, TicketKey, TicketLabel, Timeline, TimelineInput, TimelinePolicy, TimelineStep,
    TimelineTarget, summarize, ticket_key, timeline,
};

use super::scheduler::WAKE_DIRECTORY;
use super::tickets::OrchestrationTicket;
use super::*;

/// The longest a timeline read waits for a change.
pub const MAX_TIMELINE_WAIT_MS: u64 = 20_000;

/// HS2-XHQ1EH's outcome for an action that waits on a permission request through the
/// permission bridge; its `outcome_detail` names the request.
pub const PERMISSION_REQUIRED: &str = "permission_required";

fn code<T: serde::Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(Into::into))
}

fn label(ticket: &Ticket) -> TicketLabel {
    TicketLabel {
        slug: Some(ticket.slug.clone()),
        title: Some(ticket.title.clone()),
        status: code(&ticket.status),
        priority: code(&ticket.priority),
        category: Some(ticket.category.clone()).filter(|category| !category.is_empty()),
    }
}

/// The goal ticket's unanswered feedback request, when it has one.
pub(super) fn pending_decision(ticket: &Ticket) -> Option<DecisionWait> {
    if !ticket.feedback_needed() {
        return None;
    }
    let note = ticket
        .notes
        .iter()
        .filter(|note| {
            note.kind == NoteKind::FeedbackNeeded
                && !ticket
                    .notes
                    .iter()
                    .any(|reply| reply.response_to == Some(note.id))
        })
        .max_by_key(|note| note.id);
    Some(DecisionWait {
        note_id: note.map(|note| note.id.to_string()),
    })
}

/// What the scheduler last said about each step, from its decision log.
fn hints(records: &[DecisionRecord]) -> BTreeMap<String, PlacementHint> {
    let mut hints: BTreeMap<String, PlacementHint> = BTreeMap::new();
    for record in records {
        let decision = match record.kind {
            DecisionKind::Start => "start",
            DecisionKind::Keep => "keep",
            DecisionKind::Unplaced => "unplaced",
            DecisionKind::Tick
            | DecisionKind::WindDown
            | DecisionKind::Assignment
            | DecisionKind::Reconcile
            | DecisionKind::RoleUnplaced => continue,
        };
        let Some(step_id) = &record.step_id else {
            continue;
        };
        hints.insert(
            step_id.clone(),
            PlacementHint {
                at_ms: record.at_ms,
                decision: decision.into(),
                reason: record.reason.clone(),
                tool: record.tool.clone(),
                model: record.model.clone(),
                session_id: record.session_id.clone(),
                outcome: record.outcome.clone(),
                detail: record.detail.clone(),
                retry_at_ms: record.retry_at_ms,
            },
        );
    }
    hints
}

impl OrchestrationRuntime {
    /// Every retained scheduler decision of the owner, oldest first.
    fn retained_decisions(
        &self,
        owner_id: &str,
    ) -> Result<Vec<DecisionRecord>, OrchestrationError> {
        let journal = DecisionJournal::new(&self.home, owner_id)?;
        let (mut records, mut after) = (Vec::new(), 0);
        loop {
            let page = journal.read(after, MAX_AUDIT_PAGE)?;
            let done = !page.more || page.cursor == after;
            after = page.cursor;
            records.extend(page.entries.into_iter().map(|entry| entry.record));
            if done {
                return Ok(records);
            }
        }
    }

    /// HS2-J89JC7 seam: the permission request each step waits on. HS2-XHQ1EH logs a
    /// start that waits for a person as a decision whose outcome is
    /// `permission_required` with the orchestrator request id as its detail.
    fn permission_waits(
        &self,
        owner_id: &str,
        records: &[DecisionRecord],
        now_ms: u64,
    ) -> Result<BTreeMap<String, PermissionWait>, OrchestrationError> {
        let mut waits: BTreeMap<String, PermissionWait> = BTreeMap::new();
        // The latest decision about a step decides whether it still waits.
        let mut latest: BTreeMap<&str, &DecisionRecord> = BTreeMap::new();
        for record in records
            .iter()
            .filter(|record| record.kind == DecisionKind::Start)
        {
            if let Some(step) = &record.step_id {
                latest.insert(step, record);
            }
        }
        // The ledger names the action asked (a session start, or the step's run as
        // `work_start`) and whether a person answered since the pass logged it.
        let ledger = self.permission_ledger().list(owner_id, now_ms)?;
        for (step, record) in latest {
            if record.outcome.as_deref() != Some(PERMISSION_REQUIRED) {
                continue;
            }
            let Some(request_id) = &record.outcome_detail else {
                continue;
            };
            let request = ledger
                .iter()
                .find(|request| &request.request_id == request_id);
            if request.is_some_and(|request| request.state != PermissionState::Pending) {
                continue;
            }
            waits.insert(
                step.to_owned(),
                PermissionWait {
                    kind: "permission".into(),
                    request_id: request_id.clone(),
                    action: request.map_or_else(
                        || "session_start".into(),
                        |request| request.key.action.code().into(),
                    ),
                    operation_id: request
                        .map(|request| request.operation_id.clone())
                        .or_else(|| record.operation_id.clone()),
                    requested_at_ms: Some(
                        request.map_or(record.at_ms, |request| request.requested_at_ms),
                    ),
                },
            );
        }
        Ok(waits)
    }

    /// Declared targets with where this machine's binding puts them.
    pub(super) fn timeline_targets(
        &self,
        owner_id: &str,
    ) -> Result<(bool, Vec<TimelineTarget>), OrchestrationError> {
        let declarations = self.declarations(owner_id)?;
        let remotes = self.remotes.list().unwrap_or_default();
        let bindings = BindingRegistry::new(&self.home);
        let targets = declarations
            .targets
            .into_iter()
            .filter(|target| target.state == DeclarationState::Active)
            .map(|target| {
                let destination = Self::target_ref(owner_id, &target.target_id)
                    .ok()
                    .and_then(|reference| bindings.get(&reference).ok())
                    .and_then(|binding| binding.destination);
                let (location, machine_label) = match &destination {
                    Some(TargetDestination::Local { .. }) => (TargetLocation::Local, None),
                    Some(TargetDestination::Remote {
                        registration_id, ..
                    }) => (
                        TargetLocation::Remote,
                        remotes
                            .iter()
                            .find(|remote| remote.id == *registration_id)
                            .map(|remote| remote.label.clone()),
                    ),
                    None => (TargetLocation::Unbound, None),
                };
                TimelineTarget {
                    checkout_id: destination
                        .as_ref()
                        .map(|destination| destination.checkout_id().to_owned()),
                    target_id: target.target_id,
                    label: target.label,
                    location,
                    machine_label,
                }
            })
            .collect();
        Ok((declarations.enabled, targets))
    }

    /// A target ticket's label from its store when the target is a checkout on this
    /// machine; `None` otherwise.
    pub(super) fn local_ticket_label(
        &self,
        target: &TimelineTarget,
        key: &TicketKey,
    ) -> Option<TicketLabel> {
        if target.location != TargetLocation::Local {
            return None;
        }
        let checkout = self
            .checkouts
            .describe(target.checkout_id.as_deref()?)
            .ok()?;
        let store = checkout.verified_git_source(&key.1).ok()?;
        ops::resolve(&store, &key.2)
            .ok()
            .flatten()
            .map(|ticket| label(&ticket))
    }

    /// One timeline from the records as they are now, and the ticket stores whose
    /// change could clear a pending decision.
    pub(super) fn timeline_now(
        &self,
        owner_id: &str,
        tx: &mut Transaction<'_>,
    ) -> Result<(Timeline, Vec<PathBuf>), OrchestrationError> {
        let now_ms = tx.clock.now_ms();
        let (declared, targets) = self.timeline_targets(owner_id)?;
        let views = if declared {
            self.plan_evaluate(owner_id, now_ms)?
        } else {
            Vec::new()
        };
        let steps: Vec<TimelineStep> = views
            .into_iter()
            .map(|view| TimelineStep {
                provisional_work: view.readiness.level == ReadinessLevel::InProgress
                    && view.provisional.is_some(),
                // HS2-H5H9QP: the card words the ledger record as the Workers view does.
                provisional: view.provisional.as_ref().map(timeline_provisional),
                readiness: view.readiness,
            })
            .collect();
        // Labels: the plan's last read of each ticket, and the store itself for a
        // target on this machine.
        let mut tickets: BTreeMap<TicketKey, TicketLabel> = BTreeMap::new();
        for record in self.plan().tickets(owner_id)? {
            if let Some(seen) = record.facts.observed.filter(|seen| seen.exists) {
                tickets.insert(
                    ticket_key(&record.target),
                    TicketLabel {
                        slug: seen.slug,
                        title: None,
                        status: Some(seen.status),
                        priority: seen.priority,
                        category: None,
                    },
                );
            }
        }
        for step in &steps {
            let key = ticket_key(&step.readiness.step.target);
            if let Some(target) = targets.iter().find(|target| target.target_id == key.0)
                && let Some(local) = self.local_ticket_label(target, &key)
            {
                tickets.insert(key, local);
            }
        }
        let mut goals: BTreeMap<(String, String), TicketLabel> = BTreeMap::new();
        let mut decisions: BTreeMap<(String, String), DecisionWait> = BTreeMap::new();
        let mut watched: Vec<PathBuf> = Vec::new();
        for step in &steps {
            let goal = &step.readiness.step.ticket;
            let key = (goal.connection_id.clone(), goal.native_id.clone());
            if goals.contains_key(&key) {
                continue;
            }
            let Ok((store, id, _)) = self.orchestration_store(
                owner_id,
                &OrchestrationTicket {
                    connection_id: goal.connection_id.clone(),
                    native_id: goal.native_id.clone(),
                },
            ) else {
                goals.insert(key, TicketLabel::default());
                continue;
            };
            let Ok(ticket) = store.read_ticket(&id) else {
                goals.insert(key, TicketLabel::default());
                continue;
            };
            if let Some(decision) = pending_decision(&ticket) {
                decisions.insert(key.clone(), decision);
                if !watched.iter().any(|path| path == store.root()) {
                    watched.push(store.root().to_path_buf());
                }
            }
            goals.insert(key, label(&ticket));
        }
        let records = self.retained_decisions(owner_id)?;
        let policy = self.policy(owner_id)?;
        let body = policy.policy.as_ref();
        let orchestrating = self.orchestrating(owner_id)?;
        let runs = self.run_list(owner_id, false)?;
        // HS2-6GE7WM: the runs the scheduler dispatched for each step, whichever site of
        // the step's project it placed them on.
        let step_runs: BTreeMap<String, BTreeSet<String>> = steps
            .iter()
            .map(|step| {
                let step_id = &step.readiness.step.step_id;
                let placed = runs
                    .iter()
                    .filter(|run| super::session_executor::scheduled_run_of(run, step_id))
                    .map(|run| run.run_id.clone())
                    .collect();
                (step_id.clone(), placed)
            })
            .collect();
        // HS2-CQ0PNT: runs, labels and prerequisites join a step through any site.
        let sites = self.project_sites(owner_id)?;
        // HS2-89578K: each run's CI verdict, so a Done card whose CI failed says so;
        // HS2-GPABYY: its deployment and review verdicts too. An unreadable gate record
        // shows no verdict rather than failing the timeline.
        let mut by_run: BTreeMap<String, Vec<Gate>> = BTreeMap::new();
        for gate in self
            .gate_registry()
            .list(owner_id, &GateFilter::default())
            .unwrap_or_default()
        {
            by_run.entry(gate.run_id.clone()).or_default().push(gate);
        }
        let run_gates: BTreeMap<String, RunGateVerdicts> = by_run
            .into_iter()
            .map(|(run_id, gates)| {
                let summary = summarize(gates);
                let verdicts = RunGateVerdicts {
                    ci: (summary.ci_reason.as_deref() != Some("no_gate")).then_some(summary.ci),
                    deployment: summary.deployment,
                    review: summary.review,
                };
                (run_id, verdicts)
            })
            .collect();
        // HS2-EX594V: an unreadable ledger shows no outage rather than failing the view.
        let home_outages = hotsheet_ticketing::orchestration::HomeOutageStore::new(&self.home)
            .outages(owner_id)
            .unwrap_or_default();
        let input = TimelineInput {
            home_outages: &home_outages,
            run_gates: &run_gates,
            sites: &sites,
            owner_checkout_id: owner_id,
            now_ms,
            declared,
            targets: &targets,
            steps: &steps,
            runs: &runs,
            step_runs: &step_runs,
            sessions: &SessionRegistry::new(&self.home).list(owner_id, false)?,
            hints: &hints(&records),
            permissions: &self.permission_waits(owner_id, &records, now_ms)?,
            decisions: &decisions,
            goals: &goals,
            tickets: &tickets,
            policy: &TimelinePolicy {
                present: policy.enabled && body.is_some(),
                revision: policy.revision.clone(),
                // HS2-J3ZF27: a Software project's kept automatic policy places
                // nothing, so it does not read as running.
                // HS2-R2N33M: unset placement is Auto, on once the plan has a step.
                automatic: orchestrating
                    && body.is_some_and(|body| body.placement.automatic_with(!steps.is_empty())),
                auto: body.is_some_and(|body| body.placement.automatic.is_none()),
                software: !orchestrating,
                // HS2-38R8J2: a plan never started reads Not Started, not Paused.
                // HS2-ZF5H5M: started from any machine sharing the policy file, or
                // (before that file recorded it) from this one.
                ever_started: self.placement_started_at(owner_id)?.is_some(),
            },
        };
        Ok((timeline(&input), watched))
    }

    /// HS2-J89JC7: the owner's plan as dependency waves with where each piece of work
    /// runs and what it waits on. With `after` equal to the current version and a
    /// `wait_ms`, waits for what the timeline shows to change, woken by the records it
    /// reads changing, and answers the same version at the deadline.
    pub fn orchestration_timeline(
        &self,
        owner_id: &str,
        after: Option<&str>,
        wait_ms: u64,
        tx: &mut Transaction<'_>,
    ) -> Result<Timeline, OrchestrationError> {
        let owner = self.owner(owner_id)?;
        if wait_ms == 0 || after.is_none() {
            return Ok(self.timeline_now(&owner.id, tx)?.0);
        }
        // HS2-WD475E: the deadline counts from the request, whatever the reads and the
        // watcher's start take, so a reader's `wait_ms` is the longest it waits.
        let deadline = std::time::Instant::now()
            + std::time::Duration::from_millis(wait_ms.min(MAX_TIMELINE_WAIT_MS));
        let runs = RunEventJournal::new(&self.home, &owner.id)?;
        let decisions = DecisionJournal::new(&self.home, &owner.id)?;
        let wake = self.home.join(WAKE_DIRECTORY);
        // HS2-ANQXQ4: a person's answer to a permission request ends the wait, whether
        // or not the owner's policy runs a scheduler to wake.
        let permissions = self.permission_signal_dir();
        let (first, stores) = self.timeline_now(&owner.id, tx)?;
        if after != Some(first.version.as_str()) {
            return Ok(first);
        }
        // HS2-EX594V: a home outage begun or ended changes what the view shows.
        let outages =
            hotsheet_ticketing::orchestration::HomeOutageStore::new(&self.home).directory();
        let mut dirs = vec![
            runs.directory(),
            decisions.directory(),
            wake.as_path(),
            permissions.as_path(),
            outages.as_path(),
        ];
        dirs.extend(stores.iter().map(PathBuf::as_path));
        // HS2-ANQXQ4: the stores to watch are known only after the first read, so the
        // watcher starts after it. HS2-WD475E: it starts in the background; until it
        // runs, each wait ends at the one-second safety re-check and the timeline is
        // read again, so a change made meanwhile (an answer given while the first read
        // ran) is found within that interval instead of after a start that took seconds
        // on a loaded machine.
        let Ok(waiter) = hotsheet_jobs::ChangeWaiter::starting(&dirs) else {
            return Ok(first);
        };
        loop {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            if remaining.is_zero() {
                return self.timeline_now(&owner.id, tx).map(|(view, _)| view);
            }
            // HS2-1C77XH: read again whether the waiter saw a change or reached its
            // safety re-check. A change no watched directory reported (an event the
            // platform watcher dropped, or a record written without one) is then found
            // within the re-check interval instead of at the deadline. Each read is
            // local files only.
            let _ = waiter.wait(remaining);
            let (view, _) = self.timeline_now(&owner.id, tx)?;
            if view.version != first.version {
                return Ok(view);
            }
        }
    }
}

/// HS2-H5H9QP: the ledger record of provisional work as a timeline card shows it.
fn timeline_provisional(
    work: &super::provisional::ProvisionalWork,
) -> hotsheet_ticketing::orchestration::TimelineProvisional {
    let name = |value: serde_json::Result<serde_json::Value>| {
        value
            .ok()
            .and_then(|value| value.as_str().map(str::to_owned))
            .unwrap_or_default()
    };
    hotsheet_ticketing::orchestration::TimelineProvisional {
        state: name(serde_json::to_value(work.state)),
        gates: work
            .gates
            .iter()
            .map(
                |gate| hotsheet_ticketing::orchestration::TimelineProvisionalGate {
                    target_id: gate.target_id.clone(),
                    connection_id: gate.connection_id.clone(),
                    native_id: gate.native_id.clone(),
                    slug: None,
                    result: gate.result.name().to_owned(),
                },
            )
            .collect(),
    }
}
