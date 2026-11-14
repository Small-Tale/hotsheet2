//! HS2-D85G19: list and search a declared target's open tickets through the gate.
//!
//! The hosted half runs where the target checkout lives and asks that source's own
//! provider for one bounded page of open tickets. The orchestrating half authorizes
//! `tickets_read` (reading never asks a person), reaches the hosted half like every
//! other target ticket operation and audits the outcome. Rows are labels and
//! identifiers only; a caller reads a ticket in full with `ticket read`.
use hotsheet_model::{Priority, Status};
use hotsheet_ticketing::TicketQuery;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{TargetAction, TargetOpError};
use serde::{Deserialize, Serialize};

use super::tickets::{encode, identifier};
use super::*;

/// Rows per page when the caller names no limit.
pub const DEFAULT_TARGET_TICKET_PAGE: u32 = 50;
/// The largest page a caller may ask for.
pub const MAX_TARGET_TICKET_PAGE: u32 = 100;
const MAX_TEXT_BYTES: usize = 200;
const MAX_CURSOR_BYTES: usize = 512;

/// What to look for among a source's open tickets. Every filter narrows; none is
/// required, so an empty search lists the open tickets page by page.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TargetTicketSearch {
    /// Case-insensitive text the provider matches against its tickets.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<String>,
    /// Only tickets carrying this tag.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tag: Option<String>,
    /// The provider's opaque cursor from the previous page.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Rows per page, 1 to [`MAX_TARGET_TICKET_PAGE`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

impl TargetTicketSearch {
    pub(super) fn validate(&self) -> Result<(), OrchestrationError> {
        if let Some(text) = &self.text
            && (text.trim().is_empty()
                || text.len() > MAX_TEXT_BYTES
                || text.chars().any(char::is_control))
        {
            return Err(OrchestrationError::Invalid("bounded search text required"));
        }
        if let Some(category) = &self.category {
            identifier(category, "bounded ticket category required")?;
        }
        if let Some(tag) = &self.tag
            && (tag.trim().is_empty() || tag.len() > 120 || tag.chars().any(char::is_control))
        {
            return Err(OrchestrationError::Invalid("bounded tag required"));
        }
        if let Some(cursor) = &self.cursor
            && (cursor.is_empty()
                || cursor.len() > MAX_CURSOR_BYTES
                || !cursor.bytes().all(|b| b.is_ascii_graphic()))
        {
            return Err(OrchestrationError::Invalid("bounded page cursor required"));
        }
        if self
            .limit
            .is_some_and(|limit| limit == 0 || limit > MAX_TARGET_TICKET_PAGE)
        {
            return Err(OrchestrationError::Invalid(
                "page limit must be between 1 and 100",
            ));
        }
        Ok(())
    }

    fn page_size(&self) -> usize {
        self.limit.unwrap_or(DEFAULT_TARGET_TICKET_PAGE) as usize
    }

    /// The hosted route's query string after `connection_id`.
    fn query(&self) -> String {
        let mut query = String::new();
        for (key, value) in [
            ("text", &self.text),
            ("category", &self.category),
            ("tag", &self.tag),
            ("cursor", &self.cursor),
        ] {
            if let Some(value) = value {
                query.push_str(&format!("&{key}={}", encode(value)));
            }
        }
        if let Some(limit) = self.limit {
            query.push_str(&format!("&limit={limit}"));
        }
        query
    }
}

/// One open ticket of a target: identifiers and labels, never details or notes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetTicketRow {
    pub native_id: String,
    pub slug: String,
    pub title: String,
    pub status: Status,
    pub priority: Priority,
    pub category: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub up_next: bool,
    pub updated_at: String,
    /// The operation that created it, when another project created it (§26.3).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transfer_operation_id: Option<String>,
    /// The ticket it was created for, as `<connection>:<id>`; a backlink.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transferred_from: Option<String>,
}

impl From<hotsheet_ticketing::ApiTicket> for TargetTicketRow {
    fn from(ticket: hotsheet_ticketing::ApiTicket) -> Self {
        Self {
            native_id: ticket.native_id,
            slug: ticket.slug,
            title: ticket.title,
            status: ticket.status,
            priority: ticket.priority,
            category: ticket.category,
            tags: ticket.tags,
            up_next: ticket.up_next,
            updated_at: ticket.updated_at,
            transfer_operation_id: ticket.transfer_operation_id,
            transferred_from: ticket.transferred_from,
        }
    }
}

/// One bounded page; `next_cursor` is absent on the last page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetTicketPage {
    pub tickets: Vec<TargetTicketRow>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// The orchestrating request: which target and source, and what to look for.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListTargetTickets {
    pub target_id: String,
    /// The target's ticket source; required, so a multi-source target is never guessed.
    pub connection_id: String,
    #[serde(flatten)]
    pub search: TargetTicketSearch,
}

impl OrchestrationRuntime {
    /// Hosted half: one page of a source's open tickets from its own provider.
    pub fn hosted_ticket_list(
        &self,
        checkout_id: &str,
        connection_id: &str,
        search: &TargetTicketSearch,
    ) -> Result<TargetTicketPage, OrchestrationError> {
        search.validate()?;
        let provider = self.hosted_provider(checkout_id, connection_id)?;
        let query = TicketQuery {
            open_only: true,
            text: search.text.clone(),
            category: search.category.clone(),
            tags: search.tag.iter().cloned().collect(),
            ..TicketQuery::default()
        };
        let page = provider
            .query_page(&query, search.cursor.as_deref(), search.page_size())
            .map_err(|error| match error {
                // A cursor the provider cannot read is the caller's, not a conflict.
                hotsheet_ticketing::ProviderError::Conflict { .. } => {
                    OrchestrationError::TargetOperation(TargetOpError::Invalid)
                }
                other => tickets::op_error(other),
            })?;
        Ok(TargetTicketPage {
            tickets: page.items.into_iter().map(TargetTicketRow::from).collect(),
            next_cursor: page.next_cursor,
        })
    }

    /// Orchestrating half: list or search a declared target's open tickets. Passes the
    /// gate as `tickets_read` and is audited; the target's source answers.
    pub fn target_ticket_list(
        &self,
        owner_id: &str,
        request: ListTargetTickets,
        actor: Option<&MutationActor>,
        tx: &mut Transaction<'_>,
    ) -> Result<TargetTicketPage, OrchestrationError> {
        let target = Self::target_ref(owner_id, &request.target_id)?;
        identifier(&request.connection_id, "bounded connection id required")?;
        request.search.validate()?;
        let authorized = self.authorize(
            &target,
            actor,
            ActionRequest::new(TargetAction::TicketsRead),
            tx,
        )?;
        let outcome = (|| {
            Self::check_source(&authorized, &request.connection_id)?;
            let reply = self.hosted_call_at(
                Self::destination(&authorized)?,
                "/target-tickets/list",
                "GET",
                &format!(
                    "?connection_id={}{}",
                    encode(&request.connection_id),
                    request.search.query()
                ),
                None,
                tx,
            )?;
            let page: TargetTicketPage = serde_json::from_value(reply)
                .map_err(|_| OrchestrationError::Observation(TargetFailure::InvalidResponse))?;
            // A peer never answers more than was asked.
            if page.tickets.len() > request.search.page_size() {
                return Err(OrchestrationError::Observation(
                    TargetFailure::InvalidResponse,
                ));
            }
            Ok(page)
        })();
        self.finish(&target, &authorized, &outcome, tx)?;
        outcome
    }
}
