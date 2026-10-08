//! Provisional Jira reads over the durable provider outbox.
//!
//! A pending field edit can change search membership and sort order. Apply it to the
//! complete native result before filtering, limiting, or choosing a keyset page.

use std::collections::{BTreeMap, HashSet};

use hotsheet_ticketing::checkout_order::MergeKey;
use hotsheet_ticketing::provider_outbox::{
    OutboxOperation, ProviderOutbox, project_pending_ticket,
};
use hotsheet_ticketing::{
    ApiTicket, ProviderError, ProviderKeysetPage, TicketProvider, TicketQuery,
    filter_provider_ticket_page, keyset_page_from_rows, unbounded_query,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum OverlayReadError {
    #[error(transparent)]
    Provider(#[from] ProviderError),
    #[error(transparent)]
    Outbox(#[from] hotsheet_ticketing::provider_outbox::OutboxError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

/// Read a full ticket with all admitted edits for its provider-native identity.
/// A durable base snapshot keeps the provisional ticket readable if the provider
/// briefly cannot return that same ticket.
pub fn get(
    provider: &dyn TicketProvider,
    outbox: &ProviderOutbox,
    connection_id: &str,
    native_id: &str,
) -> Result<ApiTicket, OverlayReadError> {
    let pending = outbox.pending_for_ticket(connection_id, native_id)?;
    if pending.is_empty() {
        return Ok(provider.get(native_id)?);
    }
    let remote = match provider.get(native_id) {
        Ok(ticket) => serde_json::to_value(ticket)?,
        Err(_) => pending[0].base_ticket.clone(),
    };
    Ok(serde_json::from_value(project_pending_ticket(
        &remote, &pending,
    )?)?)
}

/// Read a provider list with pending edits applied before the requested filters,
/// search, sort, and limit. Without pending edits the native adapter retains its
/// normal optimized query path and its precise unsupported-filter behavior.
pub fn query(
    provider: &dyn TicketProvider,
    outbox: &ProviderOutbox,
    connection_id: &str,
    query: &TicketQuery,
) -> Result<Vec<ApiTicket>, OverlayReadError> {
    let pending = pending_for_connection(outbox, connection_id)?;
    if pending.is_empty() {
        return Ok(provider.query(query)?);
    }
    projected_rows(provider, query, pending)
}

/// Read a value-keyset page from the projected Jira result. The native resume hint
/// cannot safely skip a row whose pending title or priority changed its ordering,
/// so pending reads use the stable shared keyset helper instead.
pub fn query_after(
    provider: &dyn TicketProvider,
    outbox: &ProviderOutbox,
    connection_id: &str,
    query: &TicketQuery,
    after: Option<&MergeKey>,
    resume: Option<&str>,
    limit: usize,
) -> Result<ProviderKeysetPage, OverlayReadError> {
    let pending = pending_for_connection(outbox, connection_id)?;
    if pending.is_empty() {
        return Ok(provider.query_after(query, after, resume, limit)?);
    }
    let rows = projected_rows(provider, &unbounded_query(query), pending)?;
    Ok(keyset_page_from_rows(rows, query, after, limit))
}

fn pending_for_connection(
    outbox: &ProviderOutbox,
    connection_id: &str,
) -> Result<BTreeMap<String, Vec<OutboxOperation>>, OverlayReadError> {
    let mut pending: BTreeMap<String, Vec<OutboxOperation>> = BTreeMap::new();
    for operation in outbox.list_pending()? {
        if operation.connection_id == connection_id {
            pending
                .entry(operation.native_id.clone())
                .or_default()
                .push(operation);
        }
    }
    Ok(pending)
}

fn projected_rows(
    provider: &dyn TicketProvider,
    query: &TicketQuery,
    mut pending: BTreeMap<String, Vec<OutboxOperation>>,
) -> Result<Vec<ApiTicket>, OverlayReadError> {
    // Jira rejects these filters before contacting its native search endpoint.
    // Preserve that contract when pending rows require an unfiltered native scan.
    if requires_jira_filter_validation(query) {
        let _ = provider.query(query)?;
    }
    let remote = provider.query(&TicketQuery::default())?;
    let mut seen = HashSet::new();
    let mut rows = Vec::with_capacity(remote.len() + pending.len());
    for ticket in remote {
        seen.insert(ticket.native_id.clone());
        if let Some(operations) = pending.remove(&ticket.native_id) {
            let value = project_pending_ticket(&serde_json::to_value(ticket)?, &operations)?;
            rows.push(serde_json::from_value(value)?);
        } else {
            rows.push(ticket);
        }
    }
    // A native list can lag admission or omit an issue temporarily. The admission
    // snapshot is durable, so its pending projection still participates in search.
    for (native_id, operations) in pending {
        if seen.contains(&native_id) {
            continue;
        }
        let value = project_pending_ticket(&operations[0].base_ticket, &operations)?;
        rows.push(serde_json::from_value(value)?);
    }
    Ok(filter_provider_ticket_page(rows, query))
}

fn requires_jira_filter_validation(query: &TicketQuery) -> bool {
    query.review_requested.is_some()
        || query.review_by.is_some()
        || query.claimed.is_some()
        || query.blocked.is_some()
        || query.page_after.is_some()
        || query.up_next_only
        || query.close_reason.is_some()
        || query.completed_after.is_some()
        || query.min_confidence.is_some()
        || query.max_confidence.is_some()
        || query.completed_before.is_some()
        || query.verified_after.is_some()
        || query.verified_before.is_some()
        || query.has_attachment.is_some()
        || query.has_media_annotation.is_some()
        || !query.attachment_patterns.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_model::{Priority, Status, Timestamp, Ulid};
    use hotsheet_ticketing::provider_outbox::OutboxAdmission;
    use hotsheet_ticketing::{
        FsStore, GitProvider, MutationContext, ProviderDraft, ProviderPatch, SortKey, StoreMetadata,
    };

    fn create(provider: &GitProvider, title: &str) -> ApiTicket {
        provider
            .create(
                MutationContext {
                    now: Timestamp::new("2026-10-08T12:00:00Z"),
                    generated_id: Ulid::new(),
                },
                ProviderDraft {
                    title: title.into(),
                    details: "original details".into(),
                    category: "task".into(),
                    priority: Priority::Default,
                    status: Status::Started,
                    tags: vec!["old".into()],
                    up_next: false,
                    blocked_by: vec![],
                    transfer: None,
                },
            )
            .unwrap()
    }

    #[test]
    fn projects_ordered_edits_into_full_search_and_keyset_reads() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("tickets"), &StoreMetadata::new("HS")).unwrap();
        let provider = GitProvider::new("jira-test", store);
        let first = create(&provider, "Alpha issue");
        let second = create(&provider, "Beta issue");
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        outbox
            .admit_batch(&[
                OutboxAdmission::new(
                    "op-1",
                    &first,
                    ProviderPatch {
                        title: Some("Zeta issue".into()),
                        details: Some("needle description".into()),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
                OutboxAdmission::new(
                    "op-2",
                    &first,
                    ProviderPatch {
                        priority: Some(Priority::High),
                        tags: Some(vec!["pending".into()]),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
            ])
            .unwrap();

        let full = get(&provider, &outbox, "jira-test", &first.native_id).unwrap();
        assert_eq!(full.title, "Zeta issue");
        assert_eq!(full.details, "needle description");
        assert_eq!(full.priority, Priority::High);
        assert_eq!(full.tags, vec!["pending".to_string()]);
        assert_eq!(
            full.pending_operation_ids,
            vec!["op-1".to_string(), "op-2".to_string()]
        );
        assert_eq!(full.concurrency_token, first.concurrency_token);
        assert_eq!(
            get(&provider, &outbox, "jira-test", &second.native_id)
                .unwrap()
                .pending_operation_ids,
            Vec::<String>::new()
        );

        let found = query(
            &provider,
            &outbox,
            "jira-test",
            &TicketQuery {
                text: Some("needle".into()),
                ..TicketQuery::default()
            },
        )
        .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].native_id, first.native_id);
        assert!(
            query(
                &provider,
                &outbox,
                "jira-test",
                &TicketQuery {
                    text: Some("Alpha".into()),
                    ..TicketQuery::default()
                },
            )
            .unwrap()
            .is_empty()
        );

        let by_title = TicketQuery {
            sort: SortKey::Title,
            ..TicketQuery::default()
        };
        let page = query_after(&provider, &outbox, "jira-test", &by_title, None, None, 1).unwrap();
        assert_eq!(page.items[0].ticket.native_id, second.native_id);
        assert!(!page.exhausted);
        let after = MergeKey::from_ticket(&page.items[0].ticket);
        let next = query_after(
            &provider,
            &outbox,
            "jira-test",
            &by_title,
            Some(&after),
            None,
            1,
        )
        .unwrap();
        assert_eq!(next.items[0].ticket.native_id, first.native_id);
        assert!(next.exhausted);
    }

    #[test]
    fn no_pending_edit_uses_native_provider_query() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsStore::init(dir.path().join("tickets"), &StoreMetadata::new("HS")).unwrap();
        let provider = GitProvider::new("jira-test", store);
        let ticket = create(&provider, "Original issue");
        let outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        assert_eq!(
            query(&provider, &outbox, "jira-test", &TicketQuery::default())
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            get(&provider, &outbox, "jira-test", &ticket.native_id)
                .unwrap()
                .title,
            "Original issue"
        );
    }

    #[test]
    fn durable_snapshot_keeps_a_pending_ticket_visible_when_native_read_is_stale() {
        let dir = tempfile::tempdir().unwrap();
        let source_store =
            FsStore::init(dir.path().join("source"), &StoreMetadata::new("HS")).unwrap();
        let source = GitProvider::new("jira-test", source_store);
        let ticket = create(&source, "Old title");
        let stale_store =
            FsStore::init(dir.path().join("stale"), &StoreMetadata::new("HS")).unwrap();
        let stale = GitProvider::new("jira-test", stale_store);
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        outbox
            .admit_batch(&[OutboxAdmission::new(
                "pending-1",
                &ticket,
                ProviderPatch {
                    title: Some("New title".into()),
                    ..ProviderPatch::default()
                },
            )
            .unwrap()])
            .unwrap();

        let full = get(&stale, &outbox, "jira-test", &ticket.native_id).unwrap();
        assert_eq!(full.title, "New title");
        assert_eq!(full.pending_operation_ids, ["pending-1"]);
        let rows = query(
            &stale,
            &outbox,
            "jira-test",
            &TicketQuery {
                text: Some("New".into()),
                ..TicketQuery::default()
            },
        )
        .unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].native_id, ticket.native_id);
    }
}
