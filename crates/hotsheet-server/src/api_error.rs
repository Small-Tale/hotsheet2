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
    /// HS2-XHQ1EH: one identifier field an automated caller acts on, such as the
    /// `request_id` of a permission request to answer.
    field: Option<(&'static str, String)>,
    /// HS2-8Z9VHH: one structured detail object beside `code`, such as a refused
    /// admission's reason, limit and observed value.
    detail: Option<Box<(&'static str, serde_json::Value)>>,
}

impl ApiError {
    pub(crate) fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
            code: None,
            field: None,
            detail: None,
        }
    }
    /// Attach a stable machine-readable `code` an automated caller can branch on.
    pub(crate) fn with_code(mut self, code: &'static str) -> Self {
        self.code = Some(code);
        self
    }
    /// Attach one identifier field, such as `request_id`, beside `code`.
    pub(crate) fn with_field(mut self, name: &'static str, value: impl Into<String>) -> Self {
        self.field = Some((name, value.into()));
        self
    }
    /// HS2-8Z9VHH: attach one structured detail object, such as `admission`.
    pub(crate) fn with_detail(mut self, name: &'static str, value: serde_json::Value) -> Self {
        self.detail = Some(Box::new((name, value)));
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
    /// A queued provider batch on a connection without durable write-behind: 409 with
    /// [`WRITE_BEHIND_UNAVAILABLE_CODE`] (HS2-DEVEYE). The message stays human-readable.
    pub(crate) fn write_behind_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, message).with_code(WRITE_BEHIND_UNAVAILABLE_CODE)
    }
}

/// The `code` of every expected-token 409, whatever its message names (HS2-9S8CV1).
pub(crate) const TICKET_CHANGED_CODE: &str = "ticket_changed";

/// The `code` of the 409 refusing a queued provider batch on a connection without durable
/// write-behind; clients fall back to the synchronous edit path on it, never on its text
/// (HS2-DEVEYE).
pub(crate) const WRITE_BEHIND_UNAVAILABLE_CODE: &str = "write_behind_unavailable";

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let mut body = serde_json::json!({ "error": self.message });
        if let Some(code) = self.code {
            body["code"] = serde_json::Value::from(code);
        }
        if let Some((name, value)) = self.field {
            body[name] = serde_json::Value::from(value);
        }
        if let Some(detail) = self.detail {
            let (name, value) = *detail;
            body[name] = value;
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
            field: None,
            detail: None,
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

/// HS2-Z8X4XP: a ticket whose stored attachment filename is not one safe path
/// component. The stored state, not the request, is wrong, so it is a 409 the client
/// can explain, naming the headless repair.
pub(crate) const UNSAFE_ATTACHMENT_FILENAME_CODE: &str = "unsafe_attachment_filename";

fn unsafe_attachment_filename(e: &StoreError) -> bool {
    matches!(
        e,
        StoreError::UnsafeAttachmentFilename { .. }
            | StoreError::Parse {
                source: hotsheet_model::ParseError::UnsafeAttachmentFilename { .. },
                ..
            }
    )
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        if unsafe_attachment_filename(&e) {
            return ApiError::new(
                StatusCode::CONFLICT,
                format!(
                    "{e}. Run `hotsheet-cli doctor --repair-attachment-names` to rename it \
                     to a safe name."
                ),
            )
            .with_code(UNSAFE_ATTACHMENT_FILENAME_CODE);
        }
        let status = match &e {
            StoreError::InvalidAnnotations(_)
            | StoreError::InvalidFeedbackResponse(_)
            | StoreError::StartedPhaseRequiresStarted { .. }
            | StoreError::InvalidNoteKindEdit
            | StoreError::InvalidTicketPrefix(_)
            | StoreError::InvalidGroupName(_)
            | StoreError::ImageCrop(_) => StatusCode::BAD_REQUEST,
            StoreError::FeedbackNoteEdit(_) | StoreError::Admission(_) => StatusCode::CONFLICT,
            StoreError::ClaimBackendUnreachable(_) => StatusCode::SERVICE_UNAVAILABLE,
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
            | OpError::BlockerCycle { .. }
            | OpError::EmptyNotWorkingReport
            | OpError::InvalidEta(_)) => ApiError::new(StatusCode::BAD_REQUEST, other.to_string()),
            other @ OpError::UnknownTicket(_) => {
                ApiError::new(StatusCode::NOT_FOUND, other.to_string())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// HS2-Z8X4XP: an unsafe stored attachment filename is a typed 409 naming the
    /// repair, whether the join refused it or the ticket parser did, never a 500.
    #[test]
    fn hs2_z8x4xp_unsafe_attachment_filename_is_a_typed_conflict() {
        let id = hotsheet_model::Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FB0").unwrap();
        let reason = "attachment filename contains a path separator";
        for error in [
            StoreError::UnsafeAttachmentFilename {
                attachment_id: id,
                reason,
            },
            StoreError::Parse {
                path: "tickets/AV/ticket.md".into(),
                source: hotsheet_model::ParseError::UnsafeAttachmentFilename { id, reason },
            },
        ] {
            let api = ApiError::from(error);
            assert_eq!(api.status, StatusCode::CONFLICT);
            assert_eq!(api.code, Some(UNSAFE_ATTACHMENT_FILENAME_CODE));
            assert!(
                api.message
                    .contains("hotsheet-cli doctor --repair-attachment-names"),
                "{}",
                api.message
            );
        }
        // Other parse failures keep their existing mapping.
        let other = ApiError::from(StoreError::Parse {
            path: "tickets/AV/ticket.md".into(),
            source: hotsheet_model::ParseError::MissingFrontmatter,
        });
        assert_eq!(other.status, StatusCode::INTERNAL_SERVER_ERROR);
        assert_eq!(other.code, None);
    }
}
