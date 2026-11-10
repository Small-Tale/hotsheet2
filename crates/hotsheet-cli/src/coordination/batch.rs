//! Batch integration (HS2-9G0M3V): several reviewed code handoffs integrate together, pass
//! one smoke gate, reach `main` in one push and are tested together by one CI run.
//!
//! The batch ledger lives beside the pool in `batches.json` and is read and written under
//! the pool's `state.lock`. It needs no pool schema bump:
//!
//! - a batch integrator session already persists pool schema eight, which every artifact
//!   without integration strategies refuses;
//! - only batch commands create the ledger, so serial pools never have one;
//! - the ledger has its own `version`, and this executable refuses any other.
//!
//! Every batch transition is idempotent by batch id, so replaying a command repairs a write
//! interrupted between the two files.
//!
//! ci-staged batches (HS2-GT57BQ, `batch_staged.rs`) add a `staging` record and three
//! states. A ledger holding one is written as version two, which a batch-only executable
//! refuses.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, ensure};
use serde::{Deserialize, Serialize};

use super::scope::scopes_overlap;
use super::strategy::CircuitBreaker;
use super::*;

/// The ledger file in the coordination directory.
pub const LEDGER_FILE: &str = "batches.json";
/// The newest ledger version this executable reads and writes. Version two adds staged
/// batches (HS2-GT57BQ); a ledger without one stays at [`LEDGER_VERSION_UNSTAGED`].
pub const LEDGER_VERSION: u32 = 2;
/// The version of a ledger that holds no staged batch (HS2-9G0M3V).
pub const LEDGER_VERSION_UNSTAGED: u32 = 1;
const MAX_ID_CHARS: usize = 128;
pub(super) const MAX_REASON_CHARS: usize = 500;

/// Every batch this pool has opened, plus the circuit breaker.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchLedger {
    pub version: u32,
    #[serde(default)]
    pub batches: BTreeMap<String, BatchManifest>,
    #[serde(default)]
    pub breaker: BreakerState,
}

impl Default for BatchLedger {
    fn default() -> Self {
        Self {
            version: LEDGER_VERSION_UNSTAGED,
            batches: BTreeMap::new(),
            breaker: BreakerState::default(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BatchState {
    /// Members are integrating on local `main`; nothing is pushed yet.
    Integrating,
    /// The smoke gate or integration failed before push; members returned to handoff.
    Aborted,
    /// Pushed once; the covering CI run decides the batch.
    Ci,
    Green,
    Red,
    /// ci-staged: `main` fast-forwarded to this batch's green staged SHA or a green
    /// descendant's (HS2-GT57BQ).
    Promoted,
    /// ci-staged: withdrawn from staging (red with follow-ups, stale or abandoned); every
    /// member returned to handoff for a new batch.
    Restaged,
    /// ci-staged: stacked above a batch that went red or was re-staged; every member
    /// returned to handoff.
    Invalidated,
}

impl BatchState {
    /// Whether this batch gave its members back, so they may join another batch.
    pub fn released(self) -> bool {
        matches!(self, Self::Aborted | Self::Restaged | Self::Invalidated)
    }
}

impl std::fmt::Display for BatchState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Integrating => "integrating",
            Self::Aborted => "aborted",
            Self::Ci => "ci",
            Self::Green => "green",
            Self::Red => "red",
            Self::Promoted => "promoted",
            Self::Restaged => "restaged",
            Self::Invalidated => "invalidated",
        })
    }
}

/// The batch manifest: its tickets, source and shipping SHAs, smoke checks and CI result.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchManifest {
    pub id: String,
    pub integrator: String,
    pub generation: String,
    pub state: BatchState,
    /// Unix seconds.
    pub opened_at: i64,
    /// Members in integration order (priority, then handoff time).
    pub tickets: Vec<BatchTicket>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub smoke_checks: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pushed_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub concluded_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub abort_reason: Option<String>,
    /// Red only: the attribution ran and `failures` lists every failure the batch caused.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub attributed: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<BatchFailure>,
    /// ci-staged only (HS2-GT57BQ): where the batch was staged and what became of it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging: Option<StagedBatch>,
}

