//! Machine-local checkout discovery. A checkout is a working directory, not a git or
//! ticket-store identity: several checkouts may share one repository and each checkout
//! may use several ticket stores (and vice versa).

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::file_lock::FileLock;
use crate::provider::ProviderConfigRegistry;

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
    /// Project-local source accent colors keyed by connection id; omitted entries use Gray.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub source_colors: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_source: Option<String>,
    /// Distinguishes a user-cleared default from an older registry with no recorded choice.
    #[serde(default, skip_serializing_if = "is_false")]
    pub default_source_cleared: bool,
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
    pub fn source(&self, connection_id: &str) -> Option<&TicketSource> {
        self.sources
            .iter()
            .find(|source| source.connection_id == connection_id)
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
}

const CHECKOUT_REGISTRY_SCHEMA_VERSION: u64 = 3;
const fn registry_schema_version() -> u64 {
    CHECKOUT_REGISTRY_SCHEMA_VERSION
}

impl Default for RegistryFile {
    fn default() -> Self {
        Self {
            schema_version: CHECKOUT_REGISTRY_SCHEMA_VERSION,
            checkouts: Vec::new(),
            source_aliases: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Error)]
pub enum CheckoutError {
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
}

/// Discover conventional git-backed ticket stores for a checkout. The first convention
/// is a sibling whose path is the checkout path plus `.hs2` (for example `app` and
/// `app.hs2`). Discovery is deliberately conservative: it never creates a store and never
/// guesses that one checkout maps to only one provider.
pub fn discover_ticket_stores(root: &Path) -> Result<Vec<PathBuf>, CheckoutError> {
    let canonical = root
        .canonicalize()
        .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
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
    pub(crate) fn resolve(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        self.registry.resolve_locked(reference)
    }

    pub(crate) fn list(&self) -> Result<Vec<Checkout>, CheckoutError> {
        let mut entries = self.registry.read_locked()?.checkouts;
        entries.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        Ok(entries)
    }

    pub(crate) fn remove_source(
        &self,
        reference: &str,
        connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        self.registry.remove_source_locked(reference, connection_id)
    }
}

impl CheckoutRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
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
        let _lock = self.acquire_lock()?;
        self.register_sources_locked(root, alias, repository, sources, default_source, false)
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
        let _lock = self.acquire_lock()?;
        Self::validate_provider_sources(providers, &sources)?;
        self.register_sources_locked(root, alias, repository, sources, default_source, false)
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
        let _lock = self.acquire_lock()?;
        self.open_sources_locked(root, alias, repository, sources, default_source, mode)
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
        let _lock = self.acquire_lock()?;
        Self::validate_provider_sources(providers, &selection.sources)?;
        self.open_sources_locked(
            root,
            alias,
            repository,
            selection.sources,
            selection.default_source,
            selection.mode,
        )
    }

    fn open_sources_locked(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        mut sources: Vec<TicketSource>,
        mut default_source: Option<String>,
        mode: OpenSourceMode,
    ) -> Result<Checkout, CheckoutError> {
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let existing = self
            .read_locked()?
            .checkouts
            .into_iter()
            .find(|checkout| checkout.root == root.to_string_lossy());
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
        self.register_sources_locked(
            &root,
            alias.as_deref(),
            repository,
            sources,
            default_source,
            false,
        )
    }

    fn register_sources_locked(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        mut sources: Vec<TicketSource>,
        mut default_source: Option<String>,
        explicit_clear: bool,
    ) -> Result<Checkout, CheckoutError> {
        let root = root
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(root.display().to_string()))?;
        let generated_id = checkout_id(&root)?;
        let alias = alias.map(str::to_owned).unwrap_or_else(|| {
            root.file_name()
                .and_then(|v| v.to_str())
                .unwrap_or("checkout")
                .to_owned()
        });
        for source in &mut sources {
            if source.provider == "git" {
                let normalized = TicketSource::git(&source.locator);
                if default_source.as_deref() == Some(&source.connection_id) {
                    default_source = Some(normalized.connection_id.clone());
                }
                *source = normalized;
            }
        }
        sources.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        sources.dedup_by(|a, b| a.connection_id == b.connection_id);
        if let Some(default) = &default_source
            && !sources
                .iter()
                .any(|source| &source.connection_id == default)
        {
            return Err(CheckoutError::Invalid(format!(
                "default source '{default}' is not associated with checkout {generated_id}"
            )));
        }
        let mut file = self.read_locked()?;
        // An explicitly relocated checkout keeps its original id. Re-registering the new
        // root (for example on the next app open) must update that durable entry rather
        // than reintroducing the new path-derived id beside it.
        let id = file
            .checkouts
            .iter()
            .find(|checkout| checkout.root == root.to_string_lossy())
            .map(|checkout| checkout.id.clone())
            .unwrap_or(generated_id);
        let default_source_cleared = default_source.is_none()
            && (explicit_clear
                || file
                    .checkouts
                    .iter()
                    .any(|checkout| checkout.id == id && checkout.default_source_cleared));
        let mut store_strings = sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.locator.clone())
            .collect::<Vec<_>>();
        store_strings.sort();
        let source_colors = file
            .checkouts
            .iter()
            .find(|checkout| checkout.id == id)
            .map(|checkout| {
                checkout
                    .source_colors
                    .iter()
                    .filter(|(source_id, _)| {
                        sources
                            .iter()
                            .any(|source| &source.connection_id == *source_id)
                    })
                    .map(|(source_id, color)| (source_id.clone(), color.clone()))
                    .collect()
            })
            .unwrap_or_default();
        let entry = Checkout {
            id: id.clone(),
            root: root.to_string_lossy().into_owned(),
            alias,
            repository,
            stores: store_strings,
            sources,
            source_colors,
            default_source,
            default_source_cleared,
        };
        if let Some(existing) = file.checkouts.iter_mut().find(|c| c.id == id) {
            *existing = entry.clone();
        } else {
            file.checkouts.push(entry.clone());
        }
        self.write_locked(&file)?;
        Ok(entry)
    }

    pub fn resolve(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        resolve_checkout(self.list()?, reference)
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
        Ok((checkout, source))
    }

    fn resolve_locked(&self, reference: &str) -> Result<Checkout, CheckoutError> {
        let mut entries = self.read_locked()?.checkouts;
        entries.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        resolve_checkout(entries, reference)
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
        let _lock = self.acquire_lock()?;
        self.add_source_locked(reference, source, make_default)
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
        let _lock = self.acquire_lock()?;
        Self::validate_provider_sources(providers, std::slice::from_ref(&source))?;
        self.add_source_locked(reference, source, make_default)
    }

    fn validate_provider_sources(
        providers: &ProviderConfigRegistry,
        sources: &[TicketSource],
    ) -> Result<(), CheckoutError> {
        if sources.iter().all(|source| source.provider == "git") {
            return Ok(());
        }
        let registered = providers
            .load()
            .map_err(|error| CheckoutError::Invalid(error.to_string()))?;
        for source in sources.iter().filter(|source| source.provider != "git") {
            if !registered.iter().any(|connection| {
                connection.id == source.connection_id && connection.provider == source.provider
            }) {
                return Err(CheckoutError::Invalid(format!(
                    "ticket source '{}' is not registered in providers.json",
                    source.connection_id
                )));
            }
        }
        Ok(())
    }

    fn add_source_locked(
        &self,
        reference: &str,
        source: TicketSource,
        make_default: bool,
    ) -> Result<Checkout, CheckoutError> {
        let mut checkout = self.resolve_locked(reference)?;
        checkout
            .sources
            .retain(|item| item.connection_id != source.connection_id);
        let source_id = source.connection_id.clone();
        checkout.sources.push(source);
        if make_default {
            checkout.default_source = Some(source_id);
        }
        self.register_sources_locked(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            checkout.default_source,
            false,
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
        let mut file = self.read_locked()?;
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
        let canonical = new_path
            .canonicalize()
            .map_err(|_| CheckoutError::Missing(new_path.display().to_string()))?;
        let replacement = TicketSource::git(&canonical);
        let _lock = self.acquire_lock()?;
        let mut file = self.read_locked()?;
        let selected = resolve_checkout(file.checkouts.clone(), checkout_reference)?;
        let original = selected
            .source(connection_id)
            .filter(|source| source.provider == "git")
            .ok_or_else(|| CheckoutError::NotFound(connection_id.to_string()))?;
        if replacement.connection_id == original.connection_id {
            return Ok(Vec::new());
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
        let mut file = self.read_locked()?;
        let mut updated = Vec::new();
        for checkout in &mut file.checkouts {
            let mut changed = false;
            for source in &mut checkout.sources {
                if source.connection_id == connection_id
                    && source.provider != "git"
                    && source.locator != locator
                {
                    source.locator = locator.to_string();
                    changed = true;
                }
            }
            if changed {
                updated.push(checkout.id.clone());
            }
        }
        if !updated.is_empty() {
            self.write_locked(&file)?;
        }
        Ok(updated)
    }

    pub fn remove_source(
        &self,
        reference: &str,
        connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        let _lock = self.acquire_lock()?;
        self.remove_source_locked(reference, connection_id)
    }

    fn remove_source_locked(
        &self,
        reference: &str,
        connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        let mut checkout = self.resolve_locked(reference)?;
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
        self.register_sources_locked(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            checkout.default_source,
            false,
        )
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
        let _lock = self.acquire_lock()?;
        let checkout = self.resolve_locked(reference)?;
        self.register_sources_locked(
            Path::new(&checkout.root),
            Some(&checkout.alias),
            checkout.repository,
            checkout.sources,
            connection_id.map(str::to_owned),
            connection_id.is_none(),
        )
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

fn normalize_registry(mut file: RegistryFile) -> Result<RegistryFile, CheckoutError> {
    if file.schema_version > CHECKOUT_REGISTRY_SCHEMA_VERSION {
        return Err(CheckoutError::UpgradeRequired {
            found: file.schema_version,
            supported: CHECKOUT_REGISTRY_SCHEMA_VERSION,
        });
    }
    file.schema_version = CHECKOUT_REGISTRY_SCHEMA_VERSION;
    for checkout in &mut file.checkouts {
        migrate_checkout(checkout);
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

fn migrate_checkout(checkout: &mut Checkout) {
    if checkout.sources.is_empty() {
        checkout.sources = checkout.stores.iter().map(TicketSource::git).collect();
        if checkout.sources.is_empty()
            && let Some(source) = legacy_link_source(Path::new(&checkout.root))
        {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::ProviderConnection;

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
        for path in [&checkout, &store_a, &store_b] {
            std::fs::create_dir(path).unwrap();
        }
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
        for root in [&first, &second] {
            registry
                .register_sources(
                    root,
                    None,
                    None,
                    vec![external.clone()],
                    Some("github-acme".into()),
                )
                .unwrap();
        }
        assert_eq!(registry.list().unwrap().len(), 2);
        let alternate = TicketSource {
            connection_id: "jira-eng".into(),
            provider: "jira".into(),
            locator: "ENG".into(),
        };
        registry
            .add_source(first.to_str().unwrap(), alternate, false)
            .unwrap();
        let changed = registry
            .set_default_source(first.to_str().unwrap(), Some("jira-eng"))
            .unwrap();
        assert_eq!(changed.default_source.as_deref(), Some("jira-eng"));
        let removed_default = registry
            .remove_source(first.to_str().unwrap(), "jira-eng")
            .unwrap();
        assert!(removed_default.default_source.is_none());
        let changed = registry
            .set_default_source(first.to_str().unwrap(), None)
            .unwrap();
        assert!(changed.default_source.is_none());
        let removed = registry
            .remove_source(first.to_str().unwrap(), "github-acme")
            .unwrap();
        assert!(removed.sources.is_empty());
        assert!(
            registry
                .set_default_source(second.to_str().unwrap(), Some("missing"))
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
        for root in [&first, &second] {
            registry
                .register_sources(root, None, None, vec![source.clone()], None)
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
            .register_sources(&first, None, None, vec![source.clone()], None)
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
        reopened
            .rename_source(first_ref, "github-a", "github-b")
            .unwrap();
        assert_eq!(
            reopened.resolve(first_ref).unwrap().source_colors["github-b"],
            "#3b82f6"
        );
        reopened
            .set_source_color(first_ref, "github-b", DEFAULT_SOURCE_COLOR)
            .unwrap();
        assert!(
            reopened
                .resolve(first_ref)
                .unwrap()
                .source_colors
                .is_empty()
        );
        reopened
            .set_source_color(first_ref, "github-b", "#14b8a6")
            .unwrap();
        reopened.remove_source(first_ref, "github-b").unwrap();
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
        registry
            .register_sources(
                &root,
                None,
                None,
                vec![github.clone()],
                Some(github.connection_id.clone()),
            )
            .unwrap();
        let cleared = registry
            .set_default_source(root.to_str().unwrap(), None)
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

        let jira = TicketSource {
            connection_id: "jira-eng".into(),
            provider: "jira".into(),
            locator: "ENG".into(),
        };
        assert!(
            reopened
                .add_source(root.to_str().unwrap(), jira, false)
                .unwrap()
                .default_source
                .is_none()
        );
        assert!(
            reopened
                .remove_source(root.to_str().unwrap(), "jira-eng")
                .unwrap()
                .default_source
                .is_none()
        );
        let empty = reopened
            .remove_source(root.to_str().unwrap(), &github.connection_id)
            .unwrap();
        assert!(empty.sources.is_empty());
        assert!(empty.default_source_cleared);
        assert!(
            reopened
                .add_source(root.to_str().unwrap(), github.clone(), false)
                .unwrap()
                .default_source
                .is_none()
        );
        assert!(
            reopened
                .open_sources(
                    &root,
                    None,
                    None,
                    vec![github.clone()],
                    None,
                    OpenSourceMode::Discovered
                )
                .unwrap()
                .default_source
                .is_none()
        );
        let selected = reopened
            .set_default_source(root.to_str().unwrap(), Some(&github.connection_id))
            .unwrap();
        assert_eq!(selected.default_source.as_deref(), Some("github-issues"));
        assert!(!selected.default_source_cleared);

        // An older file with no clear marker still infers its sole linked source.
        reopened
            .set_default_source(root.to_str().unwrap(), None)
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
        std::fs::create_dir(&store).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let git = TicketSource::git(&store);
        let github = TicketSource {
            connection_id: "github-issues".into(),
            provider: "github".into(),
            locator: "acme/app".into(),
        };
        registry
            .register_sources(
                &root,
                Some("renamed app"),
                Some("acme/app".into()),
                vec![git.clone(), github.clone()],
                Some(github.connection_id.clone()),
            )
            .unwrap();

        let reopened = registry
            .open_sources(
                &root,
                None,
                None,
                vec![git.clone()],
                None,
                OpenSourceMode::SelectedGitStore,
            )
            .unwrap();
        assert_eq!(reopened.alias, "renamed app");
        assert_eq!(reopened.repository.as_deref(), Some("acme/app"));
        assert_eq!(reopened.sources.len(), 2);
        assert_eq!(reopened.source(&github.connection_id), Some(&github));
        assert_eq!(reopened.default_source, Some(github.connection_id.clone()));

        let discovered = registry
            .open_sources(&root, None, None, vec![], None, OpenSourceMode::Discovered)
            .unwrap();
        assert_eq!(discovered.sources.len(), 2);
        assert_eq!(
            discovered.default_source,
            Some(github.connection_id.clone())
        );

        registry
            .remove_source(&reopened.id, &github.connection_id)
            .unwrap();
        let removed = registry
            .open_sources(
                &root,
                None,
                None,
                vec![git.clone()],
                None,
                OpenSourceMode::SelectedGitStore,
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
        for path in [&root, &old_store, &new_store] {
            std::fs::create_dir(path).unwrap();
        }
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let old = TicketSource::git(&old_store);
        let new = TicketSource::git(&new_store);
        let github = TicketSource {
            connection_id: "github-issues".into(),
            provider: "github".into(),
            locator: "acme/app".into(),
        };
        registry
            .register_sources(
                &root,
                None,
                None,
                vec![old.clone(), github.clone()],
                Some(old.connection_id.clone()),
            )
            .unwrap();
        let relinked = registry
            .open_sources(
                &root,
                None,
                None,
                vec![new.clone()],
                None,
                OpenSourceMode::SelectedGitStore,
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
            .open_sources(
                &root,
                None,
                None,
                vec![github.clone()],
                None,
                OpenSourceMode::Explicit,
            )
            .unwrap();
        assert_eq!(explicit.sources, vec![github]);
        assert!(explicit.default_source.is_none());
    }

    #[test]
    fn explicit_relocation_preserves_the_durable_checkout_reference() {
        let temp = tempfile::tempdir().unwrap();
        let original = temp.path().join("original");
        let relocated = temp.path().join("relocated");
        let store = temp.path().join("tickets");
        std::fs::create_dir(&original).unwrap();
        std::fs::create_dir(&store).unwrap();
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
    fn moved_git_source_relinks_every_checkout_and_retains_old_ids() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        let old_path = temp.path().join("old-store");
        let new_path = temp.path().join("new-store");
        for path in [&first, &second, &old_path] {
            std::fs::create_dir(path).unwrap();
        }
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
        registry
            .set_source_color(&first_checkout.id, &old.connection_id, "#3b82f6")
            .unwrap();
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
        for path in [&first, &second, &old_path, &new_path] {
            std::fs::create_dir(path).unwrap();
        }
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
    fn renamed_external_source_resolves_old_references_without_retargeting() {
        let temp = tempfile::tempdir().unwrap();
        let checkout = temp.path().join("app");
        std::fs::create_dir(&checkout).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let old = TicketSource {
            connection_id: "github-old".into(),
            provider: "github".into(),
            locator: "acme/app".into(),
        };
        let saved = registry
            .register_sources(&checkout, None, None, vec![old], Some("github-old".into()))
            .unwrap();

        let renamed = registry
            .rename_source(&saved.id, "github-old", "github-current")
            .unwrap();
        assert_eq!(renamed.sources.len(), 1);
        assert_eq!(renamed.sources[0].connection_id, "github-current");
        assert_eq!(renamed.default_source.as_deref(), Some("github-current"));
        assert_eq!(
            registry.resolve_source(&saved.id, "github-old").unwrap().1,
            renamed.sources[0]
        );
        assert_eq!(
            registry
                .resolve_source(&saved.id, "github-current")
                .unwrap()
                .1,
            renamed.sources[0]
        );

        let other = TicketSource {
            connection_id: "github-other".into(),
            provider: "github".into(),
            locator: "acme/app".into(),
        };
        let with_other = registry.add_source(&saved.id, other, false).unwrap();
        assert_eq!(with_other.sources.len(), 2);
        assert_eq!(
            registry
                .resolve_source(&saved.id, "github-old")
                .unwrap()
                .1
                .connection_id,
            "github-current",
            "a same-locator source is never silently treated as the rename target"
        );
        let renamed_again = registry
            .rename_source(&saved.id, "github-current", "github-next")
            .unwrap();
        assert_eq!(renamed_again.default_source.as_deref(), Some("github-next"));
        for historical in ["github-old", "github-current"] {
            assert_eq!(
                registry
                    .resolve_source(&saved.id, historical)
                    .unwrap()
                    .1
                    .connection_id,
                "github-next"
            );
        }
        assert!(
            registry
                .rename_source(&saved.id, "github-next", "github-old")
                .unwrap_err()
                .to_string()
                .contains("historical alias")
        );
    }

    #[test]
    fn connection_locator_updates_reach_every_linked_checkout() {
        let temp = tempfile::tempdir().unwrap();
        let store = temp.path().join("tickets");
        std::fs::create_dir(&store).unwrap();
        let registry = CheckoutRegistry::new(temp.path().join("checkouts.json"));
        let github = |locator: &str| TicketSource {
            connection_id: "github-shared".into(),
            provider: "github".into(),
            locator: locator.into(),
        };
        let mut ids = Vec::new();
        for name in ["first", "second", "unlinked"] {
            let root = temp.path().join(name);
            std::fs::create_dir(&root).unwrap();
            let mut sources = vec![TicketSource::git(&store)];
            if name != "unlinked" {
                sources.push(github("acme/old"));
            }
            ids.push(
                registry
                    .register_sources(&root, None, None, sources, None)
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

        let mut updated = registry
            .update_source_locator("github-shared", "acme/new")
            .unwrap();
        updated.sort();
        let mut expected = ids[..2].to_vec();
        expected.sort();
        assert_eq!(updated, expected);
        assert_eq!(
            locators("github-shared"),
            vec![Some("acme/new".into()), Some("acme/new".into()), None]
        );

        // Repeating the same locator changes nothing and does not rewrite the file.
        let before = std::fs::read(temp.path().join("checkouts.json")).unwrap();
        assert!(
            registry
                .update_source_locator("github-shared", "acme/new")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            std::fs::read(temp.path().join("checkouts.json")).unwrap(),
            before
        );

        // A checkout linked afterwards (with the stale value) is caught by the next edit,
        // and a rename keeps following the connection under its new id.
        registry
            .add_source(&ids[2], github("acme/stale"), false)
            .unwrap();
        registry
            .rename_source(&ids[0], "github-shared", "github-renamed")
            .unwrap();
        let updated = registry
            .update_source_locator("github-shared", "acme/third")
            .unwrap();
        assert_eq!(updated.len(), 2);
        assert_eq!(
            locators("github-shared"),
            vec![None, Some("acme/third".into()), Some("acme/third".into())]
        );
        assert_eq!(
            locators("github-renamed"),
            vec![Some("acme/new".into()), None, None]
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
}
