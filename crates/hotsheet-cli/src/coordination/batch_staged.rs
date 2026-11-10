//! ci-staged integration (HS2-GT57BQ): batch mode (HS2-9G0M3V) with `main` kept green.
//!
//! A staged batch is opened, integrated and smoke-gated like a batch, but it is pushed to an
//! integrator-owned integration branch instead of `main`. CI tests the exact staged SHA.
//! `main` fast-forwards to that SHA only after the run is green.
//!
//! - **Stacking.** While one staged batch is in CI, the next may stack on its tip. A green
//!   run on a tip implies its ancestors passed. A red run, or a re-staged batch, invalidates
//!   everything stacked above it, and those members return to handoff.
//! - **Red.** Main stays untouched. The follow-up rule of batch mode applies: every failure
//!   gets a follow-up before the batch is re-staged. Re-staging returns the members to
//!   handoff and may hold suspects until the integrator review-returns them.
//! - **Main moved.** When `main` moved and no longer fast-forwards to a green batch, the
//!   push wrapper refuses promotion and the batch is re-staged on the new `main`; `main` is
//!   never merged or rewritten.
//!
//! The state lives in the batch ledger; a ledger holding a staged batch is version two.

use std::collections::BTreeSet;

use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};

use super::batch::{
    BatchConclusion, BatchLedger, BatchManifest, BatchResultReport, BatchState, MAX_REASON_CHARS,
    StagedBatch,
};
use super::strategy::{CircuitBreaker, validate_staging_branch};
use super::*;

/// What the CLI resolves before a ci-staged batch opens: the project's staging branch and
/// the remote-tracking `main` SHA a batch without a pending parent is built on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StagingOpen {
    pub branch: String,
    pub main_commit: String,
}

/// The headless staging summary in `batch-status` and the pool snapshot (HS2-GT57BQ).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StagingStatus {
    /// The staging branch of the newest staged batch.
    pub branch: String,
    /// The pending staged batch a new batch stacks on, and its staged commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip_batch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tip: Option<String>,
    /// Staged batches whose CI run has not concluded.
    pub in_ci: Vec<String>,
    /// Green staged batches main has not been promoted to yet.
    pub awaiting_promotion: Vec<String>,
    /// Red staged batches that still need follow-ups and re-staging.
    pub red: Vec<String>,
    /// Slugs of the tickets in those batches, waiting on CI, promotion or re-staging.
    pub waiting_tickets: Vec<String>,
    /// Held suspects (assignment ids) still waiting for a review return.
    pub held: Vec<String>,
}

/// HS2-0BJ1A3: a read-only cadence projection from durable staged transitions.
/// A target is an escalation signal, never authorization to promote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CadenceStatus {
    pub goal_minutes: u32,
    pub last_green_main_at: Option<i64>,
    pub next_target_at: Option<i64>,
    pub oldest_staging_at: Option<i64>,
    pub staging_age_seconds: Option<i64>,
    pub ci_failures_since_promotion: usize,
    /// Elapsed wall time since the first unshipped red staged run, never a sum of parallel work.
    pub correction_wall_seconds: Option<i64>,
    /// Tickets promoted to main during the rolling goal interval ending at `now`.
    pub tickets_promoted_in_goal_window: usize,
    pub ready_batch: Option<String>,
    pub action: String,
    pub missed_goal_reason: Option<String>,
}