/// A ci-staged batch's integration-branch facts (HS2-GT57BQ). `main_commit` stays empty
/// until promotion, then holds the exact main SHA that contains this batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StagedBatch {
    /// The integration branch the batch was pushed to.
    pub branch: String,
    /// The commit the batch was built on: remote `main`, or its parent's staged commit.
    pub base_commit: String,
    /// The staged batch this one stacks on while that one is still in CI or unpromoted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub parent: Option<String>,
    /// The exact integration-branch SHA CI tests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staged_commit: Option<String>,
    /// Green because a descendant's run passed on a tip that contains this batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implied_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub promoted_at: Option<i64>,
    /// The red or re-staged ancestor that invalidated this batch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidated_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restage_reason: Option<String>,
    /// Members held back from re-staging as suspects until review-returned.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub held: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchTicket {
    pub assignment: String,
    pub ticket: TicketRef,
    pub slug: String,
    pub source_commit: String,
    /// Declared scope plus the commit's changed paths, used for conflict exclusion.
    pub paths: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shipping_commits: Vec<String>,
}

/// One failure of a red batch run and the follow-up bug that records it.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchFailure {
    /// The HS2-HAHX20 attribution key (run and failure identity).
    pub key: String,
    /// Slug of the follow-up bug.
    pub follow_up: String,
    /// Qualified follow-up ticket when it is in the selected local store.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub follow_up_ticket: Option<TicketRef>,
    /// Batch ticket slugs the follow-up is linked to as suspects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suspects: Vec<String>,
}

/// Falls back to serial when `main` stays red too long (HS2-9G0M3V).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakerState {
    #[serde(default)]
    pub consecutive_red: u32,
    /// Unix seconds of the first red batch since `main` was last green.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub red_since: Option<i64>,
    /// The newest red batch's pushed main commit, which a recovery run must cover.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_red_main: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tripped: Option<BreakerTrip>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovered: Option<BreakerRecovery>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakerTrip {
    pub at: i64,
    pub reason: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BreakerRecovery {
    pub at: i64,
    pub run: String,
    pub main_commit: String,
    pub tripped_at: i64,
}

impl BreakerState {
    /// Trip when either threshold is reached. Returns the reason when this call tripped it.
    fn evaluate(&mut self, now: i64, limits: CircuitBreaker) -> Option<String> {
        if self.tripped.is_some() {
            return None;
        }
        let reason = if self.consecutive_red >= limits.max_red_batches {
            format!(
                "{} consecutive red batches reached the limit of {}",
                self.consecutive_red, limits.max_red_batches
            )
        } else if let Some(since) = self.red_since
            && now - since >= i64::from(limits.max_red_minutes) * 60
        {
            format!(
                "main has been red for {} minutes, reaching the limit of {}",
                (now - since) / 60,
                limits.max_red_minutes
            )
        } else {
            return None;
        };
        self.tripped = Some(BreakerTrip {
            at: now,
            reason: reason.clone(),
        });
        Some(reason)
    }

    /// Count a red batch. `main_commit` is the red commit pushed to main; a red staged
    /// batch (HS2-GT57BQ) left main untouched and passes `None`.
    pub(super) fn record_red(
        &mut self,
        now: i64,
        main_commit: Option<&str>,
        limits: CircuitBreaker,
    ) -> Option<String> {
        self.consecutive_red += 1;
        self.red_since.get_or_insert(now);
        if let Some(main_commit) = main_commit {
            self.last_red_main = Some(main_commit.to_owned());
        }
        self.evaluate(now, limits)
    }

    pub(super) fn record_green(&mut self, now: i64, run: &str, main_commit: &str) {
        self.consecutive_red = 0;
        self.red_since = None;
        self.last_red_main = None;
        if let Some(trip) = self.tripped.take() {
            self.recovered = Some(BreakerRecovery {
                at: now,
                run: run.to_owned(),
                main_commit: main_commit.to_owned(),
                tripped_at: trip.at,
            });
        }
    }
}

impl BatchManifest {
    pub fn member(&self, assignment: &str) -> bool {
        self.tickets.iter().any(|t| t.assignment == assignment)
    }

