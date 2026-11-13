//! The I/O half of provisional assignments (HS2-2SJPCH): Git reachability, the
//! HS2-3R9V0R CI watch as the event source, ticket notes for at-risk and reopened work,
//! and the worktree branch record. The CLI commands and tests call these functions; the
//! rule is `hotsheet_ticketing::provisional` and the transitions are `speculation.rs`.

use std::time::Duration;

use anyhow::{bail, ensure};
use hotsheet_jobs::{CiTarget, GateState, JobStore, gate_of};
use hotsheet_model::{NoteKind, Status, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::provisional::{
    GateObservation, GateProbe, GateResult, ReadinessLevel, SpeculationPolicy, TicketReadiness,
    ticket_readiness,
};
use serde::Serialize;

use super::speculation::{
    OwedNote, PendingGate, Rebase, Recovery, SpeculationChange, SpeculationRow, SpeculationStatus,
    SpeculativeRecord,
};
use super::*;

/// Git config keys under `branch.<name>.` that record a worktree branch's speculation.
pub const BRANCH_BASE_KEY: &str = "hotsheet-speculative-base";
pub const BRANCH_GATE_KEY: &str = "hotsheet-pending-gate";
pub const BRANCH_STATE_KEY: &str = "hotsheet-speculation-state";

/// The facts a provisional reservation needs beyond the gate itself.
pub trait BaseProbe: GateProbe {
    /// Whether `ancestor` is contained in `descendant`. `None` when Git cannot tell.
    fn contains(&self, ancestor: &str, descendant: &str) -> Option<bool>;
}

fn git(project: &Path, args: &[&str]) -> Option<std::process::Output> {
    hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(project)
        .args(args)
        .output()
        .ok()
}

fn is_ancestor(project: &Path, ancestor: &str, descendant: &str) -> Option<bool> {
    let output = git(
        project,
        &["merge-base", "--is-ancestor", ancestor, descendant],
    )?;
    match output.status.code() {
        Some(0) => Some(true),
        Some(1) => Some(false),
        _ => None,
    }
}

/// Reachability from the shipping branch and the gate of a commit as the machine's CI
/// watch Jobs report it. It never contacts the CI provider and never waits on a timer.
pub struct LiveGateProbe {
    project: PathBuf,
    /// The resolved ref a gate commit must be reachable from; `None` when the shipping
    /// checkout has neither the policy's branch nor an upstream.
    reference: Option<String>,
    jobs: JobStore,
    checkout_id: String,
}

impl LiveGateProbe {
    /// The Job store revision; read it before the gates so a later change is not missed.
    pub fn revision(&self) -> u64 {
        self.jobs.revision(&self.checkout_id).unwrap_or(0)
    }

    /// Whether this checkout has a Job store to wait on. Without one there is no event
    /// source, so a caller must not call a waiting sync again in a loop.
    pub fn watchable(&self) -> bool {
        self.jobs
            .checkout_dir(&self.checkout_id)
            .is_ok_and(|dir| dir.is_dir())
    }

    /// Block until the Job store of this checkout changes or `timeout` passes. Wakes on
    /// file events. A store that cannot be watched reports no change.
    pub fn wait(&self, since: u64, timeout: Duration) -> u64 {
        if self.watchable() {
            self.jobs
                .wait_for_change(&self.checkout_id, since, timeout)
                .unwrap_or(since)
        } else {
            since
        }
    }
}

impl GateProbe for LiveGateProbe {
    fn reachable(&self, commit: &str) -> Option<bool> {
        is_ancestor(&self.project, commit, self.reference.as_deref()?)
    }

    fn gate(&self, commit: &str) -> GateObservation {
        let Ok(listing) = self.jobs.list(&self.checkout_id) else {
            return GateObservation::default();
        };
        // The newest watch Job of this commit, whichever workflow or branch it names.
        let Some(job) = listing.jobs.iter().rfind(|job| {
            job.external
                .as_ref()
                .and_then(CiTarget::parse)
                .is_some_and(|(target, _)| target.sha.eq_ignore_ascii_case(commit))
        }) else {
            return GateObservation::default();
        };
        let named = |run: &hotsheet_jobs::CiRun| format!("{}.{}", run.id, run.attempt);
        match gate_of(job) {
            GateState::Pending => GateObservation::of(GateResult::Pending),
            GateState::Green { run } => GateObservation::with_run(GateResult::Green, named(&run)),
            GateState::Red { run } => GateObservation::with_run(GateResult::Red, named(&run)),
            GateState::Superseded => GateObservation::of(GateResult::Superseded),
            GateState::Cancelled => GateObservation::of(GateResult::Cancelled),
            GateState::Unknown => GateObservation::of(GateResult::Unknown),
        }
    }
}

impl BaseProbe for LiveGateProbe {
    fn contains(&self, ancestor: &str, descendant: &str) -> Option<bool> {
        is_ancestor(&self.project, ancestor, descendant)
    }
}

/// A provisional reservation that passed every check: what the ledger records.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProvisionalReservation {
    pub depth: u32,
    pub gates: Vec<PendingGate>,
}

