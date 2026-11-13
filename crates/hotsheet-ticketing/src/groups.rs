//! Ticket groups (HS2-PDSHNN, HS2-8CDFKR, HS2-9KZSD4): the derived group directory, the
//! project group registry, name resolution, and the multi-ticket rename, ungroup, merge,
//! and migration workflows.
//!
//! A ticket names its group by **id**. The project's registry (one file per group under
//! the code checkout, see [`registry`]) maps each id to a display name. A group is
//! listed while at least one ticket outside Trash names it; a registry entry with no
//! members is not a visible group, and nothing is deleted automatically.
//!
//! Names are a property of a *project*. A ticket store linked by several projects gets
//! its names from each project's registry; a store or connection addressed with no code
//! checkout has no registry, so it lists ids under placeholders and accepts only ids.
//! Every surface (server, CLI, MCP) calls this module so they agree.

pub mod git_sync;
pub mod registry;

use std::collections::BTreeMap;

use hotsheet_model::{
    GroupNameError, GroupValue, Status, Ticket, Ulid, group_key, group_key_of_input,
    group_placeholder_name, group_value_key, is_group_id, normalize_group_name,
};
use serde::Serialize;

pub use registry::{GROUP_REGISTRY_DIR, GroupNames, GroupRegistry, GroupRegistryError};

use crate::store::{FsStore, StoreError};

/// One ticket's membership, as the directory sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    pub id: Ulid,
    /// The stored `group` value: an id, or a legacy name awaiting migration.
    pub value: String,
    /// Whether the ticket keeps its group alive. A ticket in Trash still carries its
    /// group (so restoring it puts it back) but does not count as a member.
    pub live: bool,
}

/// One group in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupSummary {
    /// The value member tickets store: the group id, or for an unmigrated group its
    /// legacy name (the earliest member's spelling).
    pub id: String,
    /// The display name: the registry's, a legacy group's own name, or a placeholder
    /// built from the id when this project's registry does not name it.
    pub name: String,
    /// Live members (outside Trash).
    pub count: usize,
    /// `false` when `name` is a placeholder: the registry has no entry for this id.
    pub named: bool,
    /// `true` for a pre-registry name value that the migration has not rewritten yet.
    pub legacy: bool,
    /// `true` when another listed named group has the same name key, as after two clones
    /// each created a group with that name. Both are listed; a merge folds them together.
    pub duplicate: bool,
}

