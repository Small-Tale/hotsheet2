//! A concrete filesystem-backed git store: read/write/list ticket files on disk
//! (`docs/02-ticket-storage.md` §2.3). This is the direct implementation the CLI
//! uses today; the injected `FileSystem`/`GitLocal` ports (see [`crate::ports`]) will
//! wrap it once the server needs fakeable I/O (HS2-4).
//!
//! Layout:
//! ```text
//! <root>/
//!   hotsheet-store.json      # metadata (prefix, id strategy, sharding)
//!   tickets/<2-char random-suffix shard>/<ULID>.md
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use hotsheet_model::{
    Attachment, Note, NoteKind, ParseError, SCHEMA_VERSION, Ticket, Timestamp, Ulid, parse_file,
    to_file_string,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de};
use sha2::{Digest, Sha256};

use crate::file_lock::FileLock;

/// The store metadata file at a store root.
pub const STORE_METADATA_FILE: &str = "hotsheet-store.json";
/// Current store version. Schema 3 replaces time-prefix sharding with random-suffix
/// sharding and retains the schema-2 stale-writer ticket guard.
pub const STORE_SCHEMA_VERSION: u32 = 3;
const GUARDED_STORE_SCHEMA_V2: &str = "hotsheet/v2-guarded-tickets";
const GUARDED_STORE_SCHEMA_V3: &str = "hotsheet/v3-random-suffix-shards";
const SHARD_ID_PREFIX_2: &str = "id-prefix-2";
const SHARD_ID_SUFFIX_2: &str = "id-suffix-2";
const FINDER_METADATA_FILE: &str = ".DS_Store";
const NOTE_MUTATION_LOCK_FILE: &str = ".hotsheet-note-mutation.lock";

struct AttachmentWriteOptions {
    metadata: hotsheet_model::AttachmentMetadata,
    preserve_updated_at: bool,
}

fn ticket_write_schema(ticket: &Ticket) -> u32 {
    if ticket.schema >= hotsheet_model::CROP_SCHEMA_VERSION
        || ticket
            .attachments
            .iter()
            .any(|attachment| attachment.crop.is_some())
    {
        hotsheet_model::CROP_SCHEMA_VERSION
    } else if ticket.schema >= hotsheet_model::INTENT_SCHEMA_VERSION
        || ticket.attachments.iter().any(|attachment| {
            attachment
                .annotations
                .iter()
                .any(|annotation| !annotation.intents.is_empty())
        })
    {
        hotsheet_model::INTENT_SCHEMA_VERSION
    } else if ticket.schema >= hotsheet_model::SHAPE_SCHEMA_VERSION
        || ticket.attachments.iter().any(|attachment| {
            attachment
                .annotations
                .iter()
                .any(|annotation| annotation.shape.is_some())
        })
    {
        hotsheet_model::SHAPE_SCHEMA_VERSION
    } else {
        SCHEMA_VERSION
    }
}

/// One gallery markup session. Crop coordinates are pixels of the immutable original;
/// annotations remain normalized to that original.
pub struct AttachmentMarkup {
    pub annotations: Vec<hotsheet_model::MediaAnnotation>,
    pub crop: Option<hotsheet_model::ImageCrop>,
}

struct MarkupChange {
    annotations: Vec<hotsheet_model::MediaAnnotation>,
    crop_change: Option<Option<hotsheet_model::ImageCrop>>,
}

/// Store metadata (`hotsheet-store.json`, `docs/02` §2.3). camelCase on disk.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StoreMetadata {
    #[serde(
        serialize_with = "serialize_store_schema",
        deserialize_with = "deserialize_store_schema"
    )]
    pub schema_version: u32,
    /// Display prefix for derived slugs (e.g. `HS`); the dash is added by the slug.
    pub ticket_prefix: String,
    pub id_strategy: String,
    pub shard: String,
    /// Stable identity of this store across directory moves. Legacy metadata omits it
    /// until a linked checkout migrates the store under its registry lock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,
}

fn serialize_store_schema<S: Serializer>(version: &u32, serializer: S) -> Result<S::Ok, S::Error> {
    match *version {
        2 => serializer.serialize_str(GUARDED_STORE_SCHEMA_V2),
        STORE_SCHEMA_VERSION => serializer.serialize_str(GUARDED_STORE_SCHEMA_V3),
        version => serializer.serialize_u32(version),
    }
}

fn deserialize_store_schema<'de, D: Deserializer<'de>>(deserializer: D) -> Result<u32, D::Error> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DiskSchema {
        Number(u32),
        Guard(String),
    }

    match DiskSchema::deserialize(deserializer)? {
        DiskSchema::Number(version) => Ok(version),
        DiskSchema::Guard(guard) if guard == GUARDED_STORE_SCHEMA_V2 => Ok(2),
        DiskSchema::Guard(guard) if guard == GUARDED_STORE_SCHEMA_V3 => Ok(STORE_SCHEMA_VERSION),
        DiskSchema::Guard(guard) => Err(de::Error::custom(format!(
            "unsupported store schema marker '{guard}'"
        ))),
    }
}

impl StoreMetadata {
    /// Default metadata for a new store with the given display prefix.
    pub fn new(ticket_prefix: impl Into<String>) -> Self {
        Self {
            schema_version: STORE_SCHEMA_VERSION,
            ticket_prefix: ticket_prefix.into(),
            id_strategy: "ulid".to_string(),
            shard: SHARD_ID_SUFFIX_2.to_string(),
            instance_id: Some(Ulid::new().to_string()),
        }
    }

    fn validate_instance_id(&self) -> Result<(), StoreError> {
        if let Some(id) = &self.instance_id
            && id.parse::<Ulid>().is_err()
        {
            return Err(StoreError::InvalidStoreIdentity(id.clone()));
        }
        Ok(())
    }
}

/// An error reading or writing the store.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("{0}")]
    InvalidAnnotations(&'static str),
    #[error(transparent)]
    ImageCrop(#[from] crate::image_crop::ImageCropError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("{operation} {path}: {source}")]
    IoAt {
        operation: &'static str,
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("not a Hot Sheet store (no hotsheet-store.json at {0}); run `hotsheet init` first")]
    NotAStore(PathBuf),
    #[error("invalid hotsheet-store.json: {0}")]
    Metadata(#[from] serde_json::Error),
    #[error("invalid Hot Sheet store instance id '{0}'")]
    InvalidStoreIdentity(String),
    #[error(
        "This {format} was created by a newer version of Hot Sheet 2 and cannot be opened by this version. Update Hot Sheet 2 to open it (found {found}, supported through {supported})."
    )]
    UpgradeRequired {
        format: &'static str,
        found: String,
        supported: String,
    },
    #[error(
        "store format activation required: schema {found} must be explicitly migrated to {target} before writing; stop older Hot Sheet 2 processes, announce the compatibility break, then run `hotsheet-cli activate-format --acknowledge-pre-release-breakage`"
    )]
    FormatActivationRequired { found: u32, target: u32 },
    #[error("parsing ticket {path}: {source}")]
    Parse { path: PathBuf, source: ParseError },
    #[error(
        "store schema {found} is newer than this writer supports ({supported}); update Hot Sheet before modifying it"
    )]
    UnsupportedStoreSchema { found: u32, supported: u32 },
    #[error("git {0}")]
    Git(String),
}

impl StoreError {
    /// Match an underlying filesystem kind regardless of whether the error has
    /// path/operation context attached.
    pub fn is_io_kind(&self, kind: std::io::ErrorKind) -> bool {
        match self {
            Self::Io(source) | Self::IoAt { source, .. } => source.kind() == kind,
            _ => false,
        }
    }
}

/// A ticket file that could not be parsed during a resilient enumeration
/// ([`FsStore::list_tickets_resilient`]). Surfaced instead of aborting the whole
/// scan so a single bad file can never hide every healthy ticket (HS2-PRVPCQ).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorruptTicket {
    /// The on-disk path of the unparseable file.
    pub path: PathBuf,
    /// The ticket id recovered from the filename stem (or, failing that, the
    /// frontmatter `id:` line), if it is still a valid ULID.
    pub id: Option<Ulid>,
    /// The slug recovered from a readable frontmatter `slug:` line, if any.
    pub slug: Option<String>,
    /// A human-readable description of why the file could not be read/parsed.
    pub error: String,
    /// Stable recovery class. An upgrade boundary is not damaged content.
    pub error_code: &'static str,
}

/// The result of a resilient store enumeration ([`FsStore::list_tickets_resilient`]):
/// every healthy ticket plus a separate report of the files that failed to parse.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StoreListing {
    /// The healthy tickets, sorted by id (≈ creation order).
    pub tickets: Vec<Ticket>,
    /// The files that could not be parsed, sorted by path. Empty for a clean store.
    pub corrupt: Vec<CorruptTicket>,
}

/// Evidence payload prepared for an all-or-nothing ticket mutation.
#[derive(Debug, Clone)]
pub struct AtomicAttachment {
    pub id: Ulid,
    pub filename: String,
    pub created_at: Timestamp,
    pub bytes: Vec<u8>,
}

impl AtomicAttachment {
    pub fn sanitized_filename(&self) -> String {
        Path::new(&self.filename)
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.is_empty())
            .unwrap_or("attachment")
            .to_string()
    }
}

fn attachment_filename(filename: &str) -> String {
    Path::new(filename)
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("attachment")
        .to_string()
}

fn unique_attachment_filename<'a>(
    filename: &str,
    existing: impl IntoIterator<Item = &'a str>,
) -> String {
    let filename = attachment_filename(filename);
    let used = existing
        .into_iter()
        .map(|name| name.to_lowercase())
        .collect::<std::collections::HashSet<_>>();
    if !used.contains(&filename.to_lowercase()) {
        return filename;
    }
    let path = Path::new(&filename);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = path.extension().and_then(|value| value.to_str());
    for sequence in 2_u64.. {
        let candidate = match extension {
            Some(extension) => format!("{stem} ({sequence}).{extension}"),
            None => format!("{stem} ({sequence})"),
        };
        if !used.contains(&candidate.to_lowercase()) {
            return candidate;
        }
    }
    unreachable!()
}

/// A filesystem-backed store rooted at a directory.
#[derive(Debug, Clone)]
pub struct FsStore {
    root: PathBuf,
    push_after_commit: bool,
    defer_autocommit: bool,
    #[cfg(test)]
    background_push_observer: Option<BackgroundPushObserver>,
}

#[cfg(test)]
#[derive(Debug, Clone)]
struct BackgroundPushObserver {
    spawned: std::sync::Arc<std::sync::Barrier>,
    finished: std::sync::Arc<std::sync::Barrier>,
}

impl FsStore {
    /// Initialize a new store at `root` (creates `tickets/` and writes metadata).
    /// Idempotent on the directory; updates settings but preserves its durable identity.
    pub fn init(root: impl Into<PathBuf>, meta: &StoreMetadata) -> Result<Self, StoreError> {
        let root = root.into();
        fs::create_dir_all(root.join("tickets"))?;
        let path = root.join(STORE_METADATA_FILE);
        let mut meta = meta.clone();
        if path.is_file() {
            let existing: StoreMetadata = serde_json::from_slice(&fs::read(&path)?)?;
            existing.validate_instance_id()?;
            if existing.instance_id.is_some() {
                meta.instance_id = existing.instance_id;
            }
        }
        meta.instance_id
            .get_or_insert_with(|| Ulid::new().to_string());
        meta.validate_instance_id()?;
        let json = serde_json::to_string_pretty(&meta)?;
        write_file_atomically(&path, format!("{json}\n").as_bytes())?;
        let store = Self {
            root,
            push_after_commit: true,
            defer_autocommit: false,
            #[cfg(test)]
            background_push_observer: None,
        };
        store.ensure_managed_gitignore()?;
        Ok(store)
    }