/// The answer of `coordination speculation-explain`.
#[derive(Clone, Debug, Serialize)]
pub struct SpeculationExplanation {
    pub ticket: String,
    pub slug: String,
    pub policy: SpeculationPolicy,
    pub opted_out: bool,
    pub readiness: TicketReadiness,
    /// `confirmed`, `provisional on <slug> CI <run>`, or each blocking reason.
    pub explain: String,
    /// The recorded provisional assignment of this ticket, when one exists.
    pub record: Option<SpeculationRow>,
}

/// What one result or sync pass did.
#[derive(Clone, Debug, Default, Serialize)]
pub struct SpeculationReport {
    pub changes: Vec<SpeculationChange>,
    /// Slugs that received an at-risk or repair note in this pass.
    pub notes_written: Vec<String>,
    /// At-risk notes that could not be written to this ticket source yet.
    pub notes_owed: Vec<OwedNote>,
    pub status: SpeculationStatus,
}

/// The branch record `coordination speculation-branch` wrote.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct BranchRecord {
    pub branch: String,
    pub base: String,
    pub state: String,
    /// `<slug> <commit> <result> <run or ->`, one per gate that is not green.
    pub pending_gates: Vec<String>,
}

fn risk_key(owed: &OwedNote) -> String {
    format!(
        "Speculation risk key: {}/{}/{}",
        owed.assignment, owed.gate_commit, owed.run
    )
}

fn repair_key(owed: &OwedNote) -> String {
    format!("Speculation repair key: {}/{}", owed.gate_commit, owed.run)
}

fn has_line(ticket: &Ticket, line: &str) -> bool {
    ticket
        .notes
        .iter()
        .any(|note| note.text.lines().any(|found| found.trim() == line))
}

fn add_note(store: &FsStore, id: &Ulid, summary: &str, text: String) -> Result<()> {
    ops::add_note_with_summary(
        store,
        id,
        Ulid::new(),
        Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
        NoteKind::Regular,
        Some(summary.to_owned()),
        text,
    )?;
    Ok(())
}

impl Coordinator {
    /// The live probe for this pool's shipping checkout under `policy`.
    pub fn speculation_probe(&self, policy: &SpeculationPolicy) -> LiveGateProbe {
        let resolves = |name: &str| {
            git(&self.project, &["rev-parse", "--verify", "--quiet", name])
                .is_some_and(|output| output.status.success())
        };
        let reference = match &policy.base_branch {
            Some(branch) => [
                format!("refs/remotes/origin/{branch}"),
                format!("refs/heads/{branch}"),
            ]
            .into_iter()
            .find(|name| resolves(name)),
            None => resolves("@{upstream}").then(|| "@{upstream}".to_owned()),
        };
        LiveGateProbe {
            project: self.project.clone(),
            reference,
            jobs: JobStore::at(&self.registry_home),
            checkout_id: self.project_id.clone(),
        }
    }

