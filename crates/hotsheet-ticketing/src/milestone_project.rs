//! Milestones across one project's ticket sources (HS2-A820C1, docs/38 §38.2, §38.4,
//! §38.6): the list and show with roll-ups, ticket link resolution, the `milestone:` and
//! `--due-before` filters, Delete Milestone… (clears every member first) and Move Open
//! Tickets to….
//!
//! The server, the CLI and the serverless MCP backend call these so they agree. The server
//! gathers members from its store indexes; the CLI and MCP scan the stores' files.

use std::collections::{BTreeMap, HashSet};

use hotsheet_model::{Status, Ticket};
use serde::Serialize;

use crate::groups::git_sync::RegistrySyncStatus;
use crate::milestone_rollup::{
    MilestoneRollup, RollupContext, RollupThresholds, effective_due_on, lends_due_date,
    milestone_rollup,
};
use crate::milestones::{Milestone, MilestoneError, MilestoneRegistry, MilestoneState};
use crate::project_time_zone::ProjectTimeZone;
use crate::wire::TicketRow;
use crate::{FsStore, StoreError};

/// Every ticket naming a milestone across the reading project's sources, with what the
/// roll-up needs besides them.
#[derive(Debug, Clone, Default)]
pub struct ProjectMembers {
    /// Each ticket that names a milestone, in any status, with its source's connection id.
    pub members: Vec<TicketRow>,
    /// Ids of every unfinished ticket in those sources (for the `blocked` signal).
    pub unfinished_ids: HashSet<String>,
    /// Sources that could not be read, by connection id.
    pub unavailable: Vec<String>,
}

/// Whether a ticket in this status is unfinished (open work, or Backlog).
fn unfinished(status: Status) -> bool {
    matches!(
        status,
        Status::NotStarted | Status::Started | Status::Backlog
    )
}

impl ProjectMembers {
    /// Add one source's member rows (re-tagged with `connection_id`) and unfinished ids.
    pub fn add_rows(
        &mut self,
        connection_id: &str,
        members: impl IntoIterator<Item = TicketRow>,
        unfinished_ids: impl IntoIterator<Item = String>,
    ) {
        for mut row in members {
            row.connection_id = connection_id.to_string();
            row.native_id.clone_from(&row.id);
            row.qualified_id = format!("{connection_id}:{}", row.id);
            self.members.push(row);
        }
        self.unfinished_ids.extend(unfinished_ids);
    }

    /// Add one source read from its files.
    pub fn add_tickets(&mut self, connection_id: &str, tickets: &[Ticket]) {
        self.add_rows(
            connection_id,
            tickets
                .iter()
                .filter(|ticket| ticket.milestone.is_some())
                .map(TicketRow::from),
            tickets
                .iter()
                .filter(|ticket| unfinished(ticket.status))
                .map(|ticket| ticket.id.to_string()),
        );
    }

    /// Read every store's files. A store that cannot be read is named in `unavailable`.
    pub fn scan(stores: &[&FsStore]) -> Self {
        let mut members = Self::default();
        for store in stores {
            let connection_id = crate::git_connection_id(store);
            match store.list_tickets() {
                Ok(tickets) => members.add_tickets(&connection_id, &tickets),
                Err(_) => members.unavailable.push(connection_id),
            }
        }
        members
    }

    /// The members naming `id`.
    pub fn of(&self, id: &str) -> Vec<TicketRow> {
        self.members
            .iter()
            .filter(|row| row.milestone.as_deref() == Some(id))
            .cloned()
            .collect()
    }
}

/// One milestone with its roll-up, as every list and show returns it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MilestoneSummary {
    #[serde(flatten)]
    pub milestone: Milestone,
    pub rollup: MilestoneRollup,
}

/// A project's milestone list (`GET /checkouts/{reference}/milestones`, `hotsheet-cli
/// milestones --json`, MCP `hotsheet_milestones` `list`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MilestoneListing {
    /// Whether a project registry was read. Without a checkout there is none, and only the
    /// ids tickets name are listed, as unknown placeholders.
    pub registry: bool,
    /// The project time zone in effect for due dates (docs/38 §38.3).
    pub time_zone: String,
    pub milestones: Vec<MilestoneSummary>,
    /// How the registry was last committed and published; absent without a registry or
    /// before the first attempt.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub registry_sync: Option<RegistrySyncStatus>,
}

