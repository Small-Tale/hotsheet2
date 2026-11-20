//! Provisional assignments (HS2-2SJPCH): work started on a prerequisite that is pushed and
//! waiting on remote CI, and what its gate's result does to it.
//!
//! The readiness rule itself is `hotsheet_ticketing::provisional`. This module records
//! each provisional reservation and walks it through its states:
//!
//! - `provisional`: at least one base gate is undecided;
//! - `confirmed`: every base gate is green;
//! - `at_risk`: a base gate finished red. The record names the run, the repair comes
//!   before new work, and the work is rebased onto the fix (parked) or re-verified by the
//!   next covering run (already integrated);
//! - `reopened`: the fix invalidated the work. The record stays; nothing is discarded.
//!
//! The ledger lives beside the pool in `speculation.json` and is read and written under
//! the pool's `state.lock`. Like the batch ledger it needs no pool schema bump: only a
//! provisional reservation creates it, which requires the project's opt-in policy, and it
//! carries its own `version`. Every transition is idempotent, so replaying a command
//! repairs a write interrupted between the two files.

use std::collections::BTreeMap;
use std::time::Duration;

use anyhow::{bail, ensure};
use hotsheet_ticketing::provisional::{
    GateObservation, GateResult, IntegrationRule, ProvisionalBase, explain_base,
};
use serde::{Deserialize, Serialize};

use super::*;

/// The ledger file in the coordination directory.
pub const LEDGER_FILE: &str = "speculation.json";
/// The ledger version this executable reads and writes.
pub const LEDGER_VERSION: u32 = 1;
const MAX_REASON_CHARS: usize = 500;
const MAX_RUN_CHARS: usize = 128;
/// The run name of a red result that named no run.
pub const UNNAMED_RUN: &str = "unnamed";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeculationLedger {
    pub version: u32,
    #[serde(default)]
    pub records: BTreeMap<String, SpeculativeRecord>,
}

/// HS2-J3SMMT: the most records one ledger keeps; settled ones go first beyond it.
pub const MAX_LEDGER_RECORDS: usize = 1024;

impl SpeculationLedger {
    /// Forget the oldest confirmed and reopened records until at most `max` remain.
    /// Provisional and at-risk work is never forgotten, however many there are. A
    /// record's age is its latest event, then its key. Returns the keys forgotten.
    pub fn retire_settled(&mut self, max: usize) -> Vec<String> {
        if self.records.len() <= max {
            return Vec::new();
        }
        let mut settled: Vec<(i64, String)> = self
            .records
            .iter()
            .filter(|(_, record)| {
                matches!(
                    record.state,
                    SpeculationState::Confirmed | SpeculationState::Reopened
                )
            })
            .map(|(key, record)| {
                (
                    record
                        .events
                        .iter()
                        .map(|event| event.at)
                        .max()
                        .unwrap_or(0),
                    key.clone(),
                )
            })
            .collect();
        settled.sort();
        let excess = self.records.len() - max;
        let retired: Vec<String> = settled
            .into_iter()
            .take(excess)
            .map(|(_, key)| key)
            .collect();
        for key in &retired {
            self.records.remove(key);
        }
        retired
    }
}

impl Default for SpeculationLedger {
    fn default() -> Self {
        Self {
            version: LEDGER_VERSION,
            records: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeculationState {
    Provisional,
    Confirmed,
    AtRisk,
    Reopened,
}

impl SpeculationState {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Provisional => "provisional",
            Self::Confirmed => "confirmed",
            Self::AtRisk => "at_risk",
            Self::Reopened => "reopened",
        }
    }
}

/// One base gate a provisional assignment depends on.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingGate {
    /// Native id of the prerequisite ticket.
    pub blocker: String,
    pub slug: String,
    /// The prerequisite's integrated commit.
    pub commit: String,
    pub result: GateResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
}

impl PendingGate {
    pub fn of(base: &ProvisionalBase) -> Self {
        Self {
            blocker: base.blocker.to_string(),
            slug: base.slug.clone(),
            commit: base.base_commit.clone(),
            result: base.gate.result,
            run: base.gate.run.clone(),
        }
    }

    fn observation(&self) -> GateObservation {
        GateObservation {
            result: self.result,
            run: self.run.clone(),
        }
    }
}

