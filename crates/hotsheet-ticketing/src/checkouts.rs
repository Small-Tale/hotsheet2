//! Machine-local checkout discovery. A checkout is a working directory, not a git or
//! ticket-store identity: several checkouts may share one repository and each checkout
//! may use several ticket stores (and vice versa).

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub default_source: Option<String>,
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

#[derive(Debug)]
struct RegistryLock {
    file: File,
}

impl Drop for RegistryLock {
    fn drop(&mut self) {
        unlock_file(&self.file);
    }
}

#[cfg(unix)]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::fd::AsRawFd;

    // SAFETY: flock only borrows this live file descriptor for the duration of the call.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX) } == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(unix)]
fn unlock_file(file: &File) {
    use std::os::fd::AsRawFd;

    // SAFETY: the descriptor remains live until RegistryLock is dropped.
    let _ = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
}

#[cfg(target_os = "windows")]
fn lock_file(file: &File) -> io::Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{LOCKFILE_EXCLUSIVE_LOCK, LockFileEx};
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: the handle is live and OVERLAPPED is initialized for a synchronous byte-range lock.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let result = unsafe {
        LockFileEx(
            file.as_raw_handle(),
            LOCKFILE_EXCLUSIVE_LOCK,
            0,
            u32::MAX,
            u32::MAX,
            &mut overlapped,
        )
    };
    if result != 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[cfg(target_os = "windows")]
fn unlock_file(file: &File) {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::UnlockFileEx;
    use windows_sys::Win32::System::IO::OVERLAPPED;

    // SAFETY: this unlocks the same live handle and byte range acquired by lock_file.
    let mut overlapped: OVERLAPPED = unsafe { std::mem::zeroed() };
    let _ = unsafe { UnlockFileEx(file.as_raw_handle(), 0, u32::MAX, u32::MAX, &mut overlapped) };
}

#[cfg(not(any(unix, target_os = "windows")))]
fn lock_file(_file: &File) -> io::Result<()> {
    Ok(())
}

#[cfg(not(any(unix, target_os = "windows")))]
fn unlock_file(_file: &File) {}

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

impl CheckoutRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn list(&self) -> Result<Vec<Checkout>, CheckoutError> {
        let mut entries = self.read()?.checkouts;
        entries.sort_by(|a, b| a.alias.cmp(&b.alias).then(a.id.cmp(&b.id)));
        Ok(entries)
    }

    fn acquire_lock(&self) -> Result<RegistryLock, CheckoutError> {
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
        lock_file(&file)?;
        Ok(RegistryLock { file })
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

    pub fn register_sources(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        sources: Vec<TicketSource>,
        default_source: Option<String>,
    ) -> Result<Checkout, CheckoutError> {
        let _lock = self.acquire_lock()?;
        self.register_sources_locked(root, alias, repository, sources, default_source)
    }

    fn register_sources_locked(
        &self,
        root: &Path,
        alias: Option<&str>,
        repository: Option<String>,
        mut sources: Vec<TicketSource>,
        mut default_source: Option<String>,
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
        let mut store_strings = sources
            .iter()
            .filter(|source| source.provider == "git")
            .map(|source| source.locator.clone())
            .collect::<Vec<_>>();
        store_strings.sort();
        let entry = Checkout {
            id: id.clone(),
            root: root.to_string_lossy().into_owned(),
            alias,
            repository,
            stores: store_strings,
            sources,
            default_source,
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
        )
    }

    /// Rename an external provider connection while retaining its previous id for durable
    /// ticket references. Git source ids are derived from their store path and therefore
    /// require a store migration rather than a connection rename.
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
                "git source ids cannot be renamed; migrate the store path instead".into(),
            ));
        }
        source.connection_id = new_connection_id.to_string();
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

    pub fn remove_source(
        &self,
        reference: &str,
        connection_id: &str,
    ) -> Result<Checkout, CheckoutError> {
        let _lock = self.acquire_lock()?;
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
        )
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
    let link = std::fs::read_to_string(root.join(".hotsheet/store")).ok()?;
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
    if checkout.default_source.is_none() {
        checkout.default_source =
            legacy_default_source(Path::new(&checkout.root), &checkout.sources).or_else(|| {
                (checkout.sources.len() == 1).then(|| checkout.sources[0].connection_id.clone())
            });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(project.join(".hotsheet/settings.json").is_file());
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