pub fn cadence_status(ledger: &BatchLedger, goal_minutes: u32, now: i64) -> CadenceStatus {
    let last_green_main_at = ledger
        .batches
        .values()
        .filter_map(|batch| {
            batch
                .staging
                .as_ref()
                .and_then(|staging| staging.promoted_at)
        })
        .max();
    let next_target_at =
        last_green_main_at.map(|at| at.saturating_add(i64::from(goal_minutes) * 60));
    let pending: Vec<_> = ledger
        .batches
        .values()
        .filter(|batch| {
            batch.staging.is_some()
                && matches!(
                    batch.state,
                    BatchState::Ci | BatchState::Green | BatchState::Red
                )
        })
        .collect();
    let oldest_staging_at = pending
        .iter()
        .map(|batch| batch.pushed_at.unwrap_or(batch.opened_at))
        .min();
    let ready = pending
        .iter()
        .filter(|batch| batch.state == BatchState::Green)
        .filter(|batch| {
            batch
                .staging
                .as_ref()
                .and_then(|staging| staging.staged_commit.as_ref())
                .is_some()
        })
        .max_by_key(|batch| batch.pushed_at.unwrap_or(batch.opened_at));
    let red = pending.iter().any(|batch| batch.state == BatchState::Red);
    let in_ci = pending.iter().any(|batch| batch.state == BatchState::Ci);
    let action = if ready.is_some() {
        "promote_green_tip"
    } else if red {
        "attribute_and_restage"
    } else if in_ci {
        "await_exact_sha_ci"
    } else {
        "no_ready_work"
    };
    let missed_goal_reason = next_target_at.filter(|target| now >= *target).map(|_| {
        match action {
            "promote_green_tip" => "green_staged_tip_awaiting_promotion",
            "attribute_and_restage" => "red_staging_requires_correction",
            "await_exact_sha_ci" => "staging_ci_pending_or_unavailable",
            _ => "no_ready_staged_work",
        }
        .to_owned()
    });
    let since = last_green_main_at.unwrap_or(i64::MIN);
    let goal_window_start = now.saturating_sub(i64::from(goal_minutes) * 60);
    let first_red_at = ledger
        .batches
        .values()
        .filter(|batch| {
            batch.staging.is_some() && (batch.state == BatchState::Red || batch.attributed)
        })
        .filter_map(|batch| batch.concluded_at)
        .filter(|at| *at >= since)
        .min();
    CadenceStatus {
        goal_minutes,
        last_green_main_at,
        next_target_at,
        oldest_staging_at,
        staging_age_seconds: oldest_staging_at.map(|at| now.saturating_sub(at).max(0)),
        ci_failures_since_promotion: ledger
            .batches
            .values()
            .filter(|batch| {
                batch.staging.is_some()
                    && (batch.state == BatchState::Red || batch.attributed)
                    && batch.concluded_at.is_some_and(|at| at >= since)
            })
            .count(),
        correction_wall_seconds: first_red_at.map(|at| now.saturating_sub(at).max(0)),
        tickets_promoted_in_goal_window: ledger
            .batches
            .values()
            .filter(|batch| {
                batch
                    .staging
                    .as_ref()
                    .and_then(|staging| staging.promoted_at)
                    .is_some_and(|at| at >= goal_window_start && at <= now)
            })
            .map(|batch| batch.tickets.len())
            .sum(),
        ready_batch: ready.map(|batch| batch.id.clone()),
        action: action.to_owned(),
        missed_goal_reason,
    }
}

fn pending(batch: &BatchManifest) -> bool {
    batch
        .staging
        .as_ref()
        .is_some_and(|s| s.staged_commit.is_some())
        && matches!(batch.state, BatchState::Ci | BatchState::Green)
}

/// The pending staged batch nothing else pending stacks on.
fn tip(ledger: &BatchLedger) -> Option<&BatchManifest> {
    let parents: BTreeSet<&str> = ledger
        .batches
        .values()
        .filter(|b| pending(b))
        .filter_map(|b| b.staging.as_ref()?.parent.as_deref())
        .collect();
    ledger
        .batches
        .values()
        .filter(|b| pending(b) && !parents.contains(b.id.as_str()))
        .max_by_key(|b| (b.pushed_at, b.opened_at))
}

/// Unreleased, unpromoted batches stacked (transitively) on `id`.
fn descendants(ledger: &BatchLedger, id: &str) -> Vec<String> {
    let mut found: Vec<String> = Vec::new();
    loop {
        let before = found.len();
        for b in ledger.batches.values() {
            let Some(parent) = b.staging.as_ref().and_then(|s| s.parent.as_deref()) else {
                continue;
            };
            if matches!(
                b.state,
                BatchState::Integrating | BatchState::Ci | BatchState::Green
            ) && (parent == id || found.iter().any(|f| f == parent))
                && !found.contains(&b.id)
            {
                found.push(b.id.clone());
            }
        }
        if found.len() == before {
            return found;
        }
    }
}