/// How at-risk work recovers once the failing prerequisite is repaired.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Recovery {
    /// Still on its branch: rebase onto the fix and rerun the affected gates.
    RebaseOntoFix,
    /// Already on the shipping branch: the next covering CI run re-verifies it.
    ReverifyByCoveringRun,
}

/// One red run a record was marked at risk for. `noted` turns true once the ticket note
/// naming the run is written, so a restart writes the note it still owes and no other.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RiskNotice {
    pub gate_commit: String,
    pub run: String,
    pub recovery: Recovery,
    #[serde(default)]
    pub noted: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpeculationEventKind {
    Recorded,
    GateChanged,
    Confirmed,
    AtRisk,
    Rebased,
    Reopened,
}

/// Append-only history of one record. Identifiers and times only.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeculationEvent {
    /// Unix seconds.
    pub at: i64,
    pub kind: SpeculationEventKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gate_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<GateResult>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SpeculativeRecord {
    pub assignment: String,
    pub ticket: TicketRef,
    pub slug: String,
    pub worker: String,
    /// The speculative base SHA the worker's branch starts from. It contains every gate
    /// commit below.
    pub base: String,
    /// Unconfirmed gates stacked beneath this work when it was reserved.
    pub depth: u32,
    /// The project's integration rule when the work was reserved.
    pub integration: IntegrationRule,
    pub gates: Vec<PendingGate>,
    pub state: SpeculationState,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub risks: Vec<RiskNotice>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reopen_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<SpeculationEvent>,
}

impl SpeculativeRecord {
    fn derived_state(&self) -> SpeculationState {
        if self.state == SpeculationState::Reopened {
            SpeculationState::Reopened
        } else if self.gates.iter().any(|gate| gate.result == GateResult::Red) {
            SpeculationState::AtRisk
        } else if self
            .gates
            .iter()
            .all(|gate| gate.result == GateResult::Green)
        {
            SpeculationState::Confirmed
        } else {
            SpeculationState::Provisional
        }
    }

    /// `provisional on <slug> CI <run>` for every gate that is not green, or the state.
    pub fn explain(&self) -> String {
        match self.state {
            SpeculationState::Provisional => self
                .gates
                .iter()
                .filter(|gate| gate.result != GateResult::Green)
                .map(|gate| explain_base(&gate.slug, &gate.observation()))
                .collect::<Vec<_>>()
                .join("; "),
            SpeculationState::AtRisk => self
                .gates
                .iter()
                .filter(|gate| gate.result == GateResult::Red)
                .map(|gate| {
                    format!(
                        "at risk: {} CI {} red",
                        gate.slug,
                        gate.run.as_deref().unwrap_or(UNNAMED_RUN)
                    )
                })
                .collect::<Vec<_>>()
                .join("; "),
            state => state.name().to_owned(),
        }
    }

    fn event(
        &mut self,
        at: i64,
        kind: SpeculationEventKind,
        gate: Option<(&str, GateResult, Option<&str>)>,
        actor: Option<&str>,
    ) {
        self.events.push(SpeculationEvent {
            at,
            kind,
            gate_commit: gate.map(|(commit, _, _)| commit.to_owned()),
            result: gate.map(|(_, result, _)| result),
            run: gate.and_then(|(_, _, run)| run.map(str::to_owned)),
            actor: actor.map(str::to_owned),
        });
    }
}

/// What one gate reading did to one record.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeculationChange {
    pub assignment: String,
    pub slug: String,
    pub gate_commit: String,
    pub result: GateResult,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub run: Option<String>,
    pub from: SpeculationState,
    pub to: SpeculationState,
    /// Set when the record is at risk after this reading.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
}

/// An at-risk ticket note that is still to be written.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwedNote {
    pub assignment: String,
    pub ticket: TicketRef,
    pub slug: String,
    pub base: String,
    pub blocker: String,
    pub blocker_slug: String,
    pub gate_commit: String,
    pub run: String,
    pub recovery: Recovery,
}

