//! HS2-AH8CRS: the scheduler acts on what reconciliation found.
//!
//! Reconciliation (§26.14) stores a disposition on each unfinished run but starts no
//! work. An applied scheduler pass now repeats a `retry_dispatch` run's dispatch under
//! its retained operation id (through the gate, so a rule, an Allow Once or a person
//! decides), tells a person on the goal ticket about a `needs_attention` run once per
//! stored outcome, and leaves paused and held runs alone. A dry run acts on nothing.

use hotsheet_model::NoteKind;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::ops;
use hotsheet_ticketing::orchestration::{
    DecisionAsked, DecisionJournal, DecisionKind, DenialReason, DispositionAction, MAX_AUDIT_PAGE,
    Run, RunState, disposition_action,
};

use super::tickets::timestamp;
use super::*;

/// A redispatch is tried at most this many times for one stored outcome; a new
/// reconciliation outcome allows it again.
pub const MAX_REDISPATCH_ATTEMPTS: usize = 3;
/// The decision-ledger kind of a `needs_attention` note.
const ATTENTION_KIND: &str = "run_attention";

/// A run state as its stable code.
fn state_code(state: RunState) -> String {
    serde_json::to_value(state)
        .ok()
        .and_then(|value| value.as_str().map(str::to_owned))
        .unwrap_or_default()
}

/// What the pass did about one run, for the decision log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct DispositionOutcome {
    pub run_id: String,
    pub target_id: String,
    pub operation_id: String,
    /// `redispatch` or `needs_attention`.
    pub action: &'static str,
    /// `dispatched`, `queued`, `asked`, or the refusal's code.
    pub outcome: String,
    /// The permission request a refused dispatch waits on, or the note id asked.
    pub outcome_detail: Option<String>,
    /// The stored outcome acted on, as its time (one try budget per outcome).
    pub reconciled_at_ms: u64,
}