    /// The ledger read without taking the pool lock, for a caller that already holds it
    /// (the reservation check runs under the ticket transaction and the state lock).
    fn speculation_ledger_locked(&self) -> Result<Option<speculation::SpeculationLedger>> {
        let path = self.directory.join(speculation::LEDGER_FILE);
        if !path.exists() {
            return Ok(None);
        }
        Ok(Some(Self::read_speculation(&path)?))
    }

    /// Decide a reservation under an enabled policy, from fresh ticket reads at the
    /// reservation boundary. `Ok(None)` is confirmed readiness. A provisional reservation
    /// requires that `base` contains every pending gate commit and that no recorded base
    /// gate is red: the repair of a failing prerequisite comes before new provisional work.
    pub fn speculation_reservation(
        &self,
        policy: &SpeculationPolicy,
        probe: &dyn BaseProbe,
        ticket: &Ticket,
        tickets: &[Ticket],
        base: &str,
    ) -> Result<Option<ProvisionalReservation>> {
        let readiness = ticket_readiness(policy, ticket, tickets, probe);
        match readiness.level {
            ReadinessLevel::Confirmed => return Ok(None),
            ReadinessLevel::Blocked => bail!(
                "ticket {} is not ready: {}; close the dependency, wait for its pushed CI gate commit, or remove the dependency before assigning it",
                ticket.slug,
                readiness.explain()
            ),
            ReadinessLevel::Provisional => {}
        }
        let repairs = self
            .speculation_ledger_locked()?
            .map(|ledger| ledger.repair_first())
            .unwrap_or_default();
        ensure!(
            repairs.is_empty(),
            "a base gate is red; repair {} before reserving new provisional work",
            repairs.join(", ")
        );
        let bases = readiness.provisional_bases();
        for found in &bases {
            ensure!(
                probe.contains(&found.base_commit, base) == Some(true),
                "base {base} does not contain the pushed commit {} of {}; provisional work must start from a base that contains its prerequisite",
                found.base_commit,
                found.slug
            );
        }
        Ok(Some(ProvisionalReservation {
            depth: readiness.depth,
            gates: bases.into_iter().map(PendingGate::of).collect(),
        }))
    }

    /// Why a ticket is confirmed, provisional or blocked now, with its record.
    pub fn speculation_explain(
        &self,
        policy: &SpeculationPolicy,
        probe: &dyn GateProbe,
        ticket: &Ticket,
        tickets: &[Ticket],
    ) -> Result<SpeculationExplanation> {
        let readiness = ticket_readiness(policy, ticket, tickets, probe);
        let native = ticket.id.to_string();
        let record = self.speculation_ledger()?.and_then(|ledger| {
            ledger
                .status()
                .records
                .into_iter()
                .rfind(|row| row.ticket.native_id == native)
        });
        Ok(SpeculationExplanation {
            ticket: native,
            slug: ticket.slug.clone(),
            policy: policy.clone(),
            opted_out: hotsheet_ticketing::provisional::opted_out(ticket),
            explain: record
                .as_ref()
                .map(|row| row.explain.clone())
                .unwrap_or_else(|| readiness.explain()),
            readiness,
            record,
        })
    }

