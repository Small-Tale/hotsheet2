//! GitHub Issues Trash (GH-5).
//!
//! GitHub has no soft delete, so Delete closes the issue as `not_planned` and marks it with
//! provider-owned labels:
//!
//! - `status:deleted` puts the issue in Hot Sheet's Trash; a closed issue with it reads as
//!   `deleted`, never `completed`.
//! - `deleted-from:<status>` keeps the status Restore returns to.
//! - `closed:<reason>` keeps the previous close reason, including `closed:completed` and
//!   `closed:not-planned`, because Trash overwrites GitHub's own `state_reason`.
//!
//! Permanent deletion uses GraphQL `deleteIssue`, which GitHub allows only to repository
//! admins (and maintainers where the organization permits it). The REST API cannot delete an
//! issue. Without that permission the issue stays closed in Trash and the purge says so.

use super::{GitHubProvider, closes_issue, github_message, github_state_reason, mapped_labels};
use hotsheet_model::{CloseReason, Priority, Status};
use hotsheet_sync::LockExt;
use hotsheet_ticketing::provider_trash::{PURGE_NOT_PERMITTED, ProviderPurgeOutcome};
use hotsheet_ticketing::{ApiTicket, OpError, ProviderError};
use serde_json::{Value, json};

/// The label that puts a closed issue in Trash.
pub(super) const TRASH_LABEL: &str = "status:deleted";
/// Prefix of the label that keeps the status a trashed issue had before Delete.
pub(super) const DELETED_FROM_PREFIX: &str = "deleted-from:";
/// Trash is "closed without a change", which GitHub calls `not_planned`.
pub(super) const TRASH_STATE_REASON: &str = "not_planned";

const DELETE_ISSUE_MUTATION: &str =
    "mutation($id: ID!) { deleteIssue(input: {issueId: $id}) { clientMutationId } }";

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

/// The status a trashed issue had before Delete, from its `deleted-from:` label.
pub(super) fn deleted_from(labels: &[String]) -> Option<Status> {
    labels
        .iter()
        .find_map(|label| label.strip_prefix(DELETED_FROM_PREFIX))
        .and_then(status_from_name)
}

/// Trash's explicit `closed:` label. Unlike a closed issue, a trashed one cannot fall back
/// on GitHub's `state_reason`, so Completed and Not planned are labelled too.
fn trash_reason_label(reason: CloseReason) -> &'static str {
    match reason {
        CloseReason::Completed => "closed:completed",
        CloseReason::NotPlanned => "closed:not-planned",
        CloseReason::Duplicate => "closed:duplicate",
        CloseReason::Obsolete => "closed:obsolete",
        CloseReason::WorksAsDesigned => "closed:works-as-designed",
    }
}

/// Every label of a trashed issue: its ordinary tags and category/priority, `status:deleted`,
/// and what Restore needs. Up Next never survives Delete.
pub(super) fn trashed_labels(
    category: &str,
    priority: Priority,
    tags: &[String],
    previous: Option<Status>,
    previous_reason: Option<CloseReason>,
    duplicate_of: Option<&str>,
) -> Vec<String> {
    let mut labels = mapped_labels(
        category,
        priority,
        tags,
        Some(Status::Deleted),
        false,
        None,
        duplicate_of,
    );
    if let Some(name) = previous.and_then(status_name) {
        labels.push(format!("{DELETED_FROM_PREFIX}{name}"));
    }
    if let Some(reason) = previous_reason {
        labels.push(trash_reason_label(reason).into());
    }
    labels.sort();
    labels.dedup();
    labels
}

/// Whether a GitHub refusal means "this credential may not delete issues" rather than a
/// broken or expired credential.
fn is_permission_refusal(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("not accessible by integration")
        || message.contains("correct permissions")
        || message.contains("permission")
        || message.contains("must have admin")
        || message.contains("forbidden")
}

fn retained(detail: &str) -> ProviderPurgeOutcome {
    ProviderPurgeOutcome::Retained {
        message: format!("{PURGE_NOT_PERMITTED} (GitHub said: {detail})"),
    }
}

impl GitHubProvider {
    fn trashed_issue(
        &self,
        native_id: &str,
    ) -> Result<(super::GitHubIssue, ApiTicket), ProviderError> {
        let issue = self.issue(native_id)?;
        let ticket = self.api_ticket(issue.clone(), vec![]);
        if ticket.status != Status::Deleted {
            return Err(ProviderError::Operation(OpError::NotInTrash(ticket.status)));
        }
        Ok((issue, ticket))
    }