    pub(super) fn assignments(&self) -> Vec<&str> {
        self.tickets.iter().map(|t| t.assignment.as_str()).collect()
    }

    /// Whether this batch's tickets may complete: green, or red with a recorded follow-up
    /// for every failure the batch caused.
    pub fn completable(&self) -> Result<()> {
        if self.staging.is_some() {
            // HS2-GT57BQ: a staged ticket is on main only after promotion.
            return match self.state {
                BatchState::Promoted => Ok(()),
                BatchState::Red => bail!(
                    "staged batch {} is red and never reached main; record its follow-ups, re-stage it and integrate the members again",
                    self.id
                ),
                state => bail!(
                    "staged batch {} is {state}; its tickets complete only after main is promoted to it",
                    self.id
                ),
            };
        }
        match self.state {
            BatchState::Green => Ok(()),
            BatchState::Red => {
                ensure!(
                    self.attributed && !self.failures.is_empty(),
                    "batch {} is red; run CI failure attribution and record a follow-up for each failure with batch-follow-ups before completing its tickets",
                    self.id
                );
                Ok(())
            }
            state => bail!(
                "batch {} is {state}; its tickets complete only after its CI result",
                self.id
            ),
        }
    }
}

impl BatchLedger {
    /// The batch that holds the integration unit: integrating, or pushed and awaiting CI.
    /// A staged batch in CI holds it only for a strategy that does not stage
    /// (HS2-GT57BQ); under ci-staged the next batch may stack on it.
    pub fn open_batch(&self) -> Option<&BatchManifest> {
        self.open_batch_for(false)
    }

    pub(super) fn open_batch_for(&self, staged: bool) -> Option<&BatchManifest> {
        self.batches.values().find(|b| match b.staging {
            None => matches!(b.state, BatchState::Integrating | BatchState::Ci),
            Some(_) => {
                b.state == BatchState::Integrating
                    || (!staged
                        && matches!(
                            b.state,
                            BatchState::Ci | BatchState::Green | BatchState::Red
                        ))
            }
        })
    }

    /// The batch an assignment belongs to; an aborted, re-staged or invalidated batch
    /// returned its members.
    pub fn batch_of(&self, assignment: &str) -> Option<&BatchManifest> {
        self.batches
            .values()
            .find(|b| !b.state.released() && b.member(assignment))
    }
}

/// A ready handoff the integrator may batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BatchCandidate {
    pub assignment: String,
    pub ticket: TicketRef,
    pub slug: String,
    /// Higher integrates first.
    pub priority_rank: u8,
    /// RFC 3339 instant of the handoff transition; empty sorts first.
    pub handoff_at: String,
    pub paths: Vec<String>,
    /// Why this handoff cannot join a batch now, for example a live claim.
    pub blocker: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct DeferredCandidate {
    pub assignment: String,
    pub slug: String,
    pub reason: String,
}

/// A proposed batch: members in integration order, and what waits for a later batch.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BatchPlan {
    pub selected: Vec<BatchCandidate>,
    pub deferred: Vec<DeferredCandidate>,
}

/// Higher for a more urgent ticket priority.
pub fn priority_rank(priority: hotsheet_model::Priority) -> u8 {
    match priority {
        hotsheet_model::Priority::Highest => 4,
        hotsheet_model::Priority::High => 3,
        hotsheet_model::Priority::Default => 2,
        hotsheet_model::Priority::Low => 1,
        hotsheet_model::Priority::Lowest => 0,
    }
}

