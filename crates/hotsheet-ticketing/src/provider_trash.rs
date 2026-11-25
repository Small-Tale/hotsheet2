//! Trash for external ticket providers (GH-5).
//!
//! A provider whose Delete is a restorable Trash (GitHub Issues, and GitLab since HS2-DBKM89:
//! a closed issue carrying `status:deleted`; Jira since HS2-73Z1R8: the `hotsheet-trash`
//! label) implements [`TicketProvider::restore`] and
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
            let due = changed_at(&ticket.updated_at).is_some_and(|changed| changed <= cutoff);
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

/// A provider's last-change time. RFC 3339, or the ISO 8601 basic UTC offset Jira REST v3
/// writes (`2026-11-01T00:00:00.000+0000`), which RFC 3339 parsing alone rejects; without it
/// a Jira Trash ticket would never reach retention (HS2-73Z1R8).
fn changed_at(text: &str) -> Option<time::OffsetDateTime> {
    if let Some(parsed) = Timestamp::new(text.to_string()).instant() {
        return Some(parsed);
    }
    let split = text.len().checked_sub(5)?;
    let (stamp, offset) = text.split_at_checked(split)?;
    let (sign, digits) = offset.split_at_checked(1)?;
    if !matches!(sign, "+" | "-") || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    Timestamp::new(format!("{stamp}{sign}{}:{}", &digits[..2], &digits[2..])).instant()
}

#[cfg(test)]
mod tests {
    use super::changed_at;

    #[test]
    fn hs2_73z1r8_changed_at_reads_rfc3339_and_jira_basic_offsets() {
        let rfc = changed_at("2026-11-01T08:00:00.000+08:00").unwrap();
        assert_eq!(changed_at("2026-11-01T00:00:00Z"), Some(rfc));
        assert_eq!(changed_at("2026-11-01T08:00:00.000+0800"), Some(rfc));
        assert_eq!(changed_at("2026-10-31T22:00:00.000-0200"), Some(rfc));
        for bad in [
            "",
            "+0000",
            "2026-11-01T00:00:00.000 0000",
            "2026-11-01T00:00:00.000+00x0",
            "not a time",
            "2026-11-01T00:00:00.000Zé",
            "2026-11-01T00:00:00.é000",
        ] {
            assert_eq!(changed_at(bad), None, "{bad:?}");
        }
    }
}