    /// Open an existing store, erroring if `root` is not a Hot Sheet store.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let store = Self::open_without_maintenance(root)?;
        if let Err(error) = store.ensure_managed_gitignore() {
            eprintln!("warning: could not maintain store .gitignore: {error}");
        }
        Ok(store)
    }

    /// Open for diagnostics without maintaining Git files or writing store state.
    /// Mutation methods remain available; callers of this constructor own their use.
    pub fn open_without_maintenance(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        if !root.join(STORE_METADATA_FILE).is_file() {
            return Err(StoreError::NotAStore(root));
        }
        Ok(Self {
            root,
            push_after_commit: true,
            defer_autocommit: false,
            #[cfg(test)]
            background_push_observer: None,
        })
    }

    /// Let an owning service publish commits itself (for example, the server's
    /// coalescing fetch/rebase/push loop) instead of launching a per-write push.
    #[must_use]
    pub fn with_deferred_push(mut self) -> Self {
        self.push_after_commit = false;
        self
    }

    /// Write through the normal mutation APIs without committing each ticket. The caller
    /// must commit the touched paths after the batch, including when a later item fails.
    #[must_use]
    pub fn with_deferred_autocommit(mut self) -> Self {
        self.defer_autocommit = true;
        self
    }

    #[cfg(all(test, unix))]
    fn with_background_push_observer(mut self, observer: BackgroundPushObserver) -> Self {
        self.background_push_observer = Some(observer);
        self
    }

    /// The store root directory.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Keep host-generated Finder metadata out of every store directory. The plain
    /// filename pattern applies recursively, including inside legacy and current
    /// attachment layouts.
    fn ensure_managed_gitignore(&self) -> Result<(), StoreError> {
        let path = self.root.join(".gitignore");
        let existing = fs::read_to_string(&path).unwrap_or_default();
        let mut content = existing.clone();
        for managed in [FINDER_METADATA_FILE, NOTE_MUTATION_LOCK_FILE] {
            if content.lines().any(|line| line.trim() == managed) {
                continue;
            }
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(managed);
            content.push('\n');
        }
        if content != existing {
            fs::write(path, content)?;
        }
        Ok(())
    }

    /// Keep a ticket read–revise–write sequence together across processes.
    /// The file remains at the store root for git-backed and standalone stores alike;
    /// it is ignored by Git and never removed, so waiters cannot lock different inodes.
    pub fn with_ticket_transaction<T, E: From<StoreError>>(
        &self,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        self.ensure_managed_gitignore().map_err(E::from)?;
        let path = self.root.join(NOTE_MUTATION_LOCK_FILE);
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| StoreError::IoAt {
                operation: "opening note mutation lock",
                path: path.clone(),
                source,
            })
            .map_err(E::from)?;
        let _lock = FileLock::acquire(file)
            .map_err(|source| StoreError::IoAt {
                operation: "locking note mutation transaction",
                path,
                source,
            })
            .map_err(E::from)?;
        operation()
    }

    /// Hold two stores in a stable order for copy and move operations. A store
    /// paired with itself takes one lock, avoiding a self-deadlock.
    pub fn with_store_pair_transaction<T, E: From<StoreError>>(
        &self,
        other: &FsStore,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<T, E> {
        let first = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let second = other
            .root
            .canonicalize()
            .unwrap_or_else(|_| other.root.clone());
        match first.cmp(&second) {
            std::cmp::Ordering::Less => {
                self.with_ticket_transaction(|| other.with_ticket_transaction(operation))
            }
            std::cmp::Ordering::Greater => {
                other.with_ticket_transaction(|| self.with_ticket_transaction(operation))
            }
            std::cmp::Ordering::Equal => self.with_ticket_transaction(operation),
        }
    }

    /// Read the store metadata.
    pub fn metadata(&self) -> Result<StoreMetadata, StoreError> {
        let path = self.root.join(STORE_METADATA_FILE);
        let text = fs::read_to_string(&path).map_err(|source| StoreError::IoAt {
            operation: "reading store metadata",
            path,
            source,
        })?;
        let value: serde_json::Value = serde_json::from_str(&text)?;
        if let Some(version) = value.get("schemaVersion") {
            let supported = GUARDED_STORE_SCHEMA_V3.to_string();
            let known = version
                .as_u64()
                .is_some_and(|version| (1..=u64::from(STORE_SCHEMA_VERSION)).contains(&version))
                || version.as_str() == Some(GUARDED_STORE_SCHEMA_V2)
                || version.as_str() == Some(GUARDED_STORE_SCHEMA_V3);
            if !known {
                return Err(StoreError::UpgradeRequired {
                    format: "ticket store",
                    found: version.to_string(),
                    supported,
                });
            }
        }
        let metadata: StoreMetadata = serde_json::from_value(value)?;
        metadata.validate_instance_id()?;
        Ok(metadata)
    }

    /// Assign an identity to a legacy store once, under the store's writer lock. The
    /// metadata is committed with the store so a later Git clone or move retains it.
    pub fn ensure_instance_id(&self) -> Result<String, StoreError> {
        self.with_ticket_transaction(|| {
            let mut metadata = self.metadata()?;
            let path = self.root.join(STORE_METADATA_FILE);
            if let Some(id) = metadata.instance_id {
                // Also retries publication if an earlier write reached disk but its
                // Git commit failed before the checkout link could be recorded.
                self.autocommit_paths("Initialize Hot Sheet store identity", &[path])?;
                return Ok(id);
            }
            let id = Ulid::new().to_string();
            metadata.instance_id = Some(id.clone());
            let json = serde_json::to_string_pretty(&metadata)?;
            write_file_atomically(&path, format!("{json}\n").as_bytes())?;
            self.autocommit_paths("Initialize Hot Sheet store identity", &[path])?;
            Ok(id)
        })
    }

    /// The on-disk path for a ticket id. Current stores use the final two random ULID
    /// characters; an existing legacy prefix path remains readable until activation.
    pub fn ticket_path(&self, id: &Ulid) -> PathBuf {
        let suffix = self.ticket_path_for_shard(id, SHARD_ID_SUFFIX_2);
        if suffix.exists() {
            return suffix;
        }
        let prefix = self.ticket_path_for_shard(id, SHARD_ID_PREFIX_2);
        if prefix.exists() {
            return prefix;
        }
        if self
            .metadata()
            .is_ok_and(|metadata| metadata.shard == SHARD_ID_PREFIX_2)
        {
            prefix
        } else {
            suffix
        }
    }

    fn ticket_path_for_shard(&self, id: &Ulid, shard: &str) -> PathBuf {
        let id = id.to_string();
        let shard_name = match shard {
            SHARD_ID_PREFIX_2 => &id[..2],
            SHARD_ID_SUFFIX_2 => &id[id.len() - 2..],
            _ => &id[id.len() - 2..],
        };
        self.root
            .join("tickets")
            .join(shard_name)
            .join(format!("{id}.md"))
    }

    /// Write a ticket file (creating its shard directory), returning the path.
    ///
    /// Legacy prefix-sharded stores remain readable, but require explicit format
    /// activation before a current writer can mutate them. This prevents a stale
    /// process and a current process from creating two paths for the same ticket.
    pub fn write_ticket(&self, ticket: &Ticket) -> Result<PathBuf, StoreError> {
        self.ensure_current_writer_format()?;
        self.write_ticket_unchecked(ticket)
    }

    fn write_ticket_unchecked(&self, ticket: &Ticket) -> Result<PathBuf, StoreError> {
        let path = self.ticket_path(&ticket.id);
        self.write_ticket_unchecked_at(ticket, path)
    }

    fn write_ticket_unchecked_at(
        &self,
        ticket: &Ticket,
        path: PathBuf,
    ) -> Result<PathBuf, StoreError> {
        let mut normalized = ticket.clone();
        normalized.schema = ticket_write_schema(&normalized);
        if !normalized.status.is_active() {
            normalized.up_next = false;
        }
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|source| StoreError::IoAt {
                operation: "creating ticket directory",
                path: parent.to_path_buf(),
                source,
            })?;
        }
        write_file_atomically(&path, to_file_string(&normalized).as_bytes()).map_err(|source| {
            StoreError::IoAt {
                operation: "writing ticket",
                path: path.clone(),
                source,
            }
        })?;
        Ok(path)
    }

    fn ensure_current_writer_format(&self) -> Result<(), StoreError> {
        let metadata = self.metadata()?;
        if metadata.schema_version > STORE_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedStoreSchema {
                found: metadata.schema_version,
                supported: STORE_SCHEMA_VERSION,
            });
        }
        if metadata.schema_version == STORE_SCHEMA_VERSION && metadata.shard == SHARD_ID_SUFFIX_2 {
            return Ok(());
        }
        // Keep an already-running schema-2 store writable until its owner chooses the
        // explicit activation boundary. Both old and current processes then continue
        // using the same prefix path; merely rebuilding a client cannot split a ticket
        // across prefix and suffix shards.
        if metadata.schema_version == 2 && metadata.shard == SHARD_ID_PREFIX_2 {
            return Ok(());
        }

        Err(StoreError::FormatActivationRequired {
            found: metadata.schema_version,
            target: STORE_SCHEMA_VERSION,
        })
    }

    /// Explicitly activate the current pre-release format after the caller has
    /// announced the compatibility boundary and stopped older writers.
    pub fn activate_current_format(&self) -> Result<(), StoreError> {
        let mut metadata = self.metadata()?;
        if metadata.schema_version > STORE_SCHEMA_VERSION {
            return Err(StoreError::UnsupportedStoreSchema {
                found: metadata.schema_version,
                supported: STORE_SCHEMA_VERSION,
            });
        }
        if metadata.schema_version == STORE_SCHEMA_VERSION && metadata.shard == SHARD_ID_SUFFIX_2 {
            return Ok(());
        }

        // Write each healthy ticket to its random-suffix destination before removing
        // its old prefix path. Corrupt files remain exactly where they were so the
        // recovery UI can still diagnose them. The metadata marker is written last,
        // making an interrupted migration safely repeatable.
        for source in self.ticket_file_paths()? {
            let Ok(ticket) = self.read_ticket_at(&source) else {
                continue;
            };
            let destination = self.ticket_path_for_shard(&ticket.id, SHARD_ID_SUFFIX_2);
            self.write_ticket_unchecked_at(&ticket, destination.clone())?;
            if source != destination {
                fs::remove_file(&source).map_err(|source_error| StoreError::IoAt {
                    operation: "removing migrated ticket path",
                    path: source.clone(),
                    source: source_error,
                })?;
                if let Some(parent) = source.parent() {
                    let _ = fs::remove_dir(parent);
                }
            }
        }
        metadata.schema_version = STORE_SCHEMA_VERSION;
        metadata.shard = SHARD_ID_SUFFIX_2.to_string();
        let json = serde_json::to_string_pretty(&metadata)?;
        fs::write(self.root.join(STORE_METADATA_FILE), format!("{json}\n"))?;
        Ok(())
    }

    /// Write a ticket, then **auto-commit** the change to the store's git repo and
    /// best-effort push it (HS2-VJD1W4) — so a mutation never leaves the store dirty and
    /// unshared, which matters for the headless `work` loop and multi-worker sync. The
    /// commit is best-effort: the write is what must succeed, so a git failure warns but
    /// doesn't fail the op. The mutating `ops` all go through here; `write_ticket` stays
    /// bare for bulk writers (import) that do their own single commit.
    pub fn write_ticket_committing(&self, ticket: &Ticket) -> Result<PathBuf, StoreError> {
        self.write_ticket_with_paths_committing(ticket, &[])
    }

    fn write_ticket_with_paths_committing(
        &self,
        ticket: &Ticket,
        extra_paths: &[PathBuf],
    ) -> Result<PathBuf, StoreError> {
        let path = self.write_ticket(ticket)?;
        if self.defer_autocommit {
            return Ok(path);
        }
        let status = serde_json::to_value(ticket.status)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "update".into());
        let msg = format!("{}: {status} — {}", ticket.slug, ticket.title);
        let mut paths = Vec::with_capacity(extra_paths.len() + 2);
        paths.push(path.clone());
        paths.push(self.root.join(".gitignore"));
        paths.extend_from_slice(extra_paths);
        if let Err(e) = self.autocommit_paths(&msg, &paths) {
            eprintln!("warning: hotsheet autocommit failed: {e}");
        }
        Ok(path)
    }

    /// Stage everything, commit with `message`, and launch a best-effort push. No-op when the
    /// store isn't a git repo or `HOTSHEET_NO_AUTOCOMMIT` is set. Returns whether a
    /// commit was actually made. Falls back to a bot identity when the repo has none
    /// configured, so a fresh/CI checkout still commits.
    pub fn autocommit(&self, message: &str) -> Result<bool, StoreError> {
        self.autocommit_impl(message, None)
    }

    /// Commit only the paths touched by one bounded mutation. Unrelated working-tree
    /// changes remain unstaged and do not force Git to inspect every ticket file.
    pub fn autocommit_paths(&self, message: &str, paths: &[PathBuf]) -> Result<bool, StoreError> {
        self.autocommit_impl(message, Some(paths))
    }

    fn autocommit_impl(
        &self,
        message: &str,
        paths: Option<&[PathBuf]>,
    ) -> Result<bool, StoreError> {
        if std::env::var_os("HOTSHEET_NO_AUTOCOMMIT").is_some() || !self.root.join(".git").exists()
        {
            return Ok(false);
        }
        retry_autocommit(
            || {
                // Git locks each subcommand, not the stage/check/commit sequence. A
                // second Hot Sheet writer can otherwise replace the shared index
                // after `git add`, making `diff --cached` report a false no-op.
                let _lock = self.lock_autocommit()?;
                self.autocommit_attempt(message, paths)
            },
            std::thread::sleep,
            20,
        )
    }

    /// Serialize one complete local Git transaction across processes and aliases of
    /// this store. The OS releases the lock if a writer exits unexpectedly. The file
    /// lives in Git's private directory and is never staged as ticket content.
    fn lock_autocommit(&self) -> Result<FileLock, StoreError> {
        let out = crate::git::command()
            .arg("-C")
            .arg(&self.root)
            .args(["rev-parse", "--absolute-git-dir"])
            .output()?;
        if !out.status.success() {
            return Err(StoreError::Git(format!(
                "`git rev-parse --absolute-git-dir` failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        let git_dir = PathBuf::from(
            String::from_utf8_lossy(&out.stdout)
                .trim_end_matches(['\n', '\r'])
                .to_string(),
        );
        let git_dir = fs::canonicalize(&git_dir).map_err(|source| StoreError::IoAt {
            operation: "resolving Git directory",
            path: git_dir,
            source,
        })?;
        let path = git_dir.join("hotsheet-autocommit.lock");
        let file = fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&path)
            .map_err(|source| StoreError::IoAt {
                operation: "opening autocommit lock",
                path: path.clone(),
                source,
            })?;
        FileLock::acquire(file).map_err(|source| StoreError::IoAt {
            operation: "locking autocommit transaction",
            path,
            source,
        })
    }

    fn autocommit_attempt(
        &self,
        message: &str,
        paths: Option<&[PathBuf]>,
    ) -> Result<bool, StoreError> {
        self.ensure_managed_gitignore()?;
        // An unborn repository has no baseline tree: its first mutation must also commit
        // the store metadata/attributes. Once HEAD exists, path-scoped commits are safe.
        let mut bounded_paths = if self.head_commit().is_some() {
            paths.map(<[PathBuf]>::to_vec)
        } else {
            None
        };
        if bounded_paths.is_some() {
            let has_tracked_finder_metadata = git_stdout(
                &self.root,
                &[
                    "ls-files",
                    "--",
                    FINDER_METADATA_FILE,
                    ":(glob)**/.DS_Store",
                ],
            )
            .unwrap_or_default()
            .lines()
            .next()
            .is_some();
            if has_tracked_finder_metadata {
                // This one-time legacy repair needs a repository-wide commit so Git
                // records the cached deletion while leaving the ignored local file.
                bounded_paths = None;
            }
        }
        if let Some(paths) = &bounded_paths {
            git_paths(&self.root, &["add", "-A", "--"], paths)?;
        } else {
            // Older clients may already have committed Finder's metadata. Remove only
            // those exact generated paths from the index while leaving local files intact.
            git(
                &self.root,
                &[
                    "rm",
                    "-q",
                    "-f",
                    "--cached",
                    "--ignore-unmatch",
                    "--",
                    FINDER_METADATA_FILE,
                    ":(glob)**/.DS_Store",
                ],
            )?;
            git(&self.root, &["add", "-A"])?;
        }
        // Nothing staged → nothing to commit (idempotent re-writes, no-op edits).
        let nothing_staged = match bounded_paths.as_deref() {
            Some(paths) => git_ok_paths(&self.root, &["diff", "--cached", "--quiet", "--"], paths),
            None => git_ok(&self.root, &["diff", "--cached", "--quiet"]),
        };
        if nothing_staged {
            return Ok(false);
        }
        let has_ident = git_stdout(&self.root, &["config", "user.email"])
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false);
        let ident: &[&str] = if has_ident {
            &[]
        } else {
            &[
                "-c",
                "user.name=Hot Sheet",
                "-c",
                "user.email=hotsheet@localhost",
            ]
        };
        let mut commit = ident.to_vec();
        commit.extend_from_slice(&["commit", "-q", "-m", message]);
        if let Some(paths) = bounded_paths.as_deref() {
            commit.extend_from_slice(&["--only", "--"]);
            git_paths(&self.root, &commit, paths)?;
        } else {
            git(&self.root, &commit)?;
        }
        // Remote publication must not hold a local mutation open for network latency.
        // Server-owned stores defer to their coalescing sync loop; headless callers launch
        // a child and a lightweight reaper. The child survives a short-lived CLI process.
        if self.push_after_commit
            && git_stdout(&self.root, &["remote"]).is_some_and(|s| !s.trim().is_empty())
        {
            if let Ok(mut child) = crate::git::command()
                .arg("-C")
                .arg(&self.root)
                .args(["push", "--quiet"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                #[cfg(test)]
                let observer = self.background_push_observer.clone();
                #[cfg(test)]
                if let Some(observer) = &observer {
                    observer.spawned.wait();
                }
                std::thread::spawn(move || {
                    let _ = child.wait();
                    #[cfg(test)]
                    if let Some(observer) = observer {
                        observer.finished.wait();
                    }
                });
            }
        }
        Ok(true)
    }

    /// Read one ticket by id.
    pub fn read_ticket(&self, id: &Ulid) -> Result<Ticket, StoreError> {
        self.read_ticket_at(&self.ticket_path(id))
    }

    /// Read and parse a ticket file at an explicit path.
    pub fn read_ticket_at(&self, path: &Path) -> Result<Ticket, StoreError> {
        let text = fs::read_to_string(path).map_err(|source| StoreError::IoAt {
            operation: "reading ticket",
            path: path.to_path_buf(),
            source,
        })?;
        let mut ticket = parse_file(&text).map_err(|source| StoreError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
        ticket
            .attachments
            .retain(|attachment| attachment.filename != FINDER_METADATA_FILE);
        self.add_legacy_attachment_metadata(&mut ticket)?;
        Ok(ticket)
    }

    /// The attachments directory for a ticket: `attachments/<ULID>/`.
    pub fn attachment_dir(&self, id: &Ulid) -> PathBuf {
        self.root.join("attachments").join(id.to_string())
    }

    /// Deterministic identity for a pre-metadata attachment.
    pub fn legacy_attachment_id(ticket_id: &Ulid, filename: &str) -> Ulid {
        let mut hash = Sha256::new();
        hash.update(ticket_id.to_string().as_bytes());
        hash.update([0]);
        hash.update(filename.as_bytes());
        let bytes: [u8; 16] = hash.finalize()[..16].try_into().expect("sha prefix");
        Ulid::from(u128::from_be_bytes(bytes))
    }

    /// Add an attachment payload and its durable metadata.
    pub fn write_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: Ulid,
        created_at: Timestamp,
        filename: &str,
        bytes: &[u8],
    ) -> Result<(Ticket, PathBuf), StoreError> {
        self.write_attachment_with_metadata(
            ticket_id,
            attachment_id,
            created_at,
            filename,
            bytes,
            hotsheet_model::AttachmentMetadata::default(),
        )
    }

    pub fn write_attachment_with_metadata(
        &self,
        ticket_id: &Ulid,
        attachment_id: Ulid,
        created_at: Timestamp,
        filename: &str,
        bytes: &[u8],
        metadata: hotsheet_model::AttachmentMetadata,
    ) -> Result<(Ticket, PathBuf), StoreError> {
        self.write_attachment_with_metadata_inner(
            ticket_id,
            attachment_id,
            created_at,
            filename,
            bytes,
            AttachmentWriteOptions {
                metadata,
                preserve_updated_at: false,
            },
        )
    }

    /// Import an attachment without changing the ticket's prior update time.
    /// The prior time is read under the same lock as the attachment write.
    pub fn write_import_attachment_with_metadata(
        &self,
        ticket_id: &Ulid,
        attachment_id: Ulid,
        created_at: Timestamp,
        filename: &str,
        bytes: &[u8],
        metadata: hotsheet_model::AttachmentMetadata,
    ) -> Result<(Ticket, PathBuf), StoreError> {
        self.write_attachment_with_metadata_inner(
            ticket_id,
            attachment_id,
            created_at,
            filename,
            bytes,
            AttachmentWriteOptions {
                metadata,
                preserve_updated_at: true,
            },
        )
    }

    fn write_attachment_with_metadata_inner(
        &self,
        ticket_id: &Ulid,
        attachment_id: Ulid,
        created_at: Timestamp,
        filename: &str,
        bytes: &[u8],
        options: AttachmentWriteOptions,
    ) -> Result<(Ticket, PathBuf), StoreError> {
        let AttachmentWriteOptions {
            metadata,
            preserve_updated_at,
        } = options;
        self.with_ticket_transaction(|| {
            let mut ticket = self.read_ticket(ticket_id)?;
            let previous_updated_at = ticket.updated_at.clone();
            let name = unique_attachment_filename(
                filename,
                ticket
                    .attachments
                    .iter()
                    .filter(|item| item.id != attachment_id)
                    .map(|item| item.filename.as_str()),
            );
            if let Some(existing) = ticket
                .attachments
                .iter()
                .find(|item| item.id == attachment_id)
            {
                if existing.filename != name
                    || existing.created_at != created_at
                    || existing.batch_id != metadata.batch_id
                    || existing.batch_label != metadata.batch_label
                    || existing.actor != metadata.actor
                    || existing.purpose != metadata.purpose
                {
                    return Err(StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::AlreadyExists,
                        format!("attachment id {attachment_id} has different metadata"),
                    )));
                }
            }
            let dir = self
                .attachment_dir(ticket_id)
                .join(attachment_id.to_string());
            fs::create_dir_all(&dir)?;
            let path = dir.join(&name);
            fs::write(&path, bytes)?;
            if !ticket
                .attachments
                .iter()
                .any(|item| item.id == attachment_id)
            {
                ticket.attachments.push(Attachment {
                    id: attachment_id,
                    filename: name,
                    created_at: created_at.clone(),
                    batch_id: metadata.batch_id,
                    batch_label: metadata.batch_label,
                    actor: metadata.actor,
                    purpose: metadata.purpose,
                    annotations: Vec::new(),
                    crop: None,
                });
                ticket.attachments.sort_by(|a, b| {
                    a.created_at
                        .chronological_cmp(&b.created_at)
                        .unwrap_or(std::cmp::Ordering::Equal)
                        .then(a.id.cmp(&b.id))
                });
            }
            ticket.updated_at = if preserve_updated_at {
                previous_updated_at
            } else {
                created_at
            };
            self.write_ticket_with_paths_committing(&ticket, &[self.attachment_dir(ticket_id)])?;
            Ok((ticket, path))
        })
    }

    /// Apply one durable grouping/provenance value set to a selected attachment subset.
    /// Supplying a fresh `batch_id` splits the selection; reusing one merges it.
    pub fn set_attachment_metadata(
        &self,
        ticket_id: &Ulid,
        attachment_ids: &[Ulid],
        metadata: hotsheet_model::AttachmentMetadata,
        now: Timestamp,
    ) -> Result<Ticket, StoreError> {
        self.with_ticket_transaction(|| {
            let mut ticket = self.read_ticket(ticket_id)?;
            let mut changed = false;
            for id in attachment_ids {
                let attachment = ticket
                    .attachments
                    .iter_mut()
                    .find(|attachment| &attachment.id == id)
                    .ok_or_else(|| {
                        StoreError::Io(std::io::Error::new(
                            std::io::ErrorKind::NotFound,
                            format!("attachment {id}"),
                        ))
                    })?;
                if attachment.batch_id != metadata.batch_id
                    || attachment.batch_label != metadata.batch_label
                    || attachment.actor != metadata.actor
                    || attachment.purpose != metadata.purpose
                {
                    attachment.batch_id.clone_from(&metadata.batch_id);
                    attachment.batch_label.clone_from(&metadata.batch_label);
                    attachment.actor.clone_from(&metadata.actor);
                    attachment.purpose = metadata.purpose;
                    changed = true;
                }
            }
            if !changed {
                return Ok(ticket);
            }
            ticket.updated_at = now;
            self.write_ticket_committing(&ticket)?;
            Ok(ticket)
        })
    }

    /// Publish evidence payloads and their ticket metadata as one observable mutation.
    /// Payload directories are staged first and the ticket file is renamed last, so
    /// readers either see the old ticket or the complete report. Failures before that
    /// final rename remove every staged/published payload and preserve the old ticket.
    pub fn write_ticket_with_attachments_atomic(
        &self,
        ticket: &Ticket,
        attachments: &[AtomicAttachment],
    ) -> Result<Ticket, StoreError> {
        self.ensure_current_writer_format()?;
        let ticket_path = self.ticket_path(&ticket.id);
        let ticket_parent = ticket_path
            .parent()
            .ok_or_else(|| StoreError::Io(std::io::Error::other("ticket path has no parent")))?;
        fs::create_dir_all(ticket_parent)?;
        let attachment_root = self.attachment_dir(&ticket.id);
        fs::create_dir_all(&attachment_root)?;

        for item in attachments {
            let final_dir = attachment_root.join(item.id.to_string());
            if final_dir.exists() {
                return Err(StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::AlreadyExists,
                    format!("attachment {} already exists", item.id),
                )));
            }
        }

        let nonce = format!(
            "{}-{}",
            std::process::id(),
            ticket.updated_at.as_str().replace([':', '/', '+'], "-")
        );
        let stage_root = attachment_root.join(format!(".not-working-{nonce}"));
        let staged_ticket = ticket_parent.join(format!(".{}.not-working-{nonce}.tmp", ticket.id));
        if stage_root.exists() || staged_ticket.exists() {
            return Err(StoreError::Io(std::io::Error::new(
                std::io::ErrorKind::AlreadyExists,
                "a Not Working transaction with this timestamp is already staged",
            )));
        }
        fs::create_dir(&stage_root)?;
        let mut published = Vec::new();
        let incoming_ids = attachments
            .iter()
            .map(|item| item.id)
            .collect::<std::collections::HashSet<_>>();
        let mut normalized = ticket.clone();
        let mut used_names = normalized
            .attachments
            .iter()
            .filter(|item| !incoming_ids.contains(&item.id))
            .map(|item| item.filename.clone())
            .collect::<Vec<_>>();
        let normalized_names = attachments
            .iter()
            .map(|item| {
                let name = unique_attachment_filename(
                    &item.filename,
                    used_names.iter().map(String::as_str),
                );
                used_names.push(name.clone());
                if let Some(metadata) = normalized
                    .attachments
                    .iter_mut()
                    .find(|attachment| attachment.id == item.id)
                {
                    metadata.filename.clone_from(&name);
                }
                name
            })
            .collect::<Vec<_>>();
        let result = (|| {
            for (item, name) in attachments.iter().zip(&normalized_names) {
                let dir = stage_root.join(item.id.to_string());
                fs::create_dir(&dir)?;
                fs::write(dir.join(name), &item.bytes)?;
            }
            normalized.schema = ticket_write_schema(&normalized);
            fs::write(&staged_ticket, to_file_string(&normalized))?;
            for item in attachments {
                let final_dir = attachment_root.join(item.id.to_string());
                fs::rename(stage_root.join(item.id.to_string()), &final_dir)?;
                published.push(final_dir);
            }
            fs::rename(&staged_ticket, &ticket_path)?;
            Ok::<(), std::io::Error>(())
        })();
        let _ = fs::remove_dir_all(&stage_root);
        let _ = fs::remove_file(&staged_ticket);
        if let Err(error) = result {
            for path in published {
                let _ = fs::remove_dir_all(path);
            }
            return Err(StoreError::Io(error));
        }
        let status = serde_json::to_value(ticket.status)
            .ok()
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_else(|| "update".into());
        published.push(ticket_path);
        published.push(self.root.join(".gitignore"));
        if let Err(error) = self.autocommit_paths(
            &format!("{}: {status} — {}", ticket.slug, ticket.title),
            &published,
        ) {
            eprintln!("warning: hotsheet autocommit failed: {error}");
        }
        Ok(normalized)
    }

    pub fn rename_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        now: Timestamp,
        filename: &str,
    ) -> Result<Ticket, StoreError> {
        self.with_ticket_transaction(|| {
            let mut ticket = self.read_ticket(ticket_id)?;
            let attachment_index = ticket
                .attachments
                .iter()
                .position(|item| &item.id == attachment_id)
                .ok_or_else(|| {
                    StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("attachment {attachment_id}"),
                    ))
                })?;
            let name = unique_attachment_filename(
                filename,
                ticket
                    .attachments
                    .iter()
                    .filter(|item| &item.id != attachment_id)
                    .map(|item| item.filename.as_str()),
            );
            if ticket.attachments[attachment_index].filename == name {
                return Ok(ticket);
            }
            let attachment = ticket
                .attachments
                .get_mut(attachment_index)
                .expect("attachment index came from this collection");
            if attachment.crop.is_some() {
                let extension = |name: &str| match name
                    .rsplit('.')
                    .next()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "jpg" | "jpeg" => "jpeg",
                    "png" => "png",
                    "webp" => "webp",
                    "gif" => "gif",
                    "avif" => "avif",
                    "svg" => "svg",
                    "bmp" => "bmp",
                    "ico" => "ico",
                    _ => "unsupported",
                };
                if extension(&attachment.filename) != extension(&name) {
                    return Err(StoreError::ImageCrop(
                        crate::image_crop::ImageCropError::UnsupportedFormat,
                    ));
                }
            }
            let dir = self
                .attachment_dir(ticket_id)
                .join(attachment_id.to_string());
            fs::create_dir_all(&dir)?;
            let nested_source = dir.join(&attachment.filename);
            let source = if nested_source.is_file() {
                nested_source
            } else {
                self.attachment_dir(ticket_id).join(&attachment.filename)
            };
            fs::rename(source, dir.join(&name))?;
            attachment.filename = name;
            ticket.updated_at = now;
            self.write_ticket_with_paths_committing(&ticket, &[self.attachment_dir(ticket_id)])?;
            Ok(ticket)
        })
    }

    /// Read an attachment payload using its durable ticket-scoped identity.
    pub fn read_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
    ) -> Result<(Attachment, Vec<u8>), StoreError> {
        let ticket = self.read_ticket(ticket_id)?;
        let attachment = ticket
            .attachments
            .iter()
            .find(|item| &item.id == attachment_id)
            .cloned()
            .ok_or_else(|| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("attachment {attachment_id}"),
                ))
            })?;
        let nested = self
            .attachment_dir(ticket_id)
            .join(attachment_id.to_string())
            .join(&attachment.filename);
        let legacy = self.attachment_dir(ticket_id).join(&attachment.filename);
        let bytes = fs::read(if nested.is_file() { nested } else { legacy })?;
        Ok((attachment, bytes))
    }

    /// Remove attachment metadata and its payload, committing the ticket update.
    pub fn remove_attachment(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        now: Timestamp,
    ) -> Result<Ticket, StoreError> {
        self.with_ticket_transaction(|| {
            let mut ticket = self.read_ticket(ticket_id)?;
            let index = ticket
                .attachments
                .iter()
                .position(|item| &item.id == attachment_id)
                .ok_or_else(|| {
                    StoreError::Io(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("attachment {attachment_id}"),
                    ))
                })?;
            let attachment = ticket.attachments.remove(index);
            let nested = self
                .attachment_dir(ticket_id)
                .join(attachment_id.to_string());
            if nested.exists() {
                fs::remove_dir_all(nested)?;
            } else {
                let legacy = self.attachment_dir(ticket_id).join(attachment.filename);
                if legacy.exists() {
                    fs::remove_file(legacy)?;
                }
            }
            ticket.updated_at = now;
            self.write_ticket_with_paths_committing(&ticket, &[self.attachment_dir(ticket_id)])?;
            Ok(ticket)
        })
    }

    /// Replace an attachment's annotations and append one activity note in the same commit.
    /// No-op batches neither rewrite the ticket nor add a note.
    pub fn set_attachment_annotations_with_activity(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        annotations: Vec<hotsheet_model::MediaAnnotation>,
        actor: Option<hotsheet_model::NoteActor>,
        note_id: Ulid,
        now: Timestamp,
    ) -> Result<Ticket, StoreError> {
        self.set_attachment_markup_with_activity_inner(
            ticket_id,
            attachment_id,
            MarkupChange {
                annotations,
                crop_change: None,
            },
            actor,
            note_id,
            now,
        )
    }

    /// Persist one crop and annotation session in a single ticket commit and activity note.
    /// The original image bytes and original-space annotation coordinates are never rewritten.
    pub fn set_attachment_markup_with_activity(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        markup: AttachmentMarkup,
        actor: Option<hotsheet_model::NoteActor>,
        note_id: Ulid,
        now: Timestamp,
    ) -> Result<Ticket, StoreError> {
        self.set_attachment_markup_with_activity_inner(
            ticket_id,
            attachment_id,
            MarkupChange {
                annotations: markup.annotations,
                crop_change: Some(markup.crop),
            },
            actor,
            note_id,
            now,
        )
    }

    fn set_attachment_markup_with_activity_inner(
        &self,
        ticket_id: &Ulid,
        attachment_id: &Ulid,
        markup: MarkupChange,
        actor: Option<hotsheet_model::NoteActor>,
        note_id: Ulid,
        now: Timestamp,
    ) -> Result<Ticket, StoreError> {
        self.with_ticket_transaction(|| {
        let MarkupChange {
            annotations,
            crop_change,
        } = markup;
        hotsheet_model::validate_media_annotations(&annotations)
            .map_err(StoreError::InvalidAnnotations)?;
        let mut ticket = self.read_ticket(ticket_id)?;
        let next_crop = if let Some(requested) = crop_change {
            if let Some(requested) = requested {
                let (metadata, bytes) = self.read_attachment(ticket_id, attachment_id)?;
                let dimensions = crate::image_crop::original_dimensions(&metadata.filename, &bytes)?;
                let normalized = crate::image_crop::normalize_crop(requested, dimensions)?;
                if let Some(crop) = normalized {
                    crate::image_crop::validate_rendition(&metadata.filename, &bytes, crop)?;
                }
                normalized
            } else {
                None
            }
        } else {
            ticket
                .attachments
                .iter()
                .find(|item| &item.id == attachment_id)
                .and_then(|item| item.crop)
        };
        let attachment = ticket
            .attachments
            .iter_mut()
            .find(|item| &item.id == attachment_id)
            .ok_or_else(|| {
                StoreError::Io(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("attachment {attachment_id}"),
                ))
            })?;
        let annotation_activity = crate::annotation_activity::annotation_change_activity(
            &attachment.filename,
            &attachment.annotations,
            &annotations,
        );
        let crop_changed = attachment.crop != next_crop;
        if annotation_activity.is_none() && !crop_changed {
            return Ok(ticket);
        }
        let (summary, text) = if crop_changed {
            let crop_text = if let Some(crop) = next_crop {
                format!(
                    "Cropped [attachment:{}](attachment:{}) to {} × {} px at ({}, {}) of the original.",
                    attachment.filename,
                    attachment.filename,
                    crop.width,
                    crop.height,
                    crop.x,
                    crop.y
                )
            } else {
                format!(
                    "Restored the original [attachment:{}](attachment:{}).",
                    attachment.filename, attachment.filename
                )
            };
            let text = if let Some((_, annotation_text)) = annotation_activity {
                format!("{crop_text}\n\n{annotation_text}")
            } else {
                crop_text
            };
            (format!("Updated crop for {}", attachment.filename), text)
        } else {
            annotation_activity.expect("annotation activity exists when crop did not change")
        };
        attachment.annotations = annotations;
        attachment.crop = next_crop;
        ticket.notes.push(Note {
            id: note_id,
            kind: NoteKind::Activity,
            created_at: now.clone(),
            edited_at: now.clone(),
            summary: Some(summary),
            confidence: None,
            feedback_for: None,
            ai_feedback: None,
            human_edited: false,
            actor,
            text,
        });
        ticket.updated_at = now;
        ticket.schema = ticket_write_schema(&ticket);
        self.write_ticket_committing(&ticket)?;
        Ok(ticket)
        })
    }

    fn add_legacy_attachment_metadata(&self, ticket: &mut Ticket) -> Result<(), StoreError> {
        let dir = self.attachment_dir(&ticket.id);
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        for entry in entries {
            let entry = entry?;
            if !entry.file_type()?.is_file() {
                continue;
            }
            let filename = entry.file_name().to_string_lossy().to_string();
            if filename == FINDER_METADATA_FILE {
                continue;
            }
            let id = Self::legacy_attachment_id(&ticket.id, &filename);
            if !ticket.attachments.iter().any(|item| item.id == id) {
                ticket.attachments.push(Attachment {
                    id,
                    filename,
                    created_at: ticket.created_at.clone(),
                    batch_id: None,
                    batch_label: None,
                    actor: None,
                    purpose: None,
                    annotations: Vec::new(),
                    crop: None,
                });
            }
        }
        ticket.attachments.sort_by_key(|item| item.id);
        Ok(())
    }

    /// Every `tickets/<shard>/<ULID>.md` file path in the store, unsorted. A
    /// directory-level I/O error (e.g. an unreadable shard) is fatal; per-file
    /// parse errors are the concern of the caller, not this walk.
    pub(crate) fn ticket_file_paths(&self) -> Result<Vec<PathBuf>, StoreError> {
        let mut paths = Vec::new();
        let tickets_dir = self.root.join("tickets");
        if !tickets_dir.is_dir() {
            return Ok(paths);
        }
        let shards = fs::read_dir(&tickets_dir).map_err(|source| StoreError::IoAt {
            operation: "listing ticket directory",
            path: tickets_dir.clone(),
            source,
        })?;
        for shard in shards {
            let shard = shard.map_err(|source| StoreError::IoAt {
                operation: "reading ticket directory entry",
                path: tickets_dir.clone(),
                source,
            })?;
            let shard_path = shard.path();
            if !shard
                .file_type()
                .map_err(|source| StoreError::IoAt {
                    operation: "reading ticket shard metadata",
                    path: shard_path.clone(),
                    source,
                })?
                .is_dir()
            {
                continue;
            }
            let entries = fs::read_dir(&shard_path).map_err(|source| StoreError::IoAt {
                operation: "listing ticket shard",
                path: shard_path.clone(),
                source,
            })?;
            for entry in entries {
                let path = entry
                    .map_err(|source| StoreError::IoAt {
                        operation: "reading ticket shard entry",
                        path: shard_path.clone(),
                        source,
                    })?
                    .path();
                if path.extension().and_then(|e| e.to_str()) == Some("md") {
                    paths.push(path);
                }
            }
        }
        Ok(paths)
    }

    /// Read every ticket in the store, sorted by id (≈ creation order). **Strict:**
    /// the first unparseable file aborts the whole enumeration with its
    /// [`StoreError::Parse`]. Callers that must survive a corrupt file (project open,
    /// the server ticket list) use [`FsStore::list_tickets_resilient`] instead.
    pub fn list_tickets(&self) -> Result<Vec<Ticket>, StoreError> {
        let mut out = Vec::new();
        for path in self.ticket_file_paths()? {
            out.push(self.read_ticket_at(&path)?);
        }
        out.sort_by_key(|t| t.id);
        Ok(out)
    }

    /// Enumerate the store **resiliently**: return every healthy ticket AND a separate
    /// report of the files that failed to parse, rather than letting one bad file abort
    /// the whole scan (HS2-PRVPCQ). A single unparseable `.md` — for example a ticket
    /// whose notes block is missing its `<!-- hotsheet:notes:end -->` marker — must never
    /// prevent the rest of a project from loading. Directory-level I/O errors (an
    /// unreadable `tickets/` tree) are still fatal; a per-file read or parse failure is
    /// captured as a [`CorruptTicket`] with its recoverable id/slug and error message.
    pub fn list_tickets_resilient(&self) -> Result<StoreListing, StoreError> {
        let mut tickets = Vec::new();
        let mut corrupt = Vec::new();
        for path in self.ticket_file_paths()? {
            match self.read_ticket_at(&path) {
                Ok(ticket) => tickets.push(ticket),
                Err(error) => corrupt.push(CorruptTicket::from_error(path, &error)),
            }
        }
        tickets.sort_by_key(|t| t.id);
        corrupt.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(StoreListing { tickets, corrupt })
    }

    // ---- git-diff fast path (docs/03 §3.4, HS2-90) --------------------------------

    /// The current `HEAD` commit id, or `None` if the store isn't a git repo yet or has
    /// no commits. Used to detect a HEAD move (commit/pull/checkout) for the incremental
    /// reindex fast path.
    pub fn head_commit(&self) -> Option<String> {
        git_stdout(&self.root, &["rev-parse", "HEAD"]).map(|s| s.trim().to_string())
    }

    /// Whether the working tree + index are clean (no uncommitted changes). The fast path
    /// only fires on a *pure* HEAD move; any local edits fall back to the full hash-walk.
    pub fn is_working_tree_clean(&self) -> bool {
        git_stdout(&self.root, &["status", "--porcelain"])
            .map(|s| s.trim().is_empty())
            .unwrap_or(false)
    }

    /// The ticket ULIDs whose files changed between two commits (`git diff --name-only
    /// old new -- tickets/`), parsed from the `tickets/NN/<ULID>.md` paths. Renames are
    /// broken into a delete + add (`--no-renames`) so both endpoints reconcile. A path
    /// that no longer parses as a ULID is skipped.
    pub fn changed_ticket_ids_between(
        &self,
        old: &str,
        new: &str,
    ) -> Result<Vec<Ulid>, StoreError> {
        let out = git_stdout(
            &self.root,
            &[
                "diff",
                "--name-only",
                "--no-renames",
                old,
                new,
                "--",
                "tickets",
            ],
        )
        .ok_or_else(|| StoreError::Git(format!("`git diff {old} {new}` failed")))?;

        let mut ids = Vec::new();
        for line in out.lines() {
            if let Some(id) = Path::new(line.trim())
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Ulid::from_string(s).ok())
            {
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// Ticket ULIDs changed in the index or working tree relative to `HEAD`.
    ///
    /// `--no-renames` makes both sides of a rename appear independently, which lets
    /// the index reconcile the old deletion and the new file without parsing Git's
    /// rename display syntax. Untracked ticket files are included so direct editor or
    /// script writes become visible without a full store walk.
    pub fn changed_ticket_ids_in_worktree(&self) -> Result<Vec<Ulid>, StoreError> {
        let out = git_stdout(
            &self.root,
            &[
                "status",
                "--porcelain=v1",
                "--untracked-files=all",
                "--no-renames",
                "--",
                "tickets",
            ],
        )
        .ok_or_else(|| StoreError::Git("`git status --porcelain tickets` failed".into()))?;

        let mut ids = Vec::new();
        for line in out.lines() {
            // Porcelain v1 prefixes each path with two status columns and a space.
            let path = line.get(3..).unwrap_or_default().trim_matches('"');
            if let Some(id) = Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .and_then(|s| Ulid::from_string(s).ok())
            {
                ids.push(id);
            }
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }
}

// ---- resilient-enumeration helpers -------------------------------------------------

impl CorruptTicket {
    /// The report for a ticket file whose read or parse failed with `error`: the
    /// recoverable identity plus a stable error class and user-facing message.
    pub(crate) fn from_error(path: PathBuf, error: &StoreError) -> Self {
        let (id, slug) = recover_ticket_identity(&path);
        let (error_code, message) = match error {
            StoreError::Parse { source, .. } => (source.code(), source.user_message()),
            _ => ("invalid_ticket", error.to_string()),
        };
        Self {
            path,
            id,
            slug,
            error: message,
            error_code,
        }
    }
}

/// Best-effort recovery of a failed file's identity for a [`CorruptTicket`]. The id
/// comes from the filename stem (the canonical ULID); if the stem isn't a ULID we fall
/// back to a frontmatter `id:` line. The slug comes from a frontmatter `slug:` line.
/// Everything here degrades to `None` rather than erroring — the file is already known
/// bad, so this only enriches the report.
pub fn recover_ticket_identity(path: &Path) -> (Option<Ulid>, Option<String>) {
    let stem_id = path
        .file_stem()
        .and_then(|s| s.to_str())
        .and_then(|s| Ulid::from_string(s).ok());
    let text = fs::read_to_string(path).ok();
    let slug = text.as_deref().and_then(|t| frontmatter_value(t, "slug"));
    let id = stem_id.or_else(|| {
        text.as_deref()
            .and_then(|t| frontmatter_value(t, "id"))
            .and_then(|v| Ulid::from_string(&v).ok())
    });
    (id, slug)
}

/// Pull a top-level scalar `key: value` from a file's leading `---` frontmatter block,
/// tolerating a malformed body below it. Returns the trimmed, unquoted value, or `None`
/// when there is no readable frontmatter or the key is absent/empty. Only exact,
/// unindented top-level keys match (a nested/indented key is ignored).
fn frontmatter_value(text: &str, key: &str) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let normalized = text.replace("\r\n", "\n");
    let rest = normalized.strip_prefix("---\n")?;
    let frontmatter = rest.split_once("\n---").map_or(rest, |(fm, _)| fm);
    for line in frontmatter.lines() {
        let Some(after) = line.strip_prefix(key).and_then(|a| a.strip_prefix(':')) else {
            continue;
        };
        let value = after.trim().trim_matches(['"', '\'']).trim();
        if !value.is_empty() {
            return Some(value.to_string());
        }
    }
    None
}

/// Replace `path` with `bytes` so a concurrent reader sees either the old or the new
/// complete file, never an empty or partial one (HS2-P2178F). `fs::write` truncates
/// first, so another process (a CLI `ls`, the server's watcher/reconcile) could parse
/// a ticket mid-write and report it corrupt. The staged sibling is a dot-file with a
/// non-`.md` extension, so enumeration never mistakes a leftover for a ticket.
pub(crate) fn write_file_atomically(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    static SEQUENCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let name = path
        .file_name()
        .ok_or_else(|| std::io::Error::other("atomic write target has no file name"))?
        .to_string_lossy();
    let staged = path.with_file_name(format!(
        ".{name}.{}.{}.tmp",
        std::process::id(),
        SEQUENCE.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let result = fs::write(&staged, bytes).and_then(|()| fs::rename(&staged, path));
    if result.is_err() {
        let _ = fs::remove_file(&staged);
    }
    result
}

// ---- git helpers (shell-based; the store IS a git repo, docs/02 §2.3) --------------

/// Replay the complete local transaction after lock contention. Another writer may
/// have committed during the wait, so re-stage and re-check rather than retrying a
/// stale commit alone. Never remove Git's lock or retry unrelated failures.
fn retry_autocommit(
    mut attempt: impl FnMut() -> Result<bool, StoreError>,
    mut wait: impl FnMut(Duration),
    mut retries: usize,
) -> Result<bool, StoreError> {
    let mut delay = Duration::from_millis(25);
    loop {
        let result = attempt();
        let contention = matches!(&result, Err(StoreError::Git(message)) if
            (message.contains(".lock") && message.contains("File exists"))
                || (message.contains("cannot lock ref") && message.contains("but expected")));
        if !contention || retries == 0 {
            return result;
        }
        retries -= 1;
        wait(delay);
        delay = (delay * 2).min(Duration::from_millis(250));
    }
}

/// Run `git -C root <args>`, erroring on a non-zero exit.
fn git(root: &Path, args: &[&str]) -> Result<(), StoreError> {
    git_paths(root, args, &[])
}

fn git_paths(root: &Path, args: &[&str], paths: &[PathBuf]) -> Result<(), StoreError> {
    let relative = paths
        .iter()
        .map(|path| path.strip_prefix(root).unwrap_or(path))
        .collect::<Vec<_>>();
    let output = crate::git::command()
        .env("LC_ALL", "C")
        .arg("-C")
        .arg(root)
        .args(args)
        .args(&relative)
        .output()?;
    if output.status.success() {
        Ok(())
    } else {
        Err(StoreError::Git(format!(
            "`git {}` failed for {} path(s): {}",
            args.join(" "),
            paths.len(),
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }
}

/// True when `git -C root <args>` exits 0 (used for `diff --cached --quiet`).
fn git_ok(root: &Path, args: &[&str]) -> bool {
    crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn git_ok_paths(root: &Path, args: &[&str], paths: &[PathBuf]) -> bool {
    let relative = paths
        .iter()
        .map(|path| path.strip_prefix(root).unwrap_or(path))
        .collect::<Vec<_>>();
    crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .args(relative)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Captured stdout of `git -C root <args>`, or `None` if it failed to run.
fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let out = crate::git::command()
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .ok()?;
    out.status
        .success()
        .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{ImageCrop, MediaAnnotation, derive_slug};

    #[cfg(unix)]
    struct HostSaturation {
        stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
        workers: Vec<std::thread::JoinHandle<()>>,
    }

    #[cfg(unix)]
    impl HostSaturation {
        fn start(io_path: PathBuf) -> Self {
            use std::io::{Seek, SeekFrom, Write};
            use std::sync::atomic::Ordering;

            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
            let cpu_workers = std::thread::available_parallelism()
                .map(std::num::NonZeroUsize::get)
                .unwrap_or(2);
            let mut workers = (0..cpu_workers)
                .map(|_| {
                    let stop = stop.clone();
                    std::thread::spawn(move || {
                        while !stop.load(Ordering::Relaxed) {
                            std::hint::spin_loop();
                        }
                    })
                })
                .collect::<Vec<_>>();
            let io_stop = stop.clone();
            workers.push(std::thread::spawn(move || {
                let mut file = fs::OpenOptions::new()
                    .create(true)
                    .truncate(true)
                    .write(true)
                    .open(io_path)
                    .unwrap();
                let block = [0x5a; 64 * 1024];
                while !io_stop.load(Ordering::Relaxed) {
                    file.write_all(&block).unwrap();
                    file.sync_data().unwrap();
                    file.seek(SeekFrom::Start(0)).unwrap();
                }
            }));
            Self { stop, workers }
        }
    }

    #[cfg(unix)]
    impl Drop for HostSaturation {
        fn drop(&mut self) {
            use std::sync::atomic::Ordering;

            self.stop.store(true, Ordering::Relaxed);
            for worker in self.workers.drain(..) {
                worker.join().unwrap();
            }
        }
    }

    #[cfg(unix)]
    fn fifo(path: &Path) {
        use std::ffi::CString;
        use std::os::unix::ffi::OsStrExt;

        let path = CString::new(path.as_os_str().as_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
    }

    fn ulid(s: &str) -> Ulid {
        Ulid::from_string(s).unwrap()
    }

    fn temp_store() -> (tempfile::TempDir, FsStore) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, store)
    }

    fn sample(id: Ulid) -> Ticket {
        Ticket::new(
            id,
            derive_slug(&id, "HS"),
            "A ticket",
            "bug",
            "2026-08-19T00:00:00Z",
            "2026-08-19T00:00:00Z",
        )
    }

    #[test]
    fn note_transaction_serializes_ratings_across_processes() {
        use hotsheet_model::{AiFeedback, AiFeedbackRating, AttachmentActorRole, NoteActor};
        use std::process::Command;

        const TEST_NAME: &str =
            "store::tests::note_transaction_serializes_ratings_across_processes";
        if let Ok(root) = std::env::var("HOTSHEET_NOTE_TEST_CHILD_ROOT") {
            let store = FsStore::open(root).unwrap();
            let id =
                Ulid::from_string(&std::env::var("HOTSHEET_NOTE_TEST_TICKET").unwrap()).unwrap();
            let actor = std::env::var("HOTSHEET_NOTE_TEST_ACTOR").unwrap();
            fs::write(std::env::var("HOTSHEET_NOTE_TEST_READY").unwrap(), "ready").unwrap();
            crate::ops::rate_ai_content(
                &store,
                &id,
                Timestamp::new("2026-08-19T00:02:00Z"),
                "conversation:shared",
                Some(AiFeedbackRating::Helpful),
                None,
                Some(NoteActor {
                    role: AttachmentActorRole::Human,
                    id: Some(actor),
                }),
            )
            .unwrap();
            return;
        }

        let (dir, store) = temp_store();
        let id = Ulid::new();
        store.write_ticket(&sample(id)).unwrap();
        let spawn = |actor: &str, ready: &Path| {
            Command::new(std::env::current_exe().unwrap())
                .args(["--exact", TEST_NAME, "--nocapture"])
                .env("HOTSHEET_NOTE_TEST_CHILD_ROOT", dir.path())
                .env("HOTSHEET_NOTE_TEST_TICKET", id.to_string())
                .env("HOTSHEET_NOTE_TEST_ACTOR", actor)
                .env("HOTSHEET_NOTE_TEST_READY", ready)
                .spawn()
                .unwrap()
        };
        let ready = dir.path().join("child-ready");
        let mut child = store
            .with_ticket_transaction(|| {
                let mut stale = store.read_ticket(&id)?;
                let mut child = spawn("second", &ready);
                for _ in 0..200 {
                    if ready.exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(ready.exists(), "child did not reach the rating call");
                std::thread::sleep(Duration::from_millis(150));
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "child bypassed the transaction lock"
                );
                stale.notes.push(Note {
                    id: Ulid::new(),
                    kind: NoteKind::Regular,
                    created_at: Timestamp::new("2026-08-19T00:01:00Z"),
                    edited_at: Timestamp::new("2026-08-19T00:01:00Z"),
                    summary: None,
                    confidence: None,
                    feedback_for: None,
                    ai_feedback: Some(AiFeedback {
                        target: "conversation:shared".into(),
                        rating: Some(AiFeedbackRating::NotHelpful),
                        explanation: None,
                    }),
                    human_edited: false,
                    actor: Some(NoteActor {
                        role: AttachmentActorRole::Human,
                        id: Some("first".into()),
                    }),
                    text: "First rating".into(),
                });
                store.write_ticket_committing(&stale)?;
                Ok::<_, StoreError>(child)
            })
            .unwrap();
        assert!(child.wait().unwrap().success());
        let ticket = store.read_ticket(&id).unwrap();
        assert_eq!(
            ticket.notes.len(),
            2,
            "independent raters must both survive"
        );

        // Repeated concurrent writes by one rater revise the same source note.
        let ready_a = dir.path().join("same-ready-a");
        let ready_b = dir.path().join("same-ready-b");
        let mut a = spawn("same", &ready_a);
        let mut b = spawn("same", &ready_b);
        assert!(a.wait().unwrap().success());
        assert!(b.wait().unwrap().success());
        let ticket = store.read_ticket(&id).unwrap();
        assert_eq!(ticket.notes.len(), 3);
        assert_eq!(
            ticket
                .notes
                .iter()
                .filter(
                    |note| note.actor.as_ref().and_then(|actor| actor.id.as_deref())
                        == Some("same")
                )
                .count(),
            1,
        );
    }

    #[test]
    fn claim_waits_for_note_transaction_across_processes() {
        use std::process::Command;

        const TEST_NAME: &str = "store::tests::claim_waits_for_note_transaction_across_processes";
        if let Ok(root) = std::env::var("HOTSHEET_CLAIM_TEST_CHILD_ROOT") {
            let store = FsStore::open(root).unwrap();
            let id =
                Ulid::from_string(&std::env::var("HOTSHEET_CLAIM_TEST_TICKET").unwrap()).unwrap();
            fs::write(std::env::var("HOTSHEET_CLAIM_TEST_READY").unwrap(), "ready").unwrap();
            crate::ops::claim(
                &store,
                &id,
                &Timestamp::new("2026-08-19T00:02:00Z"),
                Timestamp::new("2026-08-19T00:32:00Z"),
                "worker",
                None,
            )
            .unwrap();
            return;
        }

        let (dir, store) = temp_store();
        let id = Ulid::new();
        store.write_ticket(&sample(id)).unwrap();
        let ready = dir.path().join("claim-ready");
        let mut child = store
            .with_ticket_transaction(|| {
                let mut stale = store.read_ticket(&id)?;
                let mut child = Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", TEST_NAME, "--nocapture"])
                    .env("HOTSHEET_CLAIM_TEST_CHILD_ROOT", dir.path())
                    .env("HOTSHEET_CLAIM_TEST_TICKET", id.to_string())
                    .env("HOTSHEET_CLAIM_TEST_READY", &ready)
                    .spawn()
                    .unwrap();
                for _ in 0..200 {
                    if ready.exists() {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                assert!(ready.exists(), "child did not reach the claim call");
                std::thread::sleep(Duration::from_millis(150));
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "claim bypassed the lock"
                );
                stale.notes.push(Note {
                    id: Ulid::new(),
                    kind: NoteKind::Regular,
                    created_at: Timestamp::new("2026-08-19T00:01:00Z"),
                    edited_at: Timestamp::new("2026-08-19T00:01:00Z"),
                    summary: None,
                    confidence: None,
                    feedback_for: None,
                    ai_feedback: None,
                    human_edited: false,
                    actor: None,
                    text: "Independent note".into(),
                });
                store.write_ticket_committing(&stale)?;
                Ok::<_, StoreError>(child)
            })
            .unwrap();
        assert!(child.wait().unwrap().success());
        let ticket = store.read_ticket(&id).unwrap();
        assert_eq!(ticket.claimed_by.as_deref(), Some("worker"));
        assert_eq!(
            ticket
                .notes
                .iter()
                .filter(|note| note.text == "Independent note")
                .count(),
            1
        );
    }

    #[test]
    fn crop_changes_keep_original_bytes_and_annotations_through_edit_and_restore() {
        use std::io::Cursor;

        let (_dir, store) = temp_store();
        let ticket_id = Ulid::new();
        let attachment_id = Ulid::new();
        store.write_ticket(&sample(ticket_id)).unwrap();
        let image = image::RgbImage::from_fn(20, 16, |x, y| image::Rgb([x as u8, y as u8, 42]));
        let mut output = Cursor::new(Vec::new());
        image::DynamicImage::ImageRgb8(image)
            .write_to(&mut output, image::ImageFormat::Png)
            .unwrap();
        let original = output.into_inner();
        store
            .write_attachment(
                &ticket_id,
                attachment_id,
                Timestamp::new("2026-08-19T00:01:00Z"),
                "evidence.png",
                &original,
            )
            .unwrap();
        let annotation = MediaAnnotation {
            id: "mark-1".into(),
            x: 2_000,
            y: 3_000,
            width: 4_000,
            height: 2_000,
            start_ms: None,
            end_ms: None,
            text: "Edge".into(),
            shape: None,
            intents: Vec::new(),
        };
        let first = ImageCrop {
            x: 2,
            y: 2,
            width: 12,
            height: 10,
        };
        let second = ImageCrop {
            x: 5,
            y: 3,
            width: 10,
            height: 8,
        };
        let apply = |crop, annotations, minute| {
            store
                .set_attachment_markup_with_activity(
                    &ticket_id,
                    &attachment_id,
                    AttachmentMarkup { annotations, crop },
                    None,
                    Ulid::new(),
                    Timestamp::new(format!("2026-08-19T00:{minute:02}:00Z")),
                )
                .unwrap()
        };
        let first_ticket = apply(Some(first), vec![annotation.clone()], 2);
        assert_eq!(first_ticket.attachments[0].crop, Some(first));
        assert_eq!(first_ticket.notes.len(), 1);
        let repeated = apply(Some(first), vec![annotation.clone()], 3);
        assert_eq!(repeated.notes.len(), 1);
        let mut edited = annotation.clone();
        edited.text = "Updated edge".into();
        let edited_ticket = store
            .set_attachment_annotations_with_activity(
                &ticket_id,
                &attachment_id,
                vec![edited.clone()],
                None,
                Ulid::new(),
                Timestamp::new("2026-08-19T00:04:00Z"),
            )
            .unwrap();
        assert_eq!(edited_ticket.attachments[0].crop, Some(first));
        let next = apply(Some(second), vec![edited.clone()], 5);
        assert_eq!(next.attachments[0].crop, Some(second));
        assert_eq!(next.attachments[0].annotations, vec![edited.clone()]);
        let invalid = store.set_attachment_markup_with_activity(
            &ticket_id,
            &attachment_id,
            AttachmentMarkup {
                annotations: vec![edited.clone()],
                crop: Some(ImageCrop {
                    x: 15,
                    y: 0,
                    width: 8,
                    height: 8,
                }),
            },
            None,
            Ulid::new(),
            Timestamp::new("2026-08-19T00:06:00Z"),
        );
        assert!(matches!(invalid, Err(StoreError::ImageCrop(_))));
        assert_eq!(
            store.read_ticket(&ticket_id).unwrap().attachments[0].crop,
            Some(second)
        );
        let restored = apply(None, vec![edited.clone()], 7);
        assert_eq!(restored.attachments[0].crop, None);
        assert_eq!(restored.attachments[0].annotations, vec![edited]);
        assert_eq!(restored.notes.len(), 4);
        assert_eq!(
            store.read_attachment(&ticket_id, &attachment_id).unwrap().1,
            original
        );
    }

    /// HS2-P2178F: a ticket rewrite is atomic for concurrent readers. `fs::write`
    /// truncates first, so a reader racing a rewrite parsed an empty file
    /// (`MissingFrontmatter`); that aborted the CLI lock-retry E2E mid-poll. The reader
    /// here alternates with rewrites of very different sizes and must always parse.
    #[test]
    fn concurrent_readers_never_observe_a_partial_ticket_rewrite() {
        let (_dir, store) = temp_store();
        let id = Ulid::new();
        let path = store.write_ticket(&sample(id)).unwrap();
        let done = std::sync::atomic::AtomicBool::new(false);
        std::thread::scope(|scope| {
            let reader = scope.spawn(|| {
                let mut reads = 0usize;
                while !done.load(std::sync::atomic::Ordering::Acquire) {
                    let ticket = store.read_ticket(&id).unwrap();
                    assert!(
                        ticket.title.starts_with("A ticket") || ticket.title.starts_with("Long")
                    );
                    reads += 1;
                }
                reads
            });
            for round in 0..300 {
                let mut ticket = sample(id);
                if round % 2 == 0 {
                    ticket.title = format!("Long {}", "x".repeat(64 * 1024));
                }
                store.write_ticket(&ticket).unwrap();
            }
            done.store(true, std::sync::atomic::Ordering::Release);
            assert!(reader.join().unwrap() > 0);
        });
        // Staged siblings never linger and are never enumerated as tickets.
        let leftovers: Vec<_> = fs::read_dir(path.parent().unwrap())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .filter(|name| name.to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        fs::write(
            path.with_file_name(format!(
                ".{}.1.2.tmp",
                path.file_name().unwrap().to_string_lossy()
            )),
            "---\npartial",
        )
        .unwrap();
        assert_eq!(store.list_tickets().unwrap().len(), 1);
    }

    #[test]
    fn init_open_and_metadata_round_trip() {
        let (dir, store) = temp_store();
        let metadata = store.metadata().unwrap();
        assert_eq!(metadata.ticket_prefix, "HS");
        assert!(
            metadata
                .instance_id
                .as_deref()
                .is_some_and(|id| id.parse::<Ulid>().is_ok())
        );
        let raw = fs::read_to_string(dir.path().join(STORE_METADATA_FILE)).unwrap();
        assert!(raw.contains(r#""schemaVersion": "hotsheet/v3-random-suffix-shards""#));
        assert!(raw.contains(r#""shard": "id-suffix-2""#));
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct LegacyMetadata {
            #[allow(dead_code)]
            schema_version: u32,
        }
        assert!(serde_json::from_str::<LegacyMetadata>(&raw).is_err());
        // A second open of the same dir succeeds.
        assert!(FsStore::open(dir.path()).is_ok());
    }

    #[test]
    fn store_identity_survives_reinitialization_and_legacy_migration() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let first = store.metadata().unwrap().instance_id.unwrap();
        FsStore::init(dir.path(), &StoreMetadata::new("OTHER")).unwrap();
        assert_eq!(
            store.metadata().unwrap().instance_id.as_deref(),
            Some(first.as_str())
        );

        let mut legacy = store.metadata().unwrap();
        legacy.instance_id = None;
        fs::write(
            dir.path().join(STORE_METADATA_FILE),
            format!("{}\n", serde_json::to_string_pretty(&legacy).unwrap()),
        )
        .unwrap();
        let migrated = store.ensure_instance_id().unwrap();
        assert_ne!(migrated, first);
        assert_eq!(store.ensure_instance_id().unwrap(), migrated);
        assert_eq!(
            store.metadata().unwrap().instance_id.as_deref(),
            Some(migrated.as_str())
        );
    }

    #[test]
    fn malformed_store_identity_is_rejected_before_reinitialization_or_relink() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        let mut metadata = store.metadata().unwrap();
        metadata.instance_id = Some("not-an-ulid".into());
        fs::write(
            dir.path().join(STORE_METADATA_FILE),
            serde_json::to_vec_pretty(&metadata).unwrap(),
        )
        .unwrap();
        assert!(matches!(
            store.metadata(),
            Err(StoreError::InvalidStoreIdentity(_))
        ));
        assert!(matches!(
            FsStore::init(dir.path(), &StoreMetadata::new("HS")),
            Err(StoreError::InvalidStoreIdentity(_))
        ));
    }

    #[test]
    fn legacy_identity_migration_commits_metadata_and_retries_an_uncommitted_id() {
        let (dir, store) = temp_store();
        git(dir.path(), &["init", "-q"]).unwrap();
        assert!(store.autocommit("initial store").unwrap());
        let path = dir.path().join(STORE_METADATA_FILE);
        let mut legacy = store.metadata().unwrap();
        legacy.instance_id = None;
        fs::write(&path, serde_json::to_vec_pretty(&legacy).unwrap()).unwrap();
        assert!(
            store
                .autocommit_paths("legacy store", std::slice::from_ref(&path))
                .unwrap()
        );

        let id = store.ensure_instance_id().unwrap();
        assert_eq!(
            store.metadata().unwrap().instance_id.as_deref(),
            Some(id.as_str())
        );
        assert_eq!(git_stdout(dir.path(), &["status", "--short"]).unwrap(), "");
        assert_eq!(
            git_stdout(dir.path(), &["log", "-1", "--pretty=%s"])
                .unwrap()
                .trim(),
            "Initialize Hot Sheet store identity"
        );

        fs::write(&path, format!("{}\n", fs::read_to_string(&path).unwrap())).unwrap();
        assert_eq!(store.ensure_instance_id().unwrap(), id);
        assert_eq!(git_stdout(dir.path(), &["status", "--short"]).unwrap(), "");
    }

    #[test]
    fn schema_two_prefix_store_remains_read_write_compatible_until_explicit_activation() {
        let (_dir, store) = temp_store();
        let raw = r#"{
  "schemaVersion": 2,
  "ticketPrefix": "HS",
  "idStrategy": "ulid",
  "shard": "id-prefix-2"
}
"#;
        fs::write(store.root().join(STORE_METADATA_FILE), raw).unwrap();
        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let legacy_path = store.ticket_path_for_shard(&ticket.id, SHARD_ID_PREFIX_2);
        fs::create_dir_all(legacy_path.parent().unwrap()).unwrap();
        fs::write(&legacy_path, to_file_string(&ticket)).unwrap();

        assert_eq!(store.read_ticket(&ticket.id).unwrap(), ticket);
        store.write_ticket(&ticket).unwrap();
        let preserved = fs::read_to_string(store.root().join(STORE_METADATA_FILE)).unwrap();
        assert_eq!(preserved, raw);
        assert!(legacy_path.is_file());
        assert!(
            !store
                .ticket_path_for_shard(&ticket.id, SHARD_ID_SUFFIX_2)
                .exists()
        );

        store.activate_current_format().unwrap();
        let migrated = store.ticket_path_for_shard(&ticket.id, SHARD_ID_SUFFIX_2);
        assert!(migrated.is_file());
        assert!(!legacy_path.exists());
        assert_eq!(store.read_ticket(&ticket.id).unwrap(), ticket);
    }

    #[test]
    fn write_then_read_round_trips_through_the_file() {
        let (_dir, store) = temp_store();
        let t = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let path = store.write_ticket(&t).unwrap();
        assert!(path.ends_with("tickets/AV/01ARZ3NDEKTSV4RRFFQ69G5FAV.md"));
        assert_eq!(store.read_ticket(&t.id).unwrap(), t);
    }

    #[test]
    fn explicit_activation_migrates_legacy_tickets_and_preserves_note_history() {
        let (_dir, store) = temp_store();
        let mut metadata = store.metadata().unwrap();
        metadata.schema_version = 1;
        metadata.shard = SHARD_ID_PREFIX_2.to_string();
        fs::write(
            store.root().join(STORE_METADATA_FILE),
            format!("{}\n", serde_json::to_string_pretty(&metadata).unwrap()),
        )
        .unwrap();

        let ids = [
            ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"),
            ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV"),
        ];
        for (index, id) in ids.iter().enumerate() {
            let legacy = format!(
                "---\nid: {id}\nslug: HS-LEGACY{index}\ntitle: Legacy {index}\ncategory: bug\n\
                 created_at: 2026-08-19T00:00:00Z\nupdated_at: 2026-08-19T00:00:00Z\nschema: 1\n---\n\n\
                 legacy body {index}\n\n## Notes\n\n<!-- note: 01ARZ3NDEKTSV4RRFFQ69G5FB{index} -->\n\
                 2026-08-19T01:00:00Z — original note {index}\n"
            );
            plant_raw(&store, id, &legacy);
        }
        let corrupt_id = ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAA");
        let corrupt_path = plant_raw(
            &store,
            &corrupt_id,
            &missing_notes_end(&corrupt_id, "HS-BROKEN"),
        );

        let mut changed = store.read_ticket(&ids[0]).unwrap();
        changed.title = "Current writer mutation".into();
        assert!(matches!(
            store.write_ticket(&changed),
            Err(StoreError::FormatActivationRequired { .. })
        ));
        store.activate_current_format().unwrap();
        store.activate_current_format().unwrap();
        store.write_ticket(&changed).unwrap();

        assert_eq!(
            store.metadata().unwrap().schema_version,
            STORE_SCHEMA_VERSION
        );
        assert_eq!(store.metadata().unwrap().shard, SHARD_ID_SUFFIX_2);
        assert!(corrupt_path.is_file());
        assert_eq!(store.list_tickets_resilient().unwrap().corrupt.len(), 1);
        for (index, id) in ids.iter().enumerate() {
            let migrated = store.ticket_path_for_shard(id, SHARD_ID_SUFFIX_2);
            let legacy = store.ticket_path_for_shard(id, SHARD_ID_PREFIX_2);
            assert!(migrated.is_file());
            assert!(!legacy.exists());
            let raw = fs::read_to_string(migrated).unwrap();
            assert!(raw.contains("schema: hotsheet/v2-bounded-notes"));
            let reparsed = store.read_ticket(id).unwrap();
            assert_eq!(reparsed.notes.len(), 1);
            assert_eq!(reparsed.notes[0].text, format!("original note {index}"));
        }
    }

    #[test]
    fn newer_store_schema_is_rejected_before_writing() {
        let (_dir, store) = temp_store();
        let mut metadata = store.metadata().unwrap();
        metadata.schema_version = STORE_SCHEMA_VERSION + 1;
        fs::write(
            store.root().join(STORE_METADATA_FILE),
            format!("{}\n", serde_json::to_string_pretty(&metadata).unwrap()),
        )
        .unwrap();

        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let error = store.write_ticket(&ticket).unwrap_err();
        assert!(matches!(
            error,
            StoreError::UpgradeRequired {
                format: "ticket store",
                ..
            }
        ));
        assert!(!store.ticket_path(&ticket.id).exists());
    }

    #[test]
    fn newer_named_store_schema_is_an_upgrade_boundary_not_corruption() {
        let (_dir, store) = temp_store();
        fs::write(
            store.root().join(STORE_METADATA_FILE),
            r#"{"schemaVersion":"hotsheet/v99-future","ticketPrefix":"HS","idStrategy":"ulid","shard":"id-prefix-2"}"#,
        ).unwrap();
        let error = store.metadata().unwrap_err().to_string();
        assert!(error.contains("newer version of Hot Sheet 2"));
        assert!(error.contains("Update Hot Sheet 2"));
    }

    #[test]
    fn write_normalizes_up_next_off_inactive_statuses() {
        let (_dir, store) = temp_store();
        let mut ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        ticket.status = hotsheet_model::Status::Archive;
        ticket.up_next = true;
        store.write_ticket(&ticket).unwrap();
        assert!(!store.read_ticket(&ticket.id).unwrap().up_next);
    }

    #[test]
    fn list_returns_all_tickets_sorted_and_ignores_non_md() {
        let (_dir, store) = temp_store();
        let a = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let b = sample(ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&b).unwrap();
        store.write_ticket(&a).unwrap();
        // A stray non-ticket file must be ignored.
        fs::write(
            store
                .ticket_path(&a.id)
                .parent()
                .unwrap()
                .join("README.txt"),
            "ignore me",
        )
        .unwrap();

        let listed = store.list_tickets().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].id, a.id, "sorted by id (k-sortable)");
        assert_eq!(listed[1].id, b.id);
    }

    /// A malformed ticket file in the exact field-failure shape: a bounded body followed
    /// by a `notes:begin` marker that is never closed by `notes:end` (parses to a hard
    /// `ParseError::TrailingContent`).
    fn missing_notes_end(id: &Ulid, slug: &str) -> String {
        format!(
            "---\nid: {id}\nslug: {slug}\ntitle: broken\ncategory: bug\n\
             created_at: 2026-08-19T00:00:00Z\nupdated_at: 2026-08-19T00:00:00Z\nschema: 1\n---\n\n\
             <!-- hotsheet:body:begin -->\nbody\n<!-- hotsheet:body:end -->\n\n\
             <!-- hotsheet:notes:begin -->\n## Notes\n\nunterminated note\n"
        )
    }

    /// Write raw bytes straight to a ticket's on-disk path (bypassing the serializer), so
    /// tests can plant a corrupt file the writer would never produce.
    fn plant_raw(store: &FsStore, id: &Ulid, content: &str) -> PathBuf {
        let path = store.ticket_path(id);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn list_tickets_resilient_returns_healthy_and_reports_the_corrupt_file() {
        let (_dir, store) = temp_store();
        let a = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let b = sample(ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&a).unwrap();
        store.write_ticket(&b).unwrap();

        // The exact field-failure shape, written straight to disk.
        let bad_id = ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let bad_path = plant_raw(&store, &bad_id, &missing_notes_end(&bad_id, "HS-BROKEN"));

        // Strict enumeration still hard-fails on the bad file (documents why the resilient
        // variant exists) — this is the bug that hid the whole store.
        assert!(matches!(
            store.list_tickets(),
            Err(StoreError::Parse { .. })
        ));

        // Resilient enumeration returns every healthy ticket AND reports exactly the one
        // corrupt file, with its path, recovered id/slug, and a non-empty error.
        let listing = store.list_tickets_resilient().unwrap();
        assert_eq!(
            listing.tickets.iter().map(|t| t.id).collect::<Vec<_>>(),
            vec![a.id, b.id],
            "all healthy tickets, sorted by id"
        );
        assert_eq!(listing.corrupt.len(), 1);
        let corrupt = &listing.corrupt[0];
        assert_eq!(corrupt.path, bad_path);
        assert_eq!(corrupt.id, Some(bad_id), "id recovered from the filename");
        assert_eq!(
            corrupt.slug.as_deref(),
            Some("HS-BROKEN"),
            "slug from frontmatter"
        );
        assert!(
            !corrupt.error.is_empty(),
            "a human-readable error is reported"
        );
    }

    #[test]
    fn list_tickets_resilient_survives_multiple_and_varied_bad_files() {
        // Adversarial: several corrupt files of different failure modes, a stray non-.md
        // file next to a corrupt one, an empty file, and truncated frontmatter. Healthy
        // tickets still enumerate; every bad .md is reported exactly once; the non-.md
        // file is ignored, not reported.
        let (_dir, store) = temp_store();
        store
            .write_ticket(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();
        store
            .write_ticket(&sample(ulid("7ZARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();

        let missing_end_id = ulid("2ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let missing_end = plant_raw(
            &store,
            &missing_end_id,
            &missing_notes_end(&missing_end_id, "HS-ONE"),
        );
        let empty_id = ulid("3ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let empty = plant_raw(&store, &empty_id, "");
        let truncated_id = ulid("4ZARZ3NDEKTSV4RRFFQ69G5FAV");
        let truncated = plant_raw(
            &store,
            &truncated_id,
            &format!("---\nid: {truncated_id}\ntitle: t\nno closing fence\n"),
        );
        // A stray non-ticket file beside a corrupt one must be ignored, not reported.
        fs::write(empty.parent().unwrap().join("notes.txt"), "just some notes").unwrap();

        let listing = store.list_tickets_resilient().unwrap();
        assert_eq!(listing.tickets.len(), 2, "both healthy tickets enumerate");

        let paths: Vec<PathBuf> = listing.corrupt.iter().map(|c| c.path.clone()).collect();
        assert_eq!(
            paths,
            {
                let mut want = vec![missing_end, empty, truncated];
                want.sort();
                want
            },
            "each bad .md reported exactly once, sorted by path; non-.md ignored"
        );
        for corrupt in &listing.corrupt {
            assert!(!corrupt.error.is_empty(), "{corrupt:?} has an error");
            assert!(
                corrupt.id.is_some(),
                "{corrupt:?} recovers its id from the filename"
            );
        }
    }

    #[test]
    fn list_tickets_resilient_is_clean_for_a_healthy_store() {
        let (_dir, store) = temp_store();
        store
            .write_ticket(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();
        let listing = store.list_tickets_resilient().unwrap();
        assert_eq!(listing.tickets.len(), 1);
        assert!(listing.corrupt.is_empty(), "no corrupt files reported");
    }

    #[test]
    fn write_attachment_stores_under_the_ticket_and_strips_paths() {
        let (_dir, store) = temp_store();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        store
            .write_ticket(&Ticket::new(id, "HS-TEST", "test", "task", "t0", "t0"))
            .unwrap();
        let attachment_id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        // A traversal-y filename is reduced to its basename.
        let (ticket, path) = store
            .write_attachment(
                &id,
                attachment_id,
                Timestamp::new("2026-08-26T00:00:00Z"),
                "../../evil/shot.png",
                b"PNGDATA",
            )
            .unwrap();
        assert!(path.ends_with(
            "attachments/01ARZ3NDEKTSV4RRFFQ69G5FAV/01ARZ3NDEKTSV4RRFFQ69G5FB0/shot.png"
        ));
        assert_eq!(fs::read(&path).unwrap(), b"PNGDATA");
        assert_eq!(ticket.attachments[0].id, attachment_id);
        assert_eq!(ticket.attachments[0].filename, "shot.png");
        assert_eq!(
            ticket.attachments[0].created_at.as_str(),
            "2026-08-26T00:00:00Z"
        );
        let renamed = store
            .rename_attachment(
                &id,
                &attachment_id,
                Timestamp::new("2026-08-26T01:00:00Z"),
                "renamed.png",
            )
            .unwrap();
        assert_eq!(renamed.attachments[0].id, attachment_id);
        assert_eq!(renamed.attachments[0].filename, "renamed.png");
        assert_eq!(
            renamed.attachments[0].created_at,
            ticket.attachments[0].created_at
        );
        assert!(path.with_file_name("renamed.png").is_file());
        let (metadata, bytes) = store.read_attachment(&id, &attachment_id).unwrap();
        assert_eq!(metadata.filename, "renamed.png");
        assert_eq!(bytes, b"PNGDATA");
        let removed = store
            .remove_attachment(&id, &attachment_id, Timestamp::new("2026-08-26T02:00:00Z"))
            .unwrap();
        assert!(removed.attachments.is_empty());
        assert!(
            !store
                .attachment_dir(&id)
                .join(attachment_id.to_string())
                .exists()
        );
    }

    #[test]
    fn attachment_mutations_commit_payload_paths_and_leave_the_store_clean() {
        let (dir, store) = temp_store();
        git(dir.path(), &["init", "-q"]).unwrap();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        store
            .write_ticket_committing(&Ticket::new(id, "HS-TEST", "test", "task", "t0", "t0"))
            .unwrap();
        let attachment_id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        store
            .write_attachment(
                &id,
                attachment_id,
                Timestamp::new("2026-08-26T00:00:00Z"),
                "proof.png",
                b"PNGDATA",
            )
            .unwrap();
        let original = format!("attachments/{id}/{attachment_id}/proof.png");
        assert!(
            git_stdout(dir.path(), &["ls-files"])
                .unwrap()
                .lines()
                .any(|path| path == original)
        );
        assert!(
            git_stdout(dir.path(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );

        store
            .rename_attachment(
                &id,
                &attachment_id,
                Timestamp::new("2026-08-26T01:00:00Z"),
                "renamed.png",
            )
            .unwrap();
        let tracked = git_stdout(dir.path(), &["ls-files"]).unwrap();
        assert!(!tracked.lines().any(|path| path == original));
        assert!(tracked.lines().any(|path| path.ends_with("/renamed.png")));
        assert!(
            git_stdout(dir.path(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );

        store
            .remove_attachment(&id, &attachment_id, Timestamp::new("2026-08-26T02:00:00Z"))
            .unwrap();
        assert!(
            !git_stdout(dir.path(), &["ls-files"])
                .unwrap()
                .lines()
                .any(|path| path.contains(&attachment_id.to_string()))
        );
        assert!(
            git_stdout(dir.path(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn attachment_filenames_are_unique_within_a_ticket() {
        let (_dir, store) = temp_store();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        store
            .write_ticket(&Ticket::new(id, "HS-TEST", "test", "task", "t0", "t0"))
            .unwrap();
        let first = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        let second = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1");
        let third = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB2");

        let (_, first_path) = store
            .write_attachment(
                &id,
                first,
                Timestamp::new("2026-08-26T00:00:00Z"),
                "proof.png",
                b"first",
            )
            .unwrap();
        let (_, second_path) = store
            .write_attachment(
                &id,
                second,
                Timestamp::new("2026-08-26T00:01:00Z"),
                "proof.png",
                b"second",
            )
            .unwrap();
        let (retried, retried_path) = store
            .write_attachment(
                &id,
                second,
                Timestamp::new("2026-08-26T00:01:00Z"),
                "proof.png",
                b"second",
            )
            .unwrap();
        assert_eq!(retried.attachments[1].filename, "proof (2).png");
        assert_eq!(retried_path, second_path);
        let (ticket, third_path) = store
            .write_attachment(
                &id,
                third,
                Timestamp::new("2026-08-26T00:02:00Z"),
                "PROOF.png",
                b"third",
            )
            .unwrap();

        assert_eq!(first_path.file_name().unwrap(), "proof.png");
        assert_eq!(second_path.file_name().unwrap(), "proof (2).png");
        assert_eq!(third_path.file_name().unwrap(), "PROOF (3).png");
        assert_eq!(
            ticket
                .attachments
                .iter()
                .map(|attachment| attachment.filename.as_str())
                .collect::<Vec<_>>(),
            ["proof.png", "proof (2).png", "PROOF (3).png"]
        );
        assert_eq!(fs::read(second_path).unwrap(), b"second");

        let renamed = store
            .rename_attachment(
                &id,
                &third,
                Timestamp::new("2026-08-26T00:03:00Z"),
                "proof.png",
            )
            .unwrap();
        assert_eq!(renamed.attachments[2].filename, "proof (3).png");
    }

    #[test]
    fn attachment_batch_metadata_persists_and_can_merge_or_split() {
        let (_dir, store) = temp_store();
        let id = ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV");
        store
            .write_ticket(&Ticket::new(id, "HS-TEST", "test", "task", "t0", "t0"))
            .unwrap();
        let first = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0");
        let second = ulid("01ARZ3NDEKTSV4RRFFQ69G5FB1");
        let metadata = hotsheet_model::AttachmentMetadata {
            batch_id: Some("gesture-1".into()),
            batch_label: Some("Initial report".into()),
            actor: Some(hotsheet_model::AttachmentActor {
                identity: Some("person@example.com".into()),
                display_name: Some("Person".into()),
                role: hotsheet_model::AttachmentActorRole::Human,
            }),
            purpose: Some(hotsheet_model::AttachmentPurpose::ProblemEvidence),
        };
        for (attachment, filename) in [(first, "one.png"), (second, "two.png")] {
            store
                .write_attachment_with_metadata(
                    &id,
                    attachment,
                    Timestamp::new("2026-08-26T00:00:00Z"),
                    filename,
                    b"image",
                    metadata.clone(),
                )
                .unwrap();
        }
        let reread = store.read_ticket(&id).unwrap();
        assert!(
            reread
                .attachments
                .iter()
                .all(|attachment| attachment.batch_id.as_deref() == Some("gesture-1"))
        );

        let split = hotsheet_model::AttachmentMetadata {
            batch_id: Some("gesture-2".into()),
            batch_label: None,
            purpose: Some(hotsheet_model::AttachmentPurpose::CorrectnessEvidence),
            ..metadata
        };
        let changed = store
            .set_attachment_metadata(
                &id,
                &[second],
                split,
                Timestamp::new("2026-08-26T00:01:00Z"),
            )
            .unwrap();
        assert_eq!(
            changed.attachments[0].batch_id.as_deref(),
            Some("gesture-1")
        );
        assert_eq!(
            changed.attachments[1].batch_id.as_deref(),
            Some("gesture-2")
        );
        assert_eq!(
            changed.attachments[1].purpose,
            Some(hotsheet_model::AttachmentPurpose::CorrectnessEvidence)
        );
    }

    #[test]
    fn legacy_attachment_gets_deterministic_metadata_without_using_mtime() {
        let (_dir, store) = temp_store();
        let ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket(&ticket).unwrap();
        let dir = store.attachment_dir(&ticket.id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("legacy.txt"), b"old").unwrap();

        let first = store.read_ticket(&ticket.id).unwrap();
        let second = store.read_ticket(&ticket.id).unwrap();
        assert_eq!(first.attachments, second.attachments);
        assert_eq!(first.attachments[0].filename, "legacy.txt");
        assert_eq!(first.attachments[0].created_at, ticket.created_at);
        assert_eq!(
            first.attachments[0].id,
            FsStore::legacy_attachment_id(&ticket.id, "legacy.txt")
        );
    }

    #[test]
    fn finder_metadata_is_ignored_as_attachment_and_removed_from_git_tracking() {
        let (dir, store) = temp_store();
        let mut ticket = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        ticket.attachments.push(Attachment {
            id: ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0"),
            filename: FINDER_METADATA_FILE.into(),
            created_at: ticket.created_at.clone(),
            batch_id: None,
            batch_label: None,
            actor: None,
            purpose: None,
            annotations: Vec::new(),
            crop: None,
        });
        store.write_ticket(&ticket).unwrap();
        let attachment_dir = store.attachment_dir(&ticket.id);
        fs::create_dir_all(&attachment_dir).unwrap();
        fs::write(attachment_dir.join("legacy.txt"), b"evidence").unwrap();
        fs::write(attachment_dir.join(FINDER_METADATA_FILE), b"finder").unwrap();

        let loaded = store.read_ticket(&ticket.id).unwrap();
        assert_eq!(
            loaded
                .attachments
                .iter()
                .map(|attachment| attachment.filename.as_str())
                .collect::<Vec<_>>(),
            vec!["legacy.txt"]
        );
        fs::write(dir.path().join(".gitignore"), "worklist.md\n").unwrap();
        let store = FsStore::open(dir.path()).unwrap();
        let ignore = fs::read_to_string(dir.path().join(".gitignore")).unwrap();
        assert!(ignore.lines().any(|line| line == "worklist.md"));
        assert!(ignore.lines().any(|line| line == FINDER_METADATA_FILE));
        assert!(ignore.lines().any(|line| line == NOTE_MUTATION_LOCK_FILE));
        store
            .with_ticket_transaction(|| Ok::<(), StoreError>(()))
            .unwrap();

        git(dir.path(), &["init", "-q"]).unwrap();
        git(dir.path(), &["add", "-A"]).unwrap();
        git(
            dir.path(),
            &[
                "add",
                "-f",
                attachment_dir.join(FINDER_METADATA_FILE).to_str().unwrap(),
            ],
        )
        .unwrap();
        git(
            dir.path(),
            &[
                "-c",
                "user.name=Hot Sheet Test",
                "-c",
                "user.email=test@localhost",
                "commit",
                "-q",
                "-m",
                "legacy metadata",
            ],
        )
        .unwrap();

        store.write_ticket_committing(&loaded).unwrap();
        let tracked = git_stdout(dir.path(), &["ls-files"]).unwrap();
        assert!(
            !tracked
                .lines()
                .any(|path| path.ends_with(FINDER_METADATA_FILE))
        );
        assert!(!tracked.lines().any(|path| path == NOTE_MUTATION_LOCK_FILE));
        assert!(attachment_dir.join(FINDER_METADATA_FILE).is_file());
    }

    #[test]
    fn open_on_a_bare_directory_reports_not_a_store() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            FsStore::open(dir.path()),
            Err(StoreError::NotAStore(_))
        ));
    }

    #[test]
    fn autocommit_is_a_noop_without_a_git_repo() {
        let (_dir, store) = temp_store();
        // A store that isn't a git repo (the common test/temp case) never fails and
        // never commits — so ops in a bare dir just work.
        assert!(!store.autocommit("nope").unwrap());
        assert!(
            store
                .write_ticket_committing(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
                .is_ok()
        );
    }

    #[test]
    fn deferred_autocommit_writes_tickets_then_commits_touched_paths_once() {
        let (dir, store) = temp_store();
        git(dir.path(), &["init", "-q"]).unwrap();
        assert!(store.autocommit("initial store").unwrap());
        let original_head = git_stdout(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        let deferred = store.clone().with_deferred_autocommit();
        let first = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        let second = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAW"));
        deferred.write_ticket_committing(&first).unwrap();
        deferred.write_ticket_committing(&second).unwrap();
        assert_eq!(
            git_stdout(dir.path(), &["rev-parse", "HEAD"]).unwrap(),
            original_head
        );
        assert!(
            store
                .autocommit_paths(
                    "batch",
                    &[store.ticket_path(&first.id), store.ticket_path(&second.id)],
                )
                .unwrap()
        );
        assert_eq!(
            git_stdout(
                dir.path(),
                &[
                    "rev-list",
                    "--count",
                    &format!("{}..HEAD", original_head.trim())
                ]
            )
            .unwrap()
            .trim(),
            "1"
        );
        assert!(
            git_stdout(dir.path(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn autocommit_lock_serializes_aliases_and_releases_for_the_next_writer() {
        use std::sync::mpsc::{RecvTimeoutError, sync_channel};

        let (dir, store) = temp_store();
        git(dir.path(), &["init", "-q"]).unwrap();
        assert!(store.autocommit("initial store").unwrap());
        let aliases = tempfile::tempdir().unwrap();
        let alias = aliases.path().join("same-store");
        std::os::unix::fs::symlink(dir.path(), &alias).unwrap();
        let other = FsStore::open(&alias).unwrap();
        fs::write(dir.path().join("extra.txt"), "one more path\n").unwrap();

        let held = store.lock_autocommit().unwrap();
        let (started_sender, started_receiver) = sync_channel(1);
        let (sender, receiver) = sync_channel(1);
        let worker = std::thread::spawn(move || {
            started_sender.send(()).unwrap();
            sender
                .send(other.autocommit_paths("extra path", &[alias.join("extra.txt")]))
                .unwrap();
        });
        started_receiver
            .recv_timeout(Duration::from_secs(5))
            .unwrap();
        assert!(matches!(
            receiver.recv_timeout(Duration::from_millis(100)),
            Err(RecvTimeoutError::Timeout)
        ));
        assert!(dir.path().join(".git/hotsheet-autocommit.lock").is_file());
        drop(held);
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap()
        );
        worker.join().unwrap();
        assert!(git_ok(dir.path(), &["diff", "--quiet"]));
        assert!(git_ok(dir.path(), &["diff", "--cached", "--quiet"]));
        assert!(
            git_stdout(dir.path(), &["status", "--porcelain"])
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn autocommit_replays_lock_collisions_until_success_or_another_writers_noop() {
        let failures = [
            "Unable to create 'index.lock': File exists",
            "cannot lock ref 'HEAD': is at a but expected b",
            "Unable to create 'index.lock': File exists",
        ];
        let mut calls = 0;
        let mut waits = Vec::new();
        let committed = retry_autocommit(
            || {
                calls += 1;
                failures
                    .get(calls - 1)
                    .map_or(Ok(false), |message| Err(StoreError::Git((*message).into())))
            },
            |delay| waits.push(delay.as_millis()),
            20,
        )
        .unwrap();
        assert!(!committed);
        assert_eq!(calls, 4);
        assert_eq!(waits, [25, 50, 100]);
        assert!(retry_autocommit(|| Ok(true), |_| panic!("no wait needed"), 20).unwrap());
    }

    #[test]
    fn autocommit_bounds_lock_retries_and_does_not_retry_other_errors() {
        let mut calls = 0;
        let mut waited = Duration::ZERO;
        let error = retry_autocommit(
            || {
                calls += 1;
                Err(StoreError::Git("index.lock: File exists".into()))
            },
            |delay| waited += delay,
            20,
        )
        .unwrap_err();
        assert!(error.to_string().contains("index.lock"));
        assert_eq!(calls, 21);
        assert_eq!(waited, Duration::from_millis(4375));
        for message in ["pre-commit hook rejected", "index.lock: Permission denied"] {
            let result = retry_autocommit(
                || Err(StoreError::Git(message.into())),
                |_| panic!("non-contention errors must not be retried"),
                20,
            );
            assert!(result.unwrap_err().to_string().contains(message));
        }
        let result = retry_autocommit(
            || Err(StoreError::Io(std::io::Error::other("spawn failure"))),
            |_| panic!("process launch errors must not be retried"),
            20,
        );
        assert!(matches!(result, Err(StoreError::Io(_))));
    }

    #[test]
    fn committing_write_commits_once_and_leaves_a_clean_tree() {
        let (dir, store) = temp_store();
        // Make it a git repo, as `hotsheet init` does. No user config → autocommit's
        // bot-identity fallback still lets commits land (mirrors a fresh CI checkout).
        git(dir.path(), &["init", "-q"]).unwrap();

        let t = sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        store.write_ticket_committing(&t).unwrap();

        // Working tree is clean (nothing left uncommitted) after the mutation.
        assert!(
            git_ok(dir.path(), &["diff", "--quiet"])
                && git_ok(dir.path(), &["diff", "--cached", "--quiet"]),
            "the store should be clean after a committing write"
        );
        let count = git_stdout(dir.path(), &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(count.trim(), "1", "one commit for the mutation");
        let tracked = git_stdout(dir.path(), &["ls-files"]).unwrap();
        assert!(tracked.lines().any(|path| path == STORE_METADATA_FILE));
        assert!(tracked.lines().any(|path| path == ".gitignore"));

        // Re-writing identical content stages nothing → no empty commit.
        store.write_ticket_committing(&t).unwrap();
        let count = git_stdout(dir.path(), &["rev-list", "--count", "HEAD"]).unwrap();
        assert_eq!(
            count.trim(),
            "1",
            "an unchanged re-write must not add a commit"
        );
    }

    #[test]
    fn committing_write_leaves_unrelated_staged_files_out_of_the_commit() {
        let (dir, store) = temp_store();
        git(dir.path(), &["init", "-q"]).unwrap();
        store
            .write_ticket_committing(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")))
            .unwrap();

        fs::write(dir.path().join("unrelated.txt"), "user work").unwrap();
        git(dir.path(), &["add", "unrelated.txt"]).unwrap();
        store
            .write_ticket_committing(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FB0")))
            .unwrap();

        let staged = git_stdout(dir.path(), &["diff", "--cached", "--name-only"]).unwrap();
        assert_eq!(staged.trim(), "unrelated.txt");
        let committed =
            git_stdout(dir.path(), &["show", "--pretty=", "--name-only", "HEAD"]).unwrap();
        assert!(!committed.lines().any(|path| path == "unrelated.txt"));
    }

    #[cfg(unix)]
    #[test]
    fn committing_write_does_not_wait_for_a_slow_remote_push() {
        use std::io::Read;
        use std::os::unix::fs::PermissionsExt;
        use std::sync::{Arc, Barrier, mpsc};
        use std::time::Duration;

        let (dir, store) = temp_store();
        let remote = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q"]).unwrap();
        git(dir.path(), &["config", "user.name", "Hot Sheet Test"]).unwrap();
        git(dir.path(), &["config", "user.email", "test@localhost"]).unwrap();
        git(dir.path(), &["add", "-A"]).unwrap();
        git(dir.path(), &["commit", "-q", "-m", "initial"]).unwrap();
        git(remote.path(), &["init", "--bare", "-q"]).unwrap();
        git(
            dir.path(),
            &["remote", "add", "origin", remote.path().to_str().unwrap()],
        )
        .unwrap();
        git(dir.path(), &["push", "-q", "-u", "origin", "HEAD"]).unwrap();

        let push_started = remote.path().join("push-started");
        let allow_push = remote.path().join("allow-push");
        fifo(&push_started);
        let hook = remote.path().join("hooks/pre-receive");
        fs::write(
            &hook,
            r#"#!/bin/sh
marker_dir="$(dirname "$0")/.."
printf 'started\n' > "$marker_dir/push-started"
attempt=0
while [ ! -f "$marker_dir/allow-push" ]; do
    attempt=$((attempt + 1))
    [ "$attempt" -lt 3000 ] || exit 1
    sleep 0.01
done
"#,
        )
        .unwrap();
        let mut permissions = fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook, permissions).unwrap();

        let spawned = Arc::new(Barrier::new(2));
        let finished = Arc::new(Barrier::new(2));
        let store = store.with_background_push_observer(BackgroundPushObserver {
            spawned: spawned.clone(),
            finished: finished.clone(),
        });
        let _saturation = HostSaturation::start(dir.path().join("saturation-io"));
        let (write_result_tx, write_result_rx) = mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let result = store.write_ticket_committing(&sample(ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV")));
            write_result_tx.send(result).unwrap();
        });

        spawned.wait();
        let mut hook_signal = String::new();
        fs::File::open(&push_started)
            .unwrap()
            .read_to_string(&mut hook_signal)
            .unwrap();
        let write_result = write_result_rx.recv_timeout(Duration::from_secs(5));
        // Always unblock the git child, including on an assertion failure below.
        fs::write(&allow_push, b"").unwrap();
        assert_eq!(hook_signal, "started\n");
        write_result
            .expect("a local mutation waited for the blocked remote publication")
            .unwrap();

        finished.wait();
        let local_head = git_stdout(dir.path(), &["rev-parse", "HEAD"]).unwrap();
        let branch = git_stdout(dir.path(), &["symbolic-ref", "--short", "HEAD"]).unwrap();
        let remote_ref = format!("refs/heads/{}", branch.trim());
        let remote_head = git_stdout(remote.path(), &["rev-parse", &remote_ref]).unwrap();
        assert_eq!(remote_head.trim(), local_head.trim());
    }
}
