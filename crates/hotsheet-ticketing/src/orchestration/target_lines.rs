//! HS2-S3FKJ7: the orchestration line of target tickets, read from the target side.
//!
//! A project that an orchestration project on this machine orchestrates shows, in
//! its own List and Columns, the same orchestration line the Orchestration view's
//! cards carry (§26.26): the step's state, where it runs and the goal ticket. This
//! module is the pure fold: given the timelines of the orchestration projects whose
//! reviewed bindings put one of their targets on a checkout, it keeps one line per
//! target ticket of that checkout. It contacts nothing and writes nothing.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::timeline::{ItemState, Timeline, TimelineItem, TimelinePhase};

/// One orchestration project's timeline and which of its targets the checkout is.
pub struct TargetLineSource<'a> {
    pub owner_checkout_id: &'a str,
    /// The orchestration project's name, as the checkout registry has it.
    pub owner_label: &'a str,
    /// The owner's targets whose binding on this machine names the checkout.
    pub target_ids: &'a [String],
    pub timeline: &'a Timeline,
}

/// The orchestration line of one target ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLine {
    pub owner_checkout_id: String,
    pub owner_label: String,
    /// The owner's plan was never started, so the line reads as a dry run.
    pub proposed: bool,
    /// Other plan steps that also work this ticket, in this or another project.
    #[serde(default, skip_serializing_if = "is_zero")]
    pub also_planned: u32,
    pub item: TimelineItem,
}

fn is_zero(value: &u32) -> bool {
    *value == 0
}

/// Every orchestration line of a checkout's tickets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetLines {
    /// Changes exactly when the lines change; a reader waits with it as its cursor.
    pub version: String,
    /// Some orchestration project on this machine orchestrates the checkout, even
    /// with no line yet. A client reads again when this changes.
    pub orchestrated: bool,
    /// Ordered by the ticket's connection and native id.
    pub lines: Vec<TargetLine>,
}

/// The rank that picks one line when several steps work one ticket: unfinished work
/// first (running, then waiting on a person, then the rest), finished work last.
fn rank(state: ItemState) -> u8 {
    match state {
        ItemState::Running | ItemState::Provisional => 0,
        ItemState::WaitsForPermission | ItemState::WaitsForDecision => 1,
        ItemState::Ready | ItemState::WaitsForPrerequisite => 2,
        ItemState::Done => 3,
    }
}

fn version(lines: &[TargetLine], orchestrated: bool) -> String {
    let mut stable = lines.to_vec();
    for line in &mut stable {
        // The clock moves estimates; what the line shows does not depend on them.
        line.item.estimated_start_ms = None;
    }
    let bytes = serde_json::to_vec(&(orchestrated, &stable)).unwrap_or_default();
    Sha256::digest(&bytes)[..12]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One line per target ticket of the checkout. When several plan steps work the
/// same ticket the most pressing one is shown (see [`rank`]), ties going to the
/// owner listed first and then to the step id, and the others are counted in
/// `also_planned`. Sources are read in the order given; repeating a source or an
/// item changes nothing.
pub fn target_lines(sources: &[TargetLineSource<'_>]) -> TargetLines {
    let mut chosen: BTreeMap<(String, String), (usize, TargetLine)> = BTreeMap::new();
    let mut seen: BTreeMap<(String, String), Vec<(String, String)>> = BTreeMap::new();
    for (order, source) in sources.iter().enumerate() {
        let proposed = source.timeline.phase == TimelinePhase::Proposed;
        for wave in &source.timeline.waves {
            for item in &wave.items {
                if !source.target_ids.contains(&item.target.target_id) {
                    continue;
                }
                let key = (
                    item.target.connection_id.clone(),
                    item.target.native_id.clone(),
                );
                let step = (source.owner_checkout_id.to_owned(), item.step_id.clone());
                let steps = seen.entry(key.clone()).or_default();
                if steps.contains(&step) {
                    continue;
                }
                steps.push(step);
                let line = TargetLine {
                    owner_checkout_id: source.owner_checkout_id.to_owned(),
                    owner_label: source.owner_label.to_owned(),
                    proposed,
                    also_planned: 0,
                    item: item.clone(),
                };
                let better = chosen.get(&key).is_none_or(|(at, current)| {
                    (rank(line.item.state), order, &line.item.step_id)
                        < (rank(current.item.state), *at, &current.item.step_id)
                });
                if better {
                    chosen.insert(key, (order, line));
                }
            }
        }
    }
    let lines: Vec<TargetLine> = chosen
        .into_iter()
        .map(|(key, (_, mut line))| {
            let others = seen.get(&key).map_or(0, |steps| steps.len() - 1);
            line.also_planned = u32::try_from(others).unwrap_or(u32::MAX);
            line
        })
        .collect();
    let orchestrated = sources.iter().any(|source| !source.target_ids.is_empty());
    TargetLines {
        version: version(&lines, orchestrated),
        orchestrated,
        lines,
    }
}
