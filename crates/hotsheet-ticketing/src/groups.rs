//! Ticket groups (HS2-PDSHNN, HS2-8CDFKR): the derived group directory, canonical
//! spelling reuse, and the multi-ticket rename and ungroup mutations.
//!
//! A group has no record of its own: it exists while at least one non-deleted ticket
//! names it. Names are unique within a namespace by [`hotsheet_model::group_key`], so
//! every surface (server, CLI, MCP) resolves a requested name through this module
//! before writing it.

use std::collections::BTreeMap;

use hotsheet_model::{
    GroupNameError, Status, Ticket, Timestamp, Ulid, group_key, group_key_of_input,
    normalize_group_name,
};
use serde::Serialize;

use crate::store::{FsStore, StoreError};

/// One ticket's membership, as the directory sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupMember {
    pub id: Ulid,
    /// The display name stored on the ticket.
    pub name: String,
    /// Whether the ticket keeps its group alive. A ticket in Trash still carries its
    /// group (so restoring it puts it back) but does not count as a member.
    pub live: bool,
}

/// One group in a listing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GroupSummary {
    pub name: String,
    pub count: usize,
}

/// Why a group mutation was refused.
#[derive(Debug, thiserror::Error)]
pub enum GroupError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    InvalidName(#[from] GroupNameError),
    #[error("a group needs a name")]
    EmptyName,
    #[error("no group named '{0}'")]
    UnknownGroup(String),
    #[error("a group named '{0}' already exists")]
    NameTaken(String),
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
        members.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.name.cmp(&b.name)));
        Self { members }
    }

    /// Build from loaded tickets. Moved tombstones are redirects, never members.
    pub fn from_tickets<'a>(tickets: impl IntoIterator<Item = &'a Ticket>) -> Self {
        Self::new(tickets.into_iter().filter_map(|ticket| {
            let name = ticket.group.clone()?;
            (ticket.status != Status::Moved).then(|| GroupMember {
                id: ticket.id,
                name,
                live: ticket.status != Status::Deleted,
            })
        }))
    }

    /// Build by reading every ticket file of `store`, skipping unparseable ones.
    pub fn scan(store: &FsStore) -> Result<Self, StoreError> {
        Ok(Self::from_tickets(&store.list_tickets_resilient()?.tickets))
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

    /// The stored spelling of the group with `key`, ignoring the tickets in `excluding`
    /// (those being rewritten by the caller). Mixed spellings, which only a merge of two
    /// clones can produce, resolve to the earliest-created member's.
    pub fn canonical_name(&self, key: &str, excluding: &[Ulid]) -> Option<&str> {
        self.members
            .iter()
            .find(|member| {
                member.live && !excluding.contains(&member.id) && group_key(&member.name) == key
            })
            .map(|member| member.name.as_str())
    }

    /// Every ticket naming the group with `key`, including tickets in Trash.
    pub fn member_ids(&self, key: &str) -> Vec<Ulid> {
        self.members
            .iter()
            .filter(|member| group_key(&member.name) == key)
            .map(|member| member.id)
            .collect()
    }

    /// The groups with their live member counts, sorted case-insensitively by name.
    pub fn summaries(&self) -> Vec<GroupSummary> {
        let mut groups = BTreeMap::<String, GroupSummary>::new();
        for member in self.members.iter().filter(|member| member.live) {
            groups
                .entry(group_key(&member.name))
                .or_insert_with(|| GroupSummary {
                    name: member.name.clone(),
                    count: 0,
                })
                .count += 1;
        }
        let mut summaries: Vec<_> = groups.into_values().collect();
        summaries.sort_by(|a, b| {
            group_key(&a.name)
                .cmp(&group_key(&b.name))
                .then_with(|| a.name.cmp(&b.name))
        });
        summaries
    }
}

/// Resolve a requested group name for the tickets in `patched`: validate it, and reuse
/// the existing spelling when another ticket already names that group. `Ok(None)` means
/// the name is empty and the group should be cleared.
pub fn resolve_group_name(
    directory: &GroupDirectory,
    requested: &str,
    patched: &[Ulid],
) -> Result<Option<String>, GroupNameError> {
    let Some(name) = normalize_group_name(requested)? else {
        return Ok(None);
    };
    Ok(Some(
        directory
            .canonical_name(&group_key(&name), patched)
            .map_or(name, str::to_string),
    ))
}

/// [`resolve_group_name`] over a patch field: absent stays absent and a clear stays a
/// clear.
pub fn resolve_group_change(
    directory: &GroupDirectory,
    change: Option<Option<String>>,
    patched: &[Ulid],
) -> Result<Option<Option<String>>, GroupNameError> {
    match change {
        Some(Some(name)) => resolve_group_name(directory, &name, patched).map(Some),
        other => Ok(other),
    }
}

