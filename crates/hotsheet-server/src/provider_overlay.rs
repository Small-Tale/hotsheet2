//! Provisional Jira reads over the durable provider outbox.
//!
//! A pending field edit can change search membership and sort order. Apply it to the
//! complete native result before filtering, limiting, or choosing a keyset page.

use std::collections::{BTreeMap, HashSet};

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
    if let Some(limit) = query.limit {
        return Ok(bounded_projected_page(
            provider,
            query,
            pending,
            None,
            limit,
            MAX_PENDING_NATIVE_SCAN,
        )?
        .items
        .into_iter()
        .map(|item| item.ticket)
        .collect());
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
    bounded_projected_page(
        provider,
        query,
        pending,
        after,
        limit,
        MAX_PENDING_NATIVE_SCAN,
    )
}

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
                        "labels":[],"assignee":null,
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
                let end = (start + body["maxResults"].as_u64().unwrap() as usize).min(self.count);
                json!({
                    "issues":(start..end).map(issue).collect::<Vec<_>>(),
                    "isLast":end == self.count,
                    "nextPageToken":(end < self.count).then(||end.to_string()),
                })
            } else if method == "GET" && url.contains("/comment?") {
                json!({"comments":[],"total":0})
            } else if method == "GET" && url.contains("/issue/ENG-") {
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
                projected_rows(
                    &provider,
                    &query,
                    pending_for_connection(&outbox, "jira-test").unwrap(),
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
