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
use crate::wire::ApiTicket;
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
    pub attachments: bool,
    pub assignment: bool,
    pub review_requests: bool,
    pub dependencies: bool,
    pub up_next: bool,
    pub close_reasons: bool,
    pub claims: bool,
    pub atomic_batch: bool,
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
            attachments: true,
            assignment: true,
            review_requests: true,
            dependencies: true,
            up_next: true,
            close_reasons: true,
            claims: true,
            atomic_batch: true,
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
}

#[derive(Debug, Clone, Default)]
pub struct ProviderPatch {
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
#[derive(Debug, Clone)]
pub struct MutationContext {
    pub now: Timestamp,
    pub generated_id: Ulid,
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
    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError>;
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
}

impl GitProvider {
    pub fn new(connection_id: impl Into<String>, store: FsStore) -> Self {
        Self {
            connection_id: connection_id.into(),
            display_name: "Git tickets".into(),
            store,
            is_default: false,
        }
    }

    pub fn with_default(mut self, is_default: bool) -> Self {
        self.is_default = is_default;
        self
    }

    pub fn store(&self) -> &FsStore {
        &self.store
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
}

impl TicketProvider for GitProvider {
    fn descriptor(&self) -> ProviderDescriptor {
        ProviderDescriptor {
            connection_id: self.connection_id.clone(),
            provider: "git".into(),
            display_name: self.display_name.clone(),
            locator: self.store.root().display().to_string(),
            default: self.is_default,
            capabilities: ProviderCapabilities::git(),
        }
    }

    fn query(&self, query: &TicketQuery) -> Result<Vec<ApiTicket>, ProviderError> {
        Ok(ops::query(&self.store, query)?
            .iter()
            .map(|ticket| ApiTicket::from_provider(ticket, &self.connection_id, None))
            .collect())
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
        let ticket = ops::create(
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
        Ok(ApiTicket::from_provider(&ticket, &self.connection_id, None))
    }

    fn update(
        &self,
        native_id: &str,
        now: Timestamp,
        patch: ProviderPatch,
    ) -> Result<ApiTicket, ProviderError> {
        let ticket = self.ticket(native_id)?;
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

    fn close(
        &self,
        native_id: &str,
        now: Timestamp,
        reason: CloseReason,
        duplicate_of: Option<String>,
    ) -> Result<ApiTicket, ProviderError> {
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
}

pub struct ProviderQueryResult {
    pub descriptor: ProviderDescriptor,
    pub result: Result<Vec<ApiTicket>, ProviderError>,
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
        assert_eq!(closed.notes.len(), 1);

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
}