/// A validated rename, ready to apply to each store of the namespace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePlan {
    pub from_key: String,
    /// The display name every member will carry.
    pub name: String,
}

/// Validate renaming `from` to `to` against the whole namespace.
///
/// # Errors
/// [`GroupError::UnknownGroup`] when no live ticket names `from`;
/// [`GroupError::NameTaken`] when `to` is a different existing group (a change of only
/// letter case or spacing of the same group is allowed); a name error when `to` is
/// invalid or empty.
pub fn plan_rename(
    directory: &GroupDirectory,
    from: &str,
    to: &str,
) -> Result<RenamePlan, GroupError> {
    let name = normalize_group_name(to)?.ok_or(GroupError::EmptyName)?;
    let from_key = known_group_key(directory, from)?;
    let to_key = group_key(&name);
    if to_key != from_key
        && let Some(existing) = directory.canonical_name(&to_key, &[])
    {
        return Err(GroupError::NameTaken(existing.to_string()));
    }
    Ok(RenamePlan { from_key, name })
}

/// The key of an existing group, or [`GroupError::UnknownGroup`].
pub fn known_group_key(directory: &GroupDirectory, name: &str) -> Result<String, GroupError> {
    group_key_of_input(name)
        .filter(|key| directory.canonical_name(key, &[]).is_some())
        .ok_or_else(|| GroupError::UnknownGroup(name.trim().to_string()))
}

/// The tickets one store rewrote for a group mutation.
#[derive(Debug, Clone, Default)]
pub struct GroupWrite {
    /// Every rewritten ticket, in id order, for index and change notification.
    pub tickets: Vec<Ticket>,
    /// The store's live members of the group after the write: the number a client
    /// reports, matching the listing's count.
    pub updated: usize,
}

/// Set the group of every member of `from_key` in `store` to `to` (`None` ungroups) as
/// one mutation and one commit. `directory` supplies the candidate tickets; each is
/// re-read under the store lock and skipped when it no longer names the group.
/// `updated_at` is preserved: regrouping is organization, not ticket content.
pub fn set_group_of_members(
    store: &FsStore,
    directory: &GroupDirectory,
    from_key: &str,
    to: Option<&str>,
    _now: Timestamp,
) -> Result<GroupWrite, StoreError> {
    let candidates = directory.member_ids(from_key);
    store.with_ticket_transaction(|| {
        let mut changes = Vec::new();
        let mut updated = 0;
        for id in &candidates {
            let before = match store.read_ticket(id) {
                Ok(ticket) => ticket,
                // The directory may be a moment stale, or span several stores.
                Err(error) if error.is_io_kind(std::io::ErrorKind::NotFound) => continue,
                Err(error) => return Err(error),
            };
            if before.status == Status::Moved
                || before.group.as_deref().map(group_key).as_deref() != Some(from_key)
            {
                continue;
            }
            updated += usize::from(before.status != Status::Deleted);
            if before.group.as_deref() == to {
                continue;
            }
            let mut after = before.clone();
            after.group = to.map(str::to_string);
            changes.push((before, after));
        }
        let message = match to {
            Some(name) => format!("Rename ticket group to {name}"),
            None => "Ungroup tickets".to_string(),
        };
        store.write_ticket_changes_committing(&changes, &message)?;
        Ok(GroupWrite {
            tickets: changes.into_iter().map(|(_, after)| after).collect(),
            updated,
        })
    })
}

/// The outcome of a single-store rename.
#[derive(Debug, Clone)]
pub struct GroupRename {
    pub name: String,
    pub write: GroupWrite,
}

/// Rename a group within one store (the headless path; the server applies one
/// [`plan_rename`] to each store of a project).
pub fn rename_group(
    store: &FsStore,
    from: &str,
    to: &str,
    now: Timestamp,
) -> Result<GroupRename, GroupError> {
    let directory = GroupDirectory::scan(store)?;
    let plan = plan_rename(&directory, from, to)?;
    let write = set_group_of_members(store, &directory, &plan.from_key, Some(&plan.name), now)?;
    Ok(GroupRename {
        name: plan.name,
        write,
    })
}

/// Clear a group from every member within one store.
pub fn ungroup(store: &FsStore, name: &str, now: Timestamp) -> Result<GroupWrite, GroupError> {
    let directory = GroupDirectory::scan(store)?;
    let key = known_group_key(&directory, name)?;
    Ok(set_group_of_members(store, &directory, &key, None, now)?)
}

#[cfg(test)]
mod tests;