/// What a roll-up reads: the project zone, the time, and (until HS2-DXZRHA) no red gates.
#[derive(Debug, Clone)]
pub struct RollupClock {
    pub zone: ProjectTimeZone,
    pub now_ms: i64,
    pub thresholds: RollupThresholds,
}

impl RollupClock {
    /// The project zone at the current instant, with the default thresholds.
    pub fn now(zone: ProjectTimeZone) -> Self {
        Self {
            zone,
            now_ms: jiff::Timestamp::now().as_millisecond(),
            thresholds: RollupThresholds::default(),
        }
    }
}

fn summarize(
    milestone: Milestone,
    members: &ProjectMembers,
    clock: &RollupClock,
) -> MilestoneSummary {
    let red_gates = HashSet::new();
    let context = RollupContext {
        now_ms: clock.now_ms,
        zone: &clock.zone.zone,
        thresholds: clock.thresholds,
        unfinished_ids: &members.unfinished_ids,
        red_gates: &red_gates,
    };
    let rows = members.of(&milestone.id);
    let rollup = milestone_rollup(&milestone, &rows, &members.unavailable, &context);
    MilestoneSummary { milestone, rollup }
}

/// List the project's milestones with roll-ups: open, conflicted and unknown ones, and
/// with `all` closed ones too. An id that tickets name but the registry does not is listed
/// as an unknown placeholder (never dropped); without a registry every id is.
pub fn listing(
    registry: Option<&MilestoneRegistry>,
    all: bool,
    members: &ProjectMembers,
    clock: &RollupClock,
) -> Result<MilestoneListing, MilestoneError> {
    let mut milestones = match registry {
        Some(registry) => registry.list()?,
        None => Vec::new(),
    };
    let known: HashSet<String> = milestones
        .iter()
        .map(|milestone| milestone.id.clone())
        .collect();
    let unknown: BTreeMap<String, ()> = members
        .members
        .iter()
        .filter(|row| row.status.as_deref() != Some("moved"))
        .filter_map(|row| row.milestone.clone())
        .filter(|id| !known.contains(id))
        .map(|id| (id, ()))
        .collect();
    milestones.extend(unknown.into_keys().map(|id| Milestone::unknown(&id)));
    Ok(MilestoneListing {
        registry: registry.is_some(),
        time_zone: clock.zone.name.clone(),
        milestones: milestones
            .into_iter()
            .filter(|milestone| all || milestone.state != MilestoneState::Closed)
            .map(|milestone| summarize(milestone, members, clock))
            .collect(),
        registry_sync: registry.and_then(MilestoneRegistry::sync_status),
    })
}

/// The milestone `input` names, for reading: a registry id or title, else an id that
/// tickets name (an unknown placeholder). Without a registry only ids resolve.
pub fn find(
    registry: Option<&MilestoneRegistry>,
    members: &ProjectMembers,
    input: &str,
) -> Result<Milestone, MilestoneError> {
    let input = input.trim();
    if let Some(registry) = registry {
        match registry.resolve(input) {
            Err(MilestoneError::NotFound(_)) => {}
            other => return other,
        }
    }
    if members
        .members
        .iter()
        .any(|row| row.milestone.as_deref() == Some(input))
    {
        return Ok(Milestone::unknown(input));
    }
    // Without a registry only ids that tickets name can be read; say why the rest cannot.
    Err(match registry {
        Some(_) => MilestoneError::NotFound(input.to_string()),
        None => MilestoneError::RegistryRequired,
    })
}

/// One milestone with its roll-up (`milestone show`).
pub fn show(
    registry: Option<&MilestoneRegistry>,
    members: &ProjectMembers,
    input: &str,
    clock: &RollupClock,
) -> Result<MilestoneSummary, MilestoneError> {
    Ok(summarize(find(registry, members, input)?, members, clock))
}

