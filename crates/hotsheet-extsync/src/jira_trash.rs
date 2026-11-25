//! Jira Trash (HS2-73Z1R8, GH-5 parity).
//!
//! Jira has no soft delete, and the adapter maps no workflow transitions: workflows are
//! project-specific, and many cannot return an issue from a done status to the exact status
//! it had before. A Delete that transitioned the issue could therefore not promise an exact
//! Restore. Trash is instead a single provider-owned label, [`TRASH_LABEL`], and nothing
//! else changes:
//!
//! - Delete adds `hotsheet-trash`; the issue keeps its workflow status, so Restore, which
//!   removes the label, returns the ticket to exactly the status it had. Any issue carrying
//!   the label reads as `deleted` and appears only in Trash. Because no status changes, no
//!   deleted-from marker is needed.
//! - The label has no space or colon, so it is valid in every Jira label configuration, and
//!   it never appears among the ticket's tags.
//!
//! Permanent deletion uses REST `DELETE /rest/api/3/issue/{key}`, which needs the project's
//! Delete Issues permission. Without it Jira answers 403, and the issue stays in Trash; the
//! purge reports it as retained with a provider-neutral reason. Subtasks are never deleted
//! along with their parent (`deleteSubtasks` stays false): Jira answers 400 for a parent
//! with subtasks, and that issue is retained too.

use super::{JiraIssue, JiraProvider, response_message, validate_key};
use hotsheet_ticketing::provider_trash::{PURGE_NOT_PERMITTED, ProviderPurgeOutcome};
use hotsheet_ticketing::{ApiTicket, OpError, ProviderError, TicketProvider};
use serde_json::json;

/// The label that puts a Jira issue in Trash.
pub(super) const TRASH_LABEL: &str = "hotsheet-trash";

/// The provider-neutral explanation for a Trash ticket the provider keeps because it still
/// has child tickets (Jira subtasks) that a purge must not delete along with it.
pub(crate) const PURGE_HAS_CHILDREN: &str = "the provider will not permanently delete this ticket while it has child tickets, so it stays in Trash; delete or move its child tickets first, or delete it in the provider";

/// Whether an issue's labels put it in Trash.
pub(super) fn is_trashed(labels: &[String]) -> bool {
    labels.iter().any(|label| label == TRASH_LABEL)
}

/// The labels to write for an issue: its tags without any stray Trash label, plus the
/// Trash label when the issue is (or stays) in Trash. A tag can therefore never move an
/// issue in or out of Trash on its own.
pub(super) fn labels_with_trash(tags: Vec<String>, trashed: bool) -> Vec<String> {
    let mut labels = tags
        .into_iter()
        .filter(|label| label != TRASH_LABEL)
        .collect::<Vec<_>>();
    if trashed {
        labels.push(TRASH_LABEL.into());
    }
    labels
}

impl JiraProvider {
    fn trashed_issue(&self, native_id: &str) -> Result<JiraIssue, ProviderError> {
        let issue = self.issue(native_id)?;
        if !is_trashed(&issue.fields.labels) {
            let status = self.ticket(issue, vec![]).status;
            return Err(ProviderError::Operation(OpError::NotInTrash(status)));
        }
        Ok(issue)
    }

    /// Restore: remove the Trash label with Jira's label `remove` operation, which leaves
    /// any label added concurrently in place. The workflow status was never changed, so the
    /// ticket returns to exactly the status it had before Delete.
    pub(super) fn restore_from_trash(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        self.trashed_issue(native_id)?;
        self.request(
            "PUT",
            &self.endpoint(&format!("issue/{native_id}")),
            Some(&json!({"update": {"labels": [{"remove": TRASH_LABEL}]}})),
        )?;
        self.get(native_id)
    }

    /// Permanently delete a trashed issue through `DELETE /rest/api/3/issue/{key}`. An
    /// issue that is already gone (404) counts as purged; an issue outside Trash is refused
    /// before any write; a 403 (no Delete Issues permission) or 400 (the issue has subtasks)
    /// keeps it in Trash as retained.
    pub(super) fn delete_trashed_issue(
        &self,
        native_id: &str,
    ) -> Result<ProviderPurgeOutcome, ProviderError> {
        validate_key(native_id)?;
        match self.trashed_issue(native_id) {
            Ok(_) => {}
            Err(ProviderError::NotFound { .. }) => return Ok(ProviderPurgeOutcome::Purged),
            Err(error) => return Err(error),
        }
        let url = self.endpoint(&format!("issue/{native_id}"));
        let response = self.send("DELETE", &url, None)?;
        match response.status {
            200..=299 | 404 => Ok(ProviderPurgeOutcome::Purged),
            403 => Ok(ProviderPurgeOutcome::Retained {
                message: format!(
                    "{PURGE_NOT_PERMITTED} (Jira said: {})",
                    response_message(&response.body)
                ),
            }),
            400 => Ok(ProviderPurgeOutcome::Retained {
                message: format!(
                    "{PURGE_HAS_CHILDREN} (Jira said: {})",
                    response_message(&response.body)
                ),
            }),
            _ => self
                .checked(&url, response)
                .map(|_| ProviderPurgeOutcome::Purged),
        }
    }
}

#[cfg(test)]
#[path = "jira_trash_tests.rs"]
mod tests;