/// The parent chain of `id`, nearest first.
fn ancestors(ledger: &BatchLedger, id: &str) -> Vec<String> {
    let mut chain = Vec::new();
    let mut next = ledger
        .batches
        .get(id)
        .and_then(|b| b.staging.as_ref()?.parent.clone());
    while let Some(parent) = next {
        if chain.contains(&parent) {
            break;
        }
        next = ledger
            .batches
            .get(&parent)
            .and_then(|b| b.staging.as_ref()?.parent.clone());
        chain.push(parent);
    }
    chain
}

pub(super) fn staging_status(state: &State, ledger: &BatchLedger) -> Option<StagingStatus> {
    let newest = ledger
        .batches
        .values()
        .filter(|b| b.staging.is_some())
        .max_by_key(|b| b.opened_at)?;
    let ids = |wanted: BatchState| -> Vec<String> {
        ledger
            .batches
            .values()
            .filter(|b| b.staging.is_some() && b.state == wanted)
            .map(|b| b.id.clone())
            .collect()
    };
    let tip = tip(ledger);
    let mut waiting_tickets = Vec::new();
    for b in ledger.batches.values().filter(|b| {
        b.staging.is_some()
            && matches!(
                b.state,
                BatchState::Ci | BatchState::Green | BatchState::Red
            )
    }) {
        waiting_tickets.extend(b.tickets.iter().map(|t| t.slug.clone()));
    }
    let held = ledger
        .batches
        .values()
        .filter_map(|b| b.staging.as_ref())
        .flat_map(|s| s.held.iter())
        .filter(|id| {
            state
                .assignments
                .get(id.as_str())
                .is_some_and(|a| a.phase == Phase::Handoff && !a.returned_for_review())
        })
        .cloned()
        .collect();
    Some(StagingStatus {
        branch: newest
            .staging
            .as_ref()
            .map(|s| s.branch.clone())
            .unwrap_or_default(),
        tip_batch: tip.map(|b| b.id.clone()),
        tip: tip.and_then(|b| b.staging.as_ref()?.staged_commit.clone()),
        in_ci: ids(BatchState::Ci),
        awaiting_promotion: ids(BatchState::Green),
        red: ids(BatchState::Red),
        waiting_tickets,
        held,
    })
}

fn staged_mut<'a>(ledger: &'a mut BatchLedger, batch_id: &str) -> Result<&'a mut BatchManifest> {
    let batch = ledger
        .batches
        .get_mut(batch_id)
        .ok_or_else(|| anyhow::anyhow!("unknown batch {batch_id}"))?;
    ensure!(
        batch.staging.is_some(),
        "batch {batch_id} is not a staged batch; ci-staged commands apply only to batches a ci-staged integrator opened"
    );
    Ok(batch)
}

fn staging_mut(batch: &mut BatchManifest) -> &mut StagedBatch {
    batch
        .staging
        .as_mut()
        .expect("staged_mut checked the staging record")
}

impl Coordinator {
    /// The ci-staged summary of a ledger, or `None` before any staged batch (HS2-GT57BQ).
    pub fn staging_report(state: &State, ledger: &BatchLedger) -> Option<StagingStatus> {
        staging_status(state, ledger)
    }

    /// Where a new staged batch stacks: on the newest pending staged tip, else on remote
    /// `main`. Refused while a red batch is not re-staged, for a held suspect, or when the
    /// staging branch changed under a pending batch.
    pub(super) fn staged_open_rule(
        ledger: &BatchLedger,
        requested: &[&str],
        open: &StagingOpen,
    ) -> Result<StagedBatch> {
        validate_staging_branch(&open.branch)?;
        ensure!(
            valid_full_sha(&open.main_commit),
            "a staged batch needs the full remote main SHA"
        );
        if let Some(red) = ledger
            .batches
            .values()
            .find(|b| b.staging.is_some() && b.state == BatchState::Red)
        {
            bail!(
                "staged batch {} is red; record its follow-ups and re-stage it with batch-restage before staging another",
                red.id
            );
        }
        for b in ledger.batches.values() {
            let Some(staging) = &b.staging else { continue };
            if let Some(held) = requested
                .iter()
                .find(|r| staging.held.iter().any(|h| h == *r))
            {
                bail!(
                    "assignment {held} is held as a suspect of re-staged batch {}; review-return it to its worker instead of staging it again",
                    b.id
                );
            }
        }
        let (parent, base_commit) = match tip(ledger) {
            Some(tip) => {
                let staging = tip.staging.as_ref().expect("pending batches are staged");
                ensure!(
                    staging.branch == open.branch,
                    "staged batch {} uses staging branch {}; promote or re-stage it before switching to {}",
                    tip.id,
                    staging.branch,
                    open.branch
                );
                (
                    Some(tip.id.clone()),
                    staging
                        .staged_commit
                        .clone()
                        .expect("pending batches are pushed"),
                )
            }
            None => (None, open.main_commit.clone()),
        };
        Ok(StagedBatch {
            branch: open.branch.clone(),
            base_commit,
            parent,
            staged_commit: None,
            implied_by: None,
            promoted_at: None,
            invalidated_by: None,
            restage_reason: None,
            held: Vec::new(),
        })
    }