/// Why a group operation was refused.
#[derive(Debug, thiserror::Error)]
pub enum GroupError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Registry(#[from] GroupRegistryError),
    #[error(transparent)]
    InvalidName(#[from] GroupNameError),
    #[error("a group needs a name")]
    EmptyName,
    #[error("no group named '{0}'")]
    UnknownGroup(String),
    #[error("a group named '{0}' already exists")]
    NameTaken(String),
    #[error("several groups are named '{0}'; use a group id")]
    Ambiguous(String),
    #[error("group names need a project checkout with a group registry; '{0}' is not a group id")]
    RegistryRequired(String),
    #[error("{0}")]
    InvalidMerge(&'static str),
}

/// Every grouped ticket of one namespace: a store, or the stores a project connects.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupDirectory {
    members: Vec<GroupMember>,
}

impl GroupDirectory {
    /// Build from memberships gathered elsewhere (the index). Order does not matter.
    pub fn new(members: impl IntoIterator<Item = GroupMember>) -> Self {
        let mut members: Vec<_> = members.into_iter().collect();
        members.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.value.cmp(&b.value)));
        Self { members }
    }

    /// Build from loaded tickets. Moved tombstones are redirects, never members.
    pub fn from_tickets<'a>(tickets: impl IntoIterator<Item = &'a Ticket>) -> Self {
        Self::new(tickets.into_iter().filter_map(|ticket| {
            let value = ticket.group.clone()?;
            (ticket.status != Status::Moved).then(|| GroupMember {
                id: ticket.id,
                value,
                live: ticket.status != Status::Deleted,
            })
        }))
    }

    /// Build by reading every ticket file of `store`, skipping unparseable ones.
    pub fn scan(store: &FsStore) -> Result<Self, StoreError> {
        Ok(Self::from_tickets(&store.list_tickets_resilient()?.tickets))
    }

    /// Build over several stores of one project.
    pub fn scan_all<'a>(stores: impl IntoIterator<Item = &'a FsStore>) -> Result<Self, StoreError> {
        let mut directory = Self::default();
        for store in stores {
            directory.merge(Self::scan(store)?);
        }
        Ok(directory)
    }

    /// Fold another store's memberships into this namespace.
    pub fn merge(&mut self, other: GroupDirectory) {
        let mut members = std::mem::take(&mut self.members);
        members.extend(other.members);
        *self = Self::new(members);
    }

    pub fn members(&self) -> &[GroupMember] {
        &self.members
    }

    /// Every ticket whose value has `key` (see [`group_value_key`]), including tickets
    /// in Trash.
    pub fn member_ids(&self, key: &str) -> Vec<Ulid> {
        self.members
            .iter()
            .filter(|member| group_value_key(&member.value) == key)
            .map(|member| member.id)
            .collect()
    }

    /// Whether any ticket still holds a legacy name value.
    pub fn has_legacy(&self) -> bool {
        self.members
            .iter()
            .any(|member| matches!(GroupValue::of(&member.value), GroupValue::Legacy(_)))
    }

    /// The groups with their live member counts and display names from `names` (a
    /// project registry snapshot; `None` for a store with no project). Sorted by the
    /// display name's key, then by id.
    pub fn summaries(&self, names: Option<&GroupNames>) -> Vec<GroupSummary> {
        let mut groups = BTreeMap::<String, GroupSummary>::new();
        for member in self.members.iter().filter(|member| member.live) {
            groups
                .entry(group_value_key(&member.value))
                .or_insert_with(|| summary_of(&member.value, names))
                .count += 1;
        }
        let mut summaries: Vec<_> = groups.into_values().collect();
        let mut by_key = BTreeMap::<String, usize>::new();
        for summary in summaries.iter().filter(|summary| summary.named) {
            *by_key.entry(group_key(&summary.name)).or_default() += 1;
        }
        for summary in &mut summaries {
            summary.duplicate = summary.named
                && by_key
                    .get(&group_key(&summary.name))
                    .is_some_and(|count| *count > 1);
        }
        summaries.sort_by(|a, b| {
            group_key(&a.name)
                .cmp(&group_key(&b.name))
                .then_with(|| a.name.cmp(&b.name))
                .then_with(|| a.id.cmp(&b.id))
        });
        summaries
    }
}

fn summary_of(value: &str, names: Option<&GroupNames>) -> GroupSummary {
    let (name, named, legacy) = match GroupValue::of(value) {
        GroupValue::Id(id) => match names.and_then(|names| names.get(id)) {
            Some(name) => (name.clone(), true, false),
            None => (group_placeholder_name(id), false, false),
        },
        GroupValue::Legacy(name) => (name.to_string(), true, true),
    };
    GroupSummary {
        id: value.to_string(),
        name,
        count: 0,
        named,
        legacy,
        duplicate: false,
    }
}

/// The group a request names, found among the listed groups.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupTarget {
    /// The value key members carry ([`group_value_key`]).
    pub key: String,
    pub summary: GroupSummary,
}

/// Find the listed group `input` names: its id (or a legacy group's stored value), or
/// its display name compared without case or spacing.
///
/// # Errors
/// [`GroupError::UnknownGroup`] when no listed group matches;
/// [`GroupError::Ambiguous`] when a name matches several groups (duplicates).
pub fn find_group(
    directory: &GroupDirectory,
    names: Option<&GroupNames>,
    input: &str,
) -> Result<GroupTarget, GroupError> {
    let summaries = directory.summaries(names);
    let trimmed = input.trim();
    let target = |summary: &GroupSummary| GroupTarget {
        key: group_value_key(&summary.id),
        summary: summary.clone(),
    };
    if is_group_id(trimmed)
        && let Some(summary) = summaries.iter().find(|summary| summary.id == trimmed)
    {
        return Ok(target(summary));
    }
    let unknown = || GroupError::UnknownGroup(trimmed.to_string());
    let key = group_key_of_input(input).ok_or_else(unknown)?;
    let mut matches = summaries
        .iter()
        .filter(|summary| group_key(&summary.name) == key);
    let first = matches.next().ok_or_else(unknown)?;
    if matches.next().is_some() {
        return Err(GroupError::Ambiguous(first.name.clone()));
    }
    Ok(target(first))
}

