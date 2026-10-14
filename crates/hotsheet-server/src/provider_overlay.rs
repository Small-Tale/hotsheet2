//! Provisional Jira reads over the durable provider outbox.
//!
//! A pending field edit can change search membership and sort order. Apply it to the
//! projected row before filtering, limiting, or choosing a keyset page.

use std::collections::BTreeMap;

use hotsheet_ticketing::SortKey;
use hotsheet_ticketing::checkout_order::{self, MergeKey};
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

const NATIVE_SCAN_PAGE: usize = 100;
const MAX_PENDING_NATIVE_SCAN: usize = 100_000;
/// Above this many pending issues one full scan is cheaper than per-issue reads.
const MAX_PENDING_POINT_READS: usize = 50;

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
/// search, sort, and limit.
///
/// HS2-5XXM4P: membership of an issue without pending edits is decided by remote
/// state alone, so the native (JQL-pushdown) query answers it exactly as the
/// no-pending path does. Only pending issues need provisional treatment: they are
/// dropped from the native result, re-read individually, projected, and filtered
/// locally. Network pages therefore scale with the matching result instead of the
/// whole project. A large pending set falls back to one bounded full scan.
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
    if let Some(limit) = query.limit {
        return Ok(projected_page(provider, query, pending, None, limit)?
            .items
            .into_iter()
            .map(|item| item.ticket)
            .collect());
    }
    if pending.len() > MAX_PENDING_POINT_READS {
        return projected_rows_with_limit(provider, query, pending, MAX_PENDING_NATIVE_SCAN);
    }
    let mut rows = provider.query(&unbounded_query(query))?;
    rows.retain(|ticket| !pending.contains_key(&ticket.native_id));
    let filter_query = unbounded_query(query);
    for ticket in projected_pending(provider, pending)? {
        rows.extend(filter_provider_ticket_page(vec![ticket], &filter_query));
    }
    Ok(filter_provider_ticket_page(rows, query))
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
    projected_page(provider, query, pending, after, limit)
}

/// Native page widened by the pending count: after dropping at most `P` pending
/// rows, the first `limit + 1` remaining rows are exactly the first `limit + 1`
/// non-pending rows after `after`, which then merge with the projected pending
/// candidates. One native keyset read replaces a scan of the whole project.
fn projected_page(
    provider: &dyn TicketProvider,
    query: &TicketQuery,
    pending: BTreeMap<String, Vec<OutboxOperation>>,
    after: Option<&MergeKey>,
    limit: usize,
) -> Result<ProviderKeysetPage, OverlayReadError> {
    if pending.len() > MAX_PENDING_POINT_READS {
        return bounded_projected_page(
            provider,
            query,
            pending,
            after,
            limit,
            MAX_PENDING_NATIVE_SCAN,
        );
    }
    let native_query = unbounded_query(query);
    let native = provider.query_after(
        &native_query,
        after,
        None,
        limit.saturating_add(pending.len()).saturating_add(1),
    )?;
    let filter_query = unbounded_query(query);
    let mut candidates = Vec::with_capacity(limit.saturating_add(1).min(501));
    for item in native.items {
        if !pending.contains_key(&item.ticket.native_id) {
            retain_candidate(&mut candidates, item.ticket, &filter_query, after, limit);
        }
    }
    for ticket in projected_pending(provider, pending)? {
        retain_candidate(&mut candidates, ticket, &filter_query, after, limit);
    }
    Ok(keyset_page_from_rows(candidates, query, after, limit))
}

/// Current remote state for each pending issue with its queued edits applied. The
/// durable admission snapshot stands in when the provider cannot return the issue.
fn projected_pending(
    provider: &dyn TicketProvider,
    pending: BTreeMap<String, Vec<OutboxOperation>>,
) -> Result<Vec<ApiTicket>, OverlayReadError> {
    let mut rows = Vec::with_capacity(pending.len());
    for (native_id, operations) in pending {
        let remote = match provider.get(&native_id) {
            Ok(mut ticket) => {
                // List rows carry no comment bodies; keep the list contract uniform.
                ticket.notes.clear();
                serde_json::to_value(ticket)?
            }
            Err(_) => operations[0].base_ticket.clone(),
        };
        rows.push(serde_json::from_value(project_pending_ticket(
            &remote,
            &operations,
        )?)?);
    }
    Ok(rows)
}