/// A registry write's result with the milestone's roll-up (create, edit, close, reopen on
/// every surface).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct MilestoneWriteResult {
    pub milestone: MilestoneSummary,
    /// Whether anything was written and committed.
    pub changed: bool,
    /// The members still open: Close Milestone… lists them and offers Move Open Tickets
    /// to… (docs/38 §38.4).
    pub open_members: Vec<crate::milestone_rollup::MemberRef>,
}

impl MilestoneWriteResult {
    /// The roll-up of `write`'s milestone over `members`.
    pub fn of(
        write: crate::milestones::MilestoneWrite,
        members: &ProjectMembers,
        clock: &RollupClock,
    ) -> Self {
        let summary = summarize(write.milestone, members, clock);
        Self {
            open_members: summary.rollup.open_members.clone(),
            milestone: summary,
            changed: write.changed,
        }
    }
}

/// Resolve the milestone a ticket write names (docs/38 §38.6 "Link / unlink a ticket").
/// With a registry: an id or a title of a listed milestone; an unknown title is refused
/// (`milestone_not_found`, a link never creates a milestone) and a duplicated title needs
/// the id (`milestone_ambiguous`). Without one: only an id-shaped value
/// (`milestone_registry_required`). `Some(None)` and absent pass through.
pub fn resolve_link(
    registry: Option<&MilestoneRegistry>,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, MilestoneError> {
    let Some(Some(input)) = change else {
        return Ok(change);
    };
    let input = input.trim();
    if input.is_empty() {
        return Ok(Some(None));
    }
    match registry {
        Some(registry) => Ok(Some(Some(registry.resolve(input)?.id))),
        None if hotsheet_model::is_milestone_id(input) => Ok(Some(Some(input.to_string()))),
        None => Err(MilestoneError::RegistryRequired),
    }
}

/// The ids a `milestone:` / `--milestone` filter matches: the registry milestone with that
/// id or title, or an id-shaped value as given (an unknown id still finds its tickets).
/// A title that names nothing matches nothing.
pub fn filter_ids(registry: Option<&MilestoneRegistry>, input: &str) -> Vec<String> {
    let input = input.trim();
    let mut ids = Vec::new();
    if let Some(registry) = registry
        && let Ok(milestones) = registry.list()
    {
        let key = crate::milestones::milestone_title_key(input);
        ids.extend(
            milestones
                .into_iter()
                .filter(|milestone| {
                    milestone.id == input
                        || (milestone.state != MilestoneState::Conflicted
                            && crate::milestones::milestone_title_key(&milestone.title) == key)
                })
                .map(|milestone| milestone.id),
        );
    }
    if hotsheet_model::is_milestone_id(input) && !ids.iter().any(|id| id == input) {
        ids.push(input.to_string());
    }
    ids
}

/// The open milestones whose own date is on or before `date`, so `due_before` also finds
/// their tickets through the milestone's date (docs/38 §38.3 effective due).
pub fn due_milestone_ids(registry: Option<&MilestoneRegistry>, date: &str) -> Vec<String> {
    let Some(milestones) = registry.and_then(|registry| registry.list().ok()) else {
        return Vec::new();
    };
    milestones
        .into_iter()
        .filter(|milestone| {
            lends_due_date(milestone)
                && effective_due_on(None, Some(milestone)).is_some_and(|due| due <= date)
        })
        .map(|milestone| milestone.id)
        .collect()
}

/// Why a project milestone workflow failed.
#[derive(Debug, thiserror::Error)]
pub enum MilestoneOpError {
    #[error(transparent)]
    Milestone(#[from] MilestoneError),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// One store's rewritten tickets, in the order the stores were given.
#[derive(Debug, Clone, Default)]
pub struct MemberWrite {
    pub tickets: Vec<Ticket>,
}

/// Set the milestone of every ticket in `store` that names `from` (skipping `moved`
/// tombstones, and with `open_only` skipping finished and dropped tickets) to `to`, as one
/// mutation and one commit. `updated_at` is preserved: a link is organization.
pub fn set_milestone_of_members(
    store: &FsStore,
    from: &str,
    to: Option<&str>,
    open_only: bool,
    message: &str,
) -> Result<MemberWrite, StoreError> {
    store.with_ticket_transaction(|| {
        let mut changes = Vec::new();
        for before in store.list_tickets()? {
            if before.milestone.as_deref() != Some(from) || before.status == Status::Moved {
                continue;
            }
            let dropped = matches!(
                before.close_reason,
                Some(
                    hotsheet_model::CloseReason::NotPlanned
                        | hotsheet_model::CloseReason::Duplicate
                        | hotsheet_model::CloseReason::Obsolete
                        | hotsheet_model::CloseReason::WorksAsDesigned
                )
            );
            if open_only && (!unfinished(before.status) || dropped) {
                continue;
            }
            let mut after = before.clone();
            after.milestone = to.map(str::to_string);
            if after != before {
                changes.push((before, after));
            }
        }
        changes.sort_by_key(|(before, _)| before.id);
        store.write_ticket_changes_committing(&changes, message)?;
        Ok(MemberWrite {
            tickets: changes.into_iter().map(|(_, after)| after).collect(),
        })
    })
}

/// The outcome of Delete Milestone… or Move Open Tickets to….
#[derive(Debug, Clone, Serialize)]
pub struct MemberOutcome {
    /// The milestone deleted, or moved from.
    pub milestone: Milestone,
    /// The milestone moved to (move-open only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Milestone>,
    /// How many tickets were rewritten across every store.
    pub updated: usize,
    /// Whether anything was written (a tickets rewrite or the registry file).
    pub changed: bool,
    /// Per-store writes, for index updates and change events (not serialized).
    #[serde(skip)]
    pub writes: Vec<MemberWrite>,
}

/// Delete Milestone… (docs/38 §38.2, §38.6): clear the link from every member in every
/// store first, then delete the registry file. A conflicted file is refused before any
/// ticket changes. An unknown id (no file) only clears its members. A failed member write
/// deletes nothing, so the delete can be repeated.
pub fn delete_milestone(
    registry: Option<&MilestoneRegistry>,
    stores: &[&FsStore],
    members: &ProjectMembers,
    input: &str,
) -> Result<MemberOutcome, MilestoneOpError> {
    let registry = registry.ok_or(MilestoneError::RegistryRequired)?;
    let milestone = find(Some(registry), members, input)?;
    if let Some(conflict) = &milestone.conflict {
        return Err(MilestoneError::Conflict {
            file: conflict.file.clone(),
            detail: conflict.detail.clone(),
        }
        .into());
    }
    let message = format!("Clear milestone {} from its tickets", milestone.id);
    let mut writes = Vec::with_capacity(stores.len());
    for store in stores {
        writes.push(set_milestone_of_members(
            store,
            &milestone.id,
            None,
            false,
            &message,
        )?);
    }
    let removed = if milestone.named {
        registry.remove(&milestone.id)?.is_some()
    } else {
        false
    };
    let updated = writes.iter().map(|write| write.tickets.len()).sum();
    Ok(MemberOutcome {
        milestone,
        to: None,
        updated,
        changed: removed || updated > 0,
        writes,
    })
}

/// Move Open Tickets to… (docs/38 §38.4): relink every unfinished member of `from` to `to`,
/// one write per store. Finished and dropped members stay. Moving to itself changes nothing.
pub fn move_open_members(
    registry: Option<&MilestoneRegistry>,
    stores: &[&FsStore],
    members: &ProjectMembers,
    from: &str,
    to: &str,
) -> Result<MemberOutcome, MilestoneOpError> {
    let registry = registry.ok_or(MilestoneError::RegistryRequired)?;
    let source = find(Some(registry), members, from)?;
    let target = registry.resolve(to)?;
    let mut writes = Vec::with_capacity(stores.len());
    if source.id != target.id {
        let message = format!(
            "Move open tickets from milestone {} to {}",
            source.id, target.id
        );
        for store in stores {
            writes.push(set_milestone_of_members(
                store,
                &source.id,
                Some(&target.id),
                true,
                &message,
            )?);
        }
    }
    let updated: usize = writes.iter().map(|write| write.tickets.len()).sum();
    Ok(MemberOutcome {
        milestone: source,
        to: Some(target),
        updated,
        changed: updated > 0,
        writes,
    })
}

#[cfg(test)]
#[path = "milestone_project_tests.rs"]
mod tests;
