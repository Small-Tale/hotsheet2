//! Checkout ticket listing: merged multi-source pages and corrupt-ticket diagnostics and repair (HS2-YMG48C).

use crate::api_error::ApiError;
use crate::dto::{CreateReq, ListParams, parse_fields};
use crate::multistore::StoreEntry;
use crate::routes::checkouts::{checkout_entries, checkout_lookup_error, code_review_api_error};
use crate::routes::project_tools::checkout_settings;
use crate::routes::providers::{
    connection_disabled, provider_for, provider_read_query_after, provider_transfer_error,
};
use crate::routes::ticket_writes::do_create;
use crate::routes::tickets::{api_ticket_with_settings, contextualize_api_ticket};
use crate::{AppState, code_review};
use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use hotsheet_index::TicketSummary;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::checkout_order::{AfterKey, MergeKey};
use hotsheet_ticketing::{auto_context, checkout_page, ops};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path as FsPath;
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

/// List a checkout's tickets. `page_size` selects the bounded page envelope with a
/// continuation cursor; otherwise the response is a plain array capped by `limit`,
/// the shape MCP `hotsheet_query` and client search/lookup use. Both shapes share one
/// global order across every local and hosted-provider source (HS2-M0YTB6).
/// Response header set when an explicitly `limit`ed unpaged checkout array omitted
/// further matching rows (HS2-CYXS0N).
pub(crate) const TRUNCATED_HEADER: &str = "x-hotsheet-truncated";

/// `GET /checkouts/{reference}/tickets`. With `page_size` it returns the bounded page
/// envelope. Without it, a plain row array in the same global order, bounded by
/// `CHECKOUT_READ_MAX_ROWS` (HS2-CYXS0N): a caller's explicit `limit` truncates, flagged by
/// `x-hotsheet-truncated: true`, while an implicit read that would exceed the bound fails
/// with 400 so a large checkout is never serialized into one response or silently cut.
pub(crate) async fn list_checkout_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<ListParams>,
) -> Result<axum::response::Response, ApiError> {
    if params.page_size.is_some() {
        return list_checkout_ticket_page(&state, &reference, params)
            .map(|page| Json(page).into_response());
    }
    if params.cursor.is_some() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            "cursor requires page_size",
        ));
    }
    let cap = match params.limit {
        None => CHECKOUT_READ_MAX_ROWS,
        Some(limit) if limit <= CHECKOUT_READ_MAX_ROWS => limit,
        Some(_) => {
            return Err(ApiError::new(
                StatusCode::BAD_REQUEST,
                format!(
                    "limit must be at most {CHECKOUT_READ_MAX_ROWS}; page larger reads with page_size and cursor"
                ),
            ));
        }
    };
    if cap == 0 {
        // Still validate the checkout and query even though no row is wanted.
        merge_checkout_page(&state, &reference, &params, 1, false)?;
        return Ok(Json(serde_json::Value::Array(Vec::new())).into_response());
    }
    let (items, next_cursor, _, source_errors) =
        merge_checkout_page(&state, &reference, &params, cap, false)?;
    let truncated = next_cursor.is_some();
    if truncated && params.limit.is_none() {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!(
                "more than {CHECKOUT_READ_MAX_ROWS} tickets match; pass limit (at most {CHECKOUT_READ_MAX_ROWS}) or page with page_size and cursor"
            ),
        ));
    }
    let mut response = Json(serde_json::Value::Array(items)).into_response();
    if truncated {
        response.headers_mut().insert(
            TRUNCATED_HEADER,
            axum::http::HeaderValue::from_static("true"),
        );
    }
    if !source_errors.is_empty() {
        response.headers_mut().insert(
            "x-hotsheet-partial",
            axum::http::HeaderValue::from_static("true"),
        );
    }
    Ok(response)
}

impl From<checkout_page::CheckoutPageError> for ApiError {
    fn from(error: checkout_page::CheckoutPageError) -> Self {
        use checkout_page::CheckoutPageError;
        let status = match error {
            CheckoutPageError::StaleCursor
            | CheckoutPageError::InvalidCursor
            | CheckoutPageError::InvalidSummaryDays
            | CheckoutPageError::InvalidCounts => StatusCode::BAD_REQUEST,
            CheckoutPageError::NonAdvancing | CheckoutPageError::Internal(_) => {
                StatusCode::INTERNAL_SERVER_ERROR
            }
        };
        ApiError::new(status, error.to_string())
    }
}