    /// Open a ci-staged batch (HS2-GT57BQ); see [`Self::batch_open`].
    #[allow(clippy::too_many_arguments)] // HS2-GT57BQ: one fenced transition, every input explicit.
    pub fn batch_open_staged(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        members: Vec<(String, Vec<String>)>,
        limits: CircuitBreaker,
        staging: StagingOpen,
    ) -> Result<BatchManifest> {
        self.batch_open_unit(
            integrator,
            generation,
            now,
            batch_id,
            members,
            limits,
            Some(staging),
        )
    }

    /// Record the batch's one push to the integration branch: the exact staged SHA, each
    /// member's shipping commits and the smoke checks. Main is not touched; every member
    /// enters CI. The CLI verifies the remote-tracking staging ref, the base ancestry and
    /// the shipping containment first.
    #[allow(clippy::too_many_arguments)] // HS2-GT57BQ: one durable transition with explicit evidence.
    pub fn batch_staged(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        staged_commit: &str,
        shipping: std::collections::BTreeMap<String, Vec<String>>,
        smoke_checks: Vec<String>,
    ) -> Result<BatchManifest> {
        ensure!(
            valid_full_sha(staged_commit),
            "the staged commit must be a full SHA"
        );
        ensure!(
            !smoke_checks.is_empty() && smoke_checks.iter().all(|c| !c.trim().is_empty()),
            "record the smoke checks that passed"
        );
        ensure!(
            shipping
                .values()
                .all(|shas| !shas.is_empty() && shas.iter().all(|s| valid_full_sha(s))),
            "each member needs full shipping commit SHAs"
        );
        self.change_batches(|state, ledger| {
            Self::batch_integrator(state, integrator, generation, now)?;
            let batch = staged_mut(ledger, batch_id)?;
            let members: BTreeSet<&str> = batch
                .tickets
                .iter()
                .map(|t| t.assignment.as_str())
                .collect();
            ensure!(
                shipping.keys().map(String::as_str).collect::<BTreeSet<_>>() == members,
                "record shipping commits for exactly the batch members"
            );
            match batch.state {
                BatchState::Integrating => {
                    for ticket in &mut batch.tickets {
                        ticket.shipping_commits = shipping[&ticket.assignment].clone();
                    }
                    batch.smoke_checks = smoke_checks;
                    batch.pushed_at = Some(now);
                    batch.state = BatchState::Ci;
                    staging_mut(batch).staged_commit = Some(staged_commit.to_owned());
                }
                _ if batch.pushed_at.is_some() => {
                    ensure!(
                        batch
                            .staging
                            .as_ref()
                            .and_then(|s| s.staged_commit.as_deref())
                            == Some(staged_commit)
                            && batch.smoke_checks == smoke_checks
                            && batch
                                .tickets
                                .iter()
                                .all(|t| shipping.get(&t.assignment) == Some(&t.shipping_commits)),
                        "batch {batch_id} already recorded a different staged push"
                    );
                    return Ok(batch.clone());
                }
                state => bail!("batch {batch_id} is {state}; only an integrating batch is staged"),
            }
            let batch = batch.clone();
            for member in batch.assignments() {
                Self::move_member(state, integrator, member, Phase::Integrating, Phase::Ci)?;
            }
            Ok(batch)
        })
    }