/// A gate whose newest run says nothing final about its commit, so it is to be
/// re-evaluated against the newest covering run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StaleGate {
    pub slug: String,
    pub commit: String,
    pub result: GateResult,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeculationRow {
    pub assignment: String,
    pub ticket: TicketRef,
    pub slug: String,
    pub worker: String,
    pub base: String,
    pub depth: u32,
    pub state: SpeculationState,
    pub gates: Vec<PendingGate>,
    /// `provisional on <slug> CI <run>`.
    pub explain: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recovery: Option<Recovery>,
}

/// The read every surface shares: `coordination status`, `coordination workers`, the
/// worker route and MCP `hotsheet_workers`.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpeculationStatus {
    /// Every record that is not confirmed, then confirmed ones, by assignment id.
    pub records: Vec<SpeculationRow>,
    /// Prerequisite slugs whose gate is red: their repair comes before new work.
    pub repair_first: Vec<String>,
    /// Superseded or cancelled gates awaiting the newest covering run.
    pub reevaluate: Vec<StaleGate>,
}

/// The repaired prerequisite's gate that replaces a failed or stale one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Rebase {
    /// The gate commit being replaced.
    pub replaces: String,
    /// The repaired prerequisite's integrated commit.
    pub gate_commit: String,
    pub gate: GateObservation,
    /// The base the work was rebased onto; required while it is still on its branch.
    pub base: Option<String>,
}

fn valid_commit(commit: &str) -> Result<String> {
    ensure!(
        super::valid_full_sha(commit),
        "a gate or base commit must be a full 40-character SHA"
    );
    Ok(commit.to_ascii_lowercase())
}

fn valid_run(run: Option<&str>) -> Result<Option<String>> {
    match run.map(str::trim) {
        None | Some("") => Ok(None),
        Some(run) => {
            ensure!(
                run.chars().count() <= MAX_RUN_CHARS
                    && run
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':')),
                "run must be a provider run identifier"
            );
            Ok(Some(run.to_owned()))
        }
    }
}

impl SpeculationLedger {
    /// Record a provisional reservation. A replay keeps the existing record, so readings
    /// applied since the first write are never rolled back.
    pub fn record(&mut self, record: SpeculativeRecord, now: i64) -> Result<()> {
        ensure!(
            !record.gates.is_empty(),
            "a provisional assignment names at least one pending gate"
        );
        valid_commit(&record.base)?;
        for gate in &record.gates {
            valid_commit(&gate.commit)?;
        }
        if let Some(existing) = self.records.get(&record.assignment) {
            ensure!(
                existing.ticket == record.ticket && existing.worker == record.worker,
                "assignment {} already has a different provisional record",
                record.assignment
            );
        } else {
            let mut record = record;
            record.state = SpeculationState::Provisional;
            record.state = record.derived_state();
            record.event(now, SpeculationEventKind::Recorded, None, None);
            self.records.insert(record.assignment.clone(), record);
        }
        Ok(())
    }

