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
use std::path::{Path, PathBuf};

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

/// What [`prune_abandoned_temporary`] removed, or would remove with `dry_run` (HS2-HM0Y6X).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TemporaryCheckoutPrune {
    pub dry_run: bool,
    /// Why nothing was considered, when the registry itself is temporary (a test home).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped: Option<String>,
    /// One removal report per pruned checkout.
    pub removed: Vec<CheckoutRemoval>,
}

/// Unregister abandoned temporary checkouts from this machine's registry (HS2-HM0Y6X).
///
/// Tests, reviews, and agents create throwaway checkouts under the OS temporary
/// directory, and running Hot Sheet setup in one registers it. A candidate is a checkout
/// whose root lies under a temporary directory, whose sources are all Git (so no provider
/// connection is touched), and whose root is either gone or holds nothing but Hot Sheet's
/// regenerated `.hotsheet2/worklist.md` (older releases recreated a deleted root to write
/// it). Each is removed with [`remove_checkout`], and the generated skeleton is deleted
/// file by file; anything else in the folder keeps it registered.
///
/// A registry that itself lives under a temporary directory (a test home) is left alone,
/// so hermetic tests keep the checkouts they register.
pub fn prune_abandoned_temporary(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    dry_run: bool,
) -> Result<TemporaryCheckoutPrune, ConnectionRemovalError> {
    if registry_is_temporary(checkouts) {
        return Ok(TemporaryCheckoutPrune {
            dry_run,
            skipped: Some(TEMPORARY_REGISTRY.into()),
            removed: Vec::new(),
        });
    }
    prune_under(providers, checkouts, dry_run, &temporary_roots())
}

/// Why a prune considered nothing: the registry is a test or scratch home.
pub const TEMPORARY_REGISTRY: &str = "the checkout registry is itself temporary";

/// Whether `checkouts` lives under a temporary directory (a hermetic test home).
pub fn registry_is_temporary(checkouts: &CheckoutRegistry) -> bool {
    is_under_any(checkouts.path(), &temporary_roots())
}

fn prune_under(
    providers: &ProviderConfigRegistry,
    checkouts: &CheckoutRegistry,
    dry_run: bool,
    roots: &[PathBuf],
) -> Result<TemporaryCheckoutPrune, ConnectionRemovalError> {
    let mut report = TemporaryCheckoutPrune {
        dry_run,
        ..TemporaryCheckoutPrune::default()
    };
    for checkout in candidates_under(checkouts, roots)? {
        let removal = remove_checkout(providers, checkouts, &checkout.id, dry_run)?;
        if removal.removed {
            clear_abandoned_skeleton(Path::new(&checkout.root));
        }
        if removal.registered {
            report.removed.push(removal);
        }
    }
    Ok(report)
}

/// The checkouts [`prune_abandoned_temporary`] would remove, for a caller that removes
/// each one through a running server. Empty when the registry itself is temporary.
pub fn abandoned_temporary_candidates(
    checkouts: &CheckoutRegistry,
) -> Result<Vec<Checkout>, ConnectionRemovalError> {
    let roots = temporary_roots();
    if is_under_any(checkouts.path(), &roots) {
        return Ok(Vec::new());
    }
    candidates_under(checkouts, &roots)
}

fn candidates_under(
    checkouts: &CheckoutRegistry,
    roots: &[PathBuf],
) -> Result<Vec<Checkout>, ConnectionRemovalError> {
    Ok(checkouts
        .list()?
        .into_iter()
        .filter(|checkout| {
            let root = Path::new(&checkout.root);
            is_under_any(root, roots)
                && checkout
                    .sources
                    .iter()
                    .all(|source| source.provider == "git")
                && is_abandoned(root)
        })
        .collect())
}

/// The machine's temporary directories, in written and resolved forms.
pub fn temporary_roots() -> Vec<PathBuf> {
    let mut roots = vec![std::env::temp_dir()];
    if cfg!(unix) {
        roots.extend(
            [
                "/tmp",
                "/var/tmp",
                "/private/tmp",
                "/private/var/tmp",
                "/private/var/folders",
                "/var/folders",
            ]
            .into_iter()
            .map(PathBuf::from),
        );
    }
    let resolved: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| root.canonicalize().ok())
        .collect();
    roots.extend(resolved);
    roots.sort();
    roots.dedup();
    roots
}

fn is_under_any(path: &Path, roots: &[PathBuf]) -> bool {
    // A registered root is stored resolved and may no longer exist, so compare it with
    // each root in both its written and resolved forms.
    let resolved = path.canonicalize().ok();
    roots.iter().any(|root| {
        let resolved_root = root.canonicalize().ok();
        [Some(path), resolved.as_deref()]
            .into_iter()
            .flatten()
            .any(|path| {
                path.starts_with(root)
                    || resolved_root
                        .as_deref()
                        .is_some_and(|root| path.starts_with(root))
            })
    })
}

/// Gone, or holding only the generated `.hotsheet2/worklist.md` (or an empty `.hotsheet2`).
fn is_abandoned(root: &Path) -> bool {
    if !root.exists() {
        return true;
    }
    let Some(entries) = names(root) else {
        return false;
    };
    if entries != [".hotsheet2"] {
        return false;
    }
    names(&root.join(".hotsheet2"))
        .is_some_and(|inner| inner.is_empty() || inner == ["worklist.md"])
}

fn names(dir: &Path) -> Option<Vec<String>> {
    let mut names = std::fs::read_dir(dir)
        .ok()?
        .map(|entry| entry.map(|entry| entry.file_name().to_string_lossy().into_owned()))
        .collect::<Result<Vec<_>, _>>()
        .ok()?;
    names.sort();
    Some(names)
}

/// After a candidate was unregistered, delete the generated skeleton older releases left
/// in its root: the worklist, then the two now-empty directories. It rechecks that the
/// root still holds nothing else, and each step is non-recursive, so a file that appeared
/// in the meantime is kept.
pub fn clear_abandoned_skeleton(root: &Path) {
    if !root.exists() || !is_abandoned(root) {
        return;
    }
    let dir = root.join(".hotsheet2");
    let _ = std::fs::remove_file(dir.join("worklist.md"));
    let _ = std::fs::remove_dir(&dir);
    let _ = std::fs::remove_dir(root);
}

#[cfg(test)]
#[path = "checkout_removal/tests.rs"]
mod tests;
