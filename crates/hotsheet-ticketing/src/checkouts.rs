//! Machine-local checkout discovery. A checkout is a working directory, not a git or
//! ticket-store identity: several checkouts may share one repository and each checkout
//! may use several ticket stores (and vice versa).

mod source_observation;
pub use source_observation::CheckoutSourceProof;

use std::collections::{BTreeMap, BTreeSet};
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::file_lock::FileLock;
use crate::provider::{
    ProviderConfigRegistry, ProviderConnection, ProviderError, require_ordinary_target,
};
use crate::store::{FsStore, STORE_METADATA_FILE, StoreError};

pub const DEFAULT_SOURCE_COLOR: &str = "#6b7280";
const SOURCE_COLORS: &[&str] = &[
    "#3b82f6",
    "#22c55e",
    "#f97316",
    "#ef4444",
    "#8b5cf6",
    "#ec4899",
    "#14b8a6",
    DEFAULT_SOURCE_COLOR,
];

pub fn effective_source_color(color: Option<&str>) -> &str {
    color
        .filter(|value| SOURCE_COLORS.contains(value))
        .unwrap_or(DEFAULT_SOURCE_COLOR)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Checkout {
    pub id: String,
    pub root: String,
    pub alias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(default)]
    pub stores: Vec<String>,
    #[serde(default)]
    pub sources: Vec<TicketSource>,
    /// Stable store identities keyed by the Git source connection id. Legacy checkout
    /// links omit entries until their still-present stores can be migrated.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub store_instance_ids: BTreeMap<String, String>,
    /// A reviewed recovery that could not prove the moved store's old identity.
    /// Keyed by current Git connection id; value is the unverified former locator.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unverified_store_sources: BTreeMap<String, String>,
    /// Project-local source accent colors keyed by connection id; omitted entries use Gray.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_colors: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_source: Option<String>,
    /// Distinguishes a user-cleared default from an older registry with no recorded choice.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default_source_cleared: bool,
    /// Durable evidence that this checkout had a permission vault. A missing file must
    /// not be mistaken for a first registration after a server restart.
    #[serde(default, skip_serializing_if = "is_false")]
    pub permission_vault_registered: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TicketSource {
    pub connection_id: String,
    pub provider: String,
    pub locator: String,
}

impl TicketSource {
    pub fn git(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let canonical = path.canonicalize().unwrap_or(path);
        let locator = canonical.to_string_lossy().into_owned();
        let hash = format!("{:x}", Sha256::digest(locator.as_bytes()));
        Self {
            connection_id: hash[..16].to_string(),
            provider: "git".into(),
            locator,
        }
    }
}

impl Checkout {
    /// Authorization namespace for this registered code project. The prefix prevents a
    /// legacy ticket-store path from ever matching a checkout rule by coincidence.
    pub fn permission_identity(&self) -> String {
        format!("checkout:{}", self.id)
    }

    pub fn source(&self, connection_id: &str) -> Option<&TicketSource> {
        self.sources
            .iter()
            .find(|source| source.connection_id == connection_id)
    }

    /// HS2-AV80C4: the linked local sources project claim-next scans, in source precedence
    /// (the default source first, then registry order). Provider sources (GitHub, GitLab,
    /// Jira) have no claim leases and are skipped; when nothing else is linked the error
    /// names them, so an agent sees an unsupported operation rather than an empty queue.
    pub fn claim_next_sources(&self) -> Result<Vec<&TicketSource>, String> {
        let default = self.default_source.as_deref();
        let (mut local, unsupported): (Vec<_>, Vec<_>) = self
            .sources
            .iter()
            .partition(|source| source.provider == "git");
        if local.is_empty() {
            let providers = unsupported
                .iter()
                .map(|source| source.provider.as_str())
                .collect::<std::collections::BTreeSet<_>>();
            return Err(if providers.is_empty() {
                "checkout has no linked ticket sources to claim from".to_owned()
            } else {
                format!(
                    "claim-next is unsupported by {} ticket sources; claim an exact ticket in a local Hot Sheet source",
                    providers.into_iter().collect::<Vec<_>>().join(", ")
                )
            });
        }
        // A stable sort keeps registry order behind the default source.
        local.sort_by_key(|source| Some(source.connection_id.as_str()) != default);
        Ok(local)
    }

    /// Open a linked Git source only after its recorded store identity is verified.
    /// Permission migration uses this boundary so a moved or replaced source cannot
    /// select an unrelated legacy vault by its current path.
    pub fn verified_git_source(&self, connection_id: &str) -> Result<FsStore, CheckoutError> {
        let source = self.source(connection_id).ok_or_else(|| {
            CheckoutError::Invalid("ticket source is not linked to this project".into())
        })?;
        if source.provider != "git" {
            return Err(CheckoutError::Invalid(
                "ticket source is not a Git store".into(),
            ));
        }
        validate_linked_store_identity(self, source)?;
        FsStore::open_without_maintenance(&source.locator).map_err(CheckoutError::from)
    }

    /// Project-owned settings with deterministic read-through compatibility for files
    /// written beside this checkout's old git ticket stores. The default source wins a
    /// legacy key conflict; remaining sources retain registry order. Provider-only and
    /// source-free projects still get the same project settings location.
    pub fn settings(&self) -> crate::Settings {
        let mut stores = self
            .default_source
            .as_deref()
            .and_then(|id| self.source(id))
            .filter(|source| source.provider == "git")
            .map(|source| PathBuf::from(&source.locator))
            .into_iter()
            .collect::<Vec<_>>();
        stores.extend(
            self.sources
                .iter()
                .filter(|source| source.provider == "git")
                .filter(|source| {
                    Some(source.connection_id.as_str()) != self.default_source.as_deref()
                })
                .map(|source| PathBuf::from(&source.locator)),
        );
        crate::Settings::with_legacy_stores(&self.root, stores)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RegistryFile {
    #[serde(default = "registry_schema_version", rename = "schemaVersion")]
    schema_version: u64,
    #[serde(default)]
    checkouts: Vec<Checkout>,
    /// Historical connection ids keyed by the checkout's durable registry id. Values are
    /// the current connection ids. Kept outside `Checkout` so operational source records
    /// always contain ids understood by the active provider registry.
    #[serde(default, rename = "sourceAliases")]
    source_aliases: BTreeMap<String, BTreeMap<String, String>>,
    /// `(checkout id, connection id)` pairs that exist only because a sourceless entry
    /// was read through its root's `.hotsheet2/store` link. They were never persisted as
    /// registry links, so registering one is a new selection, not a pre-identity link
    /// that needs review (HS2-K2YHDE). Never serialized.
    #[serde(skip)]
    link_derived_sources: BTreeSet<(String, String)>,
}

const CHECKOUT_REGISTRY_SCHEMA_VERSION: u64 = 4;
const fn registry_schema_version() -> u64 {
    CHECKOUT_REGISTRY_SCHEMA_VERSION
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            schema_version: CHECKOUT_REGISTRY_SCHEMA_VERSION,
            checkouts: Vec::new(),
            source_aliases: BTreeMap::new(),
            link_derived_sources: BTreeSet::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum CheckoutError {
    #[error(transparent)]
    Endpoint(#[from] crate::github_endpoint::EndpointError),
    #[error("checkout path does not exist: {0}")]
    Missing(String),
    #[error("no checkout matches {0}")]
    NotFound(String),
    #[error("checkout reference is ambiguous: {0}")]
    Ambiguous(String),
    #[error("invalid checkout registry: {0}")]
    Invalid(String),
    #[error(
        "This project registry was created by a newer version of Hot Sheet 2 and cannot be opened by this version. Update Hot Sheet 2 to open it (found schema {found}, supported through {supported})."
    )]
    UpgradeRequired { found: u64, supported: u64 },
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}

impl From<ProviderError> for CheckoutError {
    fn from(error: ProviderError) -> Self {
        match error {
            ProviderError::Endpoint(error) => Self::Endpoint(error),
            other => Self::Invalid(other.to_string()),
        }
    }
}

/// HS2-726EXJ: a Git source joins a checkout only when its store is an owned or
/// designated Git store and is not the machine bootstrap anchor. Checked once, when the
/// link is created or changed; an existing link is not re-probed on every registration.
/// A path that holds no store yet is left to the existing missing-store handling.
fn admit_linked_git_source(source: &TicketSource) -> Result<(), CheckoutError> {
    let path = Path::new(&source.locator);
    if source.provider != "git" || !path.join(STORE_METADATA_FILE).is_file() {
        return Ok(());
    }
    crate::store_git::admit_store(path, crate::store_git::StoreRole::TicketSource)
        .map_err(|error| CheckoutError::Store(error.into()))
}

fn git_store_instance_id(source: &TicketSource) -> Result<Option<String>, CheckoutError> {
    let path = Path::new(&source.locator);
    if !path.join(STORE_METADATA_FILE).is_file() {
        return Ok(None);
    }
    Ok(Some(
        FsStore::open_without_maintenance(path)?.ensure_instance_id()?,
    ))
}

fn validate_linked_store_identity(
    checkout: &Checkout,
    source: &TicketSource,
) -> Result<(), CheckoutError> {
    if source.provider != "git" {
        return Ok(());
    }
    let Some(expected) = checkout.store_instance_ids.get(&source.connection_id) else {
        return Err(CheckoutError::Invalid(format!(
            "linked Git store at {} has no recorded identity; review its location in Ticket sources before opening it",
            source.locator
        )));
    };
    let actual = FsStore::open_without_maintenance(&source.locator)
        .and_then(|store| store.metadata())
        .map_err(|error| {
            CheckoutError::Invalid(format!(
                "linked Git store at {} cannot be verified: {error}; restore it or remove and add the replacement source",
                source.locator
            ))
        })?
        .instance_id;
    if actual.as_deref() != Some(expected) {
        return Err(CheckoutError::Invalid(format!(
            "linked Git store at {} has a different identity; restore the original store or remove and add the replacement source",
            source.locator
        )));
    }
    Ok(())
}

fn validate_linked_store_identities(checkout: &Checkout) -> Result<(), CheckoutError> {
    for source in &checkout.sources {
        validate_linked_store_identity(checkout, source)?;
    }
    Ok(())
}

/// Discover a linked or conventional git-backed ticket store for a checkout. A valid
/// `.hotsheet2/store` link takes precedence over the sibling `<checkout>.hs2` convention.
/// Discovery never creates a store or guesses that one checkout has only one provider.
pub fn discover_ticket_stores(root: &Path) -> Result<Vec<PathBuf>, CheckoutError> {
    let canonical = root
        .canonicalize()
        .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
    if let Some(linked) = legacy_link_source(&canonical) {
        let path = PathBuf::from(linked.locator);
        if path.join("hotsheet-store.json").is_file() {
            return Ok(vec![path.canonicalize().unwrap_or(path)]);
        }
    }
    let sibling = PathBuf::from(format!("{}.hs2", canonical.display()));
    if sibling.join("hotsheet-store.json").is_file() {
        Ok(vec![sibling.canonicalize().unwrap_or(sibling)])
    } else {
        Ok(Vec::new())
    }
}

/// Stable, discoverable checkout id: basename plus twelve hex chars from the canonical
/// absolute path. It deliberately is not a secret and changes when the checkout moves.
pub fn checkout_id(root: &Path) -> Result<String, CheckoutError> {
    let canonical = root
        .canonicalize()
        .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
    let name = canonical
        .file_name()
        .and_then(|v| v.to_str())
        .filter(|v| !v.is_empty())
        .unwrap_or("checkout");
    let slug: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let hash = format!(
        "{:x}",
        Sha256::digest(canonical.to_string_lossy().as_bytes())
    );
    Ok(format!("{}-{}", slug.trim_matches('-'), &hash[..12]))
}

#[derive(Debug, Clone)]
pub struct CheckoutRegistry {
    path: PathBuf,
}

/// How a project open should reconcile newly found sources with durable checkout links.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenSourceMode {
    /// A caller supplied the complete source set.
    Explicit,
    /// A caller selected a git store; replace a previous sole git store when it changed.
    SelectedGitStore,
    /// Conservative discovery must never remove an existing source.
    Discovered,
}

pub struct OpenSourceSelection {
    pub sources: Vec<TicketSource>,
    pub default_source: Option<String>,
    pub mode: OpenSourceMode,
}

/// Holds the checkout registry lock while a provider record and its project links are
/// changed together. Ordinary checkout mutations acquire this same file lock.
pub(crate) struct LockedCheckoutRegistry<'a> {
    registry: &'a CheckoutRegistry,
    _lock: FileLock,
}

impl LockedCheckoutRegistry<'_> {
    /// HS2-4J6V46: resolve under the held link lock for source observations.
    pub(crate) fn resolve(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        let mut entries = self.registry.read_locked()?.checkouts;
        entries.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        resolve_checkout(entries, reference)
    }
}

/// HS2-6H8ZY6: strict metadata prepared before entering the provider owner.
/// Publishing an external unlink never reopens Git stores or repairs the registry.
pub(crate) struct PreparedCheckoutLinks<'a> {
    locked: LockedCheckoutRegistry<'a>,
    file: RegistryFile,
    changed: bool,
}