/// Choose non-conflicting handoffs in a stable order: priority, then handoff time, then
/// assignment id. A handoff whose paths overlap an earlier selection waits for the next
/// batch; so does one with a blocker, no changed paths, or beyond `max`.
pub fn select_batch(mut candidates: Vec<BatchCandidate>, max: Option<usize>) -> BatchPlan {
    candidates.sort_by(|a, b| {
        b.priority_rank
            .cmp(&a.priority_rank)
            .then_with(|| a.handoff_at.cmp(&b.handoff_at))
            .then_with(|| a.assignment.cmp(&b.assignment))
    });
    let mut plan = BatchPlan {
        selected: Vec::new(),
        deferred: Vec::new(),
    };
    for candidate in candidates {
        let defer = |plan: &mut BatchPlan, reason: String| {
            plan.deferred.push(DeferredCandidate {
                assignment: candidate.assignment.clone(),
                slug: candidate.slug.clone(),
                reason,
            });
        };
        if let Some(blocker) = &candidate.blocker {
            defer(&mut plan, blocker.clone());
            continue;
        }
        if candidate.paths.is_empty() {
            defer(&mut plan, "the handoff changes no paths".into());
            continue;
        }
        if let Some(reason) = plan
            .selected
            .iter()
            .find(|selected| paths_overlap(&selected.paths, &candidate.paths))
            .map(|other| {
                format!(
                    "its paths overlap {} ({}); integrate it in a later batch",
                    other.slug, other.assignment
                )
            })
        {
            defer(&mut plan, reason);
            continue;
        }
        if max.is_some_and(|max| plan.selected.len() >= max) {
            defer(&mut plan, "the batch is full".into());
            continue;
        }
        plan.selected.push(candidate);
    }
    plan
}

fn paths_overlap(left: &[String], right: &[String]) -> bool {
    left.iter()
        .any(|l| right.iter().any(|r| scopes_overlap(l, r)))
}

pub(super) fn valid_id(kind: &str, id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.chars().count() <= MAX_ID_CHARS
            && id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':')),
        "{kind} must be 1-{MAX_ID_CHARS} ASCII letters, digits, '-', '_', '.' or ':'"
    );
    Ok(())
}

/// What a recorded CI result did.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BatchResultReport {
    pub batch: BatchManifest,
    pub breaker: BreakerState,
    /// Set when this result tripped the circuit breaker: relay it to the user.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notify: Option<String>,
}

/// Batch facts the pool snapshot reports (HS2-9G0M3V). Present once a ledger exists.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BatchStatus {
    /// `serial` while the circuit breaker is tripped, else the integrator's strategy.
    pub effective_strategy: IntegrationStrategy,
    pub breaker: BreakerState,
    /// Batches whose tickets are not all done: open, pushed, or concluded but uncompleted.
    pub active: Vec<BatchManifest>,
    /// ci-staged summary once a staged batch exists (HS2-GT57BQ).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging: Option<super::batch_staged::StagingStatus>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cadence: Option<super::batch_staged::CadenceStatus>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
#[serde(rename_all = "snake_case")]
pub enum BatchConclusion {
    Success,
    Failure,
}

impl Coordinator {
    fn ledger_path(&self) -> PathBuf {
        self.directory.join(LEDGER_FILE)
    }