/// Fold an index summary into checkout counts.
fn index_summary(summary: TicketSummary) -> hotsheet_ticketing::ProviderTicketSummary {
    hotsheet_ticketing::ProviderTicketSummary {
        total: summary.total,
        queued: summary.queued,
        backlog: summary.backlog,
        archive: summary.archive,
        trash: summary.trash,
        open: summary.open,
        up_next: summary.up_next,
        active: summary.active,
        started: summary.started,
        verified: summary.verified,
        completed_today: summary.completed_today,
        completion_trend: summary.completion_trend,
    }
}

use hotsheet_ticketing::checkout_order::CHECKOUT_READ_MAX_ROWS;

fn list_checkout_ticket_page(
    state: &AppState,
    reference: &str,
    params: ListParams,
) -> Result<serde_json::Value, ApiError> {
    let page_size = params.page_size.unwrap_or(checkout_page::DEFAULT_PAGE_SIZE);
    if page_size == 0 || page_size > CHECKOUT_READ_MAX_ROWS {
        return Err(ApiError::new(
            StatusCode::BAD_REQUEST,
            format!("page_size must be between 1 and {CHECKOUT_READ_MAX_ROWS}"),
        ));
    }
    let with_counts = params.counts.unwrap_or(true);
    let (items, next_cursor, counts, source_errors) =
        merge_checkout_page(state, reference, &params, page_size, with_counts)?;
    serde_json::to_value(checkout_page::CheckoutTicketPage {
        items,
        next_cursor,
        counts: with_counts.then_some(counts),
        source_errors,
    })
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))
}

/// One bounded page of the checkout-wide merge: at most `page_size` rows in the global
/// order, the continuation cursor when a row remains, and (when requested) exact counts.
/// Both the paged envelope and the capped unpaged array read through here; the merge and
/// cursor codec are shared with the serverless MCP backend (`checkout_page`, HS2-JVF20F).
type CheckoutMergeResult = (
    Vec<serde_json::Value>,
    Option<String>,
    checkout_page::CheckoutTicketCounts,
    Vec<String>,
);