    /// Apply one reading of the gate of `commit` to every record that depends on it.
    ///
    /// A red gate stays red until a green reading or a rebase replaces it: a re-run that
    /// is pending, a cancelled run or an unknown watch never clears the risk. A green
    /// gate is cleared only by a red reading. `integrated` reports whether a record's
    /// work already reached the shipping branch, which selects its recovery.
    pub fn apply(
        &mut self,
        commit: &str,
        observation: &GateObservation,
        now: i64,
        integrated: &dyn Fn(&str) -> bool,
    ) -> Result<Vec<SpeculationChange>> {
        let commit = valid_commit(commit)?;
        let run = valid_run(observation.run.as_deref())?;
        let mut changes = Vec::new();
        for record in self.records.values_mut() {
            if record.state == SpeculationState::Reopened {
                continue;
            }
            let mut touched = false;
            for gate in &mut record.gates {
                if gate.commit != commit {
                    continue;
                }
                let next = match (gate.result, observation.result) {
                    // A verdict always lands. Without a run it keeps the run of the
                    // same verdict, so a replay never invents an unnamed run.
                    (current, verdict @ (GateResult::Red | GateResult::Green)) => Some((
                        verdict,
                        run.clone()
                            .or_else(|| (current == verdict).then(|| gate.run.clone()).flatten()),
                    )),
                    // Nothing short of a verdict clears a verdict.
                    (GateResult::Red | GateResult::Green, _) => None,
                    (_, other) => Some((other, run.clone().or_else(|| gate.run.clone()))),
                };
                if let Some((result, run)) = next
                    && (gate.result != result || gate.run != run)
                {
                    gate.result = result;
                    gate.run = run;
                    touched = true;
                }
            }
            if !touched {
                continue;
            }
            let from = record.state;
            let to = record.derived_state();
            record.state = to;
            let landed = integrated(&record.assignment);
            let recovery = (to == SpeculationState::AtRisk).then_some(if landed {
                Recovery::ReverifyByCoveringRun
            } else {
                Recovery::RebaseOntoFix
            });
            if observation.result == GateResult::Red {
                let name = record
                    .gates
                    .iter()
                    .find(|gate| gate.commit == commit)
                    .and_then(|gate| gate.run.clone())
                    .unwrap_or_else(|| UNNAMED_RUN.to_owned());
                if !record
                    .risks
                    .iter()
                    .any(|risk| risk.gate_commit == commit && risk.run == name)
                {
                    record.risks.push(RiskNotice {
                        gate_commit: commit.clone(),
                        run: name,
                        recovery: recovery.unwrap_or(Recovery::RebaseOntoFix),
                        noted: false,
                    });
                }
            }
            let kind = match (from == to, to) {
                (false, SpeculationState::Confirmed) => SpeculationEventKind::Confirmed,
                (_, SpeculationState::AtRisk) if observation.result == GateResult::Red => {
                    SpeculationEventKind::AtRisk
                }
                _ => SpeculationEventKind::GateChanged,
            };
            record.event(
                now,
                kind,
                Some((&commit, observation.result, run.as_deref())),
                None,
            );
            changes.push(SpeculationChange {
                assignment: record.assignment.clone(),
                slug: record.slug.clone(),
                gate_commit: commit.clone(),
                result: observation.result,
                run: run.clone(),
                from,
                to,
                recovery,
            });
        }
        Ok(changes)
    }

    /// Replace the gate `replaces` of one record with the repaired prerequisite's gate,
    /// and move the work to `base` when it was rebased. Work that is still on its branch
    /// must name the new base.
    pub fn rebase(
        &mut self,
        assignment: &str,
        rebase: &Rebase,
        landed: bool,
        now: i64,
        actor: Option<&str>,
    ) -> Result<SpeculativeRecord> {
        let replaces = valid_commit(&rebase.replaces)?;
        let gate_commit = valid_commit(&rebase.gate_commit)?;
        let base = rebase.base.as_deref().map(valid_commit).transpose()?;
        let gate = &rebase.gate;
        let run = valid_run(gate.run.as_deref())?;
        let record = self
            .records
            .get_mut(assignment)
            .ok_or_else(|| anyhow::anyhow!("assignment {assignment} has no provisional record"))?;
        let replayed = record.gates.iter().any(|g| g.commit == gate_commit)
            && !record.gates.iter().any(|g| g.commit == replaces)
            && base.as_ref().is_none_or(|base| *base == record.base);
        if replayed {
            return Ok(record.clone());
        }
        ensure!(
            record.state != SpeculationState::Reopened,
            "assignment {assignment} was reopened; assign its ticket again instead of rebasing"
        );
        ensure!(
            replaces != gate_commit,
            "the repaired gate commit must differ from the one it replaces"
        );
        ensure!(
            landed || base.is_some(),
            "work that is still on its branch must name the base it was rebased onto"
        );
        ensure!(
            !matches!(gate.result, GateResult::Red),
            "the replacing gate is red; repair it first"
        );
        let Some(slot) = record.gates.iter_mut().find(|g| g.commit == replaces) else {
            bail!("assignment {assignment} does not depend on gate commit {replaces}");
        };
        ensure!(
            slot.result != GateResult::Green,
            "gate commit {replaces} is green; there is nothing to rebase"
        );
        slot.commit = gate_commit.clone();
        slot.result = gate.result;
        slot.run = run.clone();
        if let Some(base) = base {
            record.base = base;
        }
        record.state = SpeculationState::Provisional;
        record.state = record.derived_state();
        record.event(
            now,
            SpeculationEventKind::Rebased,
            Some((&gate_commit, gate.result, run.as_deref())),
            actor,
        );
        if record.state == SpeculationState::Confirmed {
            record.event(now, SpeculationEventKind::Confirmed, None, actor);
        }
        Ok(record.clone())
    }