impl PreparedCheckoutLinks<'_> {
    pub(crate) fn list(&self) -> &[Checkout] {
        &self.file.checkouts
    }

    pub(crate) fn resolve(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        resolve_checkout(self.file.checkouts.clone(), reference)
    }

    fn exact(&self, checkout_id: &str) -> Result<&Checkout, CheckoutError> {
        self.file
            .checkouts
            .iter()
            .find(|entry| entry.id == checkout_id)
            .ok_or_else(|| CheckoutError::NotFound(checkout_id.into()))
    }

    /// HS2-6H8ZY6: resolve the complete normalized candidate before taking any
    /// provider owner. This does not initialize stores or publish either file.
    fn registration(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        mut sources: Vec<TicketSource>,
        mut default_source: Option<String>,
        explicit_clear: bool,
    ) -> Result<(Option<Checkout>, Checkout), CheckoutError> {
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let before = self
            .file
            .checkouts
            .iter()
            .find(|row| Path::new(&row.root) == root)
            .cloned();
        unique_source_ids(&sources)?;
        for source in &mut sources {
            if source.provider == "git" {
                let normalized = TicketSource::git(&source.locator);
                if default_source.as_deref() == Some(&source.connection_id) {
                    default_source = Some(normalized.connection_id.clone());
                }
                *source = normalized;
            }
        }
        unique_source_ids(&sources)?;
        sources.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        sources.dedup();
        if let Some(before) = &before {
            unique_source_ids(&before.sources)?;
            for source in &sources {
                if before
                    .source(&source.connection_id)
                    .is_some_and(|old| old.provider != source.provider)
                {
                    return Err(CheckoutError::Invalid(
                        "ticket source id is linked to another provider kind".into(),
                    ));
                }
            }
        }
        if let Some(default) = &default_source
            && !sources
                .iter()
                .any(|source| &source.connection_id == default)
        {
            return Err(CheckoutError::Invalid(format!(
                "default source '{default}' is not associated with this checkout"
            )));
        }
        let retained = |id: &String| sources.iter().any(|source| &source.connection_id == id);
        let mut after = Checkout {
            id: before
                .as_ref()
                .map(|row| row.id.clone())
                .unwrap_or(checkout_id(&root)?),
            root: root.to_string_lossy().into_owned(),
            alias: alias.map(str::to_owned).unwrap_or_else(|| {
                root.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("checkout")
                    .to_owned()
            }),
            repository,
            stores: sources
                .iter()
                .filter(|source| source.provider == "git")
                .map(|source| source.locator.clone())
                .collect(),
            store_instance_ids: before
                .as_ref()
                .map(|row| {
                    row.store_instance_ids
                        .iter()
                        .filter(|(id, _)| retained(id))
                        .map(|(id, value)| (id.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            unverified_store_sources: before
                .as_ref()
                .map(|row| {
                    row.unverified_store_sources
                        .iter()
                        .filter(|(id, _)| retained(id))
                        .map(|(id, value)| (id.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            source_colors: before
                .as_ref()
                .map(|row| {
                    row.source_colors
                        .iter()
                        .filter(|(id, _)| retained(id))
                        .map(|(id, value)| (id.clone(), value.clone()))
                        .collect()
                })
                .unwrap_or_default(),
            default_source_cleared: default_source.is_none()
                && (explicit_clear
                    || before
                        .as_ref()
                        .is_some_and(|row| row.default_source_cleared)),
            default_source,
            permission_vault_registered: before
                .as_ref()
                .is_some_and(|row| row.permission_vault_registered),
            sources,
        };
        after.stores.sort();
        // Legacy link/default observations happen here, never inside the provider callback.
        migrate_checkout(&mut after);
        unique_source_ids(&after.sources)?;
        Ok((before, after))
    }

    fn finish_registration(
        &mut self,
        before: Option<&Checkout>,
        mut after: Checkout,
        providers: Option<&ProviderConfigRegistry>,
        explicit_default: Option<&str>,
    ) -> Result<Checkout, CheckoutError> {
        let targets = external_registration_targets(before, &after, explicit_default)?;
        if providers.is_none() && !targets.is_empty() {
            return Err(source_context_required());
        }
        let mixed = after
            .sources
            .iter()
            .chain(before.into_iter().flat_map(|row| &row.sources))
            .any(|source| source.provider != "git");
        for source in after
            .sources
            .iter()
            .filter(|source| source.provider == "git")
        {
            let previous = before.and_then(|row| row.source(&source.connection_id));
            if previous != Some(source) {
                admit_linked_git_source(source)?;
            }
            let affected = previous != Some(source)
                || explicit_default == Some(source.connection_id.as_str())
                || (before.and_then(|row| row.default_source.as_deref())
                    != after.default_source.as_deref()
                    && after.default_source.as_deref() == Some(source.connection_id.as_str()));
            if mixed && !affected {
                continue;
            }
            if mixed {
                if let Some(before) = before.filter(|_| previous.is_some()) {
                    validate_linked_store_identity(before, source)?;
                } else {
                    let instance = FsStore::open_without_maintenance(&source.locator)?.metadata()?.instance_id
                        .ok_or_else(|| CheckoutError::Invalid("new Git sources in a mixed checkout require an established store identity; run headless init/bootstrap setup first".into()))?;
                    after
                        .store_instance_ids
                        .insert(source.connection_id.clone(), instance);
                }
            } else {
                // Pure Git/bootstrap remains outside provider ownership. Retain the
                // existing unverified-link rule; never silently adopt that store.
                let prior =
                    before.and_then(|row| row.store_instance_ids.get(&source.connection_id));
                let unverified = previous.is_some()
                    && prior.is_none()
                    && !self
                        .file
                        .link_derived_sources
                        .contains(&(after.id.clone(), source.connection_id.clone()));
                let found = if unverified {
                    None
                } else {
                    git_store_instance_id(source).ok().flatten()
                };
                if let (Some(prior), Some(found)) = (prior, found.as_ref())
                    && prior != found
                {
                    return Err(CheckoutError::Invalid(
                        "ticket store has a different identity than the linked source".into(),
                    ));
                }
                if let Some(instance) = found.or_else(|| prior.cloned()) {
                    after
                        .store_instance_ids
                        .insert(source.connection_id.clone(), instance);
                }
            }
        }
        if targets.is_empty() {
            self.install_registration(&after);
            self.publish()?;
            return Ok(after);
        }
        let providers = providers.ok_or_else(source_context_required)?;
        providers.update_domain(|records| {
            // Resolve every kind/membership/locator before any endpoint predicate.
            let mut actual = Vec::new();
            for target in &targets {
                if after.source(&target.connection_id).is_some() {
                    actual.push(registered_source(records, target)?);
                } else if let Some(row) = records.iter().find(|row| row.id == target.connection_id)
                {
                    if row.provider != target.provider {
                        return Err(CheckoutError::Invalid(
                            "removed source belongs to another provider kind".into(),
                        ));
                    }
                    actual.push(row);
                }
            }
            for row in actual {
                require_ordinary_target(row)?;
            }
            self.install_registration(&after);
            self.publish()?;
            Ok(after)
        })
    }

    fn install_registration(&mut self, after: &Checkout) {
        if let Some(current) = self
            .file
            .checkouts
            .iter_mut()
            .find(|row| row.id == after.id)
        {
            self.changed |= current != after;
            *current = after.clone();
        } else {
            self.file.checkouts.push(after.clone());
            self.changed = true;
        }
    }

    fn external_eligibility(
        &self,
        checkout_id: &str,
        source: &TicketSource,
    ) -> Result<(), CheckoutError> {
        if source.provider == "git" {
            return Err(CheckoutError::Invalid(
                "Git sources use the linked-store workflow".into(),
            ));
        }
        for existing in self
            .exact(checkout_id)?
            .sources
            .iter()
            .filter(|item| item.connection_id == source.connection_id)
        {
            if existing.provider == "git" || existing.provider != source.provider {
                return Err(CheckoutError::Invalid(
                    "ticket source id is already linked to another provider kind".into(),
                ));
            }
        }
        Ok(())
    }

    /// Pure metadata only. The hint is computed before provider entry using the
    /// existing legacy-default rule; no path discovery or Git work occurs here.
    fn upsert_external(
        &mut self,
        checkout_id: &str,
        source: TicketSource,
        make_default: bool,
        default_hint: Option<String>,
    ) -> Result<Checkout, CheckoutError> {
        self.external_eligibility(checkout_id, &source)?;
        let entry = self
            .file
            .checkouts
            .iter_mut()
            .find(|entry| entry.id == checkout_id)
            .ok_or_else(|| CheckoutError::NotFound(checkout_id.into()))?;
        let before = entry.clone();
        let id = source.connection_id.clone();
        entry.sources.retain(|item| item.connection_id != id);
        entry.sources.push(source);
        entry
            .sources
            .sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        if make_default {
            entry.default_source = Some(id);
            entry.default_source_cleared = false;
        } else if entry.default_source.is_none() && !entry.default_source_cleared {
            entry.default_source = default_hint;
        }
        self.changed |= *entry != before;
        Ok(entry.clone())
    }

    fn set_default(
        &mut self,
        checkout_id: &str,
        source: Option<&str>,
    ) -> Result<Checkout, CheckoutError> {
        let entry = self
            .file
            .checkouts
            .iter_mut()
            .find(|entry| entry.id == checkout_id)
            .ok_or_else(|| CheckoutError::NotFound(checkout_id.into()))?;
        if let Some(id) = source
            && entry.source(id).is_none()
        {
            return Err(CheckoutError::Invalid(format!(
                "default source '{id}' is not associated with checkout {checkout_id}"
            )));
        }
        let before = entry.clone();
        entry.default_source = source.map(str::to_owned);
        entry.default_source_cleared = source.is_none();
        self.changed |= *entry != before;
        Ok(entry.clone())
    }

    fn update_external_locator(
        &mut self,
        connection: &ProviderConnection,
    ) -> Result<Vec<String>, CheckoutError> {
        let mut changed = Vec::new();
        for entry in &mut self.file.checkouts {
            let mut updated = false;
            for source in &mut entry.sources {
                if source.connection_id != connection.id {
                    continue;
                }
                if source.provider == "git" || source.provider != connection.provider {
                    return Err(CheckoutError::Invalid(
                        "ticket source id is linked to another provider kind".into(),
                    ));
                }
                if source.locator != connection.locator {
                    source.locator = connection.locator.clone();
                    updated = true;
                }
            }
            if updated {
                changed.push(entry.id.clone());
            }
        }
        self.changed |= !changed.is_empty();
        Ok(changed)
    }

    pub(crate) fn unlink_external(&mut self, checkout_id: &str, connection_id: &str) -> bool {
        let Some(checkout) = self
            .file
            .checkouts
            .iter_mut()
            .find(|entry| entry.id == checkout_id)
        else {
            return false;
        };
        let before = checkout.sources.len();
        checkout
            .sources
            .retain(|source| source.connection_id != connection_id || source.provider == "git");
        if checkout.sources.len() == before {
            return false;
        }
        // A colliding retained Git member must keep its maps and default too.
        if checkout.source(connection_id).is_none() {
            checkout.source_colors.remove(connection_id);
            checkout.unverified_store_sources.remove(connection_id);
            if checkout.default_source.as_deref() == Some(connection_id) {
                checkout.default_source = None;
            }
        }
        self.changed = true;
        true
    }

    /// HS2-X4RZ73: resolve a removal target strictly; see [`resolve_for_removal`].
    pub(crate) fn resolve_for_removal(
        &self,
        reference: &str,
    ) -> Result<Option<Checkout>, CheckoutError> {
        resolve_for_removal(&self.file.checkouts, reference)
    }

    /// HS2-X4RZ73: drop one checkout with its retained source aliases. Its sources'
    /// stores and connections are the caller's concern; nothing on disk but the
    /// registry changes when this is published.
    pub(crate) fn remove_entry(&mut self, checkout_id: &str) -> Option<Checkout> {
        let index = self
            .file
            .checkouts
            .iter()
            .position(|entry| entry.id == checkout_id)?;
        let removed = self.file.checkouts.remove(index);
        self.file.source_aliases.remove(checkout_id);
        self.file
            .link_derived_sources
            .retain(|(id, _)| id != checkout_id);
        self.changed = true;
        Some(removed)
    }

    pub(crate) fn publish(&mut self) -> Result<(), CheckoutError> {
        if self.changed {
            self.locked.registry.write_locked(&self.file)?;
            self.changed = false;
        }
        Ok(())
    }

    /// Git detach can infer an external default. Prepare that result before
    /// entering its actual provider owner instead of treating it as Git-only.
    pub(crate) fn unlink_git(
        &mut self,
        providers: &ProviderConfigRegistry,
        checkout_id: &str,
        connection_id: &str,
    ) -> Result<(), CheckoutError> {
        let before = self.exact(checkout_id)?.clone();
        let mut sources = before.sources.clone();
        sources.retain(|source| source.connection_id != connection_id);
        let default = before
            .default_source
            .clone()
            .filter(|id| id != connection_id);
        let (_, after) = self.registration(
            Path::new(&before.root),
            Some(&before.alias),
            before.repository.clone(),
            sources,
            default,
            false,
        )?;
        self.finish_registration(Some(&before), after, Some(providers), None)?;
        Ok(())
    }
}

fn registered_source<'a>(
    records: &'a [ProviderConnection],
    source: &TicketSource,
) -> Result<&'a ProviderConnection, CheckoutError> {
    let actual = records
        .iter()
        .find(|row| row.id == source.connection_id)
        .ok_or_else(|| {
            CheckoutError::Invalid(format!(
                "ticket source '{}' is not registered in providers.json",
                source.connection_id
            ))
        })?;
    if actual.provider != source.provider {
        return Err(CheckoutError::Invalid(
            "ticket source id belongs to another provider kind".into(),
        ));
    }
    if !actual.locator.eq_ignore_ascii_case(&source.locator) {
        return Err(CheckoutError::Invalid(
            "ticket source locator does not match its registered connection".into(),
        ));
    }
    Ok(actual)
}

fn exact_connection(
    actual: &ProviderConnection,
    expected: &ProviderConnection,
) -> Result<(), CheckoutError> {
    if actual != expected {
        return Err(if actual.provider == "github" {
            crate::github_endpoint::EndpointError::Conflict.into()
        } else {
            CheckoutError::Invalid(
                "provider connection changed before its project links could be updated".into(),
            )
        });
    }
    Ok(())
}

fn displaced_external(
    before: &Checkout,
    after: &Checkout,
) -> Result<Option<TicketSource>, CheckoutError> {
    if before.default_source == after.default_source {
        return Ok(None);
    }
    let source = before
        .default_source
        .as_deref()
        .map(|id| linked_source(before, id))
        .transpose()?
        .flatten();
    Ok(source.filter(|source| source.provider != "git").cloned())
}

fn linked_source<'a>(
    checkout: &'a Checkout,
    id: &str,
) -> Result<Option<&'a TicketSource>, CheckoutError> {
    let source = checkout.source(id);
    if let Some(source) = source {
        if checkout
            .sources
            .iter()
            .any(|other| other.connection_id == id && other.provider != source.provider)
        {
            return Err(CheckoutError::Invalid(
                "ticket source id is linked to conflicting provider kinds".into(),
            ));
        }
    }
    Ok(source)
}

/// Resolve all eligibility before checking any endpoint authority. An absent old
/// default can be cleared; it grants no authority and must not strand cleanup.
fn ordinary_link_targets(
    records: &[ProviderConnection],
    selected: Option<&ProviderConnection>,
    displaced: Option<&TicketSource>,
    expected: Option<&ProviderConnection>,
) -> Result<(), CheckoutError> {
    let mut targets = BTreeMap::new();
    if let Some(selected) = selected {
        targets.insert(selected.id.as_str(), selected);
    }
    if let Some(old) = displaced {
        if let Some(actual) = records.iter().find(|row| row.id == old.connection_id) {
            if actual.provider != old.provider {
                return Err(CheckoutError::Invalid(
                    "previous default source belongs to another provider kind".into(),
                ));
            }
            targets.insert(actual.id.as_str(), actual);
        }
    }
    if let Some(expected) = expected {
        let actual = selected
            .ok_or_else(|| CheckoutError::Invalid("exact provider candidate is missing".into()))?;
        exact_connection(actual, expected)?;
    }
    for actual in targets.values() {
        require_ordinary_target(actual)?;
    }
    Ok(())
}

fn source_context_required() -> CheckoutError {
    CheckoutError::Invalid(
        "external source changes require the provider-aware checkout workflow".into(),
    )
}

fn unique_source_ids(sources: &[TicketSource]) -> Result<(), CheckoutError> {
    let mut seen = BTreeMap::new();
    for source in sources {
        if let Some(previous) = seen.insert(source.connection_id.as_str(), source)
            && previous != source
        {
            return Err(CheckoutError::Invalid(
                "ticket source id has conflicting routing metadata".into(),
            ));
        }
    }
    Ok(())
}

/// Routing deltas, not retained membership, determine ordinary authority. Default
/// inference is resolved before this comparison, outside the provider owner.
fn external_registration_targets(
    before: Option<&Checkout>,
    after: &Checkout,
    explicit_default: Option<&str>,
) -> Result<Vec<TicketSource>, CheckoutError> {
    let mut targets = BTreeMap::new();
    if let Some(before) = before {
        unique_source_ids(&before.sources)?;
        for old in before
            .sources
            .iter()
            .filter(|source| source.provider != "git")
        {
            if after.source(&old.connection_id) != Some(old) {
                targets.insert(old.connection_id.clone(), old.clone());
            }
        }
    }
    unique_source_ids(&after.sources)?;
    for source in after
        .sources
        .iter()
        .filter(|source| source.provider != "git")
    {
        if before.and_then(|row| row.source(&source.connection_id)) != Some(source) {
            targets.insert(source.connection_id.clone(), source.clone());
        }
    }
    let before_default = before.and_then(|row| row.default_source.as_deref());
    if before_default != after.default_source.as_deref() {
        if let Some(old) = before
            .and_then(|row| before_default.and_then(|id| row.source(id)))
            .filter(|source| source.provider != "git")
        {
            targets.insert(old.connection_id.clone(), old.clone());
        }
        if let Some(selected) = after
            .default_source
            .as_deref()
            .and_then(|id| after.source(id))
            .filter(|source| source.provider != "git")
        {
            targets.insert(selected.connection_id.clone(), selected.clone());
        }
    }
    if let Some(selected) = explicit_default
        .and_then(|id| after.source(id))
        .filter(|source| source.provider != "git")
    {
        targets.insert(selected.connection_id.clone(), selected.clone());
    }
    Ok(targets.into_values().collect())
}

impl CheckoutRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// The host's injected registry path, used by read-only coordination watchers.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list(&self) -> Result<Vec<Checkout>, CheckoutError> {
        let mut entries = self.read()?.checkouts;
        entries.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        Ok(entries)
    }

    fn acquire_lock(&self) -> Result<FileLock, CheckoutError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let path = self.path.with_extension("json.lock");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)?;
        Ok(FileLock::acquire(file)?)
    }

    pub(crate) fn lock_source_links(&self) -> Result<LockedCheckoutRegistry<'_>, CheckoutError> {
        Ok(LockedCheckoutRegistry {
            registry: self,
            _lock: self.acquire_lock()?,
        })
    }

    pub(crate) fn prepare_source_links(&self) -> Result<PreparedCheckoutLinks<'_>, CheckoutError> {
        let locked = self.lock_source_links()?;
        // Compatibility normalization may inspect legacy link paths here, before
        // the provider owner exists. Malformed JSON is never recovered or written.
        let file = self.read_strict_locked()?;
        Ok(PreparedCheckoutLinks {
            locked,
            file,
            changed: false,
        })
    }

    pub fn register(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        stores: Vec<PathBuf>,
    ) -> Result<Checkout, CheckoutError> {
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let alias = alias.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("checkout")
                .to_owned()
        });
        let sources = stores
            .into_iter()
            .map(TicketSource::git)
            .collect::<Vec<_>>();
        let default_source = legacy_default_source(&root, &sources)
            .or_else(|| (sources.len() == 1).then(|| sources[0].connection_id.clone()));
        self.register_sources(
            root.as_path(),
            Some(&alias),
            repository,
            sources,
            default_source,
        )
    }

    /// Register the git store selected by project setup/open without discarding other
    /// sources already linked to that checkout.
    pub fn open_git_store(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        store: &Path,
    ) -> Result<Checkout, CheckoutError> {
        self.open_sources(
            root,
            alias,
            repository,
            vec![TicketSource::git(store)],
            None,
            OpenSourceMode::SelectedGitStore,
        )
    }

    pub fn register_sources(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        sources: Vec<TicketSource>,
        default_source: Option<String>,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        let (before, after) =
            prepared.registration(root, alias, repository, sources, default_source, false)?;
        prepared.finish_registration(before.as_ref(), after, None, None)
    }

    /// Register external links only while their provider records still exist.
    pub fn register_registered_sources(
        &self,
        providers: &ProviderConfigRegistry,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        sources: Vec<TicketSource>,
        default_source: Option<String>,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        let (before, after) =
            prepared.registration(root, alias, repository, sources, default_source, false)?;
        prepared.finish_registration(before.as_ref(), after, Some(providers), None)
    }

    /// Reopening a project must retain linked providers and its selected default. Only an
    /// explicit full source set replaces every link; selecting a different sole git store
    /// replaces that git link while keeping external providers (HS2-JY6JZE).
    pub fn open_sources(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        sources: Vec<TicketSource>,
        default_source: Option<String>,
        mode: OpenSourceMode,
    ) -> Result<Checkout, CheckoutError> {
        self.open_sources_owned(
            None,
            root,
            alias,
            repository,
            OpenSourceSelection {
                sources,
                default_source,
                mode,
            },
        )
    }

    /// Project reopen validates requested external links under the removal lock. Existing
    /// links are retained when no new external source is requested.
    pub fn open_registered_sources(
        &self,
        providers: &ProviderConfigRegistry,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        selection: OpenSourceSelection,
    ) -> Result<Checkout, CheckoutError> {
        self.open_sources_owned(Some(providers), root, alias, repository, selection)
    }

    fn open_sources_owned(
        &self,
        providers: Option<&ProviderConfigRegistry>,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        selection: OpenSourceSelection,
    ) -> Result<Checkout, CheckoutError> {
        let OpenSourceSelection {
            mut sources,
            mut default_source,
            mode,
        } = selection;
        // HS2-6H8ZY6: the non-Explicit merge is last-wins by id. Reject
        // conflicting requested routes before it can erase their evidence.
        unique_source_ids(&sources)?;
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let mut prepared = self.prepare_source_links()?;
        let existing = prepared
            .file
            .checkouts
            .iter()
            .find(|checkout| checkout.root == root.to_string_lossy())
            .cloned();
        let is_new = existing.is_none();
        let mut alias = alias.map(str::to_owned);
        let mut repository = repository;
        if let Some(existing) = existing {
            alias = alias.or(Some(existing.alias));
            repository = repository.or(existing.repository);
            if mode != OpenSourceMode::Explicit {
                for source in &mut sources {
                    if source.provider == "git" {
                        *source = TicketSource::git(&source.locator);
                    }
                }
                unique_source_ids(&sources)?;
                let old_git = existing
                    .sources
                    .iter()
                    .filter(|source| source.provider == "git")
                    .collect::<Vec<_>>();
                let new_git = sources
                    .iter()
                    .filter(|source| source.provider == "git")
                    .collect::<Vec<_>>();
                let replaced_git = (mode == OpenSourceMode::SelectedGitStore
                    && old_git.len() == 1
                    && new_git.len() == 1
                    && old_git[0].connection_id != new_git[0].connection_id)
                    .then(|| {
                        (
                            old_git[0].connection_id.clone(),
                            new_git[0].connection_id.clone(),
                        )
                    });
                let mut retained = existing.sources;
                if let Some((old, _)) = &replaced_git {
                    retained.retain(|source| &source.connection_id != old);
                }
                for source in sources {
                    retained.retain(|prior| prior.connection_id != source.connection_id);
                    retained.push(source);
                }
                sources = retained;
                if default_source.is_none() {
                    default_source = existing.default_source.and_then(|id| {
                        if let Some((old, new)) = &replaced_git
                            && &id == old
                        {
                            return Some(new.clone());
                        }
                        sources
                            .iter()
                            .any(|source| source.connection_id == id)
                            .then_some(id)
                    });
                }
            }
        }
        if is_new && default_source.is_none() && sources.len() == 1 {
            default_source = Some(sources[0].connection_id.clone());
        }
        let (before, after) = prepared.registration(
            &root,
            alias.as_deref(),
            repository,
            sources,
            default_source,
            false,
        )?;
        prepared.finish_registration(before.as_ref(), after, providers, None)
    }

    pub fn resolve(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        let checkout = resolve_checkout(self.list()?, reference)?;
        validate_linked_store_identities(&checkout)?;
        Ok(checkout)
    }

    /// Read checkout configuration for recovery UI without opening linked ticket stores.
    pub fn describe(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        resolve_checkout(self.list()?, reference)
    }

    /// The checkout a removal of `reference` would remove, or `None` when nothing that
    /// strict reference names is registered (HS2-X4RZ73). Read-only.
    pub fn find_for_removal(&self, reference: &str) -> Result<Option<Checkout>, CheckoutError> {
        resolve_for_removal(&self.read()?.checkouts, reference)
    }

    /// Record vault registration before creating the file, so a crash between these
    /// operations leaves the checkout locked rather than allowing an empty replacement.
    pub fn mark_permission_vault_registered(&self, reference: &str) -> Result<(), CheckoutError> {
        let _lock = self.acquire_lock()?;
        let mut file = self.read_locked()?;
        let checkout = file
            .checkouts
            .iter_mut()
            .find(|checkout| checkout.id == reference)
            .ok_or_else(|| CheckoutError::NotFound(reference.to_owned()))?;
        if !checkout.permission_vault_registered {
            checkout.permission_vault_registered = true;
            self.write_locked(&file)?;
        }
        Ok(())
    }

    /// Move a registered checkout to a new working-tree path while preserving the id used
    /// by durable cross-project ticket references. This is explicit because one ticket
    /// source may intentionally be shared by several checkouts; source overlap alone is
    /// not safe evidence that two paths are the same project.
    pub fn relocate(
        &self,
        reference: &str,
        new_root: &Path,
        alias: Option<&str>,
    ) -> Result<Checkout, CheckoutError> {
        let root = new_root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(new_root.display().to_string()))?;
        let _lock = self.acquire_lock()?;
        let mut file = self.read_locked()?;
        let current = resolve_checkout(file.checkouts.clone(), reference)?;
        let root_text = root.to_string_lossy().into_owned();
        if file
            .checkouts
            .iter()
            .any(|checkout| checkout.id != current.id && checkout.root == root_text)
        {
            return Err(CheckoutError::Invalid(format!(
                "checkout path is already registered: {}",
                root.display()
            )));
        }
        let entry = file
            .checkouts
            .iter_mut()
            .find(|checkout| checkout.id == current.id)
            .expect("resolved checkout remains in the locked registry");
        entry.root = root_text;
        if let Some(alias) = alias {
            entry.alias = alias.to_string();
        }
        let relocated = entry.clone();
        self.write_locked(&file)?;
        Ok(relocated)
    }

    /// Resolve a source by its current id or an id retained by `rename_source`.
    pub fn resolve_source(
        &self,
        checkout_reference: &str,
        source_reference: &str,
    ) -> Result<(Checkout, TicketSource), CheckoutError> {
        let file = self.read()?;
        let checkout = resolve_checkout(file.checkouts.clone(), checkout_reference)?;
        let aliases = file.source_aliases.get(&checkout.id);
        let mut current = source_reference;
        let mut followed = 0;
        while let Some(next) = aliases.and_then(|values| values.get(current)) {
            current = next;
            followed += 1;
            if followed > aliases.map_or(0, BTreeMap::len) {
                return Err(CheckoutError::Invalid(format!(
                    "source alias cycle in checkout {}",
                    checkout.id
                )));
            }
        }
        let source = checkout.source(current).cloned().ok_or_else(|| {
            CheckoutError::NotFound(format!(
                "ticket source {source_reference} in checkout {}",
                checkout.id
            ))
        })?;
        validate_linked_store_identity(&checkout, &source)?;
        Ok((checkout, source))
    }

    fn read(&self) -> Result<RegistryFile, CheckoutError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RegistryFile::default());
            }
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_str(&text) {
            Ok(file) => normalize_registry(file),
            Err(_) => {
                // A writer may already be repairing or replacing this file. Serialize the
                // recovery attempt, then reread the latest bytes while holding the lock.
                let _lock = self.acquire_lock()?;
                self.read_locked()
            }
        }
    }

    fn read_strict_locked(&self) -> Result<RegistryFile, CheckoutError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RegistryFile::default());
            }
            Err(error) => return Err(error.into()),
        };
        let file: RegistryFile = serde_json::from_str(&text)
            .map_err(|error| CheckoutError::Invalid(error.to_string()))?;
        let mut ids = BTreeSet::new();
        let mut roots = BTreeSet::new();
        for checkout in &file.checkouts {
            if !ids.insert(&checkout.id) || !roots.insert(&checkout.root) {
                return Err(CheckoutError::Invalid(
                    "checkout routing identity is ambiguous".into(),
                ));
            }
            unique_source_ids(&checkout.sources)?;
        }
        normalize_registry(file)
    }

    fn read_locked(&self) -> Result<RegistryFile, CheckoutError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(value) => value,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(RegistryFile::default());
            }
            Err(error) => return Err(error.into()),
        };
        match serde_json::from_str(&text) {
            Ok(file) => normalize_registry(file),
            Err(error) => {
                let Some(file) = recover_duplicated_suffix(&text) else {
                    return Err(CheckoutError::Invalid(error.to_string()));
                };
                let file = normalize_registry(file)?;
                self.back_up_corrupt_registry(text.as_bytes())?;
                self.write_locked(&file)?;
                Ok(file)
            }
        }
    }

    fn back_up_corrupt_registry(&self, contents: &[u8]) -> Result<PathBuf, CheckoutError> {
        let backup = self
            .path
            .with_extension(format!("json.corrupt-{}", ulid::Ulid::new()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&backup)?;
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(backup)
    }

    fn write_locked(&self, file: &RegistryFile) -> Result<(), CheckoutError> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let tmp = self
            .path
            .with_extension(format!("json.{}.tmp", ulid::Ulid::new()));
        let result = (|| -> Result<(), io::Error> {
            let mut output = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
            output.write_all(&serde_json::to_vec_pretty(file).expect("serializable registry"))?;
            output.sync_all()?;
            std::fs::rename(&tmp, &self.path)
        })();
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result.map_err(CheckoutError::Io)
    }

    pub fn add_source(
        &self,
        reference: &str,
        source: TicketSource,
        make_default: bool,
    ) -> Result<Checkout, CheckoutError> {
        if source.provider != "git" {
            return Err(source_context_required());
        }
        self.add_git_source(None, reference, source, make_default)
    }

    /// Link an external source only while its provider record exists. This shares the
    /// checkout lock with orphan collection, so a concurrent removal cannot pass its
    /// final link scan and delete the record while this link is being written.
    pub fn add_registered_source(
        &self,
        providers: &ProviderConfigRegistry,
        reference: &str,
        source: TicketSource,
        make_default: bool,
    ) -> Result<Checkout, CheckoutError> {
        if source.provider == "git" {
            return self.add_git_source(Some(providers), reference, source, make_default);
        }
        self.add_external_connection(providers, reference, source, make_default, None, None)
    }

    /// HS2-6H8ZY6: bind a post-write link to the COMPLETE normalized returned
    /// record. Candidate equality narrows current data; it is not new authority.
    pub fn add_registered_connection(
        &self,
        providers: &ProviderConfigRegistry,
        reference: &str,
        connection: &ProviderConnection,
        make_default: bool,
    ) -> Result<Checkout, CheckoutError> {
        self.add_external_connection(
            providers,
            reference,
            TicketSource {
                connection_id: connection.id.clone(),
                provider: connection.provider.clone(),
                locator: connection.locator.clone(),
            },
            make_default,
            Some(connection),
            None,
        )
    }

    /// Publish only the checkout link named by an opaque pending GitHub operation.
    /// The strict checkout snapshot is prepared before the provider owner is entered;
    /// the exact pending candidate and any displaced default are checked under it.
    pub fn add_pending_github_connection(
        &self,
        providers: &ProviderConfigRegistry,
        connection: &ProviderConnection,
        link: &crate::github_endpoint::Link,
        recovery: &crate::github_endpoint::Recovery,
    ) -> Result<Checkout, CheckoutError> {
        if connection.provider != "github" {
            return Err(crate::github_endpoint::EndpointError::Invalid.into());
        }
        self.add_external_connection(
            providers,
            &link.checkout_id,
            TicketSource {
                connection_id: connection.id.clone(),
                provider: connection.provider.clone(),
                locator: connection.locator.clone(),
            },
            link.make_default,
            Some(connection),
            Some((recovery, link)),
        )
    }

    fn add_external_connection(
        &self,
        providers: &ProviderConfigRegistry,
        reference: &str,
        source: TicketSource,
        make_default: bool,
        expected: Option<&ProviderConnection>,
        pending: Option<(
            &crate::github_endpoint::Recovery,
            &crate::github_endpoint::Link,
        )>,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        // Reference canonicalization and legacy link discovery happen BEFORE the provider owner.
        let before = prepared.resolve(reference)?;
        prepared.external_eligibility(&before.id, &source)?;
        let default_hint =
            if before.default_source.is_none() && !before.default_source_cleared && !make_default {
                let mut sources = before.sources.clone();
                sources.retain(|item| item.connection_id != source.connection_id);
                sources.push(source.clone());
                legacy_default_source(Path::new(&before.root), &sources)
                    .or_else(|| (sources.len() == 1).then(|| source.connection_id.clone()))
            } else {
                None
            };
        providers.update_domain(|records| {
            let actual = registered_source(records, &source)?;
            let after = prepared.upsert_external(
                &before.id,
                TicketSource {
                    connection_id: actual.id.clone(),
                    provider: actual.provider.clone(),
                    locator: actual.locator.clone(),
                },
                make_default,
                default_hint,
            )?;
            let displaced = displaced_external(&before, &after)?;
            if let Some((recovery, link)) = pending {
                let expected = expected.ok_or(crate::github_endpoint::EndpointError::Conflict)?;
                exact_connection(actual, expected)?;
                if before.id != link.checkout_id {
                    return Err(crate::github_endpoint::EndpointError::Conflict.into());
                }
                recovery.check_link_candidate(providers, actual, link)?;
                ordinary_link_targets(records, None, displaced.as_ref(), None)?;
            } else {
                ordinary_link_targets(records, Some(actual), displaced.as_ref(), expected)?;
            }
            prepared.publish()?;
            Ok(after)
        })
    }

    /// Select a default with actual source authority, preserving effective
    /// normalized-before semantics and unrelated retained source independence.
    pub fn set_registered_default_source(
        &self,
        providers: &ProviderConfigRegistry,
        reference: &str,
        connection_id: Option<&str>,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        let before = prepared.resolve(reference)?;
        let selected = connection_id
            .map(|id| {
                linked_source(&before, id)?.cloned().ok_or_else(|| {
                    CheckoutError::Invalid(format!(
                        "default source '{id}' is not associated with checkout {}",
                        before.id
                    ))
                })
            })
            .transpose()?;
        let old = before
            .default_source
            .as_deref()
            .map(|id| linked_source(&before, id))
            .transpose()?
            .flatten();
        let selecting_git = selected
            .as_ref()
            .is_some_and(|source| source.provider == "git");
        let displacing_external = old.is_some_and(|source| source.provider != "git")
            && before.default_source.as_deref() != connection_id;
        if (selecting_git && !displacing_external)
            || (connection_id.is_none() && old.is_some_and(|source| source.provider == "git"))
        {
            // Existing pure Git path, outside every provider owner; no lock reentry.
            let (_, after) = prepared.registration(
                Path::new(&before.root),
                Some(&before.alias),
                before.repository.clone(),
                before.sources.clone(),
                connection_id.map(str::to_owned),
                connection_id.is_none(),
            )?;
            return prepared.finish_registration(
                Some(&before),
                after,
                Some(providers),
                connection_id,
            );
        }
        if selecting_git {
            // Read-only recorded identity verification, never initialization/autocommit.
            validate_linked_store_identity(
                &before,
                selected.as_ref().expect("selected Git source exists"),
            )?;
        }
        let after = prepared.set_default(&before.id, connection_id)?;
        let displaced = displaced_external(&before, &after)?;
        if selected.is_none() && displaced.is_none() {
            prepared.publish()?;
            return Ok(after);
        }
        providers.update_domain(|records| {
            let actual = selected
                .as_ref()
                .filter(|source| source.provider != "git")
                .map(|source| registered_source(records, source))
                .transpose()?;
            ordinary_link_targets(records, actual, displaced.as_ref(), None)?;
            prepared.publish()?;
            Ok(after)
        })
    }

    /// Fan out only an exact normalized saved candidate; never reload an ID to
    /// upgrade a caller's post-write snapshot or overwrite a colliding Git member.
    pub fn update_registered_source_locator(
        &self,
        providers: &ProviderConfigRegistry,
        connection: &ProviderConnection,
    ) -> Result<Vec<String>, CheckoutError> {
        if connection.provider == "git" {
            return Err(CheckoutError::Invalid(
                "Git sources use the linked-store workflow".into(),
            ));
        }
        let mut prepared = self.prepare_source_links()?;
        // Validate all checkout kinds before endpoint authority, using in-memory IDs only.
        let updated = prepared.update_external_locator(connection)?;
        providers.update_domain(|records| {
            let actual = registered_source(
                records,
                &TicketSource {
                    connection_id: connection.id.clone(),
                    provider: connection.provider.clone(),
                    locator: connection.locator.clone(),
                },
            )?;
            ordinary_link_targets(records, Some(actual), None, Some(connection))?;
            prepared.publish()?;
            Ok(updated)
        })
    }

    fn add_git_source(
        &self,
        providers: Option<&ProviderConfigRegistry>,
        reference: &str,
        source: TicketSource,
        make_default: bool,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        let mut checkout = prepared.resolve(reference)?;
        if checkout
            .source(&source.connection_id)
            .is_some_and(|old| old.provider != "git")
        {
            return Err(CheckoutError::Invalid(
                "ticket source id is linked to another provider kind".into(),
            ));
        }
        checkout
            .sources
            .retain(|item| item.connection_id != source.connection_id);
        let source_id = source.connection_id.clone();
        checkout.sources.push(source);
        if make_default {
            checkout.default_source = Some(source_id.clone());
        }
        let (before, after) = prepared.registration(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            checkout.default_source,
            false,
        )?;
        prepared.finish_registration(
            before.as_ref(),
            after,
            providers,
            make_default.then_some(source_id.as_str()),
        )
    }

    /// Rename an external provider connection while retaining its previous id for durable
    /// ticket references. Git source ids are derived from their store path and use
    /// `relink_git_source` when that path moves.
    pub fn rename_source(
        &self,
        reference: &str,
        connection_id: &str,
        new_connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        if new_connection_id.is_empty() {
            return Err(CheckoutError::Invalid(
                "new ticket source id cannot be empty".into(),
            ));
        }
        let _lock = self.acquire_lock()?;
        let mut file = self.read_strict_locked()?;
        let checkout = resolve_checkout(file.checkouts.clone(), reference)?;
        if file
            .source_aliases
            .get(&checkout.id)
            .is_some_and(|aliases| aliases.contains_key(new_connection_id))
        {
            return Err(CheckoutError::Invalid(format!(
                "ticket source id '{new_connection_id}' is retained as a historical alias"
            )));
        }
        let entry = file
            .checkouts
            .iter_mut()
            .find(|candidate| candidate.id == checkout.id)
            .expect("resolved checkout remains in the locked registry");
        if entry.source(new_connection_id).is_some() {
            return Err(CheckoutError::Invalid(format!(
                "ticket source '{new_connection_id}' is already associated with checkout {}",
                entry.id
            )));
        }
        let source = entry
            .sources
            .iter_mut()
            .find(|source| source.connection_id == connection_id)
            .ok_or_else(|| CheckoutError::NotFound(connection_id.into()))?;
        if source.provider == "git" {
            return Err(CheckoutError::Invalid(
                "git source ids cannot be renamed; relink the store path instead".into(),
            ));
        }
        if source.provider == "github" {
            return Err(CheckoutError::Invalid("GitHub connection ids cannot be renamed by an ordinary checkout edit; use an explicit provider replacement workflow".into()));
        }
        source.connection_id = new_connection_id.to_string();
        if let Some(color) = entry.source_colors.remove(connection_id) {
            entry
                .source_colors
                .insert(new_connection_id.to_string(), color);
        }
        if entry.default_source.as_deref() == Some(connection_id) {
            entry.default_source = Some(new_connection_id.to_string());
        }
        let aliases = file.source_aliases.entry(entry.id.clone()).or_default();
        for target in aliases.values_mut() {
            if target == connection_id {
                *target = new_connection_id.to_string();
            }
        }
        aliases.insert(connection_id.to_string(), new_connection_id.to_string());
        let renamed = entry.clone();
        self.write_locked(&file)?;
        Ok(renamed)
    }

    /// Relink a moved Git store in every checkout that shares its path-derived source id.
    /// The registry update, including defaults, colors, and historical aliases, is one
    /// locked write. Callers must validate the destination as a Hot Sheet store first.
    pub fn relink_git_source(
        &self,
        checkout_reference: &str,
        connection_id: &str,
        new_path: &Path,
    ) -> Result<Vec<Checkout>, CheckoutError> {
        self.relink_git_source_inner(checkout_reference, connection_id, new_path, false)
    }

    /// Recover a moved pre-identity link after an explicit user review. The destination
    /// remains durably marked unverified; this never overrides a known identity mismatch.
    pub fn recover_unverified_git_source(
        &self,
        checkout_reference: &str,
        connection_id: &str,
        new_path: &Path,
    ) -> Result<Vec<Checkout>, CheckoutError> {
        self.relink_git_source_inner(checkout_reference, connection_id, new_path, true)
    }

    fn relink_git_source_inner(
        &self,
        checkout_reference: &str,
        connection_id: &str,
        new_path: &Path,
        reviewed_unverified_recovery: bool,
    ) -> Result<Vec<Checkout>, CheckoutError> {
        let canonical = new_path
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(new_path.display().to_string()))?;
        let replacement = TicketSource::git(&canonical);
        if !canonical.join(STORE_METADATA_FILE).is_file() {
            return Err(CheckoutError::Invalid(format!(
                "destination is not a Hot Sheet ticket store: {}",
                canonical.display()
            )));
        }
        admit_linked_git_source(&replacement)?;
        let replacement_store = FsStore::open_without_maintenance(&canonical)?;
        let destination_instance_id = replacement_store.metadata()?.instance_id;
        let _lock = self.acquire_lock()?;
        let mut file = self.read_locked()?;
        let selected = resolve_checkout(file.checkouts.clone(), checkout_reference)?;
        let original = selected
            .source(connection_id)
            .filter(|source| source.provider == "git")
            .ok_or_else(|| CheckoutError::NotFound(connection_id.to_string()))?;
        let recorded = selected.store_instance_ids.get(connection_id);
        let shared_recorded = file
            .checkouts
            .iter()
            .filter(|checkout| checkout.source(connection_id).is_some())
            .filter_map(|checkout| checkout.store_instance_ids.get(connection_id))
            .cloned()
            .collect::<Vec<_>>();
        if shared_recorded
            .iter()
            .any(|recorded| destination_instance_id.as_ref() != Some(recorded))
        {
            return Err(CheckoutError::Invalid(
                "This location contains a different Hot Sheet ticket repository.".into(),
            ));
        }
        let original_instance_id = recorded
            .cloned()
            .or_else(|| shared_recorded.first().cloned());
        if original_instance_id
            .as_ref()
            .is_some_and(|id| destination_instance_id.as_ref() != Some(id))
        {
            return Err(CheckoutError::Invalid(
                "This location contains a different Hot Sheet ticket repository.".into(),
            ));
        }
        let unverified = original_instance_id.is_none();
        if unverified && !reviewed_unverified_recovery {
            return Err(CheckoutError::Invalid(
                "original Git store identity was never recorded; relink cannot verify this destination without reviewed unverified recovery".into(),
            ));
        }
        // Do not assign an identity to a destination that the caller has not reviewed.
        let replacement_instance_id = match destination_instance_id {
            Some(id) => id,
            None => replacement_store.ensure_instance_id()?,
        };
        if replacement.connection_id == original.connection_id {
            let mut updated = Vec::new();
            for checkout in &mut file.checkouts {
                if checkout.source(connection_id).is_some()
                    && !checkout.store_instance_ids.contains_key(connection_id)
                {
                    checkout
                        .store_instance_ids
                        .insert(connection_id.to_string(), replacement_instance_id.clone());
                    if unverified {
                        checkout
                            .unverified_store_sources
                            .insert(connection_id.to_string(), original.locator.clone());
                    }
                    updated.push(checkout.clone());
                }
            }
            if !updated.is_empty() {
                self.write_locked(&file)?;
            }
            return Ok(updated);
        }
        let mut updated = Vec::new();
        for checkout in &mut file.checkouts {
            if !checkout
                .source(connection_id)
                .is_some_and(|source| source.provider == "git")
            {
                continue;
            }
            if checkout.source(&replacement.connection_id).is_some() {
                return Err(CheckoutError::Invalid(format!(
                    "destination ticket source is already linked to checkout {}",
                    checkout.id
                )));
            }
            // Every shared recorded identity was checked against the destination above.
            let aliases = file.source_aliases.entry(checkout.id.clone()).or_default();
            if aliases
                .get(&replacement.connection_id)
                .is_some_and(|target| target != connection_id)
            {
                return Err(CheckoutError::Invalid(format!(
                    "destination ticket source id is retained for another source in checkout {}",
                    checkout.id
                )));
            }
            aliases.remove(&replacement.connection_id);
            for target in aliases.values_mut() {
                if target == connection_id {
                    *target = replacement.connection_id.clone();
                }
            }
            aliases.insert(connection_id.to_string(), replacement.connection_id.clone());
            let source = checkout
                .sources
                .iter_mut()
                .find(|source| source.connection_id == connection_id)
                .expect("checked source remains linked");
            *source = replacement.clone();
            checkout.store_instance_ids.remove(connection_id);
            checkout.store_instance_ids.insert(
                replacement.connection_id.clone(),
                replacement_instance_id.clone(),
            );
            let former = checkout.unverified_store_sources.remove(connection_id);
            if unverified {
                checkout
                    .unverified_store_sources
                    .insert(replacement.connection_id.clone(), original.locator.clone());
            } else if let Some(former) = former {
                checkout
                    .unverified_store_sources
                    .insert(replacement.connection_id.clone(), former);
            }
            checkout.stores = checkout
                .sources
                .iter()
                .filter(|source| source.provider == "git")
                .map(|source| source.locator.clone())
                .collect();
            checkout.stores.sort();
            if checkout.default_source.as_deref() == Some(connection_id) {
                checkout.default_source = Some(replacement.connection_id.clone());
            }
            if let Some(color) = checkout.source_colors.remove(connection_id) {
                checkout
                    .source_colors
                    .insert(replacement.connection_id.clone(), color);
            }
            updated.push(checkout.clone());
        }
        self.write_locked(&file)?;
        Ok(updated)
    }

    /// Rewrite the copied locator of every checkout link that names the external connection
    /// `connection_id`, so editing a connection's repository or project for every project
    /// reaches each linked checkout (HS2-RCBKA3). Git links are path-derived and never change
    /// here. Returns the ids of the checkouts whose link changed; an unchanged locator is a
    /// no-op that leaves `checkouts.json` untouched.
    pub fn update_source_locator(
        &self,
        connection_id: &str,
        locator: &str,
    ) -> Result<Vec<String>, CheckoutError> {
        let _lock = self.acquire_lock()?;
        let file = self.read_strict_locked()?;
        if file
            .checkouts
            .iter()
            .flat_map(|row| &row.sources)
            .any(|source| {
                source.connection_id == connection_id
                    && source.provider != "git"
                    && source.locator != locator
            })
        {
            return Err(source_context_required());
        }
        Ok(Vec::new())
    }

    pub fn remove_source(
        &self,
        reference: &str,
        connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        let mut checkout = prepared.resolve(reference)?;
        if checkout
            .source(connection_id)
            .is_some_and(|source| source.provider != "git")
        {
            return Err(source_context_required());
        }
        let before = checkout.sources.len();
        checkout
            .sources
            .retain(|source| source.connection_id != connection_id);
        if checkout.sources.len() == before {
            return Err(CheckoutError::NotFound(connection_id.into()));
        }
        if checkout.default_source.as_deref() == Some(connection_id) {
            checkout.default_source = None;
        }
        let (before, after) = prepared.registration(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            checkout.default_source,
            false,
        )?;
        prepared.finish_registration(before.as_ref(), after, None, None)
    }

    /// Set this project's visual accent for a linked source. It has no effect on projects
    /// sharing the same provider connection. The default Gray is represented by no map entry.
    pub fn set_source_color(
        &self,
        reference: &str,
        connection_id: &str,
        color: &str,
    ) -> Result<Checkout, CheckoutError> {
        if !SOURCE_COLORS.contains(&color) {
            return Err(CheckoutError::Invalid(format!(
                "unsupported ticket source color '{color}'"
            )));
        }
        let _lock = self.acquire_lock()?;
        let mut file = self.read_locked()?;
        let checkout = resolve_checkout(file.checkouts.clone(), reference)?;
        let entry = file
            .checkouts
            .iter_mut()
            .find(|candidate| candidate.id == checkout.id)
            .expect("resolved checkout remains in the locked registry");
        if entry.source(connection_id).is_none() {
            return Err(CheckoutError::NotFound(connection_id.into()));
        }
        if color == DEFAULT_SOURCE_COLOR {
            entry.source_colors.remove(connection_id);
        } else {
            entry
                .source_colors
                .insert(connection_id.to_string(), color.to_string());
        }
        let changed = entry.clone();
        self.write_locked(&file)?;
        Ok(changed)
    }

    pub fn set_default_source(
        &self,
        reference: &str,
        connection_id: Option<&str>,
    ) -> Result<Checkout, CheckoutError> {
        let mut prepared = self.prepare_source_links()?;
        let checkout = prepared.resolve(reference)?;
        let (before, after) = prepared.registration(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            connection_id.map(str::to_owned),
            connection_id.is_none(),
        )?;
        prepared.finish_registration(before.as_ref(), after, None, connection_id)
    }
}

