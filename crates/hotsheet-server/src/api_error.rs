//! The `{ "error": ... }` API error type and its conversions.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use hotsheet_index::IndexError;
use hotsheet_ticketing::{OpError, StoreError};
use serde::Deserialize;

// ---- errors ----------------------------------------------------------------------

/// An API error rendered as `{ "error": "…" }` with a status code.
#[derive(Debug)]
pub struct ApiError {
    pub(crate) status: StatusCode,
    pub(crate) message: String,
    /// Stable machine-readable code for errors an automated caller should branch on.
    code: Option<&'static str>,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            code: None,
        }
    }
    /// Attach a stable machine-readable `code` an automated caller can branch on.
    pub(crate) fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }
    pub(crate) fn not_found(id: &str) -> Self {
        Self::new(StatusCode::NOT_FOUND, format!("no ticket matching '{id}'"))
    }
    /// A stale `expected_token`: 409 with [`TICKET_CHANGED_CODE`], the one conflict a client may
    /// resolve by re-reading and retrying (HS2-9S8CV1). The message stays human-readable.
    pub(crate) fn ticket_changed(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message).with_code(TICKET_CHANGED_CODE)
    }
}

/// The `code` of every expected-token 409, whatever its message names (HS2-9S8CV1).
pub(crate) const TICKET_CHANGED_CODE: &str = "ticket_changed";

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "error": self.message });
        if let Some(code) = self.code {
            body["code"] = serde_json::Value::from(code);
        }
        (self.status, Json(body)).into_response()
    }
}

/// A role-specific rule refused the mutation before any write (HS2-RD4M29): 422 with the
/// rule's stable `code` and its actor-tailored message.
impl From<hotsheet_ticketing::actor::RuleViolation> for ApiError {
    fn from(violation: hotsheet_ticketing::actor::RuleViolation) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: violation.message,
            code: Some(violation.code),
        }
    }
}

/// The optional `actor` object every mutating request body may carry (HS2-RD4M29).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ActorReq {
    /// Defaulted so a missing role reads as an explicit 400, like an unknown one.
    #[serde(default)]
    role: String,
    #[serde(default)]
    id: Option<String>,
}

pub(crate) fn parse_actor(
    actor: Option<&ActorReq>,
) -> Result<Option<hotsheet_ticketing::actor::MutationActor>, ApiError> {
    actor
        .map(|actor| {
            hotsheet_ticketing::actor::MutationActor::parse(&actor.role, actor.id.clone())
                .map_err(|error| ApiError::new(StatusCode::BAD_REQUEST, format!("actor: {error}")))
        })
        .transpose()
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        let status = match &e {
            StoreError::InvalidAnnotations(_)
            | StoreError::InvalidFeedbackResponse(_)
            | StoreError::InvalidNoteKindEdit
            | StoreError::ImageCrop(_) => StatusCode::BAD_REQUEST,
            StoreError::FeedbackNoteEdit(_) => StatusCode::CONFLICT,
            error if error.is_io_kind(std::io::ErrorKind::NotFound) => StatusCode::NOT_FOUND,
            StoreError::NotAStore(_) => StatusCode::INTERNAL_SERVER_ERROR,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        ApiError::new(status, e.to_string())
    }
}

impl From<IndexError> for ApiError {
    fn from(e: IndexError) -> Self {
        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
    }
}

impl From<OpError> for ApiError {
    fn from(e: OpError) -> Self {
        match e {
            OpError::Store(s) => ApiError::from(s),
            other @ (OpError::WrongWorker { .. }
            | OpError::NotClaimed(_)
            | OpError::ClaimUnavailable { .. }
            | OpError::StaleHandoff(_)
            | OpError::NotWorkingRequiresCompleted(_)
            | OpError::NotInTrash(_)) => ApiError::new(StatusCode::CONFLICT, other.to_string()),
            other @ (OpError::DuplicateNeedsTarget
            | OpError::SelfBlock(_)
            | OpError::EmptyNotWorkingReport
            | OpError::InvalidEta(_)) => ApiError::new(StatusCode::BAD_REQUEST, other.to_string()),
            other @ OpError::UnknownTicket(_) => {
                ApiError::new(StatusCode::NOT_FOUND, other.to_string())
            }
        }
    }
}