    /// A staged batch's CI result. Green also marks its unconcluded ancestors green, since
    /// the tested tip contains them; red invalidates everything stacked above it and counts
    /// toward the circuit breaker without marking main red.
    #[allow(clippy::too_many_arguments)] // HS2-GT57BQ: one durable transition with explicit evidence.
    pub(super) fn staged_result(
        state: &mut State,
        ledger: &mut BatchLedger,
        integrator: &str,
        now: i64,
        batch_id: &str,
        run: &str,
        conclusion: BatchConclusion,
        limits: CircuitBreaker,
    ) -> Result<BatchResultReport> {
        let batch = staged_mut(ledger, batch_id)?;
        let staged_commit = batch
            .staging
            .as_ref()
            .and_then(|s| s.staged_commit.clone())
            .unwrap_or_default();
        let (current, current_run) = (batch.state, batch.run.clone());
        let target = match conclusion {
            BatchConclusion::Success => BatchState::Green,
            BatchConclusion::Failure => BatchState::Red,
        };
        let mut notify = None;
        match (current, target) {
            (current, wanted) if current == wanted && current_run.as_deref() == Some(run) => {}
            // Already green through a descendant's run, or promoted: nothing changes.
            (BatchState::Green | BatchState::Promoted, BatchState::Green) => {}
            (BatchState::Ci | BatchState::Red, BatchState::Green) => {
                let batch = ledger.batches.get_mut(batch_id).expect("checked above");
                batch.state = BatchState::Green;
                batch.run = Some(run.to_owned());
                batch.concluded_at = Some(now);
                staging_mut(batch).implied_by = None;
                for ancestor in ancestors(ledger, batch_id) {
                    let parent = ledger
                        .batches
                        .get_mut(&ancestor)
                        .expect("ancestors are recorded batches");
                    if parent.state == BatchState::Ci {
                        parent.state = BatchState::Green;
                        parent.run = Some(run.to_owned());
                        parent.concluded_at = Some(now);
                        staging_mut(parent).implied_by = Some(batch_id.to_owned());
                    }
                }
                ledger.breaker.record_green(now, run, &staged_commit);
            }
            (BatchState::Ci, BatchState::Red) => {
                let batch = ledger.batches.get_mut(batch_id).expect("checked above");
                batch.state = BatchState::Red;
                batch.run = Some(run.to_owned());
                batch.concluded_at = Some(now);
                Self::invalidate_above(state, ledger, integrator, now, batch_id)?;
                notify = ledger
                    .breaker
                    .record_red(now, None, limits)
                    .map(|reason| {
                        format!(
                            "ci-staged integration fell back to serial: {reason}. Main is unchanged; integrate serially until staging is green, then run batch-recover."
                        )
                    });
            }
            (current, _) => bail!(
                "staged batch {batch_id} is {current}; record a CI result only for a staged batch in CI (a green batch never turns red)"
            ),
        }
        Ok(BatchResultReport {
            batch: ledger.batches[batch_id].clone(),
            breaker: ledger.breaker.clone(),
            notify,
        })
    }

    /// Invalidate every batch stacked above `by`; their members return to handoff.
    fn invalidate_above(
        state: &mut State,
        ledger: &mut BatchLedger,
        integrator: &str,
        now: i64,
        by: &str,
    ) -> Result<()> {
        for id in descendants(ledger, by) {
            let batch = ledger
                .batches
                .get_mut(&id)
                .expect("descendants are recorded");
            let from = if batch.state == BatchState::Integrating {
                Phase::Integrating
            } else {
                Phase::Ci
            };
            batch.state = BatchState::Invalidated;
            batch.concluded_at = Some(now);
            staging_mut(batch).invalidated_by = Some(by.to_owned());
            let members: Vec<String> = batch.tickets.iter().map(|t| t.assignment.clone()).collect();
            for member in members {
                Self::move_member(state, integrator, &member, from, Phase::Handoff)?;
            }
        }
        Ok(())
    }

