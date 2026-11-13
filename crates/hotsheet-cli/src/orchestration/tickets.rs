//! HS2-AZ15GA: target tickets from an orchestration project.
//!
//! Two halves share this module. The *hosted* half runs where the target checkout
//! lives and performs provider-neutral ticket operations on one of its sources. The
//! *orchestrating* half authorizes an action, reaches the hosted half (in process for
//! a local destination, over the reviewed transport for a remote one) and records the
//! structured link on the orchestration ticket. The target's source stays
//! authoritative: an unsupported operation is reported, never rerouted.
use std::sync::Arc;

use hotsheet_model::{OrchestrationLink, OrchestrationRelation, Priority, Status, Timestamp, Ulid};
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    DenialReason, TargetAction, TargetDestination, TargetOpError,
};
use hotsheet_ticketing::{
    FsStore, GitProvider, MutationContext, ProviderDraft, ProviderError, ProviderPatch, TicketRef,
    TransferProvenance, ops,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::*;

/// Opens a live provider for one external connection. Injected so tests never reach an
/// account; the production implementation reads the machine's stored credential.
pub trait ExternalProviders: Send + Sync {
    fn open(
        &self,
        config: &ProviderConfigRegistry,
        connection: &ProviderConnection,
    ) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, ProviderError>;
}

pub(super) struct LiveExternalProviders {
    pub(super) home: PathBuf,
}

impl ExternalProviders for LiveExternalProviders {
    fn open(
        &self,
        config: &ProviderConfigRegistry,
        connection: &ProviderConnection,
    ) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, ProviderError> {
        let unauthenticated = |message: String| ProviderError::Authentication {
            connection_id: connection.id.clone(),
            message,
        };
        let keys = hotsheet_ticketing::KeyRegistry::new(&self.home, hotsheet_ticketing::OsKeychain);
        let read = hotsheet_extsync::ConnectionRead::capture(config, connection, &keys)
            .map_err(|error| unauthenticated(error.to_string()))?;
        let now = time::OffsetDateTime::now_utc().unix_timestamp();
        let token = hotsheet_extsync::connection_access_token(&read, now)
            .map_err(|error| unauthenticated(error.to_string()))?;
        let provider = hotsheet_extsync::live_provider(connection, token)?;
        read.check()
            .map_err(|error| unauthenticated(error.to_string()))?;
        Ok(provider)
    }
}

fn op_error(error: ProviderError) -> OrchestrationError {
    use TargetOpError as E;
    OrchestrationError::TargetOperation(match error {
        ProviderError::NotFound { .. } | ProviderError::UnknownConnection(_) => E::NotFound,
        ProviderError::Unsupported { .. } => E::Unsupported,
        ProviderError::Authentication { .. } => E::Unauthorized,
        ProviderError::Conflict { .. } => E::Conflict,
        ProviderError::RateLimited { .. } => E::RateLimited,
        ProviderError::Disabled { .. } => E::Disabled,
        ProviderError::InvalidNativeId { .. } | ProviderError::Endpoint(_) => E::Invalid,
        ProviderError::Store(error) if error.is_io_kind(std::io::ErrorKind::NotFound) => {
            E::NotFound
        }
        ProviderError::Operation(_) => E::Invalid,
        ProviderError::Store(_) => E::Failed,
    })
}

fn bounded(value: &str, limit: usize, what: &'static str) -> Result<(), OrchestrationError> {
    if value.trim().is_empty() || value.len() > limit || value.chars().any(|c| c == '\0') {
        return Err(OrchestrationError::Invalid(what));
    }
    Ok(())
}

fn identifier(value: &str, what: &'static str) -> Result<(), OrchestrationError> {
    if value.is_empty()
        || value.len() > 160
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    {
        return Err(OrchestrationError::Invalid(what));
    }
    Ok(())
}

/// A provider-native id: bounded, printable, no path or control characters.
fn native(value: &str) -> Result<(), OrchestrationError> {
    if value.is_empty()
        || value.len() > 200
        || value
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || matches!(c, '/' | '\\'))
    {
        return Err(OrchestrationError::Invalid(
            "bounded native ticket id required",
        ));
    }
    Ok(())
}