/// Fallback full scan for an unusually large pending set, where per-issue reads
/// would cost more than paging the project once.
fn bounded_projected_page(
    provider: &dyn TicketProvider,
    query: &TicketQuery,
    mut pending: BTreeMap<String, Vec<OutboxOperation>>,
    after: Option<&MergeKey>,
    limit: usize,
    max_native_scan: usize,
) -> Result<ProviderKeysetPage, OverlayReadError> {
    if requires_jira_filter_validation(query) {
        let _ = provider.query(query)?;
    }
    let scan_query = TicketQuery {
        sort: SortKey::Updated,
        ..TicketQuery::default()
    };
    let filter_query = unbounded_query(query);
    let mut cursor = None;
    let mut scanned = 0usize;
    let mut candidates = Vec::with_capacity(limit.saturating_add(1).min(501));
    loop {
        let page = provider.query_page(&scan_query, cursor.as_deref(), NATIVE_SCAN_PAGE)?;
        scanned = scanned.saturating_add(page.items.len());
        if scanned > max_native_scan {
            return Err(ProviderError::Conflict {
                ticket: provider.descriptor().connection_id,
                message: format!(
                    "pending Jira overlay scan exceeded {max_native_scan} issues; wait for pending edits to settle"
                ),
            }
            .into());
        }
        for ticket in page.items {
            let projected = if let Some(operations) = pending.remove(&ticket.native_id) {
                serde_json::from_value(project_pending_ticket(
                    &serde_json::to_value(ticket)?,
                    &operations,
                )?)?
            } else {
                ticket
            };
            retain_candidate(&mut candidates, projected, &filter_query, after, limit);
        }
        let Some(next) = page.next_cursor else { break };
        if cursor.as_deref() == Some(next.as_str()) {
            return Err(ProviderError::Conflict {
                ticket: provider.descriptor().connection_id,
                message: "pending Jira overlay scan did not advance".into(),
            }
            .into());
        }
        cursor = Some(next);
    }
    for operations in pending.into_values() {
        let projected = serde_json::from_value(project_pending_ticket(
            &operations[0].base_ticket,
            &operations,
        )?)?;
        retain_candidate(&mut candidates, projected, &filter_query, after, limit);
    }
    Ok(keyset_page_from_rows(candidates, query, after, limit))
}