    /// Record that `main` now equals this green batch's exact staged SHA. Its green
    /// ancestors are promoted with it. The push wrapper fast-forwards main first and the
    /// CLI verifies the remote-tracking main; this refuses any other SHA.
    pub fn batch_promote(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        main_commit: &str,
    ) -> Result<BatchManifest> {
        ensure!(
            valid_full_sha(main_commit),
            "promotion records the full main SHA"
        );
        self.change_batches(|state, ledger| {
            Self::batch_integrator(state, integrator, generation, now)?;
            let batch = staged_mut(ledger, batch_id)?;
            match batch.state {
                BatchState::Promoted => {
                    ensure!(
                        batch.main_commit.as_deref() == Some(main_commit),
                        "batch {batch_id} was already promoted with main {}",
                        batch.main_commit.as_deref().unwrap_or_default()
                    );
                    return Ok(batch.clone());
                }
                BatchState::Green => {
                    let staged = batch
                        .staging
                        .as_ref()
                        .and_then(|s| s.staged_commit.as_deref())
                        .unwrap_or_default();
                    ensure!(
                        staged == main_commit,
                        "promotion fast-forwards main to exactly the green staged commit {staged}, not {main_commit}"
                    );
                }
                current => bail!(
                    "staged batch {batch_id} is {current}; only a green staged batch is promoted"
                ),
            }
            let chain = ancestors(ledger, batch_id);
            for ancestor in &chain {
                let parent = &ledger.batches[ancestor];
                ensure!(
                    matches!(parent.state, BatchState::Green | BatchState::Promoted),
                    "batch {batch_id} stacks on {ancestor}, which is {}; it cannot reach main first",
                    parent.state
                );
            }
            for id in chain.iter().chain(std::iter::once(&batch_id.to_owned())) {
                let b = ledger.batches.get_mut(id).expect("checked above");
                if b.state == BatchState::Green {
                    b.state = BatchState::Promoted;
                    b.main_commit = Some(main_commit.to_owned());
                    staging_mut(b).promoted_at = Some(now);
                }
            }
            Ok(ledger.batches[batch_id].clone())
        })
    }

    /// Withdraw a staged batch from staging; main is untouched. A red batch first needs a
    /// follow-up for every failure (the batch-mode follow-up rule) and may hold suspects
    /// until they are review-returned. A batch still in CI or green but stale (main moved)
    /// is re-staged with a reason and holds nothing. Every member returns to handoff and
    /// every batch stacked above is invalidated.
    pub fn batch_restage(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        reason: &str,
        held: Vec<String>,
    ) -> Result<BatchManifest> {
        let reason = reason.trim();
        ensure!(
            !reason.is_empty() && reason.chars().count() <= MAX_REASON_CHARS,
            "a re-stage reason of 1-{MAX_REASON_CHARS} characters is required"
        );
        let distinct: BTreeSet<&String> = held.iter().collect();
        ensure!(distinct.len() == held.len(), "hold each suspect once");
        self.change_batches(|state, ledger| {
            Self::batch_integrator(state, integrator, generation, now)?;
            let batch = staged_mut(ledger, batch_id)?;
            for suspect in &held {
                ensure!(
                    batch.member(suspect),
                    "held suspect {suspect} is not a member of batch {batch_id}"
                );
            }
            match batch.state {
                BatchState::Restaged => {
                    let staging = batch.staging.as_ref().expect("staged");
                    ensure!(
                        staging.restage_reason.as_deref() == Some(reason) && staging.held == held,
                        "batch {batch_id} was already re-staged differently"
                    );
                    return Ok(batch.clone());
                }
                BatchState::Red => ensure!(
                    batch.attributed && !batch.failures.is_empty(),
                    "staged batch {batch_id} is red; run CI failure attribution and record a follow-up for each failure with batch-follow-ups before re-staging it"
                ),
                BatchState::Ci | BatchState::Green => ensure!(
                    held.is_empty(),
                    "only a red batch holds suspects; batch {batch_id} is {}",
                    batch.state
                ),
                current => bail!(
                    "staged batch {batch_id} is {current}; only a staged batch in CI, green and unpromoted, or red is re-staged"
                ),
            }
            Self::invalidate_above(state, ledger, integrator, now, batch_id)?;
            let batch = ledger.batches.get_mut(batch_id).expect("checked above");
            batch.state = BatchState::Restaged;
            batch.concluded_at.get_or_insert(now);
            let staging = staging_mut(batch);
            staging.restage_reason = Some(reason.to_owned());
            staging.held = held;
            let batch = batch.clone();
            for member in batch.assignments() {
                Self::move_member(state, integrator, member, Phase::Ci, Phase::Handoff)?;
            }
            Ok(batch)
        })
    }
}