fn timestamp(tx: &Transaction<'_>) -> Timestamp {
    let nanos = i128::from(tx.clock.now_ms()) * 1_000_000;
    Timestamp::from_datetime(
        time::OffsetDateTime::from_unix_timestamp_nanos(nanos)
            .unwrap_or(time::OffsetDateTime::UNIX_EPOCH),
    )
}

/// The fields an orchestration project may set on a target ticket.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTicketFields {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub priority: Option<Priority>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<Status>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub up_next: Option<bool>,
}

impl TargetTicketFields {
    fn validate(&self) -> Result<(), OrchestrationError> {
        if let Some(title) = &self.title {
            bounded(title, 500, "bounded ticket title required")?;
        }
        if self.details.as_ref().is_some_and(|d| d.len() > 256 * 1024) {
            return Err(OrchestrationError::Invalid("ticket details are too large"));
        }
        if let Some(category) = &self.category {
            identifier(category, "bounded ticket category required")?;
        }
        if let Some(tags) = &self.tags {
            if tags.len() > 64 {
                return Err(OrchestrationError::Invalid("too many tags"));
            }
            for tag in tags {
                bounded(tag, 120, "bounded tag required")?;
            }
        }
        Ok(())
    }

    fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Hosted create: the idempotency key and the orchestration ticket it is for.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedCreate {
    pub connection_id: String,
    pub operation_id: String,
    /// The orchestration ticket, as provenance on the created ticket.
    pub source: TicketRef,
    #[serde(flatten)]
    pub fields: TargetTicketFields,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HostedUpdate {
    pub connection_id: String,
    pub native_id: String,
    /// The provider's concurrency token from a previous read, when the caller has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_token: Option<String>,
    #[serde(flatten)]
    pub fields: TargetTicketFields,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<MutationActor>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostedTicket {
    pub ticket: Value,
    /// False when an earlier request with the same operation id already created it.
    #[serde(default)]
    pub created: bool,
}

/// The orchestration ticket an operation is for, in its owning checkout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OrchestrationTicket {
    /// Empty selects the owning checkout's only git source.
    #[serde(default)]
    pub connection_id: String,
    pub native_id: String,
}

/// One ticket in a declared target.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTicket {
    pub target_id: String,
    pub connection_id: String,
    pub native_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateTargetTicket {
    pub ticket: OrchestrationTicket,
    pub target_id: String,
    /// The target's source to create in; required, so a multi-source target is never
    /// guessed.
    pub connection_id: String,
    /// Retain across retries: the same id never creates a second ticket.
    pub operation_id: String,
    #[serde(flatten)]
    pub fields: TargetTicketFields,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadTargetTicket {
    #[serde(flatten)]
    pub target: TargetTicket,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UpdateTargetTicket {
    #[serde(flatten)]
    pub target: TargetTicket,
    #[serde(default)]
    pub expected_token: Option<String>,
    /// Identifies this change in the audit journal and for an approval.
    #[serde(default)]
    pub operation_id: Option<String>,
    #[serde(flatten)]
    pub fields: TargetTicketFields,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkTargetTicket {
    pub ticket: OrchestrationTicket,
    #[serde(flatten)]
    pub target: TargetTicket,
    /// `observed` (the default) or `validated`.
    #[serde(default)]
    pub relation: Option<OrchestrationRelation>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UnlinkTargetTicket {
    pub ticket: OrchestrationTicket,
    pub target_id: String,
    pub connection_id: String,
    #[serde(default)]
    pub native_id: Option<String>,
    #[serde(default)]
    pub operation_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListLinks {
    pub ticket: OrchestrationTicket,
    /// Also read each linked target ticket (needs `tickets_read` for each target).
    #[serde(default)]
    pub resolve: bool,
}

/// Whether the orchestration ticket's own link was recorded. A created target ticket
/// with `recorded: false` is a partial failure; repeat the same request to repair it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LinkOutcome {
    pub recorded: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<&'static str>,
}

#[derive(Debug, Clone, Serialize)]
pub struct TargetTicketResult {
    pub ticket: Value,
    pub created: bool,
    pub link: LinkOutcome,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkView {
    #[serde(flatten)]
    pub link: OrchestrationLink,
    /// `stored` (not resolved), `resolved`, or `unresolved` with a `reason` code.
    pub state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
    /// Title, status and slug of the target ticket when resolved; labels only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<Value>,
    /// The target ticket names this orchestration ticket as its origin.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub backlink: Option<bool>,
}

fn summary(ticket: &Value) -> Value {
    json!({
        "title": ticket.get("title").cloned().unwrap_or(Value::Null),
        "status": ticket.get("status").cloned().unwrap_or(Value::Null),
        "slug": ticket.get("slug").cloned().unwrap_or(Value::Null),
    })
}

impl OrchestrationRuntime {
    // ---- Hosted half: runs on the machine that hosts the target checkout. ----

    fn hosted_provider(
        &self,
        checkout_id: &str,
        connection_id: &str,
    ) -> Result<Arc<dyn hotsheet_ticketing::TicketProvider>, OrchestrationError> {
        identifier(connection_id, "bounded connection id required")?;
        let checkout = self.owner(checkout_id)?;
        let Some(source) = checkout.source(connection_id) else {
            return Err(OrchestrationError::TargetOperation(TargetOpError::NotFound));
        };
        if source.provider == "git" {
            let store = checkout
                .verified_git_source(connection_id)
                .map_err(checkout_error)?;
            return Ok(Arc::new(GitProvider::new(connection_id, store)));
        }
        let connection = self
            .providers
            .load()
            .map_err(|_| OrchestrationError::Corrupt)?
            .into_iter()
            .find(|connection| connection.id == connection_id)
            .ok_or(OrchestrationError::TargetOperation(TargetOpError::NotFound))?;
        if connection.disabled {
            // Refuse before building a client, so a disabled source is never contacted.
            return Err(OrchestrationError::TargetOperation(TargetOpError::Disabled));
        }
        self.external
            .open(&self.providers, &connection)
            .map_err(op_error)
    }

    pub fn hosted_ticket(
        &self,
        checkout_id: &str,
        connection_id: &str,
        native_id: &str,
    ) -> Result<HostedTicket, OrchestrationError> {
        native(native_id)?;
        let ticket = self
            .hosted_provider(checkout_id, connection_id)?
            .get(native_id)
            .map_err(op_error)?;
        Ok(HostedTicket {
            ticket: serde_json::to_value(ticket).map_err(|_| OrchestrationError::Corrupt)?,
            created: false,
        })
    }

    /// The ticket an operation id created, if any. Lets a caller reconcile a create
    /// whose answer it never received without creating again.
    pub fn hosted_ticket_by_operation(
        &self,
        checkout_id: &str,
        connection_id: &str,
        operation_id: &str,
    ) -> Result<Option<HostedTicket>, OrchestrationError> {
        identifier(operation_id, "bounded operation id required")?;
        self.hosted_provider(checkout_id, connection_id)?
            .find_transfer(operation_id)
            .map_err(op_error)?
            .map(|ticket| {
                Ok(HostedTicket {
                    ticket: serde_json::to_value(ticket)
                        .map_err(|_| OrchestrationError::Corrupt)?,
                    created: false,
                })
            })
            .transpose()
    }

    /// Create through the source's own provider. Idempotent by `operation_id`: a retry
    /// returns the existing ticket; the same id for a different orchestration ticket
    /// conflicts.
    pub fn hosted_ticket_create(
        &self,
        checkout_id: &str,
        request: HostedCreate,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedTicket, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        identifier(
            &request.source.connection_id,
            "bounded source connection id required",
        )?;
        native(&request.source.native_id)?;
        request.fields.validate()?;
        let title = request
            .fields
            .title
            .clone()
            .ok_or(OrchestrationError::Invalid("ticket title required"))?;
        let provider = self.hosted_provider(checkout_id, &request.connection_id)?;
        if !provider.descriptor().capabilities.create {
            return Err(OrchestrationError::TargetOperation(
                TargetOpError::Unsupported,
            ));
        }
        let existed = provider
            .find_transfer(&request.operation_id)
            .map_err(op_error)?
            .is_some();
        let generated_id = hotsheet_ticketing::mint_ulid(tx.clock, tx.rng);
        let ticket = provider
            .create(
                MutationContext {
                    now: timestamp(tx),
                    generated_id,
                },
                ProviderDraft {
                    title,
                    category: request.fields.category.unwrap_or_else(|| "issue".into()),
                    priority: request.fields.priority.unwrap_or_default(),
                    status: request.fields.status.unwrap_or_default(),
                    details: request.fields.details.unwrap_or_default(),
                    tags: request.fields.tags.unwrap_or_default(),
                    up_next: request.fields.up_next.unwrap_or(false),
                    blocked_by: Vec::new(),
                    transfer: Some(TransferProvenance {
                        operation_id: request.operation_id,
                        source: request.source,
                    }),
                    created_by: hotsheet_ticketing::actor::note_actor(request.actor.as_ref()),
                    group: None,
                },
            )
            .map_err(op_error)?;
        Ok(HostedTicket {
            ticket: serde_json::to_value(ticket).map_err(|_| OrchestrationError::Corrupt)?,
            created: !existed,
        })
    }

    pub fn hosted_ticket_update(
        &self,
        checkout_id: &str,
        request: HostedUpdate,
        tx: &mut Transaction<'_>,
    ) -> Result<HostedTicket, OrchestrationError> {
        native(&request.native_id)?;
        request.fields.validate()?;
        if request.fields.is_empty() {
            return Err(OrchestrationError::Invalid("no ticket field to update"));
        }
        let provider = self.hosted_provider(checkout_id, &request.connection_id)?;
        if !provider.descriptor().capabilities.update {
            return Err(OrchestrationError::TargetOperation(
                TargetOpError::Unsupported,
            ));
        }
        let fields = request.fields;
        let ticket = provider
            .update(
                &request.native_id,
                timestamp(tx),
                ProviderPatch {
                    expected_token: request.expected_token,
                    title: fields.title,
                    details: fields.details,
                    category: fields.category,
                    priority: fields.priority,
                    status: fields.status,
                    tags: fields.tags,
                    up_next: fields.up_next,
                    actor: hotsheet_ticketing::actor::note_actor(request.actor.as_ref()),
                    ..ProviderPatch::default()
                },
            )
            .map_err(op_error)?;
        Ok(HostedTicket {
            ticket: serde_json::to_value(ticket).map_err(|_| OrchestrationError::Corrupt)?,
            created: false,
        })
    }

    // ---- Orchestrating half. ----

    /// Reach the hosted half for an authorized destination.
    fn hosted_call(
        &self,
        destination: &TargetDestination,
        method: &'static str,
        query: &str,
        body: Option<&Value>,
        tx: &mut Transaction<'_>,
    ) -> Result<Value, OrchestrationError> {
        match destination {
            // The same machine hosts the target: no server, transport or secret needed.
            TargetDestination::Local { checkout_id, .. } => self.request(
                method,
                &format!("/checkouts/{checkout_id}/target-tickets{query}"),
                body.unwrap_or(&Value::Null),
                tx,
            ),
            TargetDestination::Remote {
                registration_id,
                identity,
                checkout_id,
                ..
            } => {
                let bytes = body
                    .map(serde_json::to_vec)
                    .transpose()
                    .map_err(|_| OrchestrationError::Corrupt)?;
                let reply = self
                    .remotes
                    .call_checkout(
                        registration_id,
                        identity,
                        checkout_id,
                        self.secrets.as_ref(),
                        self.transport.as_ref(),
                        method,
                        &format!("/target-tickets{query}"),
                        bytes.as_deref(),
                    )
                    .map_err(OrchestrationError::Observation)?;
                let parsed: Value = serde_json::from_slice(&reply.body).unwrap_or(Value::Null);
                if (200..300).contains(&reply.status) {
                    return Ok(parsed);
                }
                // The peer names its own refusal by code; anything else is classified
                // by status alone. Its message text is never forwarded.
                let code = parsed.get("code").and_then(Value::as_str);
                Err(OrchestrationError::TargetOperation(
                    code.and_then(TargetOpError::from_code)
                        .unwrap_or(match reply.status {
                            401 | 403 => TargetOpError::Unauthorized,
                            // An older peer has no such route.
                            404 | 405 if code.is_none() => TargetOpError::PeerUnsupported,
                            404 => TargetOpError::NotFound,
                            409 => TargetOpError::Conflict,
                            422 => TargetOpError::Unsupported,
                            429 => TargetOpError::RateLimited,
                            400 => TargetOpError::Invalid,
                            _ => TargetOpError::Failed,
                        }),
                ))
            }
        }
    }

    /// The git store that holds an orchestration ticket, with its ticket id and source
    /// connection. Structured links live in the ticket file, so a source that cannot
    /// store them is refused explicitly. An empty connection id selects the owner's
    /// only git source and is refused when there are several.
    fn orchestration_store(
        &self,
        owner_id: &str,
        ticket: &OrchestrationTicket,
    ) -> Result<(FsStore, Ulid, String), OrchestrationError> {
        let owner = self.owner(owner_id)?;
        let connection_id = if ticket.connection_id.is_empty() {
            let mut git = owner
                .sources
                .iter()
                .filter(|source| source.provider == "git");
            match (git.next(), git.next()) {
                (Some(only), None) => only.connection_id.clone(),
                (None, _) => {
                    return Err(OrchestrationError::TargetOperation(
                        TargetOpError::LinksUnsupported,
                    ));
                }
                _ => {
                    return Err(OrchestrationError::Invalid(
                        "several ticket sources; name the orchestration ticket's connection",
                    ));
                }
            }
        } else {
            identifier(&ticket.connection_id, "bounded connection id required")?;
            ticket.connection_id.clone()
        };
        let Some(source) = owner.source(&connection_id) else {
            return Err(OrchestrationError::TargetOperation(TargetOpError::NotFound));
        };
        if source.provider != "git" {
            return Err(OrchestrationError::TargetOperation(
                TargetOpError::LinksUnsupported,
            ));
        }
        let store = owner
            .verified_git_source(&connection_id)
            .map_err(checkout_error)?;
        let id = ops::resolve(&store, &ticket.native_id)
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Failed))?
            .ok_or(OrchestrationError::TargetOperation(TargetOpError::NotFound))?
            .id;
        Ok((store, id, connection_id))
    }

    fn target_ref(owner_id: &str, target_id: &str) -> Result<TargetRef, OrchestrationError> {
        let target = TargetRef {
            owner_checkout_id: owner_id.into(),
            target_id: target_id.into(),
        };
        target.validate()?;
        Ok(target)
    }

    fn destination(authorized: &Authorized) -> Result<&TargetDestination, OrchestrationError> {
        authorized
            .resolution
            .binding
            .destination
            .as_ref()
            .ok_or(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    }

    /// The source must be one the freshly resolved target actually has: a create is
    /// never sent to a connection the target does not list.
    fn check_source(
        authorized: &Authorized,
        connection_id: &str,
    ) -> Result<(), OrchestrationError> {
        let listed = authorized
            .resolution
            .sources
            .as_ref()
            .is_some_and(|sources| sources.iter().any(|s| s.connection_id == connection_id));
        if listed {
            Ok(())
        } else {
            Err(OrchestrationError::TargetOperation(TargetOpError::NotFound))
        }
    }

    /// Create a ticket in a target for an orchestration ticket, in three recoverable
    /// steps: record the intent, create (idempotent by operation id), record the result.
    pub fn target_ticket_create(
        &self,
        owner_id: &str,
        request: CreateTargetTicket,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetTicketResult, OrchestrationError> {
        identifier(&request.operation_id, "bounded operation id required")?;
        identifier(&request.connection_id, "bounded connection id required")?;
        request.fields.validate()?;
        if request.fields.title.is_none() {
            return Err(OrchestrationError::Invalid("ticket title required"));
        }
        let target = Self::target_ref(owner_id, &request.target_id)?;
        let (store, source_id, source_connection) =
            self.orchestration_store(owner_id, &request.ticket)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::TicketsWrite).operation(request.operation_id.clone()),
            tx,
        )?;
        let outcome = (|| {
            Self::check_source(&authorized, &request.connection_id)?;
            let link = |relation, native_id: Option<String>| OrchestrationLink {
                target_id: request.target_id.clone(),
                connection_id: request.connection_id.clone(),
                native_id,
                relation,
                operation_id: Some(request.operation_id.clone()),
            };
            // Intent first: if the process stops after the target create, the planned
            // link names the operation that finds the ticket again.
            ops::upsert_orchestration_link(
                &store,
                &source_id,
                link(OrchestrationRelation::Planned, None),
                timestamp(tx),
            )
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Failed))?;
            let body = serde_json::to_value(HostedCreate {
                connection_id: request.connection_id.clone(),
                operation_id: request.operation_id.clone(),
                source: TicketRef {
                    connection_id: source_connection.clone(),
                    native_id: source_id.to_string(),
                },
                fields: request.fields.clone(),
                actor: actor.cloned(),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let hosted: HostedTicket = serde_json::from_value(self.hosted_call(
                Self::destination(&authorized)?,
                "POST",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            let native_id = hosted
                .ticket
                .get("native_id")
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or(OrchestrationError::Observation(
                    TargetFailure::InvalidResponse,
                ))?;
            // The target ticket exists. A failed link write is reported, not hidden,
            // and the same request repairs it.
            let recorded = ops::upsert_orchestration_link(
                &store,
                &source_id,
                link(OrchestrationRelation::Created, Some(native_id)),
                timestamp(tx),
            )
            .is_ok();
            Ok(TargetTicketResult {
                ticket: hosted.ticket,
                created: hosted.created,
                link: LinkOutcome {
                    recorded,
                    error: (!recorded).then_some("link_write_failed"),
                },
            })
        })();
        // A definitive refusal will not succeed on retry, so the recorded intent is
        // withdrawn. A transient failure keeps it: it names the operation to resume.
        if let Err(OrchestrationError::TargetOperation(
            TargetOpError::Conflict
            | TargetOpError::Unsupported
            | TargetOpError::Invalid
            | TargetOpError::NotFound
            | TargetOpError::Disabled,
        )) = &outcome
        {
            let planned = OrchestrationLink {
                target_id: request.target_id.clone(),
                connection_id: request.connection_id.clone(),
                native_id: None,
                relation: OrchestrationRelation::Planned,
                operation_id: Some(request.operation_id.clone()),
            };
            let still_planned = store
                .read_ticket(&source_id)
                .is_ok_and(|ticket| ticket.orchestration_links.contains(&planned));
            if still_planned {
                let _ = ops::remove_orchestration_link(&store, &source_id, &planned, timestamp(tx));
            }
        }
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    pub fn target_ticket_read(
        &self,
        owner_id: &str,
        request: ReadTargetTicket,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Value, OrchestrationError> {
        let target = Self::target_ref(owner_id, &request.target.target_id)?;
        identifier(
            &request.target.connection_id,
            "bounded connection id required",
        )?;
        native(&request.target.native_id)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::TicketsRead),
            tx,
        )?;
        let outcome = self.read_authorized(&authorized, &request.target, tx);
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    fn read_authorized(
        &self,
        authorized: &Authorized,
        target: &TargetTicket,
        tx: &mut Transaction<'_>,
    ) -> Result<Value, OrchestrationError> {
        Self::check_source(authorized, &target.connection_id)?;
        let hosted: HostedTicket = serde_json::from_value(self.hosted_call(
            Self::destination(authorized)?,
            "GET",
            &format!(
                "?connection_id={}&native_id={}",
                encode(&target.connection_id),
                encode(&target.native_id)
            ),
            None,
            tx,
        )?)
        .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
        Ok(hosted.ticket)
    }

    pub fn target_ticket_update(
        &self,
        owner_id: &str,
        request: UpdateTargetTicket,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Value, OrchestrationError> {
        let target = Self::target_ref(owner_id, &request.target.target_id)?;
        identifier(
            &request.target.connection_id,
            "bounded connection id required",
        )?;
        native(&request.target.native_id)?;
        request.fields.validate()?;
        if request.fields.is_empty() {
            return Err(OrchestrationError::Invalid("no ticket field to update"));
        }
        let mut action = ActionRequest::new(TargetAction::TicketsWrite);
        action.operation_id = request.operation_id.clone();
        let authorized = self.authorize(&target, actor, action, tx)?;
        let outcome = (|| {
            Self::check_source(&authorized, &request.target.connection_id)?;
            let body = serde_json::to_value(HostedUpdate {
                connection_id: request.target.connection_id.clone(),
                native_id: request.target.native_id.clone(),
                expected_token: request.expected_token.clone(),
                fields: request.fields.clone(),
                actor: actor.cloned(),
            })
            .map_err(|_| OrchestrationError::Corrupt)?;
            let hosted: HostedTicket = serde_json::from_value(self.hosted_call(
                Self::destination(&authorized)?,
                "PATCH",
                "",
                Some(&body),
                tx,
            )?)
            .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            Ok(hosted.ticket)
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Link an existing target ticket. The ticket is read first, so a link is never
    /// recorded for something the actor cannot see or that does not exist.
    pub fn target_ticket_link(
        &self,
        owner_id: &str,
        request: LinkTargetTicket,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<OrchestrationLink>, OrchestrationError> {
        let relation = request.relation.unwrap_or(OrchestrationRelation::Observed);
        if !matches!(
            relation,
            OrchestrationRelation::Observed | OrchestrationRelation::Validated
        ) {
            return Err(OrchestrationError::Invalid(
                "link relation must be observed or validated",
            ));
        }
        let target = Self::target_ref(owner_id, &request.target.target_id)?;
        identifier(
            &request.target.connection_id,
            "bounded connection id required",
        )?;
        native(&request.target.native_id)?;
        let (store, source_id, _) = self.orchestration_store(owner_id, &request.ticket)?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::TicketsRead),
            tx,
        )?;
        let outcome = (|| {
            let ticket = self.read_authorized(&authorized, &request.target, tx)?;
            // Record the provider's canonical native id, not the caller's spelling.
            let native_id = ticket
                .get("native_id")
                .and_then(Value::as_str)
                .unwrap_or(&request.target.native_id)
                .to_owned();
            ops::upsert_orchestration_link(
                &store,
                &source_id,
                OrchestrationLink {
                    target_id: request.target.target_id.clone(),
                    connection_id: request.target.connection_id.clone(),
                    native_id: Some(native_id),
                    relation,
                    operation_id: None,
                },
                timestamp(tx),
            )
            .map(|ticket| ticket.orchestration_links)
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Failed))
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }

    /// Remove the orchestration ticket's own link. The target is neither contacted
    /// nor changed, so this works while the target is unbound or unreachable.
    pub fn target_ticket_unlink(
        &self,
        owner_id: &str,
        request: UnlinkTargetTicket,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<OrchestrationLink>, OrchestrationError> {
        Self::target_ref(owner_id, &request.target_id)?;
        if request.native_id.is_none() && request.operation_id.is_none() {
            return Err(OrchestrationError::Invalid(
                "a native id or an operation id is required to unlink",
            ));
        }
        let (store, source_id, _) = self.orchestration_store(owner_id, &request.ticket)?;
        ops::remove_orchestration_link(
            &store,
            &source_id,
            &OrchestrationLink {
                target_id: request.target_id,
                connection_id: request.connection_id,
                native_id: request.native_id,
                relation: OrchestrationRelation::Observed,
                operation_id: request.operation_id,
            },
            timestamp(tx),
        )
        .map(|ticket| ticket.orchestration_links)
        .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::Failed))
    }

    /// The orchestration ticket's links. Without `resolve` nothing is contacted. With
    /// it, each link is read through the gate; a link that cannot be read stays in the
    /// list with its reason, and a planned create is looked up by its operation id.
    pub fn target_ticket_links(
        &self,
        owner_id: &str,
        request: ListLinks,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<Vec<LinkView>, OrchestrationError> {
        let (store, source_id, source_connection) =
            self.orchestration_store(owner_id, &request.ticket)?;
        let links = store
            .read_ticket(&source_id)
            .map_err(|_| OrchestrationError::TargetOperation(TargetOpError::NotFound))?
            .orchestration_links;
        let origin = format!("{source_connection}:{source_id}");
        let mut views = Vec::with_capacity(links.len());
        for link in links {
            if !request.resolve {
                views.push(LinkView {
                    link,
                    state: "stored",
                    reason: None,
                    summary: None,
                    backlink: None,
                });
                continue;
            }
            let resolved = (|| {
                let target = Self::target_ref(owner_id, &link.target_id)?;
                let authorized = self.authorize(
                    &target,
                    actor,
                    ActionRequest::new(TargetAction::TicketsRead),
                    tx,
                )?;
                Self::check_source(&authorized, &link.connection_id)?;
                let query = match (&link.native_id, &link.operation_id) {
                    (Some(native_id), _) => format!(
                        "?connection_id={}&native_id={}",
                        encode(&link.connection_id),
                        encode(native_id)
                    ),
                    (None, Some(operation_id)) => format!(
                        "?connection_id={}&operation_id={}",
                        encode(&link.connection_id),
                        encode(operation_id)
                    ),
                    (None, None) => {
                        return Err(OrchestrationError::TargetOperation(TargetOpError::NotFound));
                    }
                };
                let reply =
                    self.hosted_call(Self::destination(&authorized)?, "GET", &query, None, tx)?;
                // A planned create that never happened answers null.
                serde_json::from_value::<Option<HostedTicket>>(reply)
                    .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?
                    .map(|hosted| hosted.ticket)
                    .ok_or(OrchestrationError::TargetOperation(TargetOpError::NotFound))
            })();
            views.push(match resolved {
                Ok(ticket) => LinkView {
                    backlink: Some(
                        ticket.get("transferred_from").and_then(Value::as_str)
                            == Some(origin.as_str()),
                    ),
                    summary: Some(summary(&ticket)),
                    link,
                    state: "resolved",
                    reason: None,
                },
                Err(error) => LinkView {
                    link,
                    state: "unresolved",
                    reason: Some(error.code().into()),
                    summary: None,
                    backlink: None,
                },
            });
        }
        Ok(views)
    }

    fn finish<T>(
        &self,
        target: &TargetRef,
        authorized: &Authorized,
        outcome: &Result<T, OrchestrationError>,
        tx: &mut Transaction<'_>,
    ) -> Result<(), OrchestrationError> {
        self.record_outcome(
            target,
            authorized,
            outcome.is_ok(),
            outcome.as_ref().err().map(OrchestrationError::code),
            tx,
        )
    }
}

/// Percent-encode a query value; identifiers are mostly unreserved already.
fn encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decode one percent-encoded query value; malformed input is invalid, not guessed.
pub(super) fn decode_query(value: &str) -> Result<String, OrchestrationError> {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'%' => {
                let hex = bytes
                    .get(index + 1..index + 3)
                    .and_then(|hex| std::str::from_utf8(hex).ok())
                    .and_then(|hex| u8::from_str_radix(hex, 16).ok())
                    .ok_or(OrchestrationError::Invalid("malformed query value"))?;
                out.push(hex);
                index += 3;
            }
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }
    String::from_utf8(out).map_err(|_| OrchestrationError::Invalid("malformed query value"))
}