fn merge_checkout_page(
    state: &AppState,
    reference: &str,
    params: &ListParams,
    page_size: usize,
    with_counts: bool,
) -> Result<CheckoutMergeResult, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(reference)
        .map_err(checkout_lookup_error)?;
    let contexts = auto_context::effective(&checkout.settings())
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    let entries = checkout_entries(state, reference)?;
    if let Some(source) = params.source.as_deref() {
        if source != "git" {
            return Err(ApiError::new(StatusCode::BAD_REQUEST, "source must be git"));
        }
    }
    // A disabled source is left out of the merged view rather than failing it (HS2-SF6W34).
    let mut external_sources = Vec::new();
    for source in checkout
        .sources
        .iter()
        .filter(|source| source.provider != "git" && params.source.is_none())
    {
        if !connection_disabled(state, &source.connection_id)? {
            external_sources.push(source);
        }
    }
    let source_keys = entries
        .iter()
        .map(|(store_id, _)| checkout_page::git_source_key(store_id))
        .chain(
            external_sources
                .iter()
                .map(|source| checkout_page::provider_source_key(&source.connection_id)),
        )
        .collect::<Vec<_>>();
    let merge_query = params.clone().into_query(state.store.root())?;
    let sort = merge_query.sort;
    let descending = merge_query.descending;
    let filters = checkout_page::filter_fingerprint(&merge_query, params.has_commit);
    // Validate the cursor before paying for counts.
    checkout_page::decode_cursor(
        params.cursor.as_deref(),
        &source_keys,
        sort,
        descending,
        &filters,
    )?;

    let now = OffsetDateTime::now_utc();
    let now_text = now
        .format(&Rfc3339)
        .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))?;
    let day_starts = checkout_page::completion_day_starts(params.summary_days.as_deref(), now)?;
    let mut counts = checkout_page::CheckoutTicketCounts::for_days(&day_starts);
    for (_, entry) in entries.iter().filter(|_| with_counts) {
        let summary = entry
            .index
            .lock_or_recover()
            .summary(&now_text, &day_starts)?;
        counts.add(index_summary(summary));
    }
    let mut failed_sources = vec![None; external_sources.len()];
    let mut provider_summaries = vec![None; external_sources.len()];
    for (index, source) in external_sources.iter().enumerate().filter(|_| with_counts) {
        let result = provider_for(state, &source.connection_id).and_then(|provider| {
            provider
                .summary(&now_text, &day_starts)
                .map_err(provider_transfer_error)
        });
        match result {
            Ok(summary) => provider_summaries[index] = Some(summary),
            Err(error) => {
                failed_sources[index] =
                    Some(format!("{}: {}", source.connection_id, error.message));
            }
        }
    }

    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    // Batched `has_commit` evaluation: one repository scan per fetched batch, not per row.
    let commit_filter = |slugs: Vec<String>| -> Result<Option<HashSet<String>>, ApiError> {
        if params.has_commit.is_none() || slugs.is_empty() {
            return Ok(None);
        }
        match code_review::slugs_with_commits(FsPath::new(&checkout.root), &slugs) {
            Ok(matches) => Ok(Some(matches)),
            Err(code_review::CodeReviewError::NotRepository) => Ok(Some(HashSet::new())),
            Err(error) => Err(code_review_api_error(error)),
        }
    };
    let keeps = |matches: &Option<HashSet<String>>, slug: &str| {
        params
            .has_commit
            .is_none_or(|want| matches.as_ref().is_some_and(|set| set.contains(slug)) == want)
    };
    // One bounded batch of rows strictly after the last row a source fetched (a value
    // keyset, HS2-74H84S). Rows removed by post-filters still advance the source, and a
    // batch is one index query or one provider keyset read (HS2-BGZ0NY).
    let fetch =
        |request: checkout_page::SourceFetch<'_>| -> Result<checkout_page::SourceBatch, ApiError> {
            if request.source < entries.len() {
                let (store_id, entry) = &entries[request.source];
                let mut query = params.clone().into_query(entry.store.root())?;
                query.limit = Some(request.want);
                query.page_after = None;
                query.after_key = request.after.cloned().map(|key| AfterKey {
                    key,
                    connection_id: store_id.clone(),
                });
                let rows = entry.index.lock_or_recover().query(&query)?;
                let exhausted = rows.len() < request.want;
                let matches = commit_filter(rows.iter().map(|row| row.slug.clone()).collect())?;
                let mut batch = Vec::with_capacity(rows.len());
                for mut row in rows {
                    row.set_connection(store_id);
                    let key = MergeKey::from_row(&row);
                    if !keeps(&matches, &row.slug) {
                        batch.push(checkout_page::SourceRow {
                            key,
                            resume: None,
                            value: None,
                        });
                        continue;
                    }
                    if compact {
                        row.make_compact();
                    }
                    row.add_auto_context(&contexts);
                    let mut value = serde_json::to_value(row).map_err(|error| {
                        ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                    })?;
                    // Project first: every checkout row keeps its source `store`.
                    hotsheet_ticketing::wire::project_fields(
                        std::slice::from_mut(&mut value),
                        &fields,
                    );
                    if let Some(object) = value.as_object_mut() {
                        object.insert("store".into(), store_id.clone().into());
                    }
                    batch.push(checkout_page::SourceRow {
                        key,
                        resume: None,
                        value: Some(value),
                    });
                }
                return Ok(checkout_page::SourceBatch {
                    rows: batch,
                    exhausted,
                });
            }
            let source_index = request.source - entries.len();
            let source = external_sources[source_index];
            if failed_sources[source_index].is_some() {
                return Ok(checkout_page::SourceBatch {
                    exhausted: true,
                    ..Default::default()
                });
            }
            let query = params.clone().into_query(state.store.root())?;
            let page = match provider_read_query_after(
                state,
                &source.connection_id,
                &hotsheet_ticketing::unbounded_query(&query),
                request.after,
                request.resume,
                request.want,
            ) {
                Ok(page) => page,
                Err(error) => {
                    failed_sources[source_index] =
                        Some(format!("{}: {}", source.connection_id, error.message));
                    return Ok(checkout_page::SourceBatch {
                        exhausted: true,
                        ..Default::default()
                    });
                }
            };
            let matches = commit_filter(
                page.items
                    .iter()
                    .map(|item| item.ticket.slug.clone())
                    .collect(),
            )?;
            let mut batch = Vec::with_capacity(page.items.len());
            for item in page.items {
                let mut ticket = item.ticket;
                let key = MergeKey::from_ticket(&ticket);
                if !keeps(&matches, &ticket.slug) {
                    batch.push(checkout_page::SourceRow {
                        key,
                        resume: item.resume,
                        value: None,
                    });
                    continue;
                }
                ticket.auto_context =
                    auto_context::resolve_fields(&ticket.category, &ticket.tags, &contexts);
                if compact {
                    ticket.details.clear();
                }
                let mut value = serde_json::to_value(ticket).map_err(|error| {
                    ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string())
                })?;
                hotsheet_ticketing::wire::project_fields(std::slice::from_mut(&mut value), &fields);
                if let Some(object) = value.as_object_mut() {
                    object.insert("store".into(), source.connection_id.clone().into());
                }
                batch.push(checkout_page::SourceRow {
                    key,
                    resume: item.resume,
                    value: Some(value),
                });
            }
            Ok(checkout_page::SourceBatch {
                rows: batch,
                exhausted: page.exhausted,
            })
        };
    let page = checkout_page::merge_page(
        &source_keys,
        params.cursor.as_deref(),
        sort,
        descending,
        &filters,
        page_size,
        fetch,
    )?;
    // A provider can succeed at summary but fail while fetching its page. Its counts are
    // incomplete in that case too, so only add sources whose entire read succeeded.
    for (index, summary) in provider_summaries.into_iter().enumerate() {
        if failed_sources[index].is_none() {
            if let Some(summary) = summary {
                counts.add(summary);
            }
        }
    }
    Ok((
        page.items,
        page.next_cursor,
        counts,
        failed_sources.into_iter().flatten().collect(),
    ))
}