    /// The fix invalidated this work. The record and its history stay.
    pub fn reopen(
        &mut self,
        assignment: &str,
        reason: &str,
        now: i64,
        actor: Option<&str>,
    ) -> Result<SpeculativeRecord> {
        let reason = reason.trim();
        ensure!(
            !reason.is_empty() && reason.chars().count() <= MAX_REASON_CHARS,
            "a reopen needs a reason of at most {MAX_REASON_CHARS} characters"
        );
        let record = self
            .records
            .get_mut(assignment)
            .ok_or_else(|| anyhow::anyhow!("assignment {assignment} has no provisional record"))?;
        if record.state == SpeculationState::Reopened {
            return Ok(record.clone());
        }
        ensure!(
            record.state != SpeculationState::Confirmed,
            "assignment {assignment} is confirmed; a confirmed base cannot invalidate it"
        );
        record.state = SpeculationState::Reopened;
        record.reopen_reason = Some(reason.to_owned());
        record.event(now, SpeculationEventKind::Reopened, None, actor);
        Ok(record.clone())
    }

    /// Whether this work may enter integration now.
    pub fn integration_rule(&self, assignment: &str) -> Result<()> {
        let Some(record) = self.records.get(assignment) else {
            return Ok(());
        };
        match (record.state, record.integration) {
            (SpeculationState::Confirmed, _) => Ok(()),
            (SpeculationState::Reopened, _) => bail!(
                "{} was reopened after its base gate failed; it cannot be integrated from this assignment",
                record.slug
            ),
            (SpeculationState::AtRisk, _) => bail!(
                "{} is at risk ({}); rebase it onto the fix and rerun its affected gates before integrating",
                record.slug,
                record.explain()
            ),
            (SpeculationState::Provisional, IntegrationRule::Parked) => bail!(
                "{} stays parked on its branch until its base gate is green ({})",
                record.slug,
                record.explain()
            ),
            (SpeculationState::Provisional, IntegrationRule::PushPerTicket) => Ok(()),
        }
    }

    /// Gate commits whose result can still change this ledger: every gate of a record
    /// that is neither confirmed nor reopened and is not green.
    pub fn watched_commits(&self) -> Vec<String> {
        let mut commits: Vec<String> = self
            .records
            .values()
            .filter(|record| {
                !matches!(
                    record.state,
                    SpeculationState::Confirmed | SpeculationState::Reopened
                )
            })
            .flat_map(|record| &record.gates)
            .filter(|gate| gate.result != GateResult::Green)
            .map(|gate| gate.commit.clone())
            .collect();
        commits.sort();
        commits.dedup();
        commits
    }

    /// Prerequisite slugs with a red gate that some record still depends on.
    pub fn repair_first(&self) -> Vec<String> {
        let mut slugs: Vec<String> = self
            .records
            .values()
            .filter(|record| record.state == SpeculationState::AtRisk)
            .flat_map(|record| &record.gates)
            .filter(|gate| gate.result == GateResult::Red)
            .map(|gate| gate.slug.clone())
            .collect();
        slugs.sort();
        slugs.dedup();
        slugs
    }

    pub fn owed_notes(&self) -> Vec<OwedNote> {
        self.records
            .values()
            .flat_map(|record| {
                record.risks.iter().filter(|risk| !risk.noted).map(|risk| {
                    let gate = record
                        .gates
                        .iter()
                        .find(|gate| gate.commit == risk.gate_commit);
                    OwedNote {
                        assignment: record.assignment.clone(),
                        ticket: record.ticket.clone(),
                        slug: record.slug.clone(),
                        base: record.base.clone(),
                        blocker: gate.map(|g| g.blocker.clone()).unwrap_or_default(),
                        blocker_slug: gate.map(|g| g.slug.clone()).unwrap_or_default(),
                        gate_commit: risk.gate_commit.clone(),
                        run: risk.run.clone(),
                        recovery: risk.recovery,
                    }
                })
            })
            .collect()
    }