/// Resolve a requested group for a ticket write to the value to store. An id is used as
/// that id. Any other text is a name: with a project registry it resolves to the group
/// with that name, creating a registry entry when none exists; without one it is
/// refused, because a store alone cannot name groups. `Ok(None)` clears the group.
pub fn resolve_group_input(
    registry: Option<&GroupRegistry>,
    input: &str,
) -> Result<Option<String>, GroupError> {
    let Some(name) = normalize_group_name(input)? else {
        return Ok(None);
    };
    if is_group_id(&name) {
        return Ok(Some(name));
    }
    let registry = registry.ok_or_else(|| GroupError::RegistryRequired(name.clone()))?;
    Ok(Some(registry.find_or_create(&name)?.0))
}

/// [`resolve_group_input`] over a patch field: absent stays absent and a clear stays a
/// clear.
pub fn resolve_group_change(
    registry: Option<&GroupRegistry>,
    change: Option<Option<String>>,
) -> Result<Option<Option<String>>, GroupError> {
    match change {
        Some(Some(input)) => resolve_group_input(registry, &input).map(Some),
        other => Ok(other),
    }
}

impl GroupError {
    /// The store-level error for a refused ticket-write value, for callers whose error
    /// type is [`StoreError`] (providers and `ops`).
    pub fn into_store_error(self) -> StoreError {
        match self {
            Self::Store(error) => error,
            Self::InvalidName(error) => StoreError::InvalidGroupName(error),
            Self::RegistryRequired(name) => StoreError::GroupRegistryRequired(name),
            other => StoreError::Io(std::io::Error::other(other.to_string())),
        }
    }
}

/// The tickets one store rewrote for a group mutation.
#[derive(Debug, Clone, Default)]
pub struct GroupWrite {
    /// Every rewritten ticket, in id order, for index and change notification.
    pub tickets: Vec<Ticket>,
    /// The store's live members that the mutation matched.
    pub updated: usize,
}

/// Rewrite the group of `candidates` in `store` as one mutation and one commit.
/// `map` receives each ticket's stored value and returns `None` when the ticket is not
/// affected, or the value to store (`Some(None)` ungroups). Each candidate is re-read
/// under the store lock, so a stale directory never rewrites the wrong ticket.
/// `updated_at` is preserved: regrouping is organization, not ticket content.
pub fn rewrite_members(
    store: &FsStore,
    candidates: &[Ulid],
    map: impl Fn(&str) -> Option<Option<String>>,
    message: &str,
) -> Result<GroupWrite, StoreError> {
    store.with_ticket_transaction(|| {
        let mut changes = Vec::new();
        let mut updated = 0;
        for id in candidates {
            let before = match store.read_ticket(id) {
                Ok(ticket) => ticket,
                // The directory may be a moment stale, or span several stores.
                Err(error) if error.is_io_kind(std::io::ErrorKind::NotFound) => continue,
                Err(error) => return Err(error),
            };
            if before.status == Status::Moved {
                continue;
            }
            let Some(to) = before.group.as_deref().and_then(&map) else {
                continue;
            };
            updated += usize::from(before.status != Status::Deleted);
            if before.group == to {
                continue;
            }
            let mut after = before.clone();
            after.group = to;
            changes.push((before, after));
        }
        store.write_ticket_changes_committing(&changes, message)?;
        Ok(GroupWrite {
            tickets: changes.into_iter().map(|(_, after)| after).collect(),
            updated,
        })
    })
}

/// Move every member of the group with value key `from_key` in `store` to `to` (`None`
/// ungroups).
pub fn set_group_of_members(
    store: &FsStore,
    directory: &GroupDirectory,
    from_key: &str,
    to: Option<&str>,
    message: &str,
) -> Result<GroupWrite, StoreError> {
    rewrite_members(
        store,
        &directory.member_ids(from_key),
        |value| (group_value_key(value) == from_key).then(|| to.map(str::to_string)),
        message,
    )
}