impl OrchestrationRuntime {
    /// HS2-AH8CRS: act on every unfinished run's stored reconciliation. Called from an
    /// applied pass only. Each act is returned for the decision log; a run left alone
    /// is not, so an unchanged run does not fill the log.
    pub(super) fn scheduler_dispositions(
        &self,
        owner_id: &str,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Vec<DispositionOutcome> {
        let Ok(runs) = self.run_list(owner_id, false) else {
            return Vec::new();
        };
        let mut done = Vec::new();
        let mut tried: Option<Vec<(String, String)>> = None;
        for run in runs {
            let Some(action) = disposition_action(&run) else {
                continue;
            };
            let Some(found) = run.reconciliation.as_ref() else {
                continue;
            };
            let at_ms = found.at_ms;
            match action {
                DispositionAction::Redispatch => {
                    let earlier = tried
                        .get_or_insert_with(|| self.redispatch_tries(owner_id))
                        .iter()
                        .filter(|(run_id, at)| *run_id == run.run_id && *at == at_ms.to_string())
                        .count();
                    if earlier >= MAX_REDISPATCH_ATTEMPTS {
                        continue;
                    }
                    done.push(self.redispatch(owner_id, &run, at_ms, actor, tx));
                }
                DispositionAction::AskPerson => {
                    if let Some(asked) = self.attention_note(owner_id, &run, at_ms, actor, tx) {
                        done.push(asked);
                    }
                }
                DispositionAction::Leave(_) => {}
            }
        }
        done
    }

    /// Earlier redispatch attempts in the decision log, as (run, outcome time).
    fn redispatch_tries(&self, owner_id: &str) -> Vec<(String, String)> {
        let Ok(journal) = DecisionJournal::new(&self.home, owner_id) else {
            return Vec::new();
        };
        let (mut tries, mut after) = (Vec::new(), 0);
        loop {
            let Ok(page) = journal.read(after, MAX_AUDIT_PAGE) else {
                return tries;
            };
            tries.extend(page.entries.into_iter().filter_map(|entry| {
                let record = entry.record;
                (record.kind == DecisionKind::Reconcile
                    && record.reason.as_deref() == Some("redispatch"))
                .then_some((record.run_id?, record.detail?))
            }));
            let done = !page.more || page.cursor == after;
            after = page.cursor;
            if done {
                return tries;
            }
        }
    }

    /// Repeat the run's own dispatch under its retained operation id. The gate decides
    /// as for any dispatch; a refusal is reported, never retried within this pass.
    fn redispatch(
        &self,
        owner_id: &str,
        run: &Run,
        at_ms: u64,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> DispositionOutcome {
        let mut outcome = DispositionOutcome {
            run_id: run.run_id.clone(),
            target_id: run.target.target_id.clone(),
            operation_id: run.operation_id.clone(),
            action: DispositionAction::Redispatch.code(),
            outcome: String::new(),
            outcome_detail: None,
            reconciled_at_ms: at_ms,
        };
        let request = DispatchRun {
            ticket: OrchestrationTicket {
                connection_id: run.ticket.connection_id.clone(),
                native_id: run.ticket.native_id.clone(),
            },
            target: TargetTicket {
                target_id: run.target.target_id.clone(),
                connection_id: run.target.connection_id.clone(),
                native_id: run.target.native_id.clone(),
            },
            operation_id: run.operation_id.clone(),
            mode: Some(run.mode),
            worker_id: run.worker_id.clone(),
            worker_label: None,
            lease_minutes: None,
            override_readiness: false,
            override_requirements: false,
            requirements_checked: false,
        };
        match self.run_dispatch(owner_id, request, actor, tx) {
            Ok(view) => outcome.outcome = state_code(view.run.state),
            Err(OrchestrationError::PermissionRequired { request_id }) => {
                outcome.outcome = DenialReason::PermissionRequired.code().into();
                outcome.outcome_detail = Some(request_id);
            }
            Err(error) => outcome.outcome = error.code().into(),
        }
        outcome
    }

    /// Tell a person on the goal ticket that a run needs them, once per stored
    /// outcome: a `FEEDBACK NEEDED` note by identifiers and codes, recorded in the
    /// decision ledger so a later pass does not repeat it. Nothing is decided for them.
    fn attention_note(
        &self,
        owner_id: &str,
        run: &Run,
        at_ms: u64,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Option<DispositionOutcome> {
        let target = Self::target_ref(owner_id, &run.target.target_id).ok()?;
        let operation_id = format!("attn-{}-{at_ms}", run.run_id);
        let ledger = self.permission_ledger();
        if ledger
            .decision(&target, ATTENTION_KIND, &operation_id)
            .ok()?
            .is_some()
        {
            return None;
        }
        let ticket = OrchestrationTicket {
            connection_id: run.ticket.connection_id.clone(),
            native_id: run.ticket.native_id.clone(),
        };
        let (store, id, connection_id) = self.orchestration_store(owner_id, &ticket).ok()?;
        let findings: Vec<String> = run
            .reconciliation
            .iter()
            .flat_map(|found| found.findings.iter())
            .map(|finding| {
                let verification = serde_json::to_value(finding.verification)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default();
                let source = serde_json::to_value(finding.source)
                    .ok()
                    .and_then(|value| value.as_str().map(str::to_owned))
                    .unwrap_or_default();
                match &finding.reason {
                    Some(reason) => format!("`{source}`: `{verification}` (`{reason}`)"),
                    None => format!("`{source}`: `{verification}`"),
                }
            })
            .collect();
        let note_id = hotsheet_ticketing::mint_ulid(tx.clock, tx.rng);
        // HS2-RQDH0Q: a site's release whose handoff the home never saw.
        let text = if let Some(code) = super::assignment::site_handoff_waiting(run) {
            format!(
                "Run `{}` on site `{}` released its claim, but the project's home site \
                 still has not seen its handoff (`{code}`) {} minutes later, so the \
                 orchestrator stopped asking the home on a timer (`needs_attention`).\n\n\
                 Check that the worker pushed its branch and added its \
                 `## Integration handoff` note before releasing, or hand off, correct or \
                 cancel the run with `orchestration run status {}` or the run's controls \
                 in the Orchestration view. A note or a claim change on the ticket asks \
                 the home again.",
                run.run_id,
                run.target.target_id,
                super::assignment::SITE_HANDOFF_GIVE_UP_MS / 60_000,
                run.run_id,
            )
        } else {
            format!(
                "Run `{}` in target `{}` needs a person: reconciliation found \
             that its record and its target disagree in a way the orchestrator does not \
             settle (`needs_attention`, state `{}`).\n\nFindings: {}.\n\nUse `orchestration \
             run status {}`, then hand it off, correct, pause or cancel it, or the run's \
             controls in the Orchestration view. The orchestrator does not act on it.",
                run.run_id,
                run.target.target_id,
                state_code(run.state),
                if findings.is_empty() {
                    "none recorded".to_owned()
                } else {
                    findings.join(", ")
                },
                run.run_id,
            )
        };
        ops::add_note_with_metadata(
            &store,
            &id,
            note_id,
            timestamp(tx),
            NoteKind::FeedbackNeeded,
            ops::NoteMetadataInput {
                kind_explicit: true,
                summary: Some(format!("Run {} needs attention", run.run_id)),
                actor: actor.map(MutationActor::note_actor),
                ..ops::NoteMetadataInput::default()
            },
            text,
        )
        .ok()?;
        ledger
            .record_decision(DecisionAsked {
                decision_id: hotsheet_ticketing::mint_ulid(tx.clock, tx.rng).to_string(),
                target,
                kind: ATTENTION_KIND.into(),
                operation_id: operation_id.clone(),
                connection_id,
                native_id: id.to_string(),
                note_id: note_id.to_string(),
                actor_id: actor
                    .and_then(|actor| actor.id.clone())
                    .unwrap_or_else(|| "orchestrator".into()),
                asked_at_ms: tx.clock.now_ms(),
                answered: None,
            })
            .ok()?;
        Some(DispositionOutcome {
            run_id: run.run_id.clone(),
            target_id: run.target.target_id.clone(),
            operation_id,
            action: DispositionAction::AskPerson.code(),
            outcome: "asked".into(),
            outcome_detail: Some(note_id.to_string()),
            reconciled_at_ms: at_ms,
        })
    }
}
