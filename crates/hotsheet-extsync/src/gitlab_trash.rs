//! GitLab Issues Trash (HS2-DBKM89, GH-5 parity).
//!
//! GitLab has no soft delete, so Delete closes the issue and marks it with provider-owned
//! labels in the same single-colon `key:value` form as `category:`, `priority:`, and
//! `status:`:
//!
//! - `status:deleted` puts the issue in Hot Sheet's Trash; a closed issue with it reads as
//!   `deleted`, never `completed`.
//! - `deleted-from:<status>` keeps the status Restore returns to. GitLab has one closed state
//!   and the adapter maps no close reasons, so `deleted-from:completed` alone records that the
//!   ticket was closed (as Completed) before Delete; no `closed:` label is needed.
//!
//! Permanent deletion uses REST `DELETE /projects/:id/issues/:iid`, which GitLab allows only
//! to project Owners and instance administrators. Anyone else gets 403 and the issue stays
//! closed in Trash; the purge reports it as retained with a provider-neutral reason.

use super::{GitLabIssue, GitLabProvider, mapped_labels, rate_limited};
use hotsheet_model::{Priority, Status};
use hotsheet_ticketing::provider_trash::{PURGE_NOT_PERMITTED, ProviderPurgeOutcome};
use hotsheet_ticketing::{ApiTicket, OpError, ProviderError};
use serde_json::{Value, json};

/// The label that puts a closed issue in Trash.
pub(super) const TRASH_LABEL: &str = "status:deleted";
/// Prefix of the label that keeps the status a trashed issue had before Delete.
pub(super) const DELETED_FROM_PREFIX: &str = "deleted-from:";

fn status_name(status: Status) -> Option<&'static str> {
    match status {
        Status::NotStarted => Some("not-started"),
        Status::Started => Some("started"),
        Status::Backlog => Some("backlog"),
        Status::Completed => Some("completed"),
        Status::Verified => Some("verified"),
        Status::Archive => Some("archived"),
        Status::Deleted | Status::Moved => None,
    }
}

fn status_from_name(name: &str) -> Option<Status> {
    [
        Status::NotStarted,
        Status::Started,
        Status::Backlog,
        Status::Completed,
        Status::Verified,
        Status::Archive,
    ]
    .into_iter()
    .find(|status| status_name(*status) == Some(name))
}

/// Whether a status is a closed GitLab issue.
pub(super) fn closes_issue(status: Status) -> bool {
    matches!(
        status,
        Status::Completed | Status::Verified | Status::Archive | Status::Deleted
    )
}

/// The status a trashed issue had before Delete, from its `deleted-from:` label.
pub(super) fn deleted_from(labels: &[String]) -> Option<Status> {
    labels
        .iter()
        .find_map(|label| label.strip_prefix(DELETED_FROM_PREFIX))
        .and_then(status_from_name)
}

/// Every label of a trashed issue: its ordinary tags and category/priority, `status:deleted`,
/// and the `deleted-from:` label Restore needs.
pub(super) fn trashed_labels(
    category: &str,
    priority: Priority,
    tags: &[String],
    previous: Option<Status>,
) -> Vec<String> {
    let mut labels = mapped_labels(category, priority, tags, None);
    labels.push(TRASH_LABEL.into());
    if let Some(name) = previous.and_then(status_name) {
        labels.push(format!("{DELETED_FROM_PREFIX}{name}"));
    }
    labels.sort();
    labels.dedup();
    labels
}

/// GitLab's refusal text: `message` (`403 Forbidden`), or an OAuth scope error's
/// `error_description`.
fn refusal_detail(body: &str) -> String {
    let value = serde_json::from_str::<Value>(body).ok();
    value
        .as_ref()
        .and_then(|value| {
            value
                .get("message")
                .or_else(|| value.get("error_description"))
                .or_else(|| value.get("error"))
        })
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| body.trim().to_string())
}

impl GitLabProvider {
    fn trashed_issue(&self, native_id: &str) -> Result<(GitLabIssue, ApiTicket), ProviderError> {
        let issue = self.issue(native_id)?;
        let ticket = self.ticket(issue.clone(), vec![]);
        if ticket.status != Status::Deleted {
            return Err(ProviderError::Operation(OpError::NotInTrash(ticket.status)));
        }
        Ok((issue, ticket))
    }

    /// Restore: drop the Trash labels and return the issue to its pre-delete status and
    /// state. An issue trashed without a `deleted-from:` label reopens as Not started.
    pub(super) fn restore_from_trash(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        let (issue, ticket) = self.trashed_issue(native_id)?;
        let previous = deleted_from(&issue.labels).unwrap_or(Status::NotStarted);
        let mut payload = json!({
            "labels": mapped_labels(&ticket.category, ticket.priority, &ticket.tags, Some(previous))
                .join(","),
        });
        // A trashed issue is closed already; only an open prior status reopens it, so a
        // Completed ticket keeps its closed state without a redundant state event.
        if !closes_issue(previous) {
            payload["state_event"] = json!("reopen");
        }
        let response = self.request(
            "PUT",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&payload),
        )?;
        Ok(self.ticket(self.json(response)?, vec![]))
    }

    /// Permanently delete a trashed issue through `DELETE /projects/:id/issues/:iid`. An
    /// issue that is already gone (404 or 410) counts as purged; an issue outside Trash is
    /// refused before any write; a 403 refusal keeps it in Trash as retained.
    pub(super) fn delete_trashed_issue(
        &self,
        native_id: &str,
    ) -> Result<ProviderPurgeOutcome, ProviderError> {
        match self.trashed_issue(native_id) {
            Ok(_) => {}
            Err(ProviderError::NotFound { .. }) => return Ok(ProviderPurgeOutcome::Purged),
            Err(error) => return Err(error),
        }
        let url = self.endpoint(&format!("issues/{native_id}"));
        let response = self.send("DELETE", &url, None)?;
        match response.status {
            200..=299 | 404 | 410 => Ok(ProviderPurgeOutcome::Purged),
            403 if !rate_limited(&response) => Ok(ProviderPurgeOutcome::Retained {
                message: format!(
                    "{PURGE_NOT_PERMITTED} (GitLab said: {})",
                    refusal_detail(&response.body)
                ),
            }),
            _ => self
                .checked(&url, response)
                .map(|_| ProviderPurgeOutcome::Purged),
        }
    }
}

#[cfg(test)]
#[path = "gitlab_trash_tests.rs"]
mod tests;