    pub fn noted(&mut self, assignment: &str, gate_commit: &str, run: &str) {
        if let Some(record) = self.records.get_mut(assignment) {
            for risk in &mut record.risks {
                if risk.gate_commit == gate_commit && risk.run == run {
                    risk.noted = true;
                }
            }
        }
    }

    pub fn status(&self) -> SpeculationStatus {
        let mut records: Vec<SpeculationRow> = self
            .records
            .values()
            .map(|record| SpeculationRow {
                assignment: record.assignment.clone(),
                ticket: record.ticket.clone(),
                slug: record.slug.clone(),
                worker: record.worker.clone(),
                base: record.base.clone(),
                depth: record.depth,
                state: record.state,
                gates: record.gates.clone(),
                explain: record.explain(),
                recovery: (record.state == SpeculationState::AtRisk)
                    .then(|| record.risks.last().map(|risk| risk.recovery))
                    .flatten(),
            })
            .collect();
        records.sort_by_key(|row| row.state == SpeculationState::Confirmed);
        let mut reevaluate: Vec<StaleGate> = self
            .records
            .values()
            .filter(|record| record.state == SpeculationState::Provisional)
            .flat_map(|record| &record.gates)
            .filter(|gate| matches!(gate.result, GateResult::Superseded | GateResult::Cancelled))
            .map(|gate| StaleGate {
                slug: gate.slug.clone(),
                commit: gate.commit.clone(),
                result: gate.result,
            })
            .collect();
        reevaluate.sort_by(|a, b| a.commit.cmp(&b.commit));
        reevaluate.dedup();
        SpeculationStatus {
            records,
            repair_first: self.repair_first(),
            reevaluate,
        }
    }
}

/// One read of the gates and the wait that follows it, without a timer: `step` applies
/// the current readings and returns the changes with the event source's revision, and
/// `wait` blocks until that revision changes or the remaining time passes. Returns the
/// changes and how many times `step` ran, which is one per wake and never per interval.
pub fn follow<T>(
    timeout: Duration,
    mut step: impl FnMut() -> Result<(Vec<T>, u64)>,
    mut wait: impl FnMut(u64, Duration) -> Result<u64>,
) -> Result<(Vec<T>, u32)> {
    let deadline = std::time::Instant::now() + timeout;
    let mut reads = 0;
    loop {
        let (changes, revision) = step()?;
        reads += 1;
        let remaining = deadline.saturating_duration_since(std::time::Instant::now());
        if !changes.is_empty() || remaining.is_zero() {
            return Ok((changes, reads));
        }
        if wait(revision, remaining)? == revision {
            // Timed out with no event: nothing changed, so nothing is read again.
            return Ok((changes, reads));
        }
    }
}

impl Coordinator {
    fn speculation_path(&self) -> PathBuf {
        self.directory.join(LEDGER_FILE)
    }

