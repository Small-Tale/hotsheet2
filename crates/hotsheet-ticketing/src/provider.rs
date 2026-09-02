//! Provider-neutral ticket contract (`docs/16`).
//!
//! Provider-native ids stay strings at this boundary. The built-in git provider is
//! the only adapter that translates them to ULIDs and delegates to [`crate::ops`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use hotsheet_model::{CloseReason, NoteKind, Priority, ReviewRequest, Status, Timestamp, Ulid};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ops::{self, NewTicket, TicketPatch, TicketQuery};
use crate::wire::{ApiAttachment, ApiTicket};
use crate::{FsStore, OpError, StoreError};

/// A ticket's durable identity. Native ids are meaningful only within a connection.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TicketRef {
    pub connection_id: String,
    pub native_id: String,
}

impl TicketRef {
    pub fn qualified(&self) -> String {
        format!("{}:{}", self.connection_id, self.native_id)
    }
}

/// Stable connection id for a git store, shared by CLI/MCP/server surfaces.
pub fn git_connection_id(store: &FsStore) -> String {
    let root = store
        .root()
        .canonicalize()
        .unwrap_or_else(|_| store.root().to_path_buf());
    let mut hash = Sha256::new();
    hash.update(root.to_string_lossy().as_bytes());
    format!("{:x}", hash.finalize())[..16].to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderDescriptor {
    pub connection_id: String,
    pub provider: String,
    pub display_name: String,
    pub locator: String,
    pub default: bool,
    pub capabilities: ProviderCapabilities,
}

/// Durable, non-secret project configuration for one provider connection.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProviderConnection {
    pub id: String,
    pub provider: String,
    pub locator: String,
    pub name: Option<String>,
    #[serde(default)]
    pub default: bool,
    #[serde(default)]
    pub settings: serde_json::Value,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct ProviderConnectionsFile {
    #[serde(default)]
    connections: Vec<ProviderConnection>,
}

/// File-backed project connection registry. It stores locators/settings only; secret
/// values remain in [`crate::KeyRegistry`] and settings carry references.
#[derive(Debug, Clone)]
pub struct ProviderConfigRegistry {
    path: PathBuf,
}

impl ProviderConfigRegistry {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn load(&self) -> Result<Vec<ProviderConnection>, ProviderError> {
        let text = match std::fs::read_to_string(&self.path) {
            Ok(text) => text,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(StoreError::Io(e).into()),
        };
        let file: ProviderConnectionsFile =
            serde_json::from_str(&text).map_err(|e| ProviderError::Conflict {
                ticket: self.path.display().to_string(),
                message: format!("invalid provider configuration: {e}"),
            })?;
        validate_connections(&file.connections)?;
        Ok(file.connections)
    }

    pub fn save(&self, connections: &[ProviderConnection]) -> Result<(), ProviderError> {
        validate_connections(connections)?;
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent).map_err(StoreError::Io)?;
        }
        let text = serde_json::to_string_pretty(&ProviderConnectionsFile {
            connections: connections.to_vec(),
        })
        .expect("provider connection config is serializable");
        std::fs::write(&self.path, format!("{text}\n")).map_err(StoreError::Io)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

fn validate_connections(connections: &[ProviderConnection]) -> Result<(), ProviderError> {
    let mut ids = std::collections::HashSet::new();
    let mut defaults = 0;
    for connection in connections {
        if connection.id.is_empty()
            || !connection
                .id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message: "connection id must contain only letters, digits, '.', '_' or '-'".into(),
            });
        }
        if !ids.insert(&connection.id) {
            return Err(ProviderError::Conflict {
                ticket: connection.id.clone(),
                message: "duplicate provider connection id".into(),
            });
        }
        defaults += usize::from(connection.default);
    }
    if defaults > 1 {
        return Err(ProviderError::Conflict {
            ticket: "providers".into(),
            message: "at most one provider connection may be the default".into(),
        });
    }
    Ok(())
}

/// Structured feature discovery. A client must not infer support from provider id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    pub create: bool,
    pub update: bool,
    pub close: bool,
    pub notes: bool,
    pub note_edit: bool,
    pub note_delete: bool,
    pub attachments: bool,
    pub assignment: bool,
    pub review_requests: bool,
    pub dependencies: bool,
    pub up_next: bool,
    pub close_reasons: bool,
    pub claims: bool,
    pub atomic_batch: bool,
    /// One all-or-nothing note/evidence/reopen operation.
    #[serde(default)]
    pub not_working_report: bool,
    pub offline_mutation: bool,
    pub history: bool,
    pub watch: bool,
    pub provider_idempotency: bool,
    pub query_fields: Vec<String>,
}