fn retain_candidate(
    candidates: &mut Vec<ApiTicket>,
    ticket: ApiTicket,
    query: &TicketQuery,
    after: Option<&MergeKey>,
    limit: usize,
) {
    let Some(ticket) = filter_provider_ticket_page(vec![ticket], query).pop() else {
        return;
    };
    let key = MergeKey::from_ticket(&ticket);
    if after.is_some_and(|after| {
        !checkout_order::compare(&key, after, query.sort, query.descending).is_gt()
    }) {
        return;
    }
    let position = candidates.partition_point(|existing| {
        checkout_order::compare(
            &MergeKey::from_ticket(existing),
            &key,
            query.sort,
            query.descending,
        )
        .is_lt()
    });
    if position <= limit {
        candidates.insert(position, ticket);
        candidates.truncate(limit.saturating_add(1));
    }
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

fn projected_rows_with_limit(
    provider: &dyn TicketProvider,
    query: &TicketQuery,
    mut pending: BTreeMap<String, Vec<OutboxOperation>>,
    max_native_scan: usize,
) -> Result<Vec<ApiTicket>, OverlayReadError> {
    // Jira rejects these filters before contacting its native search endpoint.
    // Preserve that contract when pending rows require an unfiltered native scan.
    if requires_jira_filter_validation(query) {
        let _ = provider.query(query)?;
    }
    let scan_query = TicketQuery::default();
    let filter_query = unbounded_query(query);
    let mut rows = Vec::new();
    let mut cursor = None;
    let mut scanned = 0usize;
    loop {
        let page = provider.query_page(&scan_query, cursor.as_deref(), NATIVE_SCAN_PAGE)?;
        scanned = scanned.saturating_add(page.items.len());
        if scanned > max_native_scan {
            return Err(ProviderError::Conflict {
                ticket: provider.descriptor().connection_id,
                message: format!(
                    "pending Jira overlay scan exceeded {max_native_scan} issues; wait for pending edits to settle"
                ),
            }
            .into());
        }
        let mut projected_page = Vec::with_capacity(page.items.len());
        for ticket in page.items {
            let projected = if let Some(operations) = pending.remove(&ticket.native_id) {
                serde_json::from_value(project_pending_ticket(
                    &serde_json::to_value(ticket)?,
                    &operations,
                )?)?
            } else {
                ticket
            };
            projected_page.push(projected);
        }
        rows.extend(filter_provider_ticket_page(projected_page, &filter_query));
        let Some(next) = page.next_cursor else { break };
        if cursor.as_deref() == Some(next.as_str()) {
            return Err(ProviderError::Conflict {
                ticket: provider.descriptor().connection_id,
                message: "pending Jira overlay scan did not advance".into(),
            }
            .into());
        }
        cursor = Some(next);
    }
    // A native list can lag admission or omit an issue temporarily. The admission
    // snapshot is durable, so its pending projection still participates in search.
    for operations in pending.into_values() {
        let value = project_pending_ticket(&operations[0].base_ticket, &operations)?;
        rows.extend(filter_provider_ticket_page(
            vec![serde_json::from_value(value)?],
            &filter_query,
        ));
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
    use hotsheet_extsync::{GitHubTransport, HttpResponse, JiraConfig, JiraProvider};
    use hotsheet_model::{Priority, Status, Timestamp, Ulid};
    use hotsheet_ticketing::provider_outbox::OutboxAdmission;
    use hotsheet_ticketing::{
        FsStore, GitProvider, MutationContext, ProviderDraft, ProviderPatch, SortKey, StoreMetadata,
    };
    use serde_json::{Value, json};
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct GeneratedJira {
        count: usize,
        search_calls: AtomicUsize,
        get_calls: AtomicUsize,
    }

    impl GitHubTransport for GeneratedJira {
        fn request(
            &self,
            method: &str,
            url: &str,
            _: &[(&str, String)],
            body: Option<&Value>,
        ) -> Result<HttpResponse, String> {
            let issue = |index: usize| {
                json!({
                    "key":format!("ENG-{index:06}"),
                    "fields":{
                        "summary":format!("Issue {index:06}"),"description":null,
                        "status":{"statusCategory":{"key":"new"}},
                        "priority":{"name":"Medium"},"issuetype":{"name":"Task"},
                        "labels":if index % 100 == 7 { vec!["hot"] } else { vec![] },
                        "assignee":null,
                        "created":"2026-10-01T00:00:00Z","updated":"2026-10-01T00:00:00Z",
                        "resolutiondate":null
                    }
                })
            };
            let response = if method == "POST" && url.ends_with("search/jql") {
                self.search_calls.fetch_add(1, Ordering::Relaxed);
                let body = body.unwrap();
                let start = body["nextPageToken"]
                    .as_str()
                    .unwrap_or("0")
                    .parse::<usize>()
                    .unwrap();
                // Honor the pushed-down label clause like Jira does.
                let matching = (0..self.count)
                    .filter(|index| {
                        !body["jql"].as_str().unwrap().contains("labels = \"hot\"")
                            || index % 100 == 7
                    })
                    .collect::<Vec<_>>();
                let end =
                    (start + body["maxResults"].as_u64().unwrap() as usize).min(matching.len());
                json!({
                    "issues":matching[start..end].iter().copied().map(issue).collect::<Vec<_>>(),
                    "isLast":end == matching.len(),
                    "nextPageToken":(end < matching.len()).then(||end.to_string()),
                })
            } else if method == "GET" && url.contains("/comment?") {
                json!({"comments":[],"total":0})
            } else if method == "GET" && url.contains("/issue/ENG-") {
                self.get_calls.fetch_add(1, Ordering::Relaxed);
                let index = url.rsplit('-').next().unwrap().parse::<usize>().unwrap();
                issue(index)
            } else {
                return Err(format!("unexpected Jira request {method} {url}"));
            };
            Ok(HttpResponse {
                status: 200,
                headers: HashMap::new(),
                body: response.to_string(),
            })
        }
    }

    fn generated_jira(count: usize) -> (JiraProvider, Arc<GeneratedJira>) {
        let transport = Arc::new(GeneratedJira {
            count,
            search_calls: AtomicUsize::new(0),
            get_calls: AtomicUsize::new(0),
        });
        let provider = JiraProvider::new(
            JiraConfig {
                connection_id: "jira-test".into(),
                project_key: "ENG".into(),
                base_url: "https://jira.test".into(),
                email: "dev@example.com".into(),
                token: "fixture".into(),
                default: false,
            },
            transport.clone(),
        );
        (provider, transport)
    }

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
    fn paged_overlay_scans_fixed_native_pages_and_moves_pending_rows_across_filters() {
        let (provider, transport) = generated_jira(250);
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        let first = provider.get("ENG-000000").unwrap();
        let last = provider.get("ENG-000249").unwrap();
        let missing = provider.get("ENG-000300").unwrap();
        outbox
            .admit_batch(&[
                OutboxAdmission::new(
                    "first-moves-out",
                    &first,
                    ProviderPatch {
                        title: Some("Zzz moved".into()),
                        tags: Some(vec!["pending".into()]),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
                OutboxAdmission::new(
                    "last-moves-in",
                    &last,
                    ProviderPatch {
                        title: Some("Aaa moved".into()),
                        tags: Some(vec!["pending".into()]),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
                OutboxAdmission::new(
                    "missing-native",
                    &missing,
                    ProviderPatch {
                        title: Some("Mmm pending".into()),
                        tags: Some(vec!["pending".into()]),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
            ])
            .unwrap();
        let by_title = TicketQuery {
            sort: SortKey::Title,
            tags: vec!["pending".into()],
            ..TicketQuery::default()
        };
        let first_page =
            query_after(&provider, &outbox, "jira-test", &by_title, None, None, 1).unwrap();
        assert_eq!(first_page.items[0].ticket.native_id, last.native_id);
        assert!(!first_page.exhausted);
        let after = MergeKey::from_ticket(&first_page.items[0].ticket);
        let second_page = query_after(
            &provider,
            &outbox,
            "jira-test",
            &by_title,
            Some(&after),
            None,
            1,
        )
        .unwrap();
        assert_eq!(second_page.items[0].ticket.native_id, missing.native_id);
        assert!(!second_page.exhausted);
        let third_page = query_after(
            &provider,
            &outbox,
            "jira-test",
            &by_title,
            Some(&MergeKey::from_ticket(&second_page.items[0].ticket)),
            None,
            1,
        )
        .unwrap();
        assert_eq!(third_page.items[0].ticket.native_id, first.native_id);
        assert!(third_page.exhausted);
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 9);

        let only_moved = TicketQuery {
            text: Some("Aaa moved".into()),
            limit: Some(1),
            ..TicketQuery::default()
        };
        assert_eq!(
            query(&provider, &outbox, "jira-test", &only_moved).unwrap()[0].native_id,
            last.native_id
        );
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 12);
        let oversized = bounded_projected_page(
            &provider,
            &by_title,
            pending_for_connection(&outbox, "jira-test").unwrap(),
            None,
            1,
            200,
        );
        assert!(matches!(
            oversized,
            Err(OverlayReadError::Provider(ProviderError::Conflict { .. }))
        ));
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 15);
    }

    #[test]
    fn unpaged_overlay_filters_each_native_page_and_keeps_missing_pending_rows() {
        let (provider, transport) = generated_jira(250);
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        let native = provider.get("ENG-000249").unwrap();
        let missing = provider.get("ENG-000300").unwrap();
        outbox
            .admit_batch(&[
                OutboxAdmission::new(
                    "native-enters",
                    &native,
                    ProviderPatch {
                        title: Some("Matching native".into()),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
                OutboxAdmission::new(
                    "missing-enters",
                    &missing,
                    ProviderPatch {
                        title: Some("Matching missing".into()),
                        ..ProviderPatch::default()
                    },
                )
                .unwrap(),
            ])
            .unwrap();

        let rows = query(
            &provider,
            &outbox,
            "jira-test",
            &TicketQuery {
                text: Some("Matching".into()),
                sort: SortKey::Title,
                ..TicketQuery::default()
            },
        )
        .unwrap();
        assert_eq!(
            rows.iter()
                .map(|row| row.native_id.as_str())
                .collect::<Vec<_>>(),
            ["ENG-000300", "ENG-000249"]
        );
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 3);
    }

    fn tag_patch(id: &str, ticket: &ApiTicket, tags: &[&str]) -> OutboxAdmission {
        OutboxAdmission::new(
            id,
            ticket,
            ProviderPatch {
                tags: Some(tags.iter().map(|tag| (*tag).to_string()).collect()),
                ..ProviderPatch::default()
            },
        )
        .unwrap()
    }

    /// HS2-5XXM4P: a pushed-down label filter reads only matching native pages while
    /// pending edits still move issues into and out of the result.
    #[test]
    fn pushdown_overlay_reads_matching_pages_and_moves_pending_rows() {
        let (provider, transport) = generated_jira(10_000);
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        let leaves = provider.get("ENG-000007").unwrap();
        let enters = provider.get("ENG-000008").unwrap();
        let missing = provider.get("ENG-020000").unwrap();
        outbox
            .admit_batch(&[
                tag_patch("leaves", &leaves, &[]),
                tag_patch("enters", &enters, &["hot"]),
                tag_patch("missing-enters", &missing, &["hot"]),
            ])
            .unwrap();
        transport.search_calls.store(0, Ordering::Relaxed);
        transport.get_calls.store(0, Ordering::Relaxed);
        let hot = TicketQuery {
            tags: vec!["hot".into()],
            sort: SortKey::Created,
            ..TicketQuery::default()
        };

        let rows = query(&provider, &outbox, "jira-test", &hot).unwrap();
        let ids = rows
            .iter()
            .map(|row| row.native_id.as_str())
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 101);
        assert!(!ids.contains(&"ENG-000007"));
        assert!(ids.contains(&"ENG-000008") && ids.contains(&"ENG-020000"));
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 1);
        assert_eq!(transport.get_calls.load(Ordering::Relaxed), 3);

        // Walk every keyset page; the union must equal the unpaged result exactly.
        transport.search_calls.store(0, Ordering::Relaxed);
        let mut seen = Vec::new();
        let mut after = None;
        loop {
            let page = query_after(
                &provider,
                &outbox,
                "jira-test",
                &hot,
                after.as_ref(),
                None,
                20,
            )
            .unwrap();
            seen.extend(page.items.iter().map(|item| item.ticket.native_id.clone()));
            if page.exhausted {
                break;
            }
            after = Some(MergeKey::from_ticket(&page.items.last().unwrap().ticket));
        }
        let mut expected = ids.iter().map(|id| (*id).to_string()).collect::<Vec<_>>();
        expected.sort();
        let mut walked = seen.clone();
        walked.sort();
        walked.dedup();
        assert_eq!(walked.len(), seen.len(), "keyset pages repeated a row");
        assert_eq!(walked, expected);
        // Six pages over a 100-row label result, never the 100-page project scan.
        assert!(transport.search_calls.load(Ordering::Relaxed) <= 12);

        // Baseline: the pre-pushdown full scan pages the whole project.
        transport.search_calls.store(0, Ordering::Relaxed);
        let baseline = projected_rows_with_limit(
            &provider,
            &TicketQuery {
                tags: vec![],
                ..hot.clone()
            },
            pending_for_connection(&outbox, "jira-test").unwrap(),
            MAX_PENDING_NATIVE_SCAN,
        )
        .unwrap();
        assert_eq!(baseline.len(), 10_001);
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 100);
    }

    /// Adversarial: a pending edit that is later re-edited back, an empty-then-refilled
    /// pending set, and more pending issues than point reads (full-scan fallback) all
    /// return the same membership as the projection rule.
    #[test]
    fn pushdown_overlay_matches_full_scan_across_pending_transitions() {
        let (provider, transport) = generated_jira(1_000);
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 200).unwrap();
        let hot = TicketQuery {
            tags: vec!["hot".into()],
            sort: SortKey::Created,
            ..TicketQuery::default()
        };
        let ids = |rows: Vec<ApiTicket>| {
            let mut ids = rows
                .into_iter()
                .map(|row| row.native_id)
                .collect::<Vec<_>>();
            ids.sort();
            ids
        };
        let full = |outbox: &ProviderOutbox| {
            ids(projected_rows_with_limit(
                &provider,
                &hot,
                pending_for_connection(outbox, "jira-test").unwrap(),
                MAX_PENDING_NATIVE_SCAN,
            )
            .unwrap())
        };
        assert_eq!(
            ids(query(&provider, &outbox, "jira-test", &hot).unwrap()).len(),
            10
        );

        let leaves = provider.get("ENG-000107").unwrap();
        outbox
            .admit_batch(&[
                tag_patch("out", &leaves, &[]),
                tag_patch("back", &leaves, &["hot"]),
            ])
            .unwrap();
        let reedited = ids(query(&provider, &outbox, "jira-test", &hot).unwrap());
        assert!(reedited.contains(&"ENG-000107".to_string()));
        assert_eq!(reedited, full(&outbox));

        let many = (0..60)
            .map(|index| {
                let ticket = provider.get(&format!("ENG-{:06}", index * 10 + 1)).unwrap();
                tag_patch(&format!("many-{index}"), &ticket, &["hot"])
            })
            .collect::<Vec<_>>();
        outbox.admit_batch(&many).unwrap();
        transport.search_calls.store(0, Ordering::Relaxed);
        transport.get_calls.store(0, Ordering::Relaxed);
        let fallback = ids(query(&provider, &outbox, "jira-test", &hot).unwrap());
        assert_eq!(fallback.len(), 70);
        // Over MAX_PENDING_POINT_READS: one full scan and no per-issue reads.
        assert_eq!(transport.get_calls.load(Ordering::Relaxed), 0);
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 10);
        assert_eq!(fallback, full(&outbox));
        let first_page = query_after(&provider, &outbox, "jira-test", &hot, None, None, 5).unwrap();
        assert_eq!(first_page.items.len(), 5);
        assert!(!first_page.exhausted);
    }

    #[test]
    fn unpaged_overlay_rejects_a_native_scan_over_the_limit() {
        let (provider, transport) = generated_jira(250);
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 10).unwrap();
        let native = provider.get("ENG-000249").unwrap();
        outbox
            .admit_batch(&[OutboxAdmission::new(
                "bounded-unpaged",
                &native,
                ProviderPatch {
                    title: Some("Matching native".into()),
                    ..ProviderPatch::default()
                },
            )
            .unwrap()])
            .unwrap();

        let error = projected_rows_with_limit(
            &provider,
            &TicketQuery::default(),
            pending_for_connection(&outbox, "jira-test").unwrap(),
            200,
        )
        .unwrap_err();
        assert!(error.to_string().contains("exceeded 200 issues"));
        assert_eq!(transport.search_calls.load(Ordering::Relaxed), 3);
    }

    /// Run with HOTSHEET_JIRA_OVERLAY_PROFILE_COUNT=1000|10000|100000 and
    /// HOTSHEET_JIRA_OVERLAY_PROFILE_MODE=full|bounded, one process per sample.
    #[cfg(unix)]
    #[test]
    #[ignore = "opt-in synthetic Jira page and resident-memory profile"]
    fn profile_pending_jira_overlay_page() {
        let count = std::env::var("HOTSHEET_JIRA_OVERLAY_PROFILE_COUNT")
            .unwrap()
            .parse::<usize>()
            .unwrap();
        let mode = std::env::var("HOTSHEET_JIRA_OVERLAY_PROFILE_MODE").unwrap();
        let (provider, transport) = generated_jira(count);
        let dir = tempfile::tempdir().unwrap();
        let mut outbox = ProviderOutbox::open(dir.path().join("outbox.sqlite"), 2).unwrap();
        let first = provider.get("ENG-000000").unwrap();
        outbox
            .admit_batch(&[OutboxAdmission::new(
                "bench-edit",
                &first,
                ProviderPatch {
                    title: Some("Zzz pending".into()),
                    ..ProviderPatch::default()
                },
            )
            .unwrap()])
            .unwrap();
        let query = TicketQuery {
            sort: SortKey::Title,
            ..TicketQuery::default()
        };
        let started = std::time::Instant::now();
        let page = match mode.as_str() {
            "full" => keyset_page_from_rows(
                projected_rows_with_limit(
                    &provider,
                    &query,
                    pending_for_connection(&outbox, "jira-test").unwrap(),
                    MAX_PENDING_NATIVE_SCAN,
                )
                .unwrap(),
                &query,
                None,
                50,
            ),
            "bounded" => {
                query_after(&provider, &outbox, "jira-test", &query, None, None, 50).unwrap()
            }
            _ => panic!("expected full or bounded profile mode"),
        };
        let elapsed = started.elapsed();
        let mut usage = std::mem::MaybeUninit::<libc::rusage>::zeroed();
        // SAFETY: getrusage initializes the supplied rusage struct on success.
        let result = unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) };
        assert_eq!(result, 0);
        // SAFETY: the successful getrusage call above initialized this value.
        let rss = unsafe { usage.assume_init() }.ru_maxrss;
        #[cfg(target_os = "macos")]
        let rss_kib = rss / 1024;
        #[cfg(not(target_os = "macos"))]
        let rss_kib = rss;
        assert_eq!(page.items.len(), 50);
        println!(
            "jira-overlay-profile count={count} mode={mode} elapsed_ms={} peak_rss_kib={rss_kib} search_pages={}",
            elapsed.as_millis(),
            transport.search_calls.load(Ordering::Relaxed),
        );
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