    /// Write every at-risk note still owed for tickets of `store`: one regular note on
    /// the dependent naming the run and its recovery, and one on the failing
    /// prerequisite saying its repair comes first. A key line makes each idempotent
    /// across a restart between the note and the ledger.
    pub fn speculation_write_notes(&self, store: Option<&FsStore>) -> Result<Vec<String>> {
        let Some(ledger) = self.speculation_ledger()? else {
            return Ok(Vec::new());
        };
        let Some(store) = store else {
            return Ok(Vec::new());
        };
        let connection = git_connection_id(store);
        let mut written = Vec::new();
        for owed in ledger.owed_notes() {
            if owed.ticket.connection_id != connection {
                continue;
            }
            let id: Ulid = owed.ticket.native_id.parse()?;
            let ticket = store.read_ticket(&id)?;
            let key = risk_key(&owed);
            if !has_line(&ticket, &key) {
                let recovery = match owed.recovery {
                    Recovery::RebaseOntoFix => {
                        "It is still on its branch: rebase it onto the fix, rerun its affected gates, then record the new base with `coordination speculation-rebase`."
                    }
                    Recovery::ReverifyByCoveringRun => {
                        "It is already on the shipping branch: the next covering CI run re-verifies it; record the repaired gate with `coordination speculation-rebase`."
                    }
                };
                add_note(
                    store,
                    &id,
                    "Provisional work at risk",
                    format!(
                        "## Provisional work at risk\n\nThis ticket was started on `{}` at base `{}` before that ticket's CI gate finished. CI run `{}` on gate commit `{}` finished red.\n\n- {recovery}\n- The repair of `{}` comes before new work.\n- Nothing is discarded. If the fix invalidates this work, reopen it with `coordination speculation-reopen` and a reason.\n\n{key}\n",
                        owed.blocker_slug, owed.base, owed.run, owed.gate_commit, owed.blocker_slug
                    ),
                )?;
                written.push(owed.slug.clone());
            }
            if let Ok(blocker) = owed.blocker.parse::<Ulid>()
                && let Ok(found) = store.read_ticket(&blocker)
            {
                let key = repair_key(&owed);
                if !has_line(&found, &key) {
                    add_note(
                        store,
                        &blocker,
                        "CI red with provisional dependents",
                        format!(
                            "## CI red with provisional dependents\n\nCI run `{}` on gate commit `{}` finished red, and other tickets were started on that commit. Repair this ticket before new work; `coordination speculation-status` lists the dependents.\n\n{key}\n",
                            owed.run, owed.gate_commit
                        ),
                    )?;
                    written.push(found.slug.clone());
                }
            }
            self.speculation_noted(&owed.assignment, &owed.gate_commit, &owed.run)?;
        }
        Ok(written)
    }

    fn speculation_report(
        &self,
        changes: Vec<SpeculationChange>,
        store: Option<&FsStore>,
    ) -> Result<SpeculationReport> {
        let notes_written = self.speculation_write_notes(store)?;
        let ledger = self.speculation_ledger()?.unwrap_or_default();
        Ok(SpeculationReport {
            changes,
            notes_written,
            notes_owed: ledger.owed_notes(),
            status: ledger.status(),
        })
    }

    /// Record one CI result for a gate commit, as the watch or a person reports it.
    pub fn speculation_result(
        &self,
        commit: &str,
        observation: GateObservation,
        store: Option<&FsStore>,
        now: i64,
    ) -> Result<SpeculationReport> {
        let changes = self.speculation_apply(&[(commit.to_owned(), observation)], now)?;
        self.speculation_report(changes, store)
    }

    /// Read every gate this ledger still waits on from `probe` and apply the readings.
    /// An unwatched gate carries no information and changes nothing. Notes owed from an
    /// earlier interrupted pass are written too.
    pub fn speculation_sync(
        &self,
        probe: &dyn GateProbe,
        store: Option<&FsStore>,
        now: i64,
    ) -> Result<SpeculationReport> {
        let readings: Vec<(String, GateObservation)> = self
            .speculation_ledger()?
            .map(|ledger| ledger.watched_commits())
            .unwrap_or_default()
            .into_iter()
            .map(|commit| {
                let observation = probe.gate(&commit);
                (commit, observation)
            })
            .filter(|(_, observation)| observation.result != GateResult::Unknown)
            .collect();
        let changes = self.speculation_apply(&readings, now)?;
        self.speculation_report(changes, store)
    }

    /// Record a rebase onto the repaired prerequisite after checking it against Git: the
    /// repaired gate commit must be reachable from the shipping branch and, when the work
    /// moved, contained in its new base.
    pub fn speculation_rebase_checked(
        &self,
        probe: &dyn BaseProbe,
        assignment: &str,
        rebase: &Rebase,
        now: i64,
        actor: Option<&str>,
    ) -> Result<SpeculativeRecord> {
        ensure!(
            probe.reachable(&rebase.gate_commit) == Some(true),
            "the repaired gate commit is not reachable from the shipping branch"
        );
        if let Some(base) = &rebase.base {
            ensure!(
                probe.contains(&rebase.gate_commit, base) == Some(true),
                "the new base does not contain the repaired gate commit"
            );
        }
        self.speculation_rebase(assignment, rebase, now, actor)
    }