impl ProviderCapabilities {
    pub fn git() -> Self {
        Self {
            create: true,
            update: true,
            close: true,
            notes: true,
            note_edit: true,
            note_delete: true,
            attachments: true,
            assignment: true,
            review_requests: true,
            dependencies: true,
            up_next: true,
            close_reasons: true,
            claims: true,
            atomic_batch: true,
            not_working_report: true,
            offline_mutation: true,
            history: true,
            watch: true,
            provider_idempotency: false,
            query_fields: vec![
                "status",
                "priority",
                "category",
                "tags",
                "text",
                "up_next",
                "close_reason",
                "assignee",
                "review",
                "claimed",
                "blocked",
                "created_at",
                "updated_at",
            ]
            .into_iter()
            .map(str::to_string)
            .collect(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderDraft {
    pub title: String,
    pub category: String,
    pub priority: Priority,
    pub details: String,
    pub tags: Vec<String>,
    pub up_next: bool,
    pub blocked_by: Vec<String>,
    pub transfer: Option<TransferProvenance>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TransferProvenance {
    pub operation_id: String,
    pub source: TicketRef,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderPatch {
    pub expected_token: Option<String>,
    pub title: Option<String>,
    pub details: Option<String>,
    pub category: Option<String>,
    pub priority: Option<Priority>,
    pub status: Option<Status>,
    pub tags: Option<Vec<String>>,
    pub up_next: Option<bool>,
    pub blocked_by: Option<Vec<String>>,
}

/// Caller-owned time/id inputs keep provider implementations deterministic in tests.
/// `generated_id` is also the provider-neutral idempotency key: adapters must return the
/// existing object when a create/note request with the same id is retried. Providers whose
/// remote API chooses ids should persist or recognize the caller id in remote provenance.
#[derive(Debug, Clone)]
pub struct MutationContext {
    pub now: Timestamp,
    pub generated_id: Ulid,
}

#[derive(Debug, Clone)]
pub struct ProviderEvidence {
    pub id: Ulid,
    pub filename: String,
    pub created_at: Timestamp,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct NotWorkingReport {
    pub expected_token: Option<String>,
    pub note: Option<(Ulid, String)>,
    pub evidence: Vec<ProviderEvidence>,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("ticket provider connection '{0}' was not found")]
    UnknownConnection(String),
    #[error("ticket '{native_id}' was not found in provider connection '{connection_id}'")]
    NotFound {
        connection_id: String,
        native_id: String,
    },
    #[error("provider connection '{connection_id}' does not support '{capability}'")]
    Unsupported {
        connection_id: String,
        capability: &'static str,
    },
    #[error("provider authentication failed for '{connection_id}': {message}")]
    Authentication {
        connection_id: String,
        message: String,
    },
    #[error("provider conflict for '{ticket}': {message}")]
    Conflict { ticket: String, message: String },
    #[error("provider '{connection_id}' is rate limited{retry}", retry = retry_after_seconds.map(|n| format!("; retry after {n}s")).unwrap_or_default())]
    RateLimited {
        connection_id: String,
        retry_after_seconds: Option<u64>,
    },
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Operation(#[from] OpError),
    #[error("invalid native id '{0}' for the git provider")]
    InvalidNativeId(String),
}

/// Synchronous domain boundary; async network adapters are wrapped at the host edge.
pub trait TicketProvider: Send + Sync {
    fn descriptor(&self) -> ProviderDescriptor;
    fn supports_note_edit(&self) -> bool {
        self.descriptor().capabilities.note_edit
    }
    fn supports_note_delete(&self) -> bool {
        self.descriptor().capabilities.note_delete
    }
    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError>;
    fn find_transfer(&self, operation_id: &str) -> Result<Option<ApiTicket>, ProviderError>;
    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError>;
    fn create(
        &self,
        ctx: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError>;
    fn update(
        &self,
        native_id: &str,
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError>;
    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError>;
    fn report_not_working(
        &self,
        _native_id: &str,
        _now: Timestamp,
        _report: NotWorkingReport,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "not_working_report",
        })
    }
    fn edit_note(
        &self,
        _native_id: &str,
        _note_id: &str,
        _now: Timestamp,
        _text: String,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "note_edit",
        })
    }
    fn delete_note(
        &self,
        _native_id: &str,
        _note_id: &str,
        _now: Timestamp,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "note_delete",
        })
    }
    fn attachment_bytes(
        &self,
        _native_id: &str,
        _attachment_id: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "attachments",
        })
    }
    fn add_attachment(
        &self,
        _native_id: &str,
        _attachment: ApiAttachment,
        _bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        Err(ProviderError::Unsupported {
            connection_id: self.descriptor().connection_id,
            capability: "attachments",
        })
    }
    fn close(
        &self,
        native_id: &str,
        now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError>;
    fn assign(
        &self,
        native_id: &str,
        now: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError>;
    fn claim_next(
        &self,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
        label: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError>;
    fn release(
        &self,
        native_id: &str,
        now: Timestamp,
        worker: &str,
        force: bool,
    ) -> Result<ApiTicket, ProviderError>;
    fn renew(
        &self,
        native_id: &str,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
    ) -> Result<ApiTicket, ProviderError>;
}

#[derive(Debug, Clone)]
pub struct GitProvider {
    connection_id: String,
    display_name: String,
    store: FsStore,
    is_default: bool,
    transfer_lock: Arc<Mutex<()>>,
    #[cfg(test)]
    fail_close_once: Arc<std::sync::atomic::AtomicBool>,
    #[cfg(test)]
    test_capabilities: Option<ProviderCapabilities>,
}

impl GitProvider {
    pub fn new(connection_id: impl Into<String>, store: FsStore) -> Self {
        Self {
            connection_id: connection_id.into(),
            display_name: "Git tickets".into(),
            store,
            is_default: false,
            transfer_lock: Arc::new(Mutex::new(())),
            #[cfg(test)]
            fail_close_once: Arc::new(std::sync::atomic::AtomicBool::new(false)),
            #[cfg(test)]
            test_capabilities: None,
        }
    }

    pub fn with_default(mut self, is_default: bool) -> Self {
        self.is_default = is_default;
        self
    }

    pub fn store(&self) -> &FsStore {
        &self.store
    }

    #[cfg(test)]
    fn with_close_failure_once(self) -> Self {
        self.fail_close_once
            .store(true, std::sync::atomic::Ordering::SeqCst);
        self
    }

    #[cfg(test)]
    fn with_test_capabilities(mut self, capabilities: ProviderCapabilities) -> Self {
        self.test_capabilities = Some(capabilities);
        self
    }

    fn ticket(&self, native_id: &str) -> Result<hotsheet_model::Ticket, ProviderError> {
        ops::resolve(&self.store, native_id)?.ok_or_else(|| ProviderError::NotFound {
            connection_id: self.connection_id.clone(),
            native_id: native_id.to_string(),
        })
    }

    fn blockers(&self, values: &[String]) -> Result<Vec<Ulid>, ProviderError> {
        Ok(ops::resolve_blockers(&self.store, None, values)?)
    }

    fn capabilities(&self) -> ProviderCapabilities {
        #[cfg(test)]
        if let Some(capabilities) = &self.test_capabilities {
            return capabilities.clone();
        }
        ProviderCapabilities::git()
    }
}

impl TicketProvider for GitProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            connection_id: self.connection_id.clone(),
            provider: "git".into(),
            display_name: self.display_name.clone(),
            locator: self.store.root().display().to_string(),
            default: self.is_default,
            capabilities: self.capabilities(),
        }
    }

    fn supports_note_edit(&self) -> bool {
        true
    }

    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        Ok(ops::query(&self.store, query)?
            .iter()
            .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None))
            .collect())
    }

    fn find_transfer(&self, operation_id: &str) -> Result<Option<ApiTicket>, ProviderError> {
        Ok(self
            .store
            .list_tickets()?
            .into_iter()
            .find(|ticket| ticket.transfer_operation_id.as_deref() == Some(operation_id))
            .as_ref()
            .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None)))
    }

    fn get(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn create(
        &self,
        ctx: MutationContext,
        draft: ProviderDraft,
    ) -> Result<ApiTicket, ProviderError> {
        let prefix = self.store.metadata()?.ticket_prefix;
        let blocked_by = self.blockers(&draft.blocked_by)?;
        let _transfer_guard = if draft.transfer.is_some() {
            Some(
                self.transfer_lock
                    .lock()
                    .map_err(|_| ProviderError::Conflict {
                        ticket: self.connection_id.clone(),
                        message: "transfer lock poisoned".into(),
                    })?,
            )
        } else {
            None
        };
        if let Some(transfer) = &draft.transfer
            && let Some(existing) = self.find_transfer(&transfer.operation_id)?
        {
            if existing.transferred_from.as_deref() == Some(&transfer.source.qualified()) {
                return Ok(existing);
            }
            return Err(ProviderError::Conflict {
                ticket: transfer.operation_id.clone(),
                message: "transfer operation id is already associated with another source".into(),
            });
        }
        let mut ticket = ops::create(
            &self.store,
            ctx.generated_id,
            &prefix,
            ctx.now,
            NewTicket {
                title: draft.title,
                category: draft.category,
                priority: draft.priority,
                details: draft.details,
                tags: draft.tags,
                up_next: draft.up_next,
                blocked_by,
            },
        )?;
        if let Some(transfer) = draft.transfer {
            ticket.transfer_operation_id = Some(transfer.operation_id);
            ticket.transferred_from = Some(transfer.source.qualified());
            self.store.write_ticket_committing(&ticket)?;
        }
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn update(
        &self,
        native_id: &str,
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        if patch
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "ticket changed since it was read".into(),
            });
        }
        let blocked_by = patch
            .blocked_by
            .as_deref()
            .map(|v| self.blockers(v))
            .transpose()?;
        let updated = ops::update(
            &self.store,
            &ticket.id,
            now,
            TicketPatch {
                title: patch.title,
                details: patch.details,
                category: patch.category,
                priority: patch.priority,
                status: patch.status,
                tags: patch.tags,
                up_next: patch.up_next,
                blocked_by,
            },
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn add_note(
        &self,
        native_id: &str,
        ctx: MutationContext,
        kind: NoteKind,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        if ticket.notes.iter().any(|note| note.id == ctx.generated_id) {
            return Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None));
        }
        let updated = ops::add_note(
            &self.store,
            &ticket.id,
            ctx.generated_id,
            ctx.now,
            kind,
            text,
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn report_not_working(
        &self,
        native_id: &str,
        now: Timestamp,
        report: NotWorkingReport,
    ) -> Result<ApiTicket, ProviderError> {
        if !self.capabilities().not_working_report {
            return Err(ProviderError::Unsupported {
                connection_id: self.connection_id.clone(),
                capability: "not_working_report",
            });
        }
        let mut ticket = self.ticket(native_id)?;
        if report
            .expected_token
            .as_deref()
            .is_some_and(|token| token != ticket.updated_at.as_str())
        {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "ticket changed since it was read".into(),
            });
        }
        if report.evidence.iter().any(|item| {
            ticket
                .attachments
                .iter()
                .any(|current| current.id == item.id)
        }) {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "evidence attachment id already exists".into(),
            });
        }
        ops::prepare_not_working(&mut ticket, now, report.note, !report.evidence.is_empty())?;
        let evidence = report
            .evidence
            .into_iter()
            .map(|item| crate::store::AtomicAttachment {
                id: item.id,
                filename: item.filename,
                created_at: item.created_at,
                bytes: item.bytes,
            })
            .collect::<Vec<_>>();
        for item in &evidence {
            ticket.attachments.push(hotsheet_model::Attachment {
                id: item.id,
                filename: item.sanitized_filename(),
                created_at: item.created_at.clone(),
            });
        }
        ticket.attachments.sort_by(|a, b| {
            a.created_at
                .chronological_cmp(&b.created_at)
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(a.id.cmp(&b.id))
        });
        self.store
            .write_ticket_with_attachments_atomic(&ticket, &evidence)?;
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn edit_note(
        &self,
        native_id: &str,
        note_id: &str,
        now: Timestamp,
        text: String,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let note_id = Ulid::from_string(note_id)
            .map_err(|_| ProviderError::InvalidNativeId(note_id.into()))?;
        let updated = ops::edit_note(&self.store, &ticket.id, &note_id, now, text)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn delete_note(
        &self,
        native_id: &str,
        note_id: &str,
        now: Timestamp,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let note_id = Ulid::from_string(note_id)
            .map_err(|_| ProviderError::InvalidNativeId(note_id.into()))?;
        let updated = ops::delete_note(&self.store, &ticket.id, &note_id, now)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn attachment_bytes(
        &self,
        native_id: &str,
        attachment_id: &str,
    ) -> Result<Vec<u8>, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let attachment_id = Ulid::from_string(attachment_id)
            .map_err(|_| ProviderError::InvalidNativeId(attachment_id.into()))?;
        let attachment = ticket
            .attachments
            .iter()
            .find(|item| item.id == attachment_id)
            .ok_or_else(|| ProviderError::NotFound {
                connection_id: self.connection_id.clone(),
                native_id: attachment_id.to_string(),
            })?;
        Ok(std::fs::read(
            self.store
                .attachment_dir(&ticket.id)
                .join(attachment.id.to_string())
                .join(&attachment.filename),
        )
        .map_err(StoreError::Io)?)
    }

    fn add_attachment(
        &self,
        native_id: &str,
        attachment: ApiAttachment,
        bytes: Vec<u8>,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let attachment_id = Ulid::from_string(&attachment.id)
            .map_err(|_| ProviderError::InvalidNativeId(attachment.id))?;
        let (updated, _) = self.store.write_attachment(
            &ticket.id,
            attachment_id,
            Timestamp::new(attachment.created_at),
            &attachment.filename,
            &bytes,
        )?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn close(
        &self,
        native_id: &str,
        now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
        #[cfg(test)]
        if self
            .fail_close_once
            .swap(false, std::sync::atomic::Ordering::SeqCst)
        {
            return Err(ProviderError::Conflict {
                ticket: native_id.into(),
                message: "injected close failure".into(),
            });
        }
        let ticket = self.ticket(native_id)?;
        let duplicate = duplicate_of
            .as_deref()
            .map(|id| self.ticket(id).map(|t| t.id))
            .transpose()?;
        let updated = ops::close(&self.store, &ticket.id, now, reason, duplicate)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn assign(
        &self,
        native_id: &str,
        now: Timestamp,
        assignees: Option<Vec<String>>,
        reviews: Vec<ReviewRequest>,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let updated = ops::assign(&self.store, &ticket.id, now, assignees, reviews)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn claim_next(
        &self,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
        label: Option<String>,
    ) -> Result<Option<ApiTicket>, ProviderError> {
        Ok(
            ops::claim_next(&self.store, &now, lease_expires, worker, label)?
                .as_ref()
                .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None)),
        )
    }

    fn release(
        &self,
        native_id: &str,
        now: Timestamp,
        worker: &str,
        force: bool,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let updated = ops::release(&self.store, &ticket.id, now, worker, force)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }

    fn renew(
        &self,
        native_id: &str,
        now: Timestamp,
        lease_expires: Timestamp,
        worker: &str,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
        let updated = ops::renew(&self.store, &ticket.id, now, lease_expires, worker)?;
        Ok(ApiTicket::from_provider(
            &updated,
            &self.connection_id,
            None,
        ))
    }
}

/// Project-scoped provider routing and aggregation.
#[derive(Clone, Default)]
pub struct ProviderRegistry {
    providers: Arc<Mutex<HashMap<String, Arc<dyn TicketProvider>>>>,
    transfer_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl ProviderRegistry {
    pub fn register(&self, provider: Arc<dyn TicketProvider>) -> Result<(), ProviderError> {
        let id = provider.descriptor().connection_id;
        let mut providers = self.providers.lock().map_err(|_| ProviderError::Conflict {
            ticket: id.clone(),
            message: "provider registry lock poisoned".into(),
        })?;
        providers.insert(id, provider);
        Ok(())
    }

    pub fn get(&self, connection_id: &str) -> Result<Arc<dyn TicketProvider>, ProviderError> {
        self.providers
            .lock()
            .ok()
            .and_then(|p| p.get(connection_id).cloned())
            .ok_or_else(|| ProviderError::UnknownConnection(connection_id.into()))
    }

    pub fn descriptors(&self) -> Vec<ProviderDescriptor> {
        let mut values = self
            .providers
            .lock()
            .map(|p| p.values().map(|v| v.descriptor()).collect::<Vec<_>>())
            .unwrap_or_default();
        values.sort_by(|a, b| a.connection_id.cmp(&b.connection_id));
        values
    }

    pub fn query_all(&self, query: &TicketQuery) -> Vec<ProviderQueryResult> {
        self.descriptors()
            .into_iter()
            .map(|descriptor| {
                let result = self
                    .get(&descriptor.connection_id)
                    .and_then(|provider| provider.query(query));
                ProviderQueryResult { descriptor, result }
            })
            .collect()
    }

    fn transfer_lock(&self, operation_id: &str) -> Result<Arc<Mutex<()>>, ProviderError> {
        let mut locks = self
            .transfer_locks
            .lock()
            .map_err(|_| ProviderError::Conflict {
                ticket: operation_id.into(),
                message: "transfer lock registry poisoned".into(),
            })?;
        Ok(locks
            .entry(operation_id.into())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone())
    }
}

pub struct ProviderQueryResult {
    pub descriptor: ProviderDescriptor,
    pub result: Result<Vec<ApiTicket>, ProviderError>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TransferOutcome {
    pub operation_id: String,
    pub source: TicketRef,
    pub destination: TicketRef,
    pub moved: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum TransferError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error("transfer operation id must not be empty")]
    EmptyOperationId,
    #[error("cross-provider dependencies require an explicit remap")]
    DependenciesNeedMapping,
    #[error("destination provider '{connection_id}' cannot represent source field '{field}'")]
    UnsupportedField {
        connection_id: String,
        field: &'static str,
    },
    #[error("destination ticket {destination} was created, but source close failed: {message}")]
    SourceCloseFailed {
        destination: String,
        message: String,
    },
}

fn transfer_ulid(operation_id: &str, suffix: &str) -> Ulid {
    let mut hash = Sha256::new();
    hash.update(operation_id.as_bytes());
    hash.update([0]);
    hash.update(suffix.as_bytes());
    let bytes: [u8; 16] = hash.finalize()[..16].try_into().expect("sha prefix");
    Ulid::from(u128::from_be_bytes(bytes))
}

pub fn copy_between(
    registry: &ProviderRegistry,
    source: TicketRef,
    destination_connection: &str,
    operation_id: &str,
    now: Timestamp,
) -> Result<TransferOutcome, TransferError> {
    if operation_id.trim().is_empty() {
        return Err(TransferError::EmptyOperationId);
    }
    let transfer_lock = registry.transfer_lock(operation_id)?;
    let _guard = transfer_lock.lock().map_err(|_| ProviderError::Conflict {
        ticket: operation_id.into(),
        message: "transfer operation lock poisoned".into(),
    })?;
    let source_provider = registry.get(&source.connection_id)?;
    let destination = registry.get(destination_connection)?;
    let ticket = source_provider.get(&source.native_id)?;
    let capabilities = destination.descriptor().capabilities;
    let attachments = ticket.attachments.clone();
    if !ticket.blocked_by.is_empty() {
        return Err(TransferError::DependenciesNeedMapping);
    }
    for (present, supported, field) in [
        (!ticket.notes.is_empty(), capabilities.notes, "notes"),
        (
            !ticket.assignees.is_empty(),
            capabilities.assignment,
            "assignees",
        ),
        (
            !ticket.review_requests.is_empty(),
            capabilities.review_requests,
            "review_requests",
        ),
    ] {
        if present && !supported {
            return Err(TransferError::UnsupportedField {
                connection_id: destination_connection.into(),
                field,
            });
        }
    }
    if ticket
        .notes
        .iter()
        .any(|note| note.edited_at != note.created_at)
        && !destination.supports_note_edit()
    {
        return Err(TransferError::UnsupportedField {
            connection_id: destination_connection.into(),
            field: "edited notes",
        });
    }
    let draft = ProviderDraft {
        title: ticket.title,
        category: ticket.category,
        priority: ticket.priority,
        details: ticket.details,
        tags: ticket.tags,
        up_next: ticket.up_next && capabilities.up_next,
        blocked_by: vec![],
        transfer: Some(TransferProvenance {
            operation_id: operation_id.into(),
            source: source.clone(),
        }),
    };
    let created = destination.create(
        MutationContext {
            now: now.clone(),
            generated_id: transfer_ulid(operation_id, destination_connection),
        },
        draft,
    )?;
    for note in ticket.notes {
        let generated_id = transfer_ulid(operation_id, &format!("note:{}", note.id));
        destination.add_note(
            &created.native_id,
            MutationContext {
                now: Timestamp::new(note.created_at.clone()),
                generated_id,
            },
            note.kind,
            note.text.clone(),
        )?;
        if note.edited_at != note.created_at {
            destination.edit_note(
                &created.native_id,
                &generated_id.to_string(),
                Timestamp::new(note.edited_at),
                note.text,
            )?;
        }
    }
    for attachment in attachments {
        let bytes = source_provider.attachment_bytes(&source.native_id, &attachment.id)?;
        destination.add_attachment(&created.native_id, attachment, bytes)?;
    }
    if !ticket.assignees.is_empty() || !ticket.review_requests.is_empty() {
        destination.assign(
            &created.native_id,
            now,
            Some(ticket.assignees),
            ticket.review_requests,
        )?;
    }
    Ok(TransferOutcome {
        operation_id: operation_id.into(),
        source,
        destination: TicketRef {
            connection_id: destination_connection.into(),
            native_id: created.native_id,
        },
        moved: false,
    })
}

pub fn move_between(
    registry: &ProviderRegistry,
    source: TicketRef,
    destination_connection: &str,
    operation_id: &str,
    now: Timestamp,
) -> Result<TransferOutcome, TransferError> {
    let mut outcome = copy_between(
        registry,
        source.clone(),
        destination_connection,
        operation_id,
        now.clone(),
    )?;
    registry
        .get(&source.connection_id)?
        .close(&source.native_id, now, CloseReason::Obsolete, None)
        .map_err(|error| TransferError::SourceCloseFailed {
            destination: outcome.destination.qualified(),
            message: error.to_string(),
        })?;
    outcome.moved = true;
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::StoreMetadata;

    fn git_provider() -> (tempfile::TempDir, GitProvider) {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
        (dir, GitProvider::new("local", store).with_default(true))
    }

    fn ctx(id: Ulid, at: &str) -> MutationContext {
        MutationContext {
            now: Timestamp::new(at),
            generated_id: id,
        }
    }

    #[test]
    fn git_provider_conforms_to_create_get_query_update_note_close() {
        let (_dir, provider) = git_provider();
        let id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAV").unwrap();
        let created = provider
            .create(
                ctx(id, "2026-08-26T00:00:00Z"),
                ProviderDraft {
                    title: "provider ticket".into(),
                    category: "task".into(),
                    priority: Priority::High,
                    details: "details".into(),
                    tags: vec!["provider".into()],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        assert_eq!(created.connection_id, "local");
        assert_eq!(created.qualified_id, format!("local:{id}"));
        assert_eq!(
            provider.get(&id.to_string()).unwrap().title,
            "provider ticket"
        );
        assert_eq!(provider.query(&TicketQuery::default()).unwrap().len(), 1);

        provider
            .update(
                &id.to_string(),
                Timestamp::new("2026-08-26T00:01:00Z"),
                ProviderPatch {
                    status: Some(Status::Started),
                    ..Default::default()
                },
            )
            .unwrap();
        provider
            .add_note(
                &id.to_string(),
                ctx(Ulid::new(), "2026-08-26T00:02:00Z"),
                NoteKind::Regular,
                "worked".into(),
            )
            .unwrap();
        let closed = provider
            .close(
                &id.to_string(),
                Timestamp::new("2026-08-26T00:03:00Z"),
                CloseReason::Completed,
                None,
            )
            .unwrap();
        assert_eq!(closed.close_reason, Some(CloseReason::Completed));
        assert_eq!(closed.notes.len(), 3);
        assert_eq!(
            closed.notes[0].text,
            "Status changed from Not Started to Started"
        );
        assert_eq!(closed.notes[1].text, "worked");
        assert_eq!(
            closed.notes[2].text,
            "Status changed from Started to Completed"
        );

        let claim_id = Ulid::new();
        provider
            .create(
                ctx(claim_id, "2026-08-26T00:04:00Z"),
                ProviderDraft {
                    title: "claimable".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    details: String::new(),
                    tags: vec![],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let claimed = provider
            .claim_next(
                Timestamp::new("2026-08-26T00:05:00Z"),
                Timestamp::new("2026-08-26T00:35:00Z"),
                "worker-a",
                None,
            )
            .unwrap()
            .unwrap();
        assert_eq!(claimed.native_id, claim_id.to_string());
        provider
            .renew(
                &claimed.native_id,
                Timestamp::new("2026-08-26T00:06:00Z"),
                Timestamp::new("2026-08-26T00:36:00Z"),
                "worker-a",
            )
            .unwrap();
        assert!(
            provider
                .release(
                    &claimed.native_id,
                    Timestamp::new("2026-08-26T00:07:00Z"),
                    "worker-a",
                    false,
                )
                .unwrap()
                .claimed_by
                .is_none()
        );
    }

    #[test]
    fn registry_aggregates_success_and_partial_provider_failure() {
        struct Failing;
        impl TicketProvider for Failing {
            fn descriptor(&self) -> ProviderDescriptor {
                ProviderDescriptor {
                    connection_id: "down".into(),
                    provider: "fake".into(),
                    display_name: "Down".into(),
                    locator: "test".into(),
                    default: false,
                    capabilities: ProviderCapabilities::git(),
                }
            }
            fn query(&self, _: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
                Err(ProviderError::Authentication {
                    connection_id: "down".into(),
                    message: "denied".into(),
                })
            }
            fn find_transfer(&self, _: &str) -> Result<Option<ApiTicket>, ProviderError> {
                unreachable!()
            }
            fn get(&self, _: &str) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn create(
                &self,
                _: MutationContext,
                _: ProviderDraft,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn update(
                &self,
                _: &str,
                _: Timestamp,
                _: ProviderPatch,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn add_note(
                &self,
                _: &str,
                _: MutationContext,
                _: NoteKind,
                _: String,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn close(
                &self,
                _: &str,
                _: Timestamp,
                _: CloseReason,
                _: Option<String>,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn assign(
                &self,
                _: &str,
                _: Timestamp,
                _: Option<Vec<String>>,
                _: Vec<ReviewRequest>,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn claim_next(
                &self,
                _: Timestamp,
                _: Timestamp,
                _: &str,
                _: Option<String>,
            ) -> Result<Option<ApiTicket>, ProviderError> {
                unreachable!()
            }
            fn release(
                &self,
                _: &str,
                _: Timestamp,
                _: &str,
                _: bool,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
            fn renew(
                &self,
                _: &str,
                _: Timestamp,
                _: Timestamp,
                _: &str,
            ) -> Result<ApiTicket, ProviderError> {
                unreachable!()
            }
        }

        let (_dir, git) = git_provider();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(git)).unwrap();
        registry.register(Arc::new(Failing)).unwrap();
        let results = registry.query_all(&TicketQuery::default());
        assert_eq!(results.len(), 2);
        assert!(
            results
                .iter()
                .any(|r| r.descriptor.connection_id == "local" && r.result.is_ok())
        );
        assert!(
            results
                .iter()
                .any(|r| r.descriptor.connection_id == "down" && r.result.is_err())
        );
    }

    #[test]
    fn config_registry_round_trips_non_secret_connections_and_rejects_ambiguity() {
        let dir = tempfile::tempdir().unwrap();
        let registry = ProviderConfigRegistry::new(dir.path().join("providers.json"));
        let connections = vec![ProviderConnection {
            id: "github-main".into(),
            provider: "github".into(),
            locator: "small-tale/hotsheet2".into(),
            name: Some("Public issues".into()),
            default: true,
            settings: serde_json::json!({"credential":{"secret":"github-small-tale"}}),
        }];
        registry.save(&connections).unwrap();
        assert_eq!(registry.load().unwrap(), connections);
        let text = std::fs::read_to_string(registry.path()).unwrap();
        assert!(!text.contains("token"));

        let mut duplicate_default = connections.clone();
        duplicate_default.push(ProviderConnection {
            id: "jira".into(),
            provider: "jira".into(),
            locator: "example.atlassian.net/ENG".into(),
            name: None,
            default: true,
            settings: serde_json::Value::Null,
        });
        assert!(registry.save(&duplicate_default).is_err());
    }

    #[test]
    fn concurrent_transfer_retries_create_one_destination_and_move_closes_source() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let destination = GitProvider::new("destination", destination_store.clone());
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T01:00:00Z"),
                ProviderDraft {
                    title: "transfer me".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    details: "body".into(),
                    tags: vec!["cross-provider".into()],
                    up_next: true,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let source_note_id = Ulid::new();
        let source_attachment_id = Ulid::new();
        source
            .store
            .write_attachment(
                &source_id,
                source_attachment_id,
                Timestamp::new("2026-08-26T01:00:05Z"),
                "provider-proof.txt",
                b"provider proof",
            )
            .unwrap();
        source
            .add_note(
                &source_id.to_string(),
                ctx(source_note_id, "2026-08-26T01:00:10Z"),
                NoteKind::Activity,
                "preserve this note".into(),
            )
            .unwrap();
        source
            .edit_note(
                &source_id.to_string(),
                &source_note_id.to_string(),
                Timestamp::new("2026-08-26T01:00:15Z"),
                "preserve this edited note".into(),
            )
            .unwrap();
        source
            .assign(
                &source_id.to_string(),
                Timestamp::new("2026-08-26T01:00:20Z"),
                Some(vec!["dev@example.com".into()]),
                vec![],
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source.clone())).unwrap();
        registry.register(Arc::new(destination)).unwrap();
        let source_ref = TicketRef {
            connection_id: "local".into(),
            native_id: source_id.to_string(),
        };
        let (a, b) = std::thread::scope(|scope| {
            let first = scope.spawn(|| {
                copy_between(
                    &registry,
                    source_ref.clone(),
                    "destination",
                    "operation-1",
                    Timestamp::new("2026-08-26T01:01:00Z"),
                )
                .unwrap()
            });
            let second = scope.spawn(|| {
                copy_between(
                    &registry,
                    source_ref.clone(),
                    "destination",
                    "operation-1",
                    Timestamp::new("2026-08-26T01:02:00Z"),
                )
                .unwrap()
            });
            (first.join().unwrap(), second.join().unwrap())
        });
        assert_eq!(a.destination, b.destination);
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);
        let copied = registry
            .get("destination")
            .unwrap()
            .get(&a.destination.native_id)
            .unwrap();
        assert_eq!(copied.notes.len(), 1);
        assert_eq!(copied.attachments.len(), 1);
        assert_eq!(copied.attachments[0].id, source_attachment_id.to_string());
        assert_eq!(copied.attachments[0].created_at, "2026-08-26T01:00:05Z");
        assert_eq!(copied.notes[0].kind, NoteKind::Activity);
        assert_eq!(copied.notes[0].text, "preserve this edited note");
        assert_eq!(copied.notes[0].created_at, "2026-08-26T01:00:10Z");
        assert_eq!(copied.notes[0].edited_at, "2026-08-26T01:00:15Z");
        assert_eq!(copied.assignees, ["dev@example.com"]);
        let moved = move_between(
            &registry,
            source_ref.clone(),
            "destination",
            "operation-1",
            Timestamp::new("2026-08-26T01:03:00Z"),
        )
        .unwrap();
        assert!(moved.moved);
        assert_eq!(
            source.get(&source_id.to_string()).unwrap().close_reason,
            Some(CloseReason::Obsolete)
        );
    }

    #[test]
    fn move_reports_created_destination_and_retry_recovers_after_source_close_failure() {
        let (_source_dir, source) = git_provider();
        let source = source.with_close_failure_once();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T02:00:00Z"),
                ProviderDraft {
                    title: "recoverable move".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source.clone())).unwrap();
        registry
            .register(Arc::new(GitProvider::new(
                "destination",
                destination_store.clone(),
            )))
            .unwrap();
        let source_ref = TicketRef {
            connection_id: "local".into(),
            native_id: source_id.to_string(),
        };
        let error = move_between(
            &registry,
            source_ref.clone(),
            "destination",
            "recover-op",
            Timestamp::new("2026-08-26T02:01:00Z"),
        )
        .unwrap_err();
        let TransferError::SourceCloseFailed { destination, .. } = error else {
            panic!("expected partial failure")
        };
        assert!(destination.starts_with("destination:"));
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);

        let recovered = move_between(
            &registry,
            source_ref,
            "destination",
            "recover-op",
            Timestamp::new("2026-08-26T02:02:00Z"),
        )
        .unwrap();
        assert_eq!(recovered.destination.qualified(), destination);
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);
        assert_eq!(
            source.get(&source_id.to_string()).unwrap().close_reason,
            Some(CloseReason::Obsolete)
        );
    }

    #[test]
    fn transfer_rejects_source_fields_the_destination_cannot_represent() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let mut capabilities = ProviderCapabilities::git();
        capabilities.notes = false;
        let destination = GitProvider::new("destination", destination_store.clone())
            .with_test_capabilities(capabilities);
        let source_id = Ulid::new();
        source
            .create(
                ctx(source_id, "2026-08-26T03:00:00Z"),
                ProviderDraft {
                    title: "has unsupported note".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        source
            .add_note(
                &source_id.to_string(),
                ctx(Ulid::new(), "2026-08-26T03:01:00Z"),
                NoteKind::Regular,
                "must not disappear".into(),
            )
            .unwrap();
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source)).unwrap();
        registry.register(Arc::new(destination)).unwrap();
        let error = copy_between(
            &registry,
            TicketRef {
                connection_id: "local".into(),
                native_id: source_id.to_string(),
            },
            "destination",
            "unsupported-op",
            Timestamp::new("2026-08-26T03:02:00Z"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TransferError::UnsupportedField { field: "notes", .. }
        ));
        assert!(destination_store.list_tickets().unwrap().is_empty());
    }

    #[test]
    fn git_provider_reports_not_working_with_note_evidence_and_reopen_in_one_write() {
        let (_dir, provider) = git_provider();
        let id = Ulid::new();
        provider
            .create(
                ctx(id, "2026-08-26T04:00:00Z"),
                ProviderDraft {
                    title: "completed work".into(),
                    category: "bug".into(),
                    priority: Priority::Default,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        provider
            .update(
                &id.to_string(),
                Timestamp::new("2026-08-26T04:01:00Z"),
                ProviderPatch {
                    status: Some(Status::Completed),
                    ..ProviderPatch::default()
                },
            )
            .unwrap();
        let evidence_id = Ulid::new();
        let result = provider
            .report_not_working(
                &id.to_string(),
                Timestamp::new("2026-08-26T04:02:00Z"),
                NotWorkingReport {
                    expected_token: Some("2026-08-26T04:01:00Z".into()),
                    note: Some((Ulid::new(), "regressed after restart".into())),
                    evidence: vec![ProviderEvidence {
                        id: evidence_id,
                        filename: "../proof.txt".into(),
                        created_at: Timestamp::new("2026-08-26T04:02:00Z"),
                        bytes: b"proof".to_vec(),
                    }],
                },
            )
            .unwrap();
        assert_eq!(result.status, Status::NotStarted);
        assert!(result.up_next);
        assert!(
            result
                .notes
                .iter()
                .any(|note| note.text == "Not working: regressed after restart")
        );
        assert!(
            result
                .notes
                .iter()
                .any(|note| note.text == "Status changed from Completed to Not Started")
        );
        assert_eq!(result.attachments[0].filename, "proof.txt");
        assert_eq!(
            provider
                .attachment_bytes(&id.to_string(), &evidence_id.to_string())
                .unwrap(),
            b"proof"
        );
    }

    #[test]
    fn not_working_validation_failure_leaves_completed_ticket_and_files_unchanged() {
        let (_dir, provider) = git_provider();
        let id = Ulid::new();
        provider
            .create(
                ctx(id, "2026-08-26T05:00:00Z"),
                ProviderDraft {
                    title: "still in progress".into(),
                    category: "bug".into(),
                    priority: Priority::Default,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        let error = provider
            .report_not_working(
                &id.to_string(),
                Timestamp::new("2026-08-26T05:01:00Z"),
                NotWorkingReport {
                    expected_token: None,
                    note: Some((Ulid::new(), "not done".into())),
                    evidence: vec![],
                },
            )
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("only be reported for a completed ticket")
        );
        let unchanged = provider.get(&id.to_string()).unwrap();
        assert_eq!(unchanged.status, Status::NotStarted);
        assert!(unchanged.notes.is_empty());
        assert!(unchanged.attachments.is_empty());
    }

    #[test]
    fn not_working_publish_failure_leaves_completed_ticket_and_prior_files_unchanged() {
        let (_dir, provider) = git_provider();
        let id = Ulid::new();
        provider
            .create(
                ctx(id, "2026-08-26T06:00:00Z"),
                ProviderDraft {
                    title: "completed".into(),
                    category: "bug".into(),
                    priority: Priority::Default,
                    details: String::new(),
                    tags: vec![],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap();
        provider
            .update(
                &id.to_string(),
                Timestamp::new("2026-08-26T06:01:00Z"),
                ProviderPatch {
                    status: Some(Status::Completed),
                    ..ProviderPatch::default()
                },
            )
            .unwrap();
        let evidence_id = Ulid::new();
        let conflict = provider
            .store
            .attachment_dir(&id)
            .join(evidence_id.to_string());
        std::fs::create_dir_all(&conflict).unwrap();
        std::fs::write(conflict.join("existing.txt"), b"keep").unwrap();
        let error = provider
            .report_not_working(
                &id.to_string(),
                Timestamp::new("2026-08-26T06:02:00Z"),
                NotWorkingReport {
                    expected_token: None,
                    note: Some((Ulid::new(), "regressed".into())),
                    evidence: vec![ProviderEvidence {
                        id: evidence_id,
                        filename: "proof.txt".into(),
                        created_at: Timestamp::new("2026-08-26T06:02:00Z"),
                        bytes: b"new".to_vec(),
                    }],
                },
            )
            .unwrap_err();
        assert!(error.to_string().contains("already exists"));
        let unchanged = provider.get(&id.to_string()).unwrap();
        assert_eq!(unchanged.status, Status::Completed);
        assert!(!unchanged.up_next);
        assert!(
            unchanged
                .notes
                .iter()
                .all(|note| !note.text.starts_with("Not working:"))
        );
        assert!(unchanged.attachments.is_empty());
        assert_eq!(
            std::fs::read(conflict.join("existing.txt")).unwrap(),
            b"keep"
        );
    }

    #[test]
    fn transfer_operation_id_cannot_collide_across_sources() {
        let (_source_dir, source) = git_provider();
        let destination_dir = tempfile::tempdir().unwrap();
        let destination_store =
            FsStore::init(destination_dir.path(), &StoreMetadata::new("DST")).unwrap();
        let ids = [Ulid::new(), Ulid::new()];
        for (index, id) in ids.iter().enumerate() {
            source
                .create(
                    ctx(*id, &format!("2026-08-26T04:0{index}:00Z")),
                    ProviderDraft {
                        title: format!("source {index}"),
                        category: "task".into(),
                        priority: Priority::Default,
                        details: String::new(),
                        tags: vec![],
                        up_next: false,
                        blocked_by: vec![],
                        transfer: None,
                    },
                )
                .unwrap();
        }
        let registry = ProviderRegistry::default();
        registry.register(Arc::new(source)).unwrap();
        registry
            .register(Arc::new(GitProvider::new(
                "destination",
                destination_store.clone(),
            )))
            .unwrap();
        let source_ref = |id: Ulid| TicketRef {
            connection_id: "local".into(),
            native_id: id.to_string(),
        };
        copy_between(
            &registry,
            source_ref(ids[0]),
            "destination",
            "shared-op",
            Timestamp::new("2026-08-26T04:03:00Z"),
        )
        .unwrap();
        let error = copy_between(
            &registry,
            source_ref(ids[1]),
            "destination",
            "shared-op",
            Timestamp::new("2026-08-26T04:04:00Z"),
        )
        .unwrap_err();
        assert!(matches!(
            error,
            TransferError::Provider(ProviderError::Conflict { .. })
        ));
        assert_eq!(destination_store.list_tickets().unwrap().len(), 1);
    }
}
