//! Removing a registered checkout from the machine registry (HS2-X4RZ73).
//!
//! The registry is shared by every Hot Sheet client and server on the machine, so the server
//! route (`DELETE /checkouts/{reference}`) and `hotsheet-cli checkout remove` both call
//! [`remove_checkout`]. A removal drops only the registry entry and its retained source
//! aliases. It never deletes the checkout directory, a linked Git ticket store, a worklist
//! file, a permission vault, or an account credential. Checkout-to-source links are
//! many-to-many (HS2-SM9PM8): an external connection that another checkout still links is
//! kept for it, and one that no remaining checkout links is deleted from `providers.json`,
//! exactly as removing the project's last link with `detach_source` would.
//!
//! Removal is idempotent: a reference that names nothing registered reports
//! `registered: false` and changes nothing.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::checkouts::{Checkout, CheckoutRegistry};
use crate::connection_removal::{ConnectionRemovalError, credential_of};
use crate::provider::{ProviderConfigRegistry, require_ordinary_target};

/// What [`remove_checkout`] did, or would do with `dry_run`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CheckoutRemoval {
    pub reference: String,
    pub dry_run: bool,
    /// Whether the reference named a registered checkout when the removal ran.
    pub registered: bool,
    /// Whether this call removed the entry (`registered` and not a dry run).
    pub removed: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub checkout_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alias: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// Linked Git ticket stores. They are only unlinked; their files stay.
    pub kept_git_stores: Vec<String>,
    /// External connections deleted from `providers.json` because no other checkout links them.
    pub removed_connections: Vec<String>,
    /// External connections kept because other checkouts still link them, with those ids.
    pub shared_connections: BTreeMap<String, Vec<String>>,
    /// Accounts the removed connections used. They stay signed in (HS2-SM9PM8).
    pub kept_credentials: Vec<String>,
}

/// Remove the checkout `reference` names (an exact id, a root path, or a unique alias).
/// See the module docs for what is removed and what is kept.
pub fn remove_checkout(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    reference: &str,
    dry_run: bool,
) -> Result<CheckoutRemoval, ConnectionRemovalError> {
    let mut links = checkouts.prepare_source_links()?;
    let mut report = CheckoutRemoval {
        reference: reference.to_owned(),
        dry_run,
        ..CheckoutRemoval::default()
    };
    let Some(checkout) = links.resolve_for_removal(reference)? else {
        return Ok(report);
    };
    report.registered = true;
    report.checkout_id = Some(checkout.id.clone());
    report.alias = Some(checkout.alias.clone());
    report.root = Some(checkout.root.clone());
    report.kept_git_stores = checkout
        .sources
        .iter()
        .filter(|source| source.provider == "git")
        .map(|source| source.locator.clone())
        .collect();
    let orphans = classify_external_sources(&checkout, links.list(), &mut report);

    if orphans.is_empty() {
        if !dry_run {
            links.remove_entry(&checkout.id);
            links.publish()?;
            report.removed = true;
        }
        return Ok(report);
    }
    if dry_run {
        let connections = providers.load()?;
        collect_removed(&connections, &orphans, &mut report);
        return Ok(report);
    }
    providers.update_domain(|connections| {
        for connection in connections
            .iter()
            .filter(|connection| is_orphan(connection, &orphans))
        {
            require_ordinary_target(connection)?;
        }
        collect_removed(connections, &orphans, &mut report);
        connections.retain(|connection| !is_orphan(connection, &orphans));
        links.remove_entry(&checkout.id);
        // As in `detach_source`, the registry is published first: if the provider write
        // then fails, the orphaned record is retained rather than a link left dangling.
        links.publish()?;
        report.removed = true;
        Ok::<_, ConnectionRemovalError>(())
    })?;
    Ok(report)
}

/// Record shared connections in `report` and return the ones no other checkout links.
fn classify_external_sources(
    checkout: &Checkout,
    all: &[Checkout],
    report: &mut CheckoutRemoval,
) -> BTreeSet<String> {
    let mut orphans = BTreeSet::new();
    for source in checkout
        .sources
        .iter()
        .filter(|source| source.provider != "git")
    {
        let mut users = all
            .iter()
            .filter(|other| {
                other.id != checkout.id && other.source(&source.connection_id).is_some()
            })
            .collect::<Vec<_>>();
        users.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        if users.is_empty() {
            orphans.insert(source.connection_id.clone());
        } else {
            report.shared_connections.insert(
                source.connection_id.clone(),
                users.into_iter().map(|user| user.id.clone()).collect(),
            );
        }
    }
    orphans
}

fn is_orphan(connection: &crate::provider::ProviderConnection, orphans: &BTreeSet<String>) -> bool {
    connection.provider != "git" && orphans.contains(&connection.id)
}

fn collect_removed(
    connections: &[crate::provider::ProviderConnection],
    orphans: &BTreeSet<String>,
    report: &mut CheckoutRemoval,
) {
    let mut credentials = BTreeSet::new();
    for connection in connections
        .iter()
        .filter(|connection| is_orphan(connection, orphans))
    {
        report.removed_connections.push(connection.id.clone());
        if let Some(credential) = credential_of(connection) {
            credentials.insert(credential.to_owned());
        }
    }
    report.removed_connections.sort();
    report.kept_credentials = credentials.into_iter().collect();
}

#[cfg(test)]
#[path = "checkout_removal/tests.rs"]
mod tests;