    fn read_ledger(path: &Path) -> Result<BatchLedger> {
        let ledger = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<BatchLedger>(&bytes)
                .map_err(|error| anyhow::anyhow!("invalid batch ledger: {error}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BatchLedger::default());
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            (LEDGER_VERSION_UNSTAGED..=LEDGER_VERSION).contains(&ledger.version),
            "batch ledger version {} is not supported by this executable",
            ledger.version
        );
        ensure!(
            ledger.version >= LEDGER_VERSION
                || ledger.batches.values().all(|b| b.staging.is_none()),
            "batch ledger version {} cannot hold staged batches",
            ledger.version
        );
        Ok(ledger)
    }

    /// Change the pool and the batch ledger under one lock. The ledger is written only
    /// when it changed, so serial pools never create it.
    pub(super) fn change_batches<T>(
        &self,
        update: impl FnOnce(&mut State, &mut BatchLedger) -> Result<T>,
    ) -> Result<T> {
        let path = self.ledger_path();
        self.change(|state| {
            let mut ledger = Self::read_ledger(&path)?;
            let before = ledger.clone();
            let result = update(state, &mut ledger)?;
            if ledger != before {
                let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
                serde_json::to_writer_pretty(&mut temporary, &ledger)?;
                temporary.write_all(b"\n")?;
                temporary.as_file().sync_all()?;
                temporary.persist(&path)?;
            }
            Ok(result)
        })
    }

    /// The batch ledger, or `None` when no batch was ever opened in this pool.
    pub fn batch_ledger(&self) -> Result<Option<BatchLedger>> {
        let path = self.ledger_path();
        self.change(|_| {
            if path.exists() {
                Ok(Some(Self::read_ledger(&path)?))
            } else {
                Ok(None)
            }
        })
    }

    pub(super) fn batch_status(state: &State, ledger: &BatchLedger) -> BatchStatus {
        let active_strategy = Self::active_strategy(state);
        let effective_strategy =
            if active_strategy.rules().batch_integration && ledger.breaker.tripped.is_some() {
                IntegrationStrategy::Serial
            } else {
                active_strategy
            };
        BatchStatus {
            effective_strategy,
            breaker: ledger.breaker.clone(),
            staging: super::batch_staged::staging_status(state, ledger),
            cadence: None,
            active: ledger
                .batches
                .values()
                .filter(|b| !b.state.released())
                .filter(|b| {
                    b.tickets.iter().any(|t| {
                        state
                            .assignments
                            .get(&t.assignment)
                            .is_none_or(|a| a.phase != Phase::Done)
                    })
                })
                .cloned()
                .collect(),
        }
    }

    pub(super) fn batch_integrator(
        state: &State,
        id: &str,
        generation: &str,
        now: i64,
    ) -> Result<()> {
        Self::integrator(state, id, generation, now)?;
        let strategy = Self::active_strategy(state);
        ensure!(
            strategy.rules().batch_integration,
            "batch integration requires an integrator session registered with the batch strategy; this session runs {strategy}"
        );
        Ok(())
    }

    pub(super) fn move_member(
        state: &mut State,
        integrator: &str,
        assignment: &str,
        from: Phase,
        to: Phase,
    ) -> Result<()> {
        let a = state
            .assignments
            .get_mut(assignment)
            .ok_or_else(|| anyhow::anyhow!("unknown assignment {assignment}"))?;
        if a.phase == to {
            return Ok(());
        }
        ensure!(
            a.phase == from,
            "batch member {assignment} is {:?}, not {:?}",
            a.phase,
            from
        );
        a.phase = to;
        a.record_transition(
            to.into(),
            time::OffsetDateTime::now_utc(),
            Role::Integrator,
            integrator,
        )
    }

    /// Open a batch: every member moves from handoff to integrating atomically. Refused
    /// while another integration unit is open or the circuit breaker is tripped. A
    /// tripped breaker found here by elapsed time is recorded before refusing.
    #[allow(clippy::too_many_arguments)] // HS2-9G0M3V: one fenced transition, every input explicit.
    pub fn batch_open(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        members: Vec<(String, Vec<String>)>,
        limits: CircuitBreaker,
    ) -> Result<BatchManifest> {
        self.batch_open_unit(integrator, generation, now, batch_id, members, limits, None)
    }

    /// [`Self::batch_open`] for every strategy: `staging` is required exactly when the
    /// session runs ci-staged (HS2-GT57BQ).
    #[allow(clippy::too_many_arguments)] // HS2-GT57BQ: one fenced transition, every input explicit.
    pub(super) fn batch_open_unit(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        members: Vec<(String, Vec<String>)>,
        limits: CircuitBreaker,
        staging: Option<super::batch_staged::StagingOpen>,
    ) -> Result<BatchManifest> {
        valid_id("batch id", batch_id)?;
        ensure!(!members.is_empty(), "a batch needs at least one handoff");
        let outcome = self.change_batches(|state, ledger| {
            Self::batch_integrator(state, integrator, generation, now)?;
            let staged = Self::active_strategy(state).rules().staged_integration;
            ensure!(
                staged == staging.is_some(),
                "{}",
                if staged {
                    "a ci-staged batch needs its staging branch and remote main base"
                } else {
                    "only a ci-staged integrator stages a batch"
                }
            );
            let requested: Vec<&str> = members.iter().map(|(a, _)| a.as_str()).collect();
            if let Some(existing) = ledger.batches.get(batch_id) {
                ensure!(
                    existing.state == BatchState::Integrating && existing.assignments() == requested,
                    "batch id {batch_id} already records a different or later batch"
                );
                for member in &requested {
                    Self::move_member(state, integrator, member, Phase::Handoff, Phase::Integrating)?;
                }
                return Ok(Ok(existing.clone()));
            }
            if let Some(reason) = ledger.breaker.evaluate(now, limits) {
                return Ok(Err(reason));
            }
            if let Some(trip) = &ledger.breaker.tripped {
                return Ok(Err(trip.reason.clone()));
            }
            if let Some(open) = ledger.open_batch_for(staged) {
                bail!(
                    "batch {} is {}; record its push or abort it before opening another",
                    open.id,
                    open.state
                );
            }
            // HS2-GT57BQ: stack on the newest staged tip, never over a red batch or a held
            // suspect.
            let staged_batch = staging
                .as_ref()
                .map(|open| Self::staged_open_rule(ledger, &requested, open))
                .transpose()?;
            let distinct: BTreeSet<&str> = requested.iter().copied().collect();
            ensure!(
                distinct.len() == requested.len(),
                "a batch lists each handoff once"
            );
            ensure!(
                !state.assignments.values().any(|a| matches!(
                    a.phase,
                    Phase::Integrating | Phase::Verifying | Phase::SuspendedIntegration
                )),
                "another ticket is integrating or verifying; finish it before opening a batch"
            );
            let mut tickets = Vec::with_capacity(members.len());
            for (assignment, paths) in &members {
                let a = state
                    .assignments
                    .get(assignment)
                    .ok_or_else(|| anyhow::anyhow!("unknown assignment {assignment}"))?;
                ensure!(
                    a.phase == Phase::Handoff && !a.returned_for_review(),
                    "batch member {assignment} must be a ready handoff"
                );
                let Some(HandoffEvidence::Code { commit }) = &a.handoff else {
                    bail!("batch member {assignment} is not a code handoff");
                };
                ensure!(
                    !paths.is_empty(),
                    "batch member {assignment} changes no paths"
                );
                if let Some(other) = tickets
                    .iter()
                    .find(|t: &&BatchTicket| paths_overlap(&t.paths, paths))
                {
                    bail!(
                        "batch members {} and {assignment} change overlapping paths; integrate one in a later batch",
                        other.assignment
                    );
                }
                tickets.push(BatchTicket {
                    assignment: assignment.clone(),
                    ticket: a.ticket.clone(),
                    slug: a.slug.clone(),
                    source_commit: commit.clone(),
                    paths: paths.clone(),
                    shipping_commits: Vec::new(),
                });
            }
            for member in &requested {
                Self::move_member(state, integrator, member, Phase::Handoff, Phase::Integrating)?;
            }
            let manifest = BatchManifest {
                id: batch_id.to_owned(),
                integrator: integrator.to_owned(),
                generation: generation.to_owned(),
                state: BatchState::Integrating,
                opened_at: now,
                tickets,
                main_commit: None,
                smoke_checks: Vec::new(),
                pushed_at: None,
                run: None,
                concluded_at: None,
                abort_reason: None,
                attributed: false,
                failures: Vec::new(),
                staging: staged_batch,
            };
            if manifest.staging.is_some() {
                ledger.version = LEDGER_VERSION;
            }
            ledger.batches.insert(batch_id.to_owned(), manifest.clone());
            Ok(Ok(manifest))
        })?;
        outcome.map_err(|reason| {
            anyhow::anyhow!(
                "the batch circuit breaker is tripped ({reason}); integrate serially until main is green, then run batch-recover"
            )
        })
    }

    /// The smoke gate or integration failed before push: nothing reached the remote, and
    /// every member returns to handoff for a later batch.
    pub fn batch_abort(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        reason: &str,
    ) -> Result<BatchManifest> {
        let reason = reason.trim();
        ensure!(
            !reason.is_empty() && reason.chars().count() <= MAX_REASON_CHARS,
            "an abort reason of 1-{MAX_REASON_CHARS} characters is required"
        );
        self.change_batches(|state, ledger| {
            Self::integrator(state, integrator, generation, now)?;
            let batch = ledger
                .batches
                .get_mut(batch_id)
                .ok_or_else(|| anyhow::anyhow!("unknown batch {batch_id}"))?;
            match batch.state {
                BatchState::Aborted => {
                    ensure!(
                        batch.abort_reason.as_deref() == Some(reason),
                        "batch {batch_id} was already aborted for another reason"
                    );
                }
                BatchState::Integrating => {
                    batch.state = BatchState::Aborted;
                    batch.abort_reason = Some(reason.to_owned());
                    batch.concluded_at = Some(now);
                }
                state => {
                    bail!("batch {batch_id} is {state}; only an unpushed batch can be aborted")
                }
            }
            let members: Vec<String> = batch.tickets.iter().map(|t| t.assignment.clone()).collect();
            let batch = batch.clone();
            for member in members {
                Self::move_member(
                    state,
                    integrator,
                    &member,
                    Phase::Integrating,
                    Phase::Handoff,
                )?;
            }
            Ok(batch)
        })
    }

    /// Record the one push: the exact pushed main commit, each member's shipping commits
    /// and the smoke checks that passed. Every member enters CI.
    #[allow(clippy::too_many_arguments)] // HS2-9G0M3V: one durable transition with explicit evidence.
    pub fn batch_pushed(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        main_commit: &str,
        shipping: BTreeMap<String, Vec<String>>,
        smoke_checks: Vec<String>,
    ) -> Result<BatchManifest> {
        ensure!(
            valid_full_sha(main_commit),
            "the batch main commit must be a full SHA"
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
            let batch = ledger
                .batches
                .get_mut(batch_id)
                .ok_or_else(|| anyhow::anyhow!("unknown batch {batch_id}"))?;
            ensure!(
                batch.staging.is_none(),
                "batch {batch_id} is staged; record its integration-branch push with batch-staged"
            );
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
                BatchState::Ci => {
                    ensure!(
                        batch.main_commit.as_deref() == Some(main_commit)
                            && batch.smoke_checks == smoke_checks
                            && batch
                                .tickets
                                .iter()
                                .all(|t| shipping.get(&t.assignment) == Some(&t.shipping_commits)),
                        "batch {batch_id} already recorded a different push"
                    );
                }
                BatchState::Integrating => {
                    for ticket in &mut batch.tickets {
                        ticket.shipping_commits = shipping[&ticket.assignment].clone();
                    }
                    batch.main_commit = Some(main_commit.to_owned());
                    batch.smoke_checks = smoke_checks;
                    batch.pushed_at = Some(now);
                    batch.state = BatchState::Ci;
                }
                state => bail!("batch {batch_id} is {state}; only an integrating batch is pushed"),
            }
            let batch = batch.clone();
            for member in batch.assignments() {
                Self::move_member(state, integrator, member, Phase::Integrating, Phase::Ci)?;
            }
            Ok(batch)
        })
    }

    /// Record the covering CI run. Green completes nothing by itself but lets the tickets
    /// complete and clears the breaker; red counts toward the breaker. A rerun that turns a
    /// red batch green is accepted; a green batch never turns red.
    #[allow(clippy::too_many_arguments)] // HS2-9G0M3V: one durable transition with explicit evidence.
    pub fn batch_result(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        run: &str,
        conclusion: BatchConclusion,
        limits: CircuitBreaker,
    ) -> Result<BatchResultReport> {
        valid_id("CI run id", run)?;
        self.change_batches(|state, ledger| {
            Self::integrator(state, integrator, generation, now)?;
            if ledger
                .batches
                .get(batch_id)
                .is_some_and(|b| b.staging.is_some())
            {
                return Self::staged_result(
                    state, ledger, integrator, now, batch_id, run, conclusion, limits,
                );
            }
            let batch = ledger
                .batches
                .get_mut(batch_id)
                .ok_or_else(|| anyhow::anyhow!("unknown batch {batch_id}"))?;
            let main_commit = batch.main_commit.clone().unwrap_or_default();
            let target = match conclusion {
                BatchConclusion::Success => BatchState::Green,
                BatchConclusion::Failure => BatchState::Red,
            };
            let mut notify = None;
            if batch.state == target && batch.run.as_deref() == Some(run) {
                // Replay of the same result.
            } else {
                match (batch.state, target) {
                    (BatchState::Ci, _) | (BatchState::Red, BatchState::Green) => {}
                    (state, _) => bail!(
                        "batch {batch_id} is {state}; record a CI result only for a pushed batch"
                    ),
                }
                batch.state = target;
                batch.run = Some(run.to_owned());
                batch.concluded_at = Some(now);
                match target {
                    BatchState::Green => ledger.breaker.record_green(now, run, &main_commit),
                    _ => {
                        notify = ledger
                            .breaker
                            .record_red(now, Some(&main_commit), limits)
                            .map(|reason| {
                                format!(
                                    "Batch integration fell back to serial: {reason}. Integrate serially until main is green, then run batch-recover."
                                )
                            });
                    }
                }
            }
            Ok(BatchResultReport {
                batch: ledger.batches[batch_id].clone(),
                breaker: ledger.breaker.clone(),
                notify,
            })
        })
    }

    /// Record the follow-up bug for every failure a red batch caused. The set replaces
    /// nothing: a known key must keep its follow-up.
    pub fn batch_follow_ups(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        batch_id: &str,
        failures: Vec<BatchFailure>,
    ) -> Result<BatchManifest> {
        ensure!(
            !failures.is_empty(),
            "record at least one failure follow-up"
        );
        for failure in &failures {
            valid_id("failure key", &failure.key)?;
            ensure!(
                !failure.follow_up.trim().is_empty(),
                "failure {} needs a follow-up ticket",
                failure.key
            );
        }
        self.change_batches(|state, ledger| {
            Self::integrator(state, integrator, generation, now)?;
            let batch = ledger
                .batches
                .get_mut(batch_id)
                .ok_or_else(|| anyhow::anyhow!("unknown batch {batch_id}"))?;
            ensure!(
                batch.state == BatchState::Red,
                "batch {batch_id} is {}; follow-ups record a red batch's failures",
                batch.state
            );
            for failure in failures {
                if let Some(known) = batch.failures.iter_mut().find(|f| f.key == failure.key) {
                    ensure!(
                        known.follow_up == failure.follow_up,
                        "failure {} already has follow-up {}",
                        failure.key,
                        known.follow_up
                    );
                    for suspect in failure.suspects {
                        if !known.suspects.contains(&suspect) {
                            known.suspects.push(suspect);
                        }
                    }
                    if known.follow_up_ticket.is_none() {
                        known.follow_up_ticket = failure.follow_up_ticket;
                    }
                } else {
                    batch.failures.push(failure);
                }
            }
            batch.attributed = true;
            Ok(batch.clone())
        })
    }

    /// Clear a tripped breaker after a green covering run on `main`. The CLI verifies the
    /// run's commit contains the newest red batch's push.
    pub fn batch_recover(
        &self,
        integrator: &str,
        generation: &str,
        now: i64,
        run: &str,
        main_commit: &str,
    ) -> Result<BreakerState> {
        valid_id("CI run id", run)?;
        ensure!(
            valid_full_sha(main_commit),
            "recovery requires the green run's full main SHA"
        );
        self.change_batches(|state, ledger| {
            Self::integrator(state, integrator, generation, now)?;
            if ledger.breaker.tripped.is_none() {
                ensure!(
                    ledger
                        .breaker
                        .recovered
                        .as_ref()
                        .is_some_and(|r| r.run == run && r.main_commit == main_commit),
                    "the batch circuit breaker is not tripped"
                );
                return Ok(ledger.breaker.clone());
            }
            ledger.breaker.record_green(now, run, main_commit);
            Ok(ledger.breaker.clone())
        })
    }

    /// Refuse an individual mailbox move that would bypass the batch it belongs to.
    pub(super) fn batch_mark_rule(
        ledger: &BatchLedger,
        id: &str,
        from: Phase,
        to: Phase,
    ) -> Result<()> {
        let Some(batch) = ledger.batch_of(id) else {
            return Ok(());
        };
        if from == to {
            return Ok(());
        }
        match to {
            Phase::Done => batch.completable(),
            Phase::Integrating | Phase::Ci | Phase::Handoff | Phase::SuspendedIntegration => bail!(
                "assignment {id} belongs to batch {}; batch members move with batch-open, batch-abort and batch-pushed",
                batch.id
            ),
            _ => Ok(()),
        }
    }
}