    pub(super) fn read_speculation(path: &Path) -> Result<SpeculationLedger> {
        let ledger = match fs::read(path) {
            Ok(bytes) => serde_json::from_slice::<SpeculationLedger>(&bytes)
                .map_err(|error| anyhow::anyhow!("invalid speculation ledger: {error}"))?,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(SpeculationLedger::default());
            }
            Err(error) => return Err(error.into()),
        };
        ensure!(
            ledger.version == LEDGER_VERSION,
            "speculation ledger version {} is not supported by this executable",
            ledger.version
        );
        Ok(ledger)
    }

    /// Read the pool and change the speculation ledger under one lock. The ledger is
    /// written only when it changed, so a pool without provisional work never has one.
    pub(super) fn change_speculation<T>(
        &self,
        update: impl FnOnce(&State, &mut SpeculationLedger) -> Result<T>,
    ) -> Result<T> {
        self.change(|state| self.speculation_locked(state, update))
    }

    /// Change the speculation ledger while the caller holds the pool lock (inside
    /// `change`). Written only when it changed.
    pub(super) fn speculation_locked<T>(
        &self,
        state: &State,
        update: impl FnOnce(&State, &mut SpeculationLedger) -> Result<T>,
    ) -> Result<T> {
        let path = self.speculation_path();
        let mut ledger = Self::read_speculation(&path)?;
        let before = ledger.clone();
        let result = update(state, &mut ledger)?;
        if ledger != before {
            let mut temporary = tempfile::NamedTempFile::new_in(&self.directory)?;
            serde_json::to_writer_pretty(&mut temporary, &ledger)?;
            temporary.write_all(b"\n")?;
            temporary.as_file().sync_all()?;
            hotsheet_sync::files::persist(temporary, &path)?;
        }
        Ok(result)
    }

    /// The ledger, or `None` when this pool never reserved provisional work.
    pub fn speculation_ledger(&self) -> Result<Option<SpeculationLedger>> {
        let path = self.speculation_path();
        if !path.exists() {
            return Ok(None);
        }
        self.change_speculation(|_, ledger| Ok(Some(ledger.clone())))
    }

    /// Record the speculative base and pending gates of an existing reservation.
    pub fn speculation_record(
        &self,
        assignment: &str,
        depth: u32,
        integration: IntegrationRule,
        gates: Vec<PendingGate>,
        now: i64,
    ) -> Result<SpeculativeRecord> {
        self.change_speculation(|state, ledger| {
            Self::record_reservation(state, ledger, assignment, depth, integration, gates, now)
        })
    }

    /// The record of an existing reservation, written into `ledger`. A replayed
    /// reservation keeps its first record.
    pub(super) fn record_reservation(
        state: &State,
        ledger: &mut SpeculationLedger,
        assignment: &str,
        depth: u32,
        integration: IntegrationRule,
        gates: Vec<PendingGate>,
        now: i64,
    ) -> Result<SpeculativeRecord> {
        {
            let reserved = state
                .assignments
                .get(assignment)
                .ok_or_else(|| anyhow::anyhow!("unknown assignment {assignment}"))?;
            ledger.record(
                SpeculativeRecord {
                    assignment: reserved.id.clone(),
                    ticket: reserved.ticket.clone(),
                    slug: reserved.slug.clone(),
                    worker: reserved.worker.clone(),
                    base: reserved.base.to_ascii_lowercase(),
                    depth,
                    integration,
                    gates,
                    state: SpeculationState::Provisional,
                    risks: Vec::new(),
                    reopen_reason: None,
                    events: Vec::new(),
                },
                now,
            )?;
            Ok(ledger.records[assignment].clone())
        }
    }

    fn landed(state: &State, assignment: &str) -> bool {
        state
            .assignments
            .get(assignment)
            .is_some_and(|a| matches!(a.phase, Phase::Ci | Phase::Done))
    }

    /// Apply gate readings, in order. Returns every change they caused.
    pub fn speculation_apply(
        &self,
        readings: &[(String, GateObservation)],
        now: i64,
    ) -> Result<Vec<SpeculationChange>> {
        if readings.is_empty() || !self.speculation_path().exists() {
            return Ok(Vec::new());
        }
        self.change_speculation(|state, ledger| {
            let mut changes = Vec::new();
            for (commit, observation) in readings {
                changes.extend(ledger.apply(commit, observation, now, &|assignment| {
                    Self::landed(state, assignment)
                })?);
            }
            Ok(changes)
        })
    }

    pub fn speculation_rebase(
        &self,
        assignment: &str,
        rebase: &Rebase,
        now: i64,
        actor: Option<&str>,
    ) -> Result<SpeculativeRecord> {
        self.change_speculation(|state, ledger| {
            ledger.rebase(
                assignment,
                rebase,
                Self::landed(state, assignment),
                now,
                actor,
            )
        })
    }

    pub fn speculation_reopen(
        &self,
        assignment: &str,
        reason: &str,
        now: i64,
        actor: Option<&str>,
    ) -> Result<SpeculativeRecord> {
        self.change_speculation(|_, ledger| ledger.reopen(assignment, reason, now, actor))
    }

    pub fn speculation_noted(&self, assignment: &str, gate_commit: &str, run: &str) -> Result<()> {
        self.change_speculation(|_, ledger| {
            ledger.noted(assignment, gate_commit, run);
            Ok(())
        })
    }

    /// The integration rule for provisional work (HS2-2SJPCH). A pool without a ledger,
    /// and an assignment without a record, pass untouched.
    pub fn speculation_integration_rule(&self, assignment: &str) -> Result<()> {
        match self.speculation_ledger()? {
            Some(ledger) => ledger.integration_rule(assignment),
            None => Ok(()),
        }
    }
}