/// The outcome of a group workflow over the stores of one namespace.
#[derive(Debug, Clone, Default)]
pub struct GroupOutcome {
    /// The group the members now belong to (rename, merge).
    pub id: Option<String>,
    /// Its display name (rename, merge).
    pub name: Option<String>,
    /// Live members matched across every store.
    pub updated: usize,
    /// One write per store, in the order the stores were given.
    pub writes: Vec<GroupWrite>,
}

fn rewrite_everywhere(
    stores: &[&FsStore],
    directory: &GroupDirectory,
    from_key: &str,
    to: Option<&str>,
    message: &str,
) -> Result<(usize, Vec<GroupWrite>), GroupError> {
    let mut updated = 0;
    let mut writes = Vec::with_capacity(stores.len());
    for store in stores {
        let write = set_group_of_members(store, directory, from_key, to, message)?;
        updated += write.updated;
        writes.push(write);
    }
    Ok((updated, writes))
}

fn registry_names(registry: Option<&GroupRegistry>) -> Result<Option<GroupNames>, GroupError> {
    registry
        .map(GroupRegistry::names)
        .transpose()
        .map_err(Into::into)
}

/// The listed groups of a namespace, named by `registry` when the namespace is a project.
pub fn list_groups(
    directory: &GroupDirectory,
    registry: Option<&GroupRegistry>,
) -> Result<Vec<GroupSummary>, GroupError> {
    Ok(directory.summaries(registry_names(registry)?.as_ref()))
}

/// Create (or find, by name key) the registry group `name` without assigning tickets.
/// Returns the id, the stored display name, and whether an entry was created.
pub fn create_group(
    registry: Option<&GroupRegistry>,
    name: &str,
) -> Result<(String, String, bool), GroupError> {
    let registry = registry.ok_or_else(|| GroupError::RegistryRequired(name.trim().into()))?;
    if normalize_group_name(name)?.is_none() {
        return Err(GroupError::EmptyName);
    }
    Ok(registry.find_or_create(name)?)
}

/// Rename the group `group` (an id or a name) to `to`. For an id this is one registry
/// edit and no ticket changes, and it also names a placeholder. A legacy group is given
/// a registry entry and its members are rewritten to that id.
///
/// # Errors
/// [`GroupError::RegistryRequired`] without a project registry;
/// [`GroupError::NameTaken`] when `to` is a different listed group's name (a change of
/// only letter case or spacing of the same group is allowed); the lookup errors of
/// [`find_group`]; a name error when `to` is invalid or empty.
pub fn rename_group(
    stores: &[&FsStore],
    directory: &GroupDirectory,
    registry: Option<&GroupRegistry>,
    group: &str,
    to: &str,
) -> Result<GroupOutcome, GroupError> {
    let registry = registry.ok_or_else(|| GroupError::RegistryRequired(group.trim().into()))?;
    let name = normalize_group_name(to)?.ok_or(GroupError::EmptyName)?;
    let names = registry.names()?;
    let target = find_group(directory, Some(&names), group)?;
    let to_key = group_key(&name);
    if let Some(taken) = directory
        .summaries(Some(&names))
        .into_iter()
        .find(|summary| {
            summary.named && summary.id != target.summary.id && group_key(&summary.name) == to_key
        })
    {
        return Err(GroupError::NameTaken(taken.name));
    }
    if !target.summary.legacy {
        let name = registry.set_name(&target.summary.id, &name)?;
        return Ok(GroupOutcome {
            id: Some(target.summary.id),
            name: Some(name),
            updated: 0,
            writes: stores.iter().map(|_| GroupWrite::default()).collect(),
        });
    }
    // A legacy group becomes a registry group: reuse an unlisted entry with the new
    // name's key, else create one, then move the members onto its id.
    let (id, _, _) = registry.find_or_create(&name)?;
    let name = registry.set_name(&id, &name)?;
    let (updated, writes) = rewrite_everywhere(
        stores,
        directory,
        &target.key,
        Some(&id),
        &format!("Rename ticket group to {name}"),
    )?;
    Ok(GroupOutcome {
        id: Some(id),
        name: Some(name),
        updated,
        writes,
    })
}