fn resolve_checkout(entries: Vec<Checkout>, reference: &str) -> Result<Checkout, CheckoutError> {
    let canonical = Path::new(reference).canonicalize().ok();
    let matches: Vec<_> = entries
        .into_iter()
        .filter(|checkout| {
            checkout.id == reference
                || checkout.id.starts_with(reference)
                || checkout.alias == reference
                || canonical
                    .as_ref()
                    .is_some_and(|path| checkout.root == path.to_string_lossy())
        })
        .collect();
    match matches.as_slice() {
        [one] => Ok(one.clone()),
        [] => Err(CheckoutError::NotFound(reference.to_owned())),
        _ => Err(CheckoutError::Ambiguous(reference.to_owned())),
    }
}

/// HS2-X4RZ73: removal is destructive, so it accepts only an exact id, a root path, or an
/// alias that names one checkout — never the id prefix [`resolve_checkout`] allows. A path
/// matches even after its directory was deleted, because a vanished checkout is the usual
/// reason to remove one. `None` means nothing is registered under that reference.
fn resolve_for_removal(
    entries: &[Checkout],
    reference: &str,
) -> Result<Option<Checkout>, CheckoutError> {
    let reference = reference.trim();
    if reference.is_empty() {
        return Err(CheckoutError::Invalid(
            "checkout reference is empty".to_owned(),
        ));
    }
    if let Some(entry) = entries.iter().find(|entry| entry.id == reference) {
        return Ok(Some(entry.clone()));
    }
    let paths = removal_path_candidates(Path::new(reference));
    if let Some(entry) = entries.iter().find(|entry| paths.contains(&entry.root)) {
        return Ok(Some(entry.clone()));
    }
    let aliased: Vec<_> = entries
        .iter()
        .filter(|entry| entry.alias == reference)
        .collect();
    match aliased.as_slice() {
        [] => Ok(None),
        [one] => Ok(Some((*one).clone())),
        _ => Err(CheckoutError::Ambiguous(format!(
            "{reference} is the alias of {} checkouts ({}); remove one by its id or path",
            aliased.len(),
            aliased
                .iter()
                .map(|entry| entry.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ))),
    }
}

/// The registry spellings a removal path may have: its canonical form when it exists, or
/// its canonical parent joined with its name once the directory itself is gone.
fn removal_path_candidates(reference: &Path) -> Vec<String> {
    let absolute = if reference.is_absolute() {
        reference.to_path_buf()
    } else {
        match std::env::current_dir() {
            Ok(cwd) => cwd.join(reference),
            Err(_) => return Vec::new(),
        }
    };
    let mut paths = vec![absolute.to_string_lossy().into_owned()];
    if let Ok(canonical) = absolute.canonicalize() {
        paths.push(canonical.to_string_lossy().into_owned());
    } else if let (Some(parent), Some(name)) = (absolute.parent(), absolute.file_name())
        && let Ok(parent) = parent.canonicalize()
    {
        paths.push(parent.join(name).to_string_lossy().into_owned());
    }
    paths
}

fn normalize_registry(mut file: RegistryFile) -> Result<RegistryFile, CheckoutError> {
    if file.schema_version > CHECKOUT_REGISTRY_SCHEMA_VERSION {
        return Err(CheckoutError::UpgradeRequired {
            found: file.schema_version,
            supported: CHECKOUT_REGISTRY_SCHEMA_VERSION,
        });
    }
    file.schema_version = CHECKOUT_REGISTRY_SCHEMA_VERSION;
    for checkout in &mut file.checkouts {
        if let Some(connection_id) = migrate_checkout(checkout) {
            file.link_derived_sources
                .insert((checkout.id.clone(), connection_id));
        }
    }
    Ok(file)
}

/// Recover only the race signature produced by an older shared-temp writer: one complete
/// registry followed by an exact duplicated suffix of that same document. Any unrelated
/// trailing bytes remain a hard error so recovery cannot silently discard new data.
fn recover_duplicated_suffix(text: &str) -> Option<RegistryFile> {
    let mut values = serde_json::Deserializer::from_str(text).into_iter::<RegistryFile>();
    let file = values.next()?.ok()?;
    let offset = values.byte_offset();
    let complete = text.get(..offset)?.trim_end();
    let trailing = text.get(offset..)?.trim();
    (trailing.len() >= 4
        && trailing.contains(']')
        && trailing.ends_with('}')
        && complete.ends_with(trailing))
    .then_some(file)
}

fn legacy_default_source(root: &Path, sources: &[TicketSource]) -> Option<String> {
    let linked = legacy_link_source(root)?;
    sources
        .iter()
        .find(|source| source.connection_id == linked.connection_id)
        .map(|source| source.connection_id.clone())
}

fn legacy_link_source(root: &Path) -> Option<TicketSource> {
    let link = [".hotsheet2/store", ".hotsheet/store"]
        .into_iter()
        .find_map(|path| {
            std::fs::read_to_string(root.join(path))
                .ok()
                .filter(|link| !link.trim().is_empty())
        })?;
    let path = PathBuf::from(link.trim());
    let path = if path.is_absolute() {
        path
    } else {
        root.join(path)
    };
    Some(TicketSource::git(path))
}

/// Returns the connection id of a source adopted from the root's link file, if any.
fn migrate_checkout(checkout: &mut Checkout) -> Option<String> {
    let mut link_derived = None;
    if checkout.sources.is_empty() {
        checkout.sources = checkout.stores.iter().map(TicketSource::git).collect();
        if checkout.sources.is_empty()
            && let Some(source) = legacy_link_source(Path::new(&checkout.root))
        {
            link_derived = Some(source.connection_id.clone());
            checkout.sources.push(source);
        }
    }
    if checkout.stores.is_empty() {
        checkout.stores = checkout
            .sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.locator.clone())
            .collect();
    }
    if checkout
        .default_source
        .as_ref()
        .is_some_and(|default| checkout.source(default).is_none())
    {
        checkout.default_source = None;
    }
    if checkout.default_source.is_some() {
        checkout.default_source_cleared = false;
    }
    if checkout.default_source.is_none() && !checkout.default_source_cleared {
        checkout.default_source =
            legacy_default_source(Path::new(&checkout.root), &checkout.sources).or_else(|| {
                (checkout.sources.len() == 1).then(|| checkout.sources[0].connection_id.clone())
            });
    }
    link_derived
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConnection;

    fn source(connection_id: &str, provider: &str) -> TicketSource {
        TicketSource {
            connection_id: connection_id.into(),
            provider: provider.into(),
            locator: format!("{connection_id}-locator"),
        }
    }

    fn claim_checkout(sources: Vec<TicketSource>, default_source: Option<&str>) -> Checkout {
        let mut checkout: Checkout = serde_json::from_value(serde_json::json!({
            "id": "project-0123456789", "root": "project", "alias": "project"
        }))
        .unwrap();
        checkout.sources = sources;
        checkout.default_source = default_source.map(str::to_owned);
        checkout
    }

    #[test]
    fn hs2_av80c4_claim_next_sources_put_the_default_first_and_skip_providers() {
        let checkout = claim_checkout(
            vec![
                source("first", "git"),
                source("issues", "github"),
                source("second", "git"),
                source("third", "git"),
            ],
            Some("third"),
        );
        let ids = |checkout: &Checkout| {
            checkout
                .claim_next_sources()
                .unwrap()
                .iter()
                .map(|source| source.connection_id.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&checkout), ["third", "first", "second"]);
        // A provider or absent default keeps registry order.
        for default in [Some("issues"), None] {
            let checkout = claim_checkout(checkout.sources.clone(), default);
            assert_eq!(ids(&checkout), ["first", "second", "third"]);
        }
    }

    #[test]
    fn hs2_av80c4_claim_next_names_unsupported_provider_only_sources() {
        let providers = claim_checkout(
            vec![
                source("work", "jira"),
                source("issues", "github"),
                source("more", "github"),
            ],
            Some("work"),
        );
        let error = providers.claim_next_sources().unwrap_err();
        assert!(
            error.contains("claim-next is unsupported by github, jira ticket sources"),
            "{error}"
        );
        let empty = claim_checkout(Vec::new(), None);
        assert_eq!(
            empty.claim_next_sources().unwrap_err(),
            "checkout has no linked ticket sources to claim from"
        );
    }

    /// HS2-K2YHDE: the app opens a new project folder before it creates the sibling
    /// store, so the store is linked into an already-registered checkout. That link must
    /// still record the store identity, or the next open reports "no recorded identity".
    #[test]
    fn linking_a_new_store_into_a_registered_sourceless_checkout_records_its_identity() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        registry
            .open_sources(
                &project,
                None,
                None,
                Vec::new(),
                None,
                OpenSourceMode::Discovered,
            )
            .unwrap();
        let store_path = temp.path().join("project.hs2");
        let store =
            FsStore::init_git(&store_path, &crate::store::StoreMetadata::new("HS2")).unwrap();
        // Bootstrap writes the checkout's store link before it registers the source, so
        // the sourceless entry already reads back with a link-derived source here.
        std::fs::create_dir_all(project.join(".hotsheet2")).unwrap();
        std::fs::write(
            project.join(".hotsheet2/store"),
            store_path
                .canonicalize()
                .unwrap()
                .to_string_lossy()
                .as_bytes(),
        )
        .unwrap();
        assert_eq!(registry.list().unwrap()[0].sources.len(), 1);
        let linked = registry
            .open_git_store(&project, None, None, &store_path)
            .unwrap();
        let source_id = linked.sources[0].connection_id.clone();
        assert_eq!(
            linked.store_instance_ids.get(&source_id),
            store.metadata().unwrap().instance_id.as_ref()
        );
        registry.resolve_source(&linked.id, &source_id).unwrap();
    }

    #[test]
    fn registered_links_must_carry_their_connection_locator() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        std::fs::create_dir(&project).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        providers
            .save(&[ProviderConnection {
                id: "jira-app".into(),
                provider: "jira".into(),
                locator: "APP".into(),
                name: None,
                default: false,
                settings: serde_json::Value::Null,
                disabled: false,
            }])
            .unwrap();
        registry
            .register(&project, Some("app"), None, Vec::new())
            .unwrap();
        let link = |locator: &str| TicketSource {
            connection_id: "jira-app".into(),
            provider: "jira".into(),
            locator: locator.into(),
        };
        // A mismatched project key would silently stop matching unqualified APP-N ids.
        let error = registry
            .add_registered_source(&providers, "app", link("OPS"), false)
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("locator does not match its registered connection"),
            "{error}"
        );
        assert!(matches!(
            registry.register_registered_sources(
                &providers,
                &project,
                None,
                None,
                vec![link("OPS")],
                None
            ),
            Err(CheckoutError::Invalid(_))
        ));
        assert!(registry.resolve("app").unwrap().sources.is_empty());
        // Locators compare case-insensitively, like the offline native-id shape check.
        let linked = registry
            .add_registered_source(&providers, "app", link("app"), true)
            .unwrap();
        assert_eq!(linked.sources.len(), 1);
        assert_eq!(linked.default_source.as_deref(), Some("jira-app"));
    }

    #[test]
    fn registered_project_links_require_a_live_provider_record() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        let connection = ProviderConnection {
            id: "github-main".into(),
            provider: "github".into(),
            locator: "acme/repo".into(),
            name: None,
            default: false,
            settings: serde_json::Value::Null,
            disabled: false,
        };
        let source = TicketSource {
            connection_id: connection.id.clone(),
            provider: connection.provider.clone(),
            locator: connection.locator.clone(),
        };
        providers.save(std::slice::from_ref(&connection)).unwrap();
        registry
            .register_registered_sources(&providers, &first, None, None, vec![source.clone()], None)
            .unwrap();
        providers.save(&[]).unwrap();
        assert!(matches!(
            registry.register_registered_sources(
                &providers,
                &second,
                None,
                None,
                vec![source.clone()],
                None
            ),
            Err(CheckoutError::Invalid(_))
        ));
        assert!(matches!(
            registry.open_registered_sources(
                &providers,
                &second,
                None,
                None,
                OpenSourceSelection {
                    sources: vec![source.clone()],
                    default_source: None,
                    mode: OpenSourceMode::Explicit,
                }
            ),
            Err(CheckoutError::Invalid(_))
        ));
        assert_eq!(registry.list().unwrap().len(), 1);
        let reopened = registry
            .open_registered_sources(
                &providers,
                &first,
                None,
                None,
                OpenSourceSelection {
                    sources: Vec::new(),
                    default_source: None,
                    mode: OpenSourceMode::Discovered,
                },
            )
            .unwrap();
        assert!(reopened.source("github-main").is_some());
        providers.save(&[connection]).unwrap();
        assert!(
            registry
                .open_registered_sources(
                    &providers,
                    &second,
                    None,
                    None,
                    OpenSourceSelection {
                        sources: vec![source],
                        default_source: None,
                        mode: OpenSourceMode::Explicit,
                    },
                )
                .unwrap()
                .source("github-main")
                .is_some()
        );
    }

    #[test]
    fn ids_are_readable_stable_and_distinguish_checkouts() {
        let temp = tempfile::tempdir().unwrap();
        let a = temp.path().join("project");
        let b = temp.path().join("other").join("project");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        assert!(checkout_id(&a).unwrap().starts_with("project-"));
        assert_eq!(checkout_id(&a).unwrap(), checkout_id(&a).unwrap());
        assert_ne!(checkout_id(&a).unwrap(), checkout_id(&b).unwrap());
    }

    #[test]
    fn registry_supports_many_to_many_and_resolution() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let store_a = temp.path().join("tickets-a");
        let store_b = temp.path().join("tickets-b");
        std::fs::create_dir(&checkout).unwrap();
        FsStore::init_git(&store_a, &crate::store::StoreMetadata::new("A")).unwrap();
        FsStore::init_git(&store_b, &crate::store::StoreMetadata::new("B")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let saved = registry
            .register(
                &checkout,
                Some("frontend"),
                Some("github.com/acme/app".into()),
                vec![store_a, store_b],
            )
            .unwrap();
        assert_eq!(saved.stores.len(), 2);
        assert_eq!(saved.sources.len(), 2);
        assert!(saved.default_source.is_none());
        assert_eq!(registry.resolve("frontend").unwrap(), saved);
        assert_eq!(registry.resolve(&saved.id[..8]).unwrap(), saved);
        assert_eq!(registry.resolve(checkout.to_str().unwrap()).unwrap(), saved);
    }

    #[test]
    fn permission_vault_registration_survives_registry_reopen_and_checkout_refresh() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        std::fs::create_dir(&checkout).unwrap();
        let path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&path);
        let saved = registry
            .register(&checkout, None, None, Vec::new())
            .unwrap();
        assert!(!saved.permission_vault_registered);
        registry
            .mark_permission_vault_registered(&saved.id)
            .unwrap();

        let reopened = CheckoutRegistry::new(&path);
        assert!(
            reopened
                .describe(&saved.id)
                .unwrap()
                .permission_vault_registered
        );
        assert!(
            reopened
                .register(&checkout, Some("renamed"), None, Vec::new())
                .unwrap()
                .permission_vault_registered
        );
        assert!(
            reopened
                .describe(&saved.id)
                .unwrap()
                .permission_vault_registered
        );
    }

    #[test]
    fn unversioned_registry_remains_readable_and_future_registry_requires_upgrade() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("checkouts.json");
        std::fs::write(&path, r#"{"checkouts":[]}"#).unwrap();
        let registry = CheckoutRegistry::new(&path);
        assert!(registry.list().unwrap().is_empty());
        std::fs::write(&path, r#"{"schemaVersion":99,"checkouts":[]}"#).unwrap();
        let error = registry.list().unwrap_err().to_string();
        assert!(error.contains("newer version of Hot Sheet 2"));
        assert!(error.contains("Update Hot Sheet 2"));
    }

    #[test]
    fn recovers_a_duplicated_writer_suffix_and_preserves_the_corrupt_bytes() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        std::fs::create_dir(&checkout).unwrap();
        let path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&path);
        let saved = registry
            .register(&checkout, None, None, Vec::new())
            .unwrap();
        let valid = std::fs::read_to_string(&path).unwrap();
        let corrupt = format!("{valid}{}", valid.trim());
        std::fs::write(&path, &corrupt).unwrap();

        assert_eq!(registry.list().unwrap(), vec![saved]);
        let repaired = std::fs::read_to_string(&path).unwrap();
        serde_json::from_str::<RegistryFile>(&repaired).unwrap();
        let backups = std::fs::read_dir(temp.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("checkouts.json.corrupt-")
            })
            .collect::<Vec<_>>();
        assert_eq!(backups.len(), 1);
        assert_eq!(std::fs::read_to_string(backups[0].path()).unwrap(), corrupt);
    }

    #[test]
    fn refuses_to_discard_unrelated_trailing_registry_data() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("checkouts.json");
        let corrupt = r#"{"schemaVersion":2,"checkouts":[]} unrelated"#;
        std::fs::write(&path, corrupt).unwrap();
        let registry = CheckoutRegistry::new(&path);

        assert!(
            registry
                .list()
                .unwrap_err()
                .to_string()
                .contains("trailing")
        );
        assert_eq!(std::fs::read_to_string(&path).unwrap(), corrupt);
        assert!(
            std::fs::read_dir(temp.path())
                .unwrap()
                .filter_map(Result::ok)
                .all(|entry| !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("checkouts.json.corrupt-"))
        );
    }

    #[test]
    fn concurrent_registry_writers_preserve_every_checkout_and_valid_json() {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&path);
        let count = 24;
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(count));
        let roots = (0..count)
            .map(|index| {
                let root = temp.path().join(format!("project-{index}"));
                std::fs::create_dir(&root).unwrap();
                root
            })
            .collect::<Vec<_>>();
        let handles = roots
            .into_iter()
            .map(|root| {
                let registry = registry.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    registry.register(&root, None, None, Vec::new()).unwrap()
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            handle.join().unwrap();
        }

        assert_eq!(registry.list().unwrap().len(), count);
        serde_json::from_str::<RegistryFile>(&std::fs::read_to_string(path).unwrap()).unwrap();
    }

    #[test]
    fn migrates_legacy_stores_and_link_into_explicit_sources_and_default() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let store_a = temp.path().join("a.hs2");
        let store_b = temp.path().join("b.hs2");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::create_dir(&store_a).unwrap();
        std::fs::create_dir(&store_b).unwrap();
        std::fs::create_dir(checkout.join(".hotsheet")).unwrap();
        std::fs::write(
            checkout.join(".hotsheet/store"),
            store_b.to_string_lossy().as_bytes(),
        )
        .unwrap();
        let path = temp.path().join("checkouts.json");
        std::fs::write(
            &path,
            serde_json::json!({"schemaVersion":1,"checkouts":[{
                "id":checkout_id(&checkout).unwrap(),"root":checkout,"alias":"app",
                "stores":[store_a,store_b]
            }]})
            .to_string(),
        )
        .unwrap();
        let migrated = CheckoutRegistry::new(&path).list().unwrap().remove(0);
        assert_eq!(migrated.sources.len(), 2);
        assert_eq!(
            migrated
                .source(migrated.default_source.as_deref().unwrap())
                .unwrap()
                .locator,
            store_b.canonicalize().unwrap().to_string_lossy()
        );
    }

    #[test]
    fn migrated_checkout_links_prefer_hs2_and_fall_back_from_an_empty_pointer() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let canonical = temp.path().join("canonical.hs2");
        let legacy = temp.path().join("legacy.hs2");
        std::fs::create_dir_all(checkout.join(".hotsheet2")).unwrap();
        std::fs::create_dir_all(checkout.join(".hotsheet")).unwrap();
        std::fs::write(
            checkout.join(".hotsheet2/store"),
            canonical.to_string_lossy().as_bytes(),
        )
        .unwrap();
        std::fs::write(
            checkout.join(".hotsheet/store"),
            legacy.to_string_lossy().as_bytes(),
        )
        .unwrap();

        assert_eq!(
            legacy_link_source(&checkout).unwrap().locator,
            canonical.to_string_lossy()
        );

        std::fs::write(checkout.join(".hotsheet2/store"), "\n").unwrap();
        assert_eq!(
            legacy_link_source(&checkout).unwrap().locator,
            legacy.to_string_lossy()
        );
    }

    #[test]
    fn external_sources_defaults_and_removal_are_explicit_and_many_to_many() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let external = TicketSource {
            connection_id: "github-acme".into(),
            provider: "github".into(),
            locator: "acme/issues".into(),
        };
        let alternate = TicketSource {
            connection_id: "jira-eng".into(),
            provider: "jira".into(),
            locator: "ENG".into(),
        };
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        providers
            .save(
                &[&external, &alternate]
                    .into_iter()
                    .map(|source| ProviderConnection {
                        id: source.connection_id.clone(),
                        provider: source.provider.clone(),
                        locator: source.locator.clone(),
                        name: None,
                        default: false,
                        disabled: false,
                        settings: serde_json::json!({}),
                    })
                    .collect::<Vec<_>>(),
            )
            .unwrap();
        for root in [&first, &second] {
            registry
                .register_registered_sources(
                    &providers,
                    root,
                    None,
                    None,
                    vec![external.clone()],
                    Some("github-acme".into()),
                )
                .unwrap();
        }
        assert_eq!(registry.list().unwrap().len(), 2);
        registry
            .add_registered_source(&providers, first.to_str().unwrap(), alternate, false)
            .unwrap();
        let changed = registry
            .set_registered_default_source(&providers, first.to_str().unwrap(), Some("jira-eng"))
            .unwrap();
        assert_eq!(changed.default_source.as_deref(), Some("jira-eng"));
        crate::connection_removal::detach_source(
            &providers,
            &registry,
            first.to_str().unwrap(),
            "jira-eng",
        )
        .unwrap();
        let removed_default = registry.resolve(first.to_str().unwrap()).unwrap();
        assert_eq!(
            removed_default.default_source.as_deref(),
            Some("github-acme")
        );
        let changed = registry
            .set_registered_default_source(&providers, first.to_str().unwrap(), None)
            .unwrap();
        assert!(changed.default_source.is_none());
        crate::connection_removal::detach_source(
            &providers,
            &registry,
            first.to_str().unwrap(),
            "github-acme",
        )
        .unwrap();
        let removed = registry.resolve(first.to_str().unwrap()).unwrap();
        assert!(removed.sources.is_empty());
        assert!(
            registry
                .set_registered_default_source(
                    &providers,
                    second.to_str().unwrap(),
                    Some("missing")
                )
                .unwrap_err()
                .to_string()
                .contains("not associated")
        );
    }

    #[test]
    fn source_colors_are_project_local_and_follow_source_lifecycle() {
        assert_eq!(effective_source_color(None), DEFAULT_SOURCE_COLOR);
        assert_eq!(
            effective_source_color(Some("transparent")),
            DEFAULT_SOURCE_COLOR
        );
        assert_eq!(effective_source_color(Some("#3b82f6")), "#3b82f6");
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&path);
        let source = TicketSource {
            connection_id: "github-a".into(),
            provider: "github".into(),
            locator: "acme/a".into(),
        };
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        providers
            .save(&[ProviderConnection {
                id: source.connection_id.clone(),
                provider: source.provider.clone(),
                locator: source.locator.clone(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({}),
            }])
            .unwrap();
        for root in [&first, &second] {
            registry
                .register_registered_sources(
                    &providers,
                    root,
                    None,
                    None,
                    vec![source.clone()],
                    None,
                )
                .unwrap();
        }
        let first_ref = first.to_str().unwrap();
        let second_ref = second.to_str().unwrap();
        assert!(
            registry
                .resolve(first_ref)
                .unwrap()
                .source_colors
                .is_empty()
        );
        registry
            .set_source_color(first_ref, "github-a", "#3b82f6")
            .unwrap();
        assert!(
            registry
                .resolve(second_ref)
                .unwrap()
                .source_colors
                .is_empty()
        );
        registry
            .register_registered_sources(&providers, &first, None, None, vec![source.clone()], None)
            .unwrap();
        let reopened = CheckoutRegistry::new(&path);
        assert_eq!(
            reopened.resolve(first_ref).unwrap().source_colors["github-a"],
            "#3b82f6"
        );
        assert!(
            reopened
                .set_source_color(first_ref, "missing", "#ef4444")
                .is_err()
        );
        assert!(
            reopened
                .set_source_color(first_ref, "github-a", "red")
                .is_err()
        );
        assert!(
            reopened
                .rename_source(first_ref, "github-a", "github-b")
                .is_err()
        );
        assert_eq!(
            reopened.resolve(first_ref).unwrap().source_colors["github-a"],
            "#3b82f6"
        );
        reopened
            .set_source_color(first_ref, "github-a", DEFAULT_SOURCE_COLOR)
            .unwrap();
        assert!(
            reopened
                .resolve(first_ref)
                .unwrap()
                .source_colors
                .is_empty()
        );
        reopened
            .set_source_color(first_ref, "github-a", "#14b8a6")
            .unwrap();
        crate::connection_removal::detach_source(&providers, &reopened, first_ref, "github-a")
            .unwrap();
        assert!(
            reopened
                .resolve(first_ref)
                .unwrap()
                .source_colors
                .is_empty()
        );
    }

    #[test]
    fn clearing_a_default_survives_reads_reopen_and_source_changes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&path);
        let github = TicketSource {
            connection_id: "github-issues".into(),
            provider: "github".into(),
            locator: "example/issues".into(),
        };
        let jira = TicketSource {
            connection_id: "jira-eng".into(),
            provider: "jira".into(),
            locator: "ENG".into(),
        };
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        let connection = |source: &TicketSource| ProviderConnection {
            id: source.connection_id.clone(),
            provider: source.provider.clone(),
            locator: source.locator.clone(),
            name: None,
            default: false,
            disabled: false,
            settings: serde_json::json!({}),
        };
        providers
            .save(&[connection(&github), connection(&jira)])
            .unwrap();
        registry
            .register_registered_sources(
                &providers,
                &root,
                None,
                None,
                vec![github.clone()],
                Some(github.connection_id.clone()),
            )
            .unwrap();
        let cleared = registry
            .set_registered_default_source(&providers, root.to_str().unwrap(), None)
            .unwrap();
        assert!(cleared.default_source.is_none());
        assert!(cleared.default_source_cleared);
        let reopened = CheckoutRegistry::new(&path);
        assert!(
            reopened
                .resolve(root.to_str().unwrap())
                .unwrap()
                .default_source
                .is_none()
        );
        assert!(reopened.list().unwrap()[0].default_source_cleared);

        assert!(
            reopened
                .add_registered_source(&providers, root.to_str().unwrap(), jira, false)
                .unwrap()
                .default_source
                .is_none()
        );
        crate::connection_removal::detach_source(
            &providers,
            &reopened,
            root.to_str().unwrap(),
            "jira-eng",
        )
        .unwrap();
        assert!(
            reopened
                .resolve(root.to_str().unwrap())
                .unwrap()
                .default_source
                .is_none()
        );
        crate::connection_removal::detach_source(
            &providers,
            &reopened,
            root.to_str().unwrap(),
            &github.connection_id,
        )
        .unwrap();
        let empty = reopened.resolve(root.to_str().unwrap()).unwrap();
        assert!(empty.sources.is_empty());
        assert!(empty.default_source_cleared);
        providers.save(&[connection(&github)]).unwrap();
        assert!(
            reopened
                .add_registered_source(&providers, root.to_str().unwrap(), github.clone(), false)
                .unwrap()
                .default_source
                .is_none()
        );
        assert!(
            reopened
                .open_registered_sources(
                    &providers,
                    &root,
                    None,
                    None,
                    OpenSourceSelection {
                        sources: vec![github.clone()],
                        default_source: None,
                        mode: OpenSourceMode::Discovered,
                    }
                )
                .unwrap()
                .default_source
                .is_none()
        );
        let selected = reopened
            .set_registered_default_source(
                &providers,
                root.to_str().unwrap(),
                Some(&github.connection_id),
            )
            .unwrap();
        assert_eq!(selected.default_source.as_deref(), Some("github-issues"));
        assert!(!selected.default_source_cleared);

        // An older file with no clear marker still infers its sole linked source.
        reopened
            .set_registered_default_source(&providers, root.to_str().unwrap(), None)
            .unwrap();
        let mut legacy: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let entry = legacy["checkouts"][0].as_object_mut().unwrap();
        entry.remove("default_source_cleared");
        entry.remove("default_source");
        std::fs::write(&path, serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert_eq!(
            CheckoutRegistry::new(&path)
                .resolve(root.to_str().unwrap())
                .unwrap()
                .default_source
                .as_deref(),
            Some("github-issues")
        );
    }

    #[test]
    fn reopening_preserves_external_links_defaults_and_explicit_removal() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        let store = temp.path().join("app.hs2");
        std::fs::create_dir(&root).unwrap();
        FsStore::init_git(&store, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let git = TicketSource::git(&store);
        let github = TicketSource {
            connection_id: "github-issues".into(),
            provider: "github".into(),
            locator: "acme/app".into(),
        };
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        providers
            .save(&[ProviderConnection {
                id: github.connection_id.clone(),
                provider: github.provider.clone(),
                locator: github.locator.clone(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({}),
            }])
            .unwrap();
        registry
            .register_registered_sources(
                &providers,
                &root,
                Some("renamed app"),
                Some("acme/app".into()),
                vec![git.clone(), github.clone()],
                Some(github.connection_id.clone()),
            )
            .unwrap();

        let reopened = registry
            .open_registered_sources(
                &providers,
                &root,
                None,
                None,
                OpenSourceSelection {
                    sources: vec![git.clone()],
                    default_source: None,
                    mode: OpenSourceMode::SelectedGitStore,
                },
            )
            .unwrap();
        assert_eq!(reopened.alias, "renamed app");
        assert_eq!(reopened.repository.as_deref(), Some("acme/app"));
        assert_eq!(reopened.sources.len(), 2);
        assert_eq!(reopened.source(&github.connection_id), Some(&github));
        assert_eq!(reopened.default_source, Some(github.connection_id.clone()));

        let discovered = registry
            .open_registered_sources(
                &providers,
                &root,
                None,
                None,
                OpenSourceSelection {
                    sources: vec![],
                    default_source: None,
                    mode: OpenSourceMode::Discovered,
                },
            )
            .unwrap();
        assert_eq!(discovered.sources.len(), 2);
        assert_eq!(
            discovered.default_source,
            Some(github.connection_id.clone())
        );

        crate::connection_removal::detach_source(
            &providers,
            &registry,
            &reopened.id,
            &github.connection_id,
        )
        .unwrap();
        let removed = registry
            .open_registered_sources(
                &providers,
                &root,
                None,
                None,
                OpenSourceSelection {
                    sources: vec![git.clone()],
                    default_source: None,
                    mode: OpenSourceMode::SelectedGitStore,
                },
            )
            .unwrap();
        assert!(removed.source(&github.connection_id).is_none());
        assert_eq!(removed.default_source, Some(git.connection_id));
    }

    #[test]
    fn opening_a_relinked_git_store_keeps_external_sources_and_updates_git_default() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("app");
        let old_store = temp.path().join("old.hs2");
        let new_store = temp.path().join("new.hs2");
        std::fs::create_dir(&root).unwrap();
        FsStore::init_git(&old_store, &crate::store::StoreMetadata::new("HS")).unwrap();
        FsStore::init_git(&new_store, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let old = TicketSource::git(&old_store);
        let new = TicketSource::git(&new_store);
        let github = TicketSource {
            connection_id: "github-issues".into(),
            provider: "github".into(),
            locator: "acme/app".into(),
        };
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        providers
            .save(&[ProviderConnection {
                id: github.connection_id.clone(),
                provider: github.provider.clone(),
                locator: github.locator.clone(),
                name: None,
                default: false,
                disabled: false,
                settings: serde_json::json!({}),
            }])
            .unwrap();
        registry
            .register_registered_sources(
                &providers,
                &root,
                None,
                None,
                vec![old.clone(), github.clone()],
                Some(old.connection_id.clone()),
            )
            .unwrap();
        let relinked = registry
            .open_registered_sources(
                &providers,
                &root,
                None,
                None,
                OpenSourceSelection {
                    sources: vec![new.clone()],
                    default_source: None,
                    mode: OpenSourceMode::SelectedGitStore,
                },
            )
            .unwrap();
        assert_eq!(relinked.default_source, Some(new.connection_id.clone()));
        assert!(relinked.source(&old.connection_id).is_none());
        assert_eq!(relinked.source(&github.connection_id), Some(&github));
        assert_eq!(relinked.stores, vec![new.locator]);

        registry
            .add_source(&relinked.id, old.clone(), false)
            .unwrap();
        let multiple_git = registry
            .open_git_store(&root, None, None, &new_store)
            .unwrap();
        assert!(multiple_git.source(&old.connection_id).is_some());
        assert!(multiple_git.source(&new.connection_id).is_some());
        assert!(multiple_git.source(&github.connection_id).is_some());
        assert_eq!(multiple_git.default_source, Some(new.connection_id.clone()));

        let explicit = registry
            .open_registered_sources(
                &providers,
                &root,
                None,
                None,
                OpenSourceSelection {
                    sources: vec![github.clone()],
                    default_source: None,
                    mode: OpenSourceMode::Explicit,
                },
            )
            .unwrap();
        assert_eq!(explicit.sources, vec![github]);
        assert_eq!(explicit.default_source.as_deref(), Some("github-issues"));
    }

    #[test]
    fn explicit_relocation_preserves_the_durable_checkout_reference() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        let relocated = temp.path().join("relocated");
        let store = temp.path().join("tickets");
        std::fs::create_dir(&original).unwrap();
        FsStore::init_git(&store, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let before = registry
            .register(&original, Some("app"), None, vec![store.clone()])
            .unwrap();

        std::fs::rename(&original, &relocated).unwrap();
        let after = registry
            .relocate(&before.id, &relocated, Some("app-moved"))
            .unwrap();

        assert_eq!(
            after.id, before.id,
            "persisted project references stay valid"
        );
        assert_eq!(after.alias, "app-moved");
        assert_eq!(registry.resolve(&before.id).unwrap(), after);
        assert_eq!(
            registry.resolve(relocated.to_str().unwrap()).unwrap(),
            after
        );
        let reopened = registry
            .register(&relocated, Some("app-moved"), None, vec![store])
            .unwrap();
        assert_eq!(
            reopened.id, before.id,
            "ordinary reopen keeps the migrated id"
        );
        assert_eq!(registry.list().unwrap(), vec![reopened]);
    }

    #[test]
    fn ordinary_reads_reject_a_replaced_linked_store_without_blocking_other_projects() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let linked = temp.path().join("linked");
        let original = temp.path().join("original");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        FsStore::init_git(&linked, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry_path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&registry_path);
        let source = TicketSource::git(&linked);
        let first_checkout = registry
            .register_sources(&first, None, None, vec![source.clone()], None)
            .unwrap();
        let second_checkout = registry
            .register_sources(&second, None, None, Vec::new(), None)
            .unwrap();
        assert!(
            first_checkout
                .store_instance_ids
                .contains_key(&source.connection_id)
        );

        std::fs::rename(&linked, &original).unwrap();
        FsStore::init_git(&linked, &crate::store::StoreMetadata::new("HS")).unwrap();
        let restarted = CheckoutRegistry::new(&registry_path);
        let error = restarted
            .resolve(&first_checkout.id)
            .unwrap_err()
            .to_string();
        assert!(error.contains("different identity"), "{error}");
        let error = restarted
            .resolve_source(&first_checkout.id, &source.connection_id)
            .unwrap_err()
            .to_string();
        assert!(error.contains("different identity"), "{error}");
        assert_eq!(
            restarted.resolve(&second_checkout.id).unwrap(),
            second_checkout
        );

        std::fs::rename(&linked, temp.path().join("replacement")).unwrap();
        std::fs::rename(&original, &linked).unwrap();
        assert_eq!(
            restarted
                .resolve(&first_checkout.id)
                .unwrap()
                .store_instance_ids,
            first_checkout.store_instance_ids
        );
        assert_eq!(
            restarted
                .resolve_source(&first_checkout.id, &source.connection_id)
                .unwrap()
                .1,
            source
        );
    }

    #[test]
    fn moved_git_source_relinks_every_checkout_and_retains_old_ids() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let old_path = temp.path().join("old-store");
        let new_path = temp.path().join("new-store");
        for path in [&first, &second] {
            std::fs::create_dir(path).unwrap();
        }
        FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let old = TicketSource::git(&old_path);
        let first_checkout = registry
            .register_sources(
                &first,
                None,
                None,
                vec![old.clone()],
                Some(old.connection_id.clone()),
            )
            .unwrap();
        let second_checkout = registry
            .register_sources(&second, None, None, vec![old.clone()], None)
            .unwrap();
        let store_id = FsStore::open(&old_path)
            .unwrap()
            .metadata()
            .unwrap()
            .instance_id
            .unwrap();
        assert_eq!(
            first_checkout.store_instance_ids[&old.connection_id],
            store_id
        );
        assert_eq!(
            second_checkout.store_instance_ids[&old.connection_id],
            store_id
        );
        registry
            .set_source_color(&first_checkout.id, &old.connection_id, "#3b82f6")
            .unwrap();
        let unrelated = temp.path().join("unrelated-store");
        FsStore::init_git(&unrelated, &crate::store::StoreMetadata::new("HS")).unwrap();
        let before_wrong_destination = registry.list().unwrap();
        assert!(
            registry
                .relink_git_source(&first_checkout.id, &old.connection_id, &unrelated)
                .unwrap_err()
                .to_string()
                .contains("different Hot Sheet ticket repository")
        );
        assert_eq!(registry.list().unwrap(), before_wrong_destination);
        std::fs::rename(&old_path, &new_path).unwrap();
        let new = TicketSource::git(&new_path);

        let updated = registry
            .relink_git_source(&first_checkout.id, &old.connection_id, &new_path)
            .unwrap();
        assert_eq!(updated.len(), 2);
        let first_after = registry.resolve(&first_checkout.id).unwrap();
        let second_after = registry.resolve(&second_checkout.id).unwrap();
        for checkout in [&first_after, &second_after] {
            assert_eq!(checkout.sources, vec![new.clone()]);
            assert_eq!(checkout.store_instance_ids[&new.connection_id], store_id);
            assert_eq!(checkout.stores, vec![new.locator.clone()]);
            assert_eq!(
                registry
                    .resolve_source(&checkout.id, &old.connection_id)
                    .unwrap()
                    .1,
                new
            );
        }
        assert_eq!(first_after.default_source, Some(new.connection_id.clone()));
        assert_eq!(second_after.default_source, Some(new.connection_id.clone()));
        assert_eq!(first_after.source_colors[&new.connection_id], "#3b82f6");
        assert!(!first_after.source_colors.contains_key(&old.connection_id));
        assert!(
            registry
                .relink_git_source(&first_checkout.id, &new.connection_id, &new_path)
                .unwrap()
                .is_empty()
        );
        registry
            .register_sources(
                &first,
                None,
                None,
                vec![new.clone()],
                Some(new.connection_id.clone()),
            )
            .unwrap();
        assert_eq!(
            registry
                .resolve_source(&first_checkout.id, &old.connection_id)
                .unwrap()
                .1,
            new
        );
        std::fs::rename(&new_path, &old_path).unwrap();
        registry
            .relink_git_source(&first_checkout.id, &new.connection_id, &old_path)
            .unwrap();
        for checkout in [&first_checkout, &second_checkout] {
            assert_eq!(
                registry
                    .resolve_source(&checkout.id, &new.connection_id)
                    .unwrap()
                    .1,
                old
            );
        }
    }

    #[test]
    fn git_relink_collision_does_not_partially_update_shared_checkouts() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let old_path = temp.path().join("old-store");
        let new_path = temp.path().join("new-store");
        for path in [&first, &second] {
            std::fs::create_dir(path).unwrap();
        }
        let original =
            FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        FsStore::init_git(&new_path, &original.metadata().unwrap()).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let old = TicketSource::git(&old_path);
        let new = TicketSource::git(&new_path);
        let first_checkout = registry
            .register_sources(&first, None, None, vec![old.clone()], None)
            .unwrap();
        let second_checkout = registry
            .register_sources(&second, None, None, vec![old.clone(), new], None)
            .unwrap();
        let before = registry.list().unwrap();
        assert!(
            registry
                .relink_git_source(&first_checkout.id, &old.connection_id, &new_path)
                .is_err()
        );
        assert_eq!(registry.list().unwrap(), before);
        assert!(
            registry
                .resolve_source(&second_checkout.id, &old.connection_id)
                .is_ok()
        );
    }

    #[test]
    fn legacy_checkout_links_require_review_before_recording_identity() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        let old_path = temp.path().join("old-store");
        let new_path = temp.path().join("new-store");
        std::fs::create_dir(&checkout).unwrap();
        let store = FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry_path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&registry_path);
        let linked = registry
            .register(&checkout, None, None, vec![old_path.clone()])
            .unwrap();
        let source_id = linked.sources[0].connection_id.clone();
        let mut metadata = store.metadata().unwrap();
        metadata.instance_id = None;
        std::fs::write(
            old_path.join(STORE_METADATA_FILE),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
        file["checkouts"][0]
            .as_object_mut()
            .unwrap()
            .remove("store_instance_ids");
        std::fs::write(&registry_path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();

        let unreviewed = registry.list().unwrap();
        assert!(!unreviewed[0].store_instance_ids.contains_key(&source_id));
        assert!(registry.resolve_source(&linked.id, &source_id).is_err());
        assert!(
            registry
                .relink_git_source(&linked.id, &source_id, &old_path)
                .unwrap_err()
                .to_string()
                .contains("identity was never recorded")
        );
        assert_eq!(store.metadata().unwrap().instance_id, None);
        let reviewed = registry
            .recover_unverified_git_source(&linked.id, &source_id, &old_path)
            .unwrap();
        assert_eq!(reviewed.len(), 1);
        let durable_id = reviewed[0].store_instance_ids[&source_id].clone();
        assert_eq!(registry.list().unwrap(), reviewed);
        assert!(registry.resolve_source(&linked.id, &source_id).is_ok());
        std::fs::rename(&old_path, &new_path).unwrap();
        let updated = registry
            .relink_git_source(&linked.id, &source_id, &new_path)
            .unwrap();
        let new_id = updated[0].sources[0].connection_id.clone();
        assert_eq!(updated[0].store_instance_ids[&new_id], durable_id);
        assert!(updated[0].unverified_store_sources.contains_key(&new_id));
    }

    #[test]
    fn adding_a_new_git_source_to_an_existing_checkout_records_its_identity() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let first = temp.path().join("first-store");
        let second = temp.path().join("second-store");
        std::fs::create_dir(&project).unwrap();
        FsStore::init_git(&first, &crate::store::StoreMetadata::new("HS")).unwrap();
        let added = FsStore::init_git(&second, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        registry
            .register(&project, None, None, vec![first.clone()])
            .unwrap();
        let updated = registry
            .register(&project, None, None, vec![first, second.clone()])
            .unwrap();
        let second_id = TicketSource::git(&second).connection_id;
        assert_eq!(
            updated.store_instance_ids[&second_id],
            added.metadata().unwrap().instance_id.unwrap()
        );
        assert!(registry.resolve_source(&updated.id, &second_id).is_ok());
    }

    #[test]
    fn a_replacement_at_the_legacy_path_is_not_silently_adopted_for_shared_checkouts() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let old_path = temp.path().join("store");
        let backup = temp.path().join("original");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry_path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&registry_path);
        let a = registry
            .register(&first, None, None, vec![old_path.clone()])
            .unwrap();
        let b = registry
            .register(&second, None, None, vec![old_path.clone()])
            .unwrap();
        let source_id = a.sources[0].connection_id.clone();
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
        for checkout in file["checkouts"].as_array_mut().unwrap() {
            checkout
                .as_object_mut()
                .unwrap()
                .remove("store_instance_ids");
        }
        std::fs::write(&registry_path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();
        std::fs::rename(&old_path, &backup).unwrap();
        let replacement =
            FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let replacement_id = replacement.metadata().unwrap().instance_id.unwrap();

        assert!(
            registry
                .list()
                .unwrap()
                .iter()
                .all(|checkout| { !checkout.store_instance_ids.contains_key(&source_id) })
        );
        let reopened = registry
            .register(&first, None, None, vec![old_path.clone()])
            .unwrap();
        assert!(!reopened.store_instance_ids.contains_key(&source_id));
        for checkout in [&a, &b] {
            assert!(registry.resolve_source(&checkout.id, &source_id).is_err());
        }
        assert!(
            registry
                .relink_git_source(&a.id, &source_id, &old_path)
                .unwrap_err()
                .to_string()
                .contains("identity was never recorded")
        );
        let reviewed = registry
            .recover_unverified_git_source(&a.id, &source_id, &old_path)
            .unwrap();
        assert_eq!(reviewed.len(), 2);
        for checkout in &reviewed {
            assert_eq!(checkout.store_instance_ids[&source_id], replacement_id);
            assert_eq!(
                checkout.unverified_store_sources[&source_id],
                a.sources[0].locator
            );
            assert!(registry.resolve_source(&checkout.id, &source_id).is_ok());
        }
        assert_eq!(
            CheckoutRegistry::new(&registry_path).list().unwrap(),
            registry.list().unwrap()
        );
    }

    #[test]
    fn unmigrated_legacy_link_cannot_verify_an_already_moved_store() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        let old_path = temp.path().join("old-store");
        let new_path = temp.path().join("new-store");
        std::fs::create_dir(&checkout).unwrap();
        let store = FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry_path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&registry_path);
        let linked = registry
            .register(&checkout, None, None, vec![old_path.clone()])
            .unwrap();
        let source_id = linked.sources[0].connection_id.clone();
        let mut metadata = store.metadata().unwrap();
        metadata.instance_id = None;
        std::fs::write(
            old_path.join(STORE_METADATA_FILE),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
        file["checkouts"][0]
            .as_object_mut()
            .unwrap()
            .remove("store_instance_ids");
        std::fs::write(&registry_path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();
        std::fs::rename(&old_path, &new_path).unwrap();

        let before = registry.list().unwrap();
        assert!(
            registry
                .relink_git_source(&linked.id, &source_id, &new_path)
                .unwrap_err()
                .to_string()
                .contains("identity was never recorded")
        );
        assert_eq!(registry.list().unwrap(), before);
    }

    #[test]
    fn reviewed_legacy_recovery_marks_shared_links_and_keeps_the_warning_on_later_moves() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let old_path = temp.path().join("old-store");
        let new_path = temp.path().join("new-store");
        let later_path = temp.path().join("later-store");
        std::fs::create_dir(&first).unwrap();
        std::fs::create_dir(&second).unwrap();
        let store = FsStore::init_git(&old_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry_path = temp.path().join("checkouts.json");
        let registry = CheckoutRegistry::new(&registry_path);
        let a = registry
            .register(&first, None, None, vec![old_path.clone()])
            .unwrap();
        registry
            .register(&second, None, None, vec![old_path.clone()])
            .unwrap();
        let old_id = a.sources[0].connection_id.clone();
        let old_locator = a.sources[0].locator.clone();
        let mut legacy = store.metadata().unwrap();
        legacy.instance_id = None;
        std::fs::write(
            old_path.join(STORE_METADATA_FILE),
            serde_json::to_vec_pretty(&legacy).unwrap(),
        )
        .unwrap();
        let mut file: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&registry_path).unwrap()).unwrap();
        for checkout in file["checkouts"].as_array_mut().unwrap() {
            checkout
                .as_object_mut()
                .unwrap()
                .remove("store_instance_ids");
        }
        std::fs::write(&registry_path, serde_json::to_vec_pretty(&file).unwrap()).unwrap();
        std::fs::rename(&old_path, &new_path).unwrap();

        let recovered = registry
            .recover_unverified_git_source(&a.id, &old_id, &new_path)
            .unwrap();
        assert_eq!(recovered.len(), 2);
        let new_id = TicketSource::git(&new_path).connection_id;
        for checkout in &recovered {
            assert_eq!(checkout.unverified_store_sources[&new_id], old_locator);
            assert!(checkout.store_instance_ids.contains_key(&new_id));
        }
        assert_eq!(
            CheckoutRegistry::new(&registry_path).list().unwrap(),
            registry.list().unwrap()
        );
        assert!(
            registry
                .register(&first, None, None, vec![new_path.clone()])
                .unwrap()
                .unverified_store_sources
                .contains_key(&new_id),
            "reopening a project preserves the review warning"
        );
        std::fs::rename(&new_path, &later_path).unwrap();
        let moved = registry
            .relink_git_source(&a.id, &new_id, &later_path)
            .unwrap();
        let later_id = TicketSource::git(&later_path).connection_id;
        assert_eq!(moved.len(), 2);
        assert!(moved.iter().all(|checkout| {
            checkout.unverified_store_sources.get(&later_id) == Some(&old_locator)
        }));
    }

    #[test]
    fn reused_store_path_cannot_hide_an_identity_change() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("project");
        let store_path = temp.path().join("store");
        std::fs::create_dir(&checkout).unwrap();
        let store =
            FsStore::init_git(&store_path, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let linked = registry
            .register(&checkout, None, None, vec![store_path.clone()])
            .unwrap();
        let before = registry.list().unwrap();
        let mut metadata = store.metadata().unwrap();
        metadata.instance_id = Some(hotsheet_model::Ulid::new().to_string());
        std::fs::write(
            store_path.join(STORE_METADATA_FILE),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
        let source_id = &linked.sources[0].connection_id;
        assert!(
            registry
                .relink_git_source(&linked.id, source_id, &store_path)
                .unwrap_err()
                .to_string()
                .contains("different Hot Sheet ticket repository")
        );
        assert!(
            registry
                .recover_unverified_git_source(&linked.id, source_id, &store_path)
                .unwrap_err()
                .to_string()
                .contains("different Hot Sheet ticket repository"),
            "reviewed recovery cannot override an actual recorded mismatch"
        );
        assert!(
            registry
                .register(&checkout, None, None, vec![store_path])
                .unwrap_err()
                .to_string()
                .contains("different identity")
        );
        assert_eq!(registry.list().unwrap(), before);
    }

    #[test]
    fn renamed_external_source_resolves_old_references_without_retargeting() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        std::fs::create_dir(&checkout).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        providers
            .save(&[
                ProviderConnection {
                    id: "jira-old".into(),
                    provider: "jira".into(),
                    locator: "APP".into(),
                    name: None,
                    default: false,
                    settings: serde_json::Value::Null,
                    disabled: false,
                },
                ProviderConnection {
                    id: "jira-other".into(),
                    provider: "jira".into(),
                    locator: "APP".into(),
                    name: None,
                    default: false,
                    settings: serde_json::Value::Null,
                    disabled: false,
                },
            ])
            .unwrap();
        let old = TicketSource {
            connection_id: "jira-old".into(),
            provider: "jira".into(),
            locator: "APP".into(),
        };
        let saved = registry
            .register_registered_sources(
                &providers,
                &checkout,
                None,
                None,
                vec![old],
                Some("jira-old".into()),
            )
            .unwrap();

        let renamed = registry
            .rename_source(&saved.id, "jira-old", "jira-current")
            .unwrap();
        assert_eq!(renamed.sources.len(), 1);
        assert_eq!(renamed.sources[0].connection_id, "jira-current");
        assert_eq!(renamed.default_source.as_deref(), Some("jira-current"));
        assert_eq!(
            registry.resolve_source(&saved.id, "jira-old").unwrap().1,
            renamed.sources[0]
        );
        assert_eq!(
            registry
                .resolve_source(&saved.id, "jira-current")
                .unwrap()
                .1,
            renamed.sources[0]
        );

        let other = TicketSource {
            connection_id: "jira-other".into(),
            provider: "jira".into(),
            locator: "APP".into(),
        };
        let with_other = registry
            .add_registered_source(&providers, &saved.id, other, false)
            .unwrap();
        assert_eq!(with_other.sources.len(), 2);
        assert_eq!(
            registry
                .resolve_source(&saved.id, "jira-old")
                .unwrap()
                .1
                .connection_id,
            "jira-current",
            "a same-locator source is never silently treated as the rename target"
        );
        let renamed_again = registry
            .rename_source(&saved.id, "jira-current", "jira-next")
            .unwrap();
        assert_eq!(renamed_again.default_source.as_deref(), Some("jira-next"));
        for historical in ["jira-old", "jira-current"] {
            assert_eq!(
                registry
                    .resolve_source(&saved.id, historical)
                    .unwrap()
                    .1
                    .connection_id,
                "jira-next"
            );
        }
        assert!(
            registry
                .rename_source(&saved.id, "jira-next", "jira-old")
                .unwrap_err()
                .to_string()
                .contains("historical alias")
        );
    }

    #[test]
    fn connection_locator_updates_reach_every_linked_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp.path().join("tickets");
        FsStore::init_git(&store, &crate::store::StoreMetadata::new("HS")).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let providers = ProviderConfigRegistry::new(temp.path().join("providers.json"));
        let mut connection = ProviderConnection {
            id: "jira-shared".into(),
            provider: "jira".into(),
            locator: "OLD".into(),
            name: None,
            default: false,
            settings: serde_json::Value::Null,
            disabled: false,
        };
        providers.save(&[connection.clone()]).unwrap();
        let source = |locator: &str| TicketSource {
            connection_id: "jira-shared".into(),
            provider: "jira".into(),
            locator: locator.into(),
        };
        let mut ids = Vec::new();
        for name in ["first", "second", "unlinked"] {
            let root = temp.path().join(name);
            std::fs::create_dir(&root).unwrap();
            let mut sources = vec![TicketSource::git(&store)];
            if name != "unlinked" {
                sources.push(source("OLD"));
            }
            ids.push(
                registry
                    .register_registered_sources(&providers, &root, None, None, sources, None)
                    .unwrap()
                    .id,
            );
        }
        let git_id = TicketSource::git(&store).connection_id;
        let locators = |connection: &str| -> Vec<Option<String>> {
            ids.iter()
                .map(|id| {
                    registry
                        .resolve(id)
                        .unwrap()
                        .source(connection)
                        .map(|source| source.locator.clone())
                })
                .collect()
        };

        connection.locator = "NEW".into();
        providers.save(&[connection.clone()]).unwrap();
        let mut updated = registry
            .update_registered_source_locator(&providers, &connection)
            .unwrap();
        updated.sort();
        let mut expected = ids[..2].to_vec();
        expected.sort();
        assert_eq!(updated, expected);
        assert_eq!(
            locators("jira-shared"),
            vec![Some("NEW".into()), Some("NEW".into()), None]
        );

        // Repeating the same locator changes nothing and does not rewrite the file.
        let before = std::fs::read(temp.path().join("checkouts.json")).unwrap();
        assert!(
            registry
                .update_registered_source_locator(&providers, &connection)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read(temp.path().join("checkouts.json")).unwrap(),
            before
        );

        // A checkout linked afterwards follows the same registered record.
        registry
            .add_registered_source(&providers, &ids[2], source("NEW"), false)
            .unwrap();
        connection.locator = "THIRD".into();
        providers.save(&[connection.clone()]).unwrap();
        let updated = registry
            .update_registered_source_locator(&providers, &connection)
            .unwrap();
        assert_eq!(updated.len(), 3);
        assert_eq!(
            locators("jira-shared"),
            vec![
                Some("THIRD".into()),
                Some("THIRD".into()),
                Some("THIRD".into())
            ]
        );

        // Git links are path-derived and never retargeted through a connection edit.
        assert!(
            registry
                .update_source_locator(&git_id, "/elsewhere")
                .unwrap()
                .is_empty()
        );
        assert!(
            locators(&git_id)
                .iter()
                .all(|locator| locator.as_deref() != Some("/elsewhere"))
        );
    }

    #[test]
    fn rename_source_rejects_git_ids() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let store = temp.path().join("tickets");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::create_dir(&store).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let saved = registry
            .register(&checkout, None, None, vec![store])
            .unwrap();
        let source = saved.sources[0].connection_id.clone();
        assert!(
            registry
                .rename_source(&saved.id, &source, "renamed")
                .unwrap_err()
                .to_string()
                .contains("git source ids cannot be renamed")
        );
    }

    #[test]
    fn checkout_settings_are_project_owned_and_prefer_the_default_legacy_store() {
        let temp = tempfile::tempdir().unwrap();
        let project = temp.path().join("project");
        let first = temp.path().join("first.hs2");
        let preferred = temp.path().join("preferred.hs2");
        for path in [&project, &first, &preferred] {
            std::fs::create_dir(path).unwrap();
        }
        std::fs::write(first.join("hotsheet-settings.json"), r#"{"theme":"first"}"#).unwrap();
        std::fs::write(
            preferred.join("hotsheet-settings.json"),
            r#"{"theme":"preferred"}"#,
        )
        .unwrap();
        let first_source = TicketSource::git(&first);
        let preferred_source = TicketSource::git(&preferred);
        let checkout = CheckoutRegistry::new(temp.path().join("checkouts.json"))
            .register_sources(
                &project,
                None,
                None,
                vec![first_source, preferred_source.clone()],
                Some(preferred_source.connection_id),
            )
            .unwrap();

        let settings = checkout.settings();
        assert_eq!(
            settings.get("theme", crate::Scope::Shared).unwrap(),
            Some(serde_json::json!("preferred"))
        );
        settings
            .set("theme", serde_json::json!("project"), crate::Scope::Shared)
            .unwrap();
        assert!(project.join(".hotsheet2/settings.json").is_file());
        assert!(!first.join(".hotsheet").exists());
        assert_eq!(
            settings.get("theme", crate::Scope::Shared).unwrap(),
            Some(serde_json::json!("project"))
        );
    }

    #[test]
    fn discovers_only_a_valid_parallel_hs2_store() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let sibling = temp.path().join("app.hs2");
        std::fs::create_dir(&checkout).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        assert!(discover_ticket_stores(&checkout).unwrap().is_empty());
        std::fs::write(sibling.join("hotsheet-store.json"), "{}").unwrap();
        assert_eq!(
            discover_ticket_stores(&checkout).unwrap(),
            vec![sibling.canonicalize().unwrap()]
        );
    }

    #[test]
    fn linked_store_takes_precedence_over_sibling_and_invalid_link_falls_back() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        let sibling = temp.path().join("app.hs2");
        let linked = temp.path().join("tickets");
        std::fs::create_dir_all(checkout.join(".hotsheet2")).unwrap();
        std::fs::create_dir(&sibling).unwrap();
        std::fs::create_dir(&linked).unwrap();
        std::fs::write(sibling.join("hotsheet-store.json"), "{}").unwrap();
        std::fs::write(linked.join("hotsheet-store.json"), "{}").unwrap();
        std::fs::write(
            checkout.join(".hotsheet2/store"),
            linked.to_string_lossy().as_bytes(),
        )
        .unwrap();
        assert_eq!(
            discover_ticket_stores(&checkout).unwrap(),
            vec![linked.canonicalize().unwrap()]
        );

        std::fs::write(checkout.join(".hotsheet2/store"), "missing").unwrap();
        assert_eq!(
            discover_ticket_stores(&checkout).unwrap(),
            vec![sibling.canonicalize().unwrap()]
        );
    }
}