#[derive(Serialize)]
pub(crate) struct CheckoutCorruptTicket {
    store: String,
    store_path: String,
    path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    slug: Option<String>,
    error: String,
    error_code: &'static str,
}

/// `GET /checkouts/{reference}/corrupt-tickets` — every unparseable ticket file across the
/// checkout's hosted git sources.
///
/// Each hosted store keeps a stat-validated [`hotsheet_ticketing::CorruptTicketCache`]
/// (HS2-KYSBT2): a request lists and `stat`s the ticket tree and re-parses only files
/// whose fingerprint changed, so the answer always matches the disk without a full parse
/// on every project activation. The cache is prewarmed when a store is hosted. The walk
/// still runs on the blocking pool so a cold cache or a slow disk never occupies an async
/// request thread (HS2-QV8B7R). It is not shared with the `/health` single-flight scan:
/// that scan covers only the primary store and may answer from a stale listing, whereas
/// the ticket list filters rows by this response and needs the current state of every
/// linked source.
pub(crate) async fn list_checkout_corrupt_tickets(
    State(state): State<AppState>,
    Path(reference): Path<String>,
) -> Result<Json<Vec<CheckoutCorruptTicket>>, ApiError> {
    let result = tokio::task::spawn_blocking(move || {
        let mut result = Vec::new();
        for (store_id, entry) in checkout_entries(&state, &reference)? {
            let store_path = entry.store.root().display().to_string();
            let listed = entry
                .corrupt
                .lock_or_recover()
                .corrupt_tickets(&entry.store)?;
            for corrupt in listed {
                result.push(CheckoutCorruptTicket {
                    store: store_id.clone(),
                    store_path: store_path.clone(),
                    path: corrupt.path.display().to_string(),
                    id: corrupt.id.map(|id| id.to_string()),
                    slug: corrupt.slug,
                    error: corrupt.error,
                    error_code: corrupt.error_code,
                });
            }
        }
        Ok::<_, ApiError>(result)
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    Ok(Json(result))
}

/// Fill a newly hosted store's corrupt-ticket cache in the background, so the first
/// project activation's `corrupt-tickets` request finds it warm (or coalesces onto the
/// scan in progress under the cache lock) instead of starting a full parse (HS2-KYSBT2).
pub(crate) fn prewarm_corrupt_tickets(entry: StoreEntry) {
    // A failed spawn only skips the warm-up.
    let _ = std::thread::Builder::new()
        .name("hs-corrupt-prewarm".into())
        .spawn(move || {
            let mut cache = entry.corrupt.lock_or_recover();
            // Warm-up only: a scan failure is reported by the next on-demand read.
            let _ = cache.corrupt_tickets(&entry.store);
        });
}

#[derive(Deserialize)]
pub(crate) struct CorruptTicketRepairReq {
    path: String,
}

/// Create (or return) an Up Next work item that directs an AI worker to repair one
/// currently-corrupt git ticket. The corrupt file itself is never modified by this API.
pub(crate) async fn create_corrupt_ticket_repair(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Json(req): Json<CorruptTicketRepairReq>,
) -> Result<Response, ApiError> {
    let (_, settings) = checkout_settings(&state, &reference)?;
    let requested = std::path::PathBuf::from(&req.path);
    for (_, entry) in checkout_entries(&state, &reference)? {
        let listing = entry.store.list_tickets_resilient()?;
        let Some(corrupt) = listing
            .corrupt
            .into_iter()
            .find(|item| item.path == requested)
        else {
            continue;
        };
        if corrupt.error_code == "upgrade_required" {
            return Err(ApiError::new(
                StatusCode::CONFLICT,
                "This ticket requires a newer Hot Sheet 2 version and cannot be safely auto-repaired.",
            ));
        }

        let marker = format!(
            "<!-- hotsheet:corrupt-repair {} -->",
            serde_json::to_string(&req.path).unwrap_or_else(|_| "null".into())
        );
        if let Some(existing) = listing
            .tickets
            .iter()
            .find(|ticket| ticket.details.contains(&marker) && ops::is_open(ticket))
        {
            return Ok((
                StatusCode::OK,
                Json(api_ticket_with_settings(&entry, existing, &settings)?),
            )
                .into_response());
        }

        let identity = corrupt
            .slug
            .clone()
            .or_else(|| corrupt.id.map(|id| id.to_string()))
            .or_else(|| {
                corrupt
                    .path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "unreadable ticket".into());
        let details = format!(
            "Repair the corrupt Hot Sheet ticket file at `{}`.\n\nThe parser reported:\n\n```text\n{}\n```\n\nPreserve all recoverable ticket content, rewrite it with the current canonical ticket format, and verify that Hot Sheet can parse and list it. Do not delete the file or discard content merely to make parsing succeed.\n\n{}",
            req.path, corrupt.error, marker
        );
        let created = do_create(
            &state,
            &entry,
            CreateReq {
                title: format!("Repair corrupt ticket {identity}"),
                category: Some("bug".into()),
                priority: Some("high".into()),
                status: None,
                details: Some(details),
                tags: Some(vec!["corrupt-ticket".into(), "ai-repair".into()]),
                up_next: Some(true),
                blocked_by: None,
            },
        )?;
        return Ok((
            StatusCode::CREATED,
            Json(contextualize_api_ticket(created, &settings)?),
        )
            .into_response());
    }
    Err(ApiError::new(
        StatusCode::NOT_FOUND,
        "The corrupt ticket is no longer present in this checkout.",
    ))
}

#[cfg(test)]
mod checkout_pagination_tests {
    use super::ListParams;
    use hotsheet_ticketing::checkout_page::filter_fingerprint;

    fn fingerprint(query: &str) -> String {
        let params: ListParams = serde_urlencoded_params(query);
        let has_commit = params.has_commit;
        let query = params.into_query(std::path::Path::new(".")).unwrap();
        filter_fingerprint(&query, has_commit)
    }

    fn serde_urlencoded_params(query: &str) -> ListParams {
        let uri: axum::http::Uri = format!("/?{query}").parse().unwrap();
        axum::extract::Query::<ListParams>::try_from_uri(&uri)
            .unwrap()
            .0
    }

    #[test]
    fn filter_fingerprint_ignores_order_and_paging_but_binds_every_filter() {
        let base = fingerprint("tags=a,b&status=started&text=x");
        assert_eq!(base, fingerprint("text=x&tags=b,a,a&status=started"));
        assert_eq!(
            base,
            fingerprint(
                "tags=a,b&status=started&text=x&sort=title&direction=descending&limit=3&page_size=9&cursor=v1.00"
            ),
            "ordering is bound separately; paging parameters are not filters"
        );
        assert_eq!(fingerprint(""), fingerprint("tags=&attachment="));
        for changed in [
            "tags=a&status=started&text=x",
            "tags=a,b&status=completed&text=x",
            "tags=a,b&status=started&text=y",
            "tags=a,b&status=started&text=x&up_next=true",
            "tags=a,b&status=started&text=x&has_commit=true",
            "tags=a,b&status=started&text=x&has_commit=false",
            "tags=a,b&status=started&text=x&updated_after=2026-01-01",
            "tags=a,b&status=started&text=x&collection=queue",
        ] {
            assert_ne!(base, fingerprint(changed), "{changed}");
        }
    }
}