    /// Restore: drop the Trash labels and return the issue to its pre-delete status and
    /// close reason. An issue trashed without a `deleted-from:` label reopens as Not started.
    pub(super) fn restore_from_trash(&self, native_id: &str) -> Result<ApiTicket, ProviderError> {
        let (issue, ticket) = self.trashed_issue(native_id)?;
        let labels = issue
            .labels
            .iter()
            .map(|label| label.name.clone())
            .collect::<Vec<_>>();
        let previous = deleted_from(&labels).unwrap_or(Status::NotStarted);
        let closed = closes_issue(previous);
        let reason = closed.then(|| ticket.close_reason.unwrap_or(CloseReason::Completed));
        let duplicate_of = if closed {
            ticket.duplicate_of.clone()
        } else {
            None
        };
        let mut payload = json!({
            "state": if closed { "closed" } else { "open" },
            "labels": mapped_labels(
                &ticket.category,
                ticket.priority,
                &ticket.tags,
                Some(previous),
                false,
                reason,
                duplicate_of.as_deref(),
            ),
        });
        if let Some(reason) = reason {
            payload["state_reason"] = json!(github_state_reason(reason));
        }
        let response = self.request(
            "PATCH",
            &self.endpoint(&format!("issues/{native_id}")),
            Some(&payload),
        )?;
        let issue: super::GitHubIssue = self.json(response)?;
        Ok(self.api_ticket(issue, vec![]))
    }

    /// Permanently delete a trashed issue through GraphQL `deleteIssue`. An issue that is
    /// already gone counts as purged; an issue outside Trash is refused before any write.
    pub(super) fn delete_trashed_issue(
        &self,
        native_id: &str,
    ) -> Result<ProviderPurgeOutcome, ProviderError> {
        let issue = match self.trashed_issue(native_id) {
            Ok((issue, _)) => issue,
            Err(ProviderError::NotFound { .. }) => return Ok(ProviderPurgeOutcome::Purged),
            Err(error) => return Err(error),
        };
        let node_id = issue.node_id.ok_or_else(|| ProviderError::Conflict {
            ticket: format!("{}:{native_id}", self.config.connection_id),
            message: "GitHub did not return the issue's GraphQL id".into(),
        })?;
        let response = match self.request(
            "POST",
            &self.graphql_endpoint(),
            Some(&json!({
                "query": DELETE_ISSUE_MUTATION,
                "variables": { "id": node_id },
            })),
        ) {
            Ok(response) => response,
            Err(ProviderError::Authentication { message, .. })
                if is_permission_refusal(&message) =>
            {
                return Ok(retained(&message));
            }
            Err(error) => return Err(error),
        };
        let value: Value =
            serde_json::from_str(&response.body).map_err(|error| ProviderError::Conflict {
                ticket: self.config.connection_id.clone(),
                message: format!("invalid GitHub response: {error}"),
            })?;
        if let Some(errors) = value.get("errors").and_then(Value::as_array)
            && !errors.is_empty()
        {
            let kind = |error: &Value| {
                error
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string()
            };
            let messages = errors
                .iter()
                .map(|error| {
                    error
                        .get("message")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                        .unwrap_or_else(|| github_message(&error.to_string()))
                })
                .collect::<Vec<_>>()
                .join("; ");
            if errors.iter().all(|error| kind(error) == "NOT_FOUND") {
                self.forget_issue_list();
                return Ok(ProviderPurgeOutcome::Purged);
            }
            if errors.iter().any(|error| kind(error) == "FORBIDDEN")
                || is_permission_refusal(&messages)
            {
                return Ok(retained(&messages));
            }
            return Err(ProviderError::Conflict {
                ticket: format!("{}:{native_id}", self.config.connection_id),
                message: format!("GitHub refused to delete the issue: {messages}"),
            });
        }
        if value
            .pointer("/data/deleteIssue")
            .is_none_or(Value::is_null)
        {
            return Err(ProviderError::Conflict {
                ticket: format!("{}:{native_id}", self.config.connection_id),
                message: "GitHub did not confirm the issue was deleted".into(),
            });
        }
        // GraphQL writes skip the REST cache invalidation in `request`.
        self.forget_issue_list();
        Ok(ProviderPurgeOutcome::Purged)
    }

    fn forget_issue_list(&self) {
        if let Some(cache) = &self.issues_cache {
            *cache.lock_or_recover() = None;
        }
    }
}

#[cfg(test)]
#[path = "github_trash_tests.rs"]
mod tests;