    /// Reopen invalidated work with a ticket note. A ticket that was already closed
    /// returns to Started, so the work is visible again rather than silently lost.
    pub fn speculation_reopen_noted(
        &self,
        store: &FsStore,
        assignment: &str,
        reason: &str,
        now: i64,
        actor: Option<&str>,
    ) -> Result<SpeculativeRecord> {
        let record = self.speculation_reopen(assignment, reason, now, actor)?;
        ensure!(
            record.ticket.connection_id == git_connection_id(store),
            "the reopened ticket belongs to another source; pass its store with -C"
        );
        let id: Ulid = record.ticket.native_id.parse()?;
        let ticket = store.read_ticket(&id)?;
        let key = format!("Speculation reopen key: {assignment}");
        if !has_line(&ticket, &key) {
            let gates = record
                .gates
                .iter()
                .map(|gate| format!("`{}` at `{}`", gate.slug, gate.commit))
                .collect::<Vec<_>>()
                .join(", ");
            add_note(
                store,
                &id,
                "Provisional work reopened",
                format!(
                    "## Provisional work reopened\n\nThis work was started at base `{}` on {gates} before CI finished, and the fix invalidated it.\n\n- Reason: {}\n- The branch and its commits are kept; assign the ticket again from the repaired base.\n\n{key}\n",
                    record.base,
                    record.reopen_reason.as_deref().unwrap_or_default()
                ),
            )?;
        }
        if matches!(ticket.status, Status::Completed | Status::Verified) {
            ops::update(
                store,
                &id,
                Timestamp::from_datetime(time::OffsetDateTime::now_utc()),
                ops::TicketPatch {
                    status: Some(Status::Started),
                    ..ops::TicketPatch::default()
                },
            )?;
        }
        Ok(record)
    }

    /// Record the speculative base and pending gates on the worker's worktree branch, as
    /// Git config under `branch.<name>.`. Run again after any change: a confirmed record
    /// clears the pending gates and keeps the base.
    pub fn speculation_branch(&self, worktree: &Path, assignment: &str) -> Result<BranchRecord> {
        let record = self
            .speculation_ledger()?
            .and_then(|ledger| ledger.records.get(assignment).cloned())
            .ok_or_else(|| anyhow::anyhow!("assignment {assignment} has no provisional record"))?;
        let head = git(worktree, &["symbolic-ref", "--quiet", "--short", "HEAD"])
            .filter(|output| output.status.success())
            .ok_or_else(|| anyhow::anyhow!("the worktree is not on a branch"))?;
        let branch = String::from_utf8(head.stdout)?.trim().to_owned();
        let key = |name: &str| format!("branch.{branch}.{name}");
        let set = |args: &[&str]| -> Result<()> {
            let output = git(worktree, args)
                .ok_or_else(|| anyhow::anyhow!("could not run git config in the worktree"))?;
            // `--unset-all` of an absent key exits 5; that is the wanted state.
            ensure!(
                output.status.success() || output.status.code() == Some(5),
                "could not record the speculation on the worktree branch"
            );
            Ok(())
        };
        let pending_gates: Vec<String> = record
            .gates
            .iter()
            .filter(|gate| gate.result != GateResult::Green)
            .map(|gate| {
                format!(
                    "{} {} {} {}",
                    gate.slug,
                    gate.commit,
                    gate.result.name(),
                    gate.run.as_deref().unwrap_or("-")
                )
            })
            .collect();
        set(&["config", &key(BRANCH_BASE_KEY), &record.base])?;
        set(&["config", &key(BRANCH_STATE_KEY), record.state.name()])?;
        set(&["config", "--unset-all", &key(BRANCH_GATE_KEY)])?;
        for gate in &pending_gates {
            set(&["config", "--add", &key(BRANCH_GATE_KEY), gate])?;
        }
        Ok(BranchRecord {
            branch,
            base: record.base,
            state: record.state.name().to_owned(),
            pending_gates,
        })
    }
}
