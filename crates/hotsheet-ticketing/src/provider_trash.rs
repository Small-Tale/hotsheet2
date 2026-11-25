//! Trash for external ticket providers (GH-5).
//!
//! A provider whose Delete is a restorable Trash (GitHub Issues: a closed issue carrying
//! `status:deleted`) implements [`TicketProvider::restore`] and
//! [`TicketProvider::purge_trashed`]. Retention expiry and Empty Trash share
//! [`purge_provider_trash`], which never reports a ticket as removed unless the provider
//! confirmed it: a ticket whose credentials cannot permanently delete it stays in Trash and
//! comes back as [`RetainedTrashTicket`] with a provider-neutral explanation.

use crate::TicketQuery;
use crate::provider::{ProviderError, TicketProvider};
use hotsheet_model::{Status, Timestamp};
use serde::Serialize;

/// The provider-neutral explanation for a Trash ticket the provider would not delete.
pub const PURGE_NOT_PERMITTED: &str = "this connection's credentials cannot permanently delete tickets in the provider, so the ticket stays closed in Trash; reconnect with an account that can delete issues, or delete it in the provider";

/// The explanation for a provider that has no permanent delete at all.
pub const PURGE_UNSUPPORTED: &str =
    "this provider cannot permanently delete tickets, so the ticket stays in Trash";

/// The result of permanently deleting one Trash ticket.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum ProviderPurgeOutcome {
    /// The provider removed the ticket, or it was already gone.
    Purged,
    /// The provider kept the ticket; it stays in Trash.
    Retained { message: String },
}

/// A Trash ticket that a purge left in place, with the reason.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RetainedTrashTicket {
    pub qualified_id: String,
    pub slug: String,
    pub message: String,
}

/// What one provider Trash purge did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct ProviderTrashPurge {
    /// Slugs of the tickets the provider permanently removed.
    pub purged: Vec<String>,
    pub retained: Vec<RetainedTrashTicket>,
}

/// Permanently delete a provider's Trash tickets.
///
/// `retention_days: None` empties Trash now; `Some(days)` removes only tickets whose last
/// change is at least `days` old. A provider ticket records no separate deletion time, so
/// its `updated_at` stands in for it: that is never earlier than the deletion, so a ticket
/// is never purged before its retention ends. A ticket whose `updated_at` cannot be parsed
/// is kept rather than guessed at, as in the git Trash.
pub fn purge_provider_trash(
    provider: &dyn TicketProvider,
    now: &Timestamp,
    retention_days: Option<i64>,
) -> Result<ProviderTrashPurge, ProviderError> {
    let cutoff = match retention_days {
        Some(days) => match now.plus_minutes(-days * 24 * 60).instant() {
            Some(cutoff) => Some(cutoff),
            None => return Ok(ProviderTrashPurge::default()),
        },
        None => None,
    };
    let trash = provider.query(&TicketQuery {
        status: Some(Status::Deleted),
        ..Default::default()
    })?;
    let mut report = ProviderTrashPurge::default();
    for ticket in trash {
        if ticket.status != Status::Deleted {
            continue;
        }
        if let Some(cutoff) = cutoff {
            let due = Timestamp::new(ticket.updated_at.clone())
                .instant()
                .is_some_and(|changed| changed <= cutoff);
            if !due {
                continue;
            }
        }
        match provider.purge_trashed(&ticket.native_id) {
            Ok(ProviderPurgeOutcome::Purged) => report.purged.push(ticket.slug),
            Ok(ProviderPurgeOutcome::Retained { message }) => {
                report.retained.push(RetainedTrashTicket {
                    qualified_id: ticket.qualified_id,
                    slug: ticket.slug,
                    message,
                });
            }
            Err(ProviderError::Unsupported { .. }) => {
                report.retained.push(RetainedTrashTicket {
                    qualified_id: ticket.qualified_id,
                    slug: ticket.slug,
                    message: PURGE_UNSUPPORTED.into(),
                });
            }
            // One failing ticket must not hide the ones already removed: report it and go
            // on, except under a rate limit, where every later request would fail too.
            Err(error) => {
                let stop = matches!(error, ProviderError::RateLimited { .. });
                report.retained.push(RetainedTrashTicket {
                    qualified_id: ticket.qualified_id,
                    slug: ticket.slug,
                    message: format!("could not permanently delete the ticket: {error}"),
                });
                if stop {
                    break;
                }
            }
        }
    }
    Ok(report)
}