/// Clear the group `group` (an id or a name) from every member. The registry entry is
/// kept, so the name returns if the id is used again.
pub fn ungroup(
    stores: &[&FsStore],
    directory: &GroupDirectory,
    registry: Option<&GroupRegistry>,
    group: &str,
) -> Result<GroupOutcome, GroupError> {
    let names = registry_names(registry)?;
    let target = find_group(directory, names.as_ref(), group)?;
    let (updated, writes) =
        rewrite_everywhere(stores, directory, &target.key, None, "Ungroup tickets")?;
    Ok(GroupOutcome {
        id: None,
        name: None,
        updated,
        writes,
    })
}

/// Move every member of `from` into `into` (each an id or a name), as after two clones
/// created the same name. Neither registry entry is deleted.
///
/// # Errors
/// [`GroupError::InvalidMerge`] when both name one group or `into` is a legacy name
/// (migrate first); the lookup errors of [`find_group`].
pub fn merge_groups(
    stores: &[&FsStore],
    directory: &GroupDirectory,
    registry: Option<&GroupRegistry>,
    from: &str,
    into: &str,
) -> Result<GroupOutcome, GroupError> {
    let names = registry_names(registry)?;
    let from = find_group(directory, names.as_ref(), from)?;
    let into = find_group(directory, names.as_ref(), into)?;
    if from.key == into.key {
        return Err(GroupError::InvalidMerge(
            "a group cannot be merged into itself",
        ));
    }
    if into.summary.legacy {
        return Err(GroupError::InvalidMerge(
            "migrate legacy group names before merging into one",
        ));
    }
    let (updated, writes) = rewrite_everywhere(
        stores,
        directory,
        &from.key,
        Some(&into.summary.id),
        &format!("Merge ticket group into {}", into.summary.name),
    )?;
    Ok(GroupOutcome {
        id: Some(into.summary.id),
        name: Some(into.summary.name),
        updated,
        writes,
    })
}

/// The outcome of the group migration.
#[derive(Debug, Clone, Default)]
pub struct GroupMigration {
    /// Registry entries created for legacy names.
    pub created: usize,
    /// Legacy names that reused an existing registry entry with the same key.
    pub reused: usize,
    /// Tickets rewritten, including tickets in Trash.
    pub updated: usize,
    /// One write per store, in the order the stores were given.
    pub writes: Vec<GroupWrite>,
}

/// The explicit, idempotent migration from legacy name values to registry ids
/// (HS2-9KZSD4, docs/19 §19.1). Each distinct legacy name key gets one registry entry
/// (an existing entry with that key is reused, the lowest id when several exist; a new
/// entry takes the earliest-created member's spelling), and every ticket naming it,
/// including tickets in Trash, is rewritten to the id in one write and one commit per
/// store. Running it again finds no legacy value and changes nothing.
pub fn migrate_groups(
    stores: &[&FsStore],
    directory: &GroupDirectory,
    registry: &GroupRegistry,
) -> Result<GroupMigration, GroupError> {
    // The directory is sorted by ticket id, and ticket ids are time-ordered, so the first
    // spelling seen for a key is the earliest-created member's.
    let mut spellings = BTreeMap::<String, String>::new();
    let mut candidates = Vec::new();
    for member in directory.members() {
        if let GroupValue::Legacy(name) = GroupValue::of(&member.value) {
            spellings
                .entry(group_key(name))
                .or_insert_with(|| name.to_string());
            candidates.push(member.id);
        }
    }
    let mut migration = GroupMigration::default();
    if spellings.is_empty() {
        migration.writes = stores.iter().map(|_| GroupWrite::default()).collect();
        return Ok(migration);
    }
    let (assignments, created, reused) = registry.assign_ids(&spellings)?;
    migration.created = created;
    migration.reused = reused;
    for store in stores {
        let write = rewrite_members(
            store,
            &candidates,
            |value| match GroupValue::of(value) {
                GroupValue::Legacy(name) => assignments.get(&group_key(name)).cloned().map(Some),
                GroupValue::Id(_) => None,
            },
            "Migrate ticket groups to registry ids",
        )?;
        migration.updated += write.tickets.len();
        migration.writes.push(write);
    }
    Ok(migration)
}

#[cfg(test)]
mod tests;
