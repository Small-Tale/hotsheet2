//! Unprefixed (primary-store) ticket handlers.

use crate::*;

// ---- handlers --------------------------------------------------------------------

pub(crate) async fn health(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    if let Err(error) = state.require_primary_store_identity() {
        // `/health` is the machine server's liveness probe for *every* hosted project.
        // Keep it healthy without reading ticket data from the replacement primary.
        return Ok(Json(serde_json::json!({
            "status": "ok",
            "generation": "hs2",
            "api_version": 1,
            "primary_store": { "status": "conflict", "error": error.message },
        })));
    }
    let metadata = state.store.metadata()?;
    let (listing, source) = state.health_listing().await?;
    Ok(Json(serde_json::json!({
        "status": "ok",
        "generation": "hs2",
        "api_version": 1,
        "ticket_prefix": metadata.ticket_prefix,
        "store_schema": metadata.schema_version,
        "tickets": listing.tickets,
        "corrupt": listing.corrupt,
        "listing": source,
    })))
}

impl AppState {
    /// The primary store's ticket count and corrupt files for `GET /health`.
    ///
    /// Resilient enumeration (HS2-PRVPCQ): a single corrupt ticket file must not make the
    /// whole store un-openable, so healthy tickets are counted and unparseable files are
    /// surfaced separately. That walk parses every ticket, so it runs on the blocking pool,
    /// single-flight, and is awaited only for [`health_scan::HEALTH_SCAN_BUDGET`]
    /// (HS2-9PPDR1): a large or blocked store never stalls the liveness probe. Past the
    /// budget the answer is the last completed scan (`"cached"`), or, before any scan has
    /// completed, the index's row count with no corrupt entries (`"index"`).
    pub(crate) async fn health_listing(
        &self,
    ) -> Result<(Arc<health_scan::HealthListing>, &'static str), ApiError> {
        use health_scan::{HealthListing, Resolution, ScanFailure};
        let store = self.store.clone();
        let resolution = self
            .health_scan
            .resolve(move || {
                let listing = store.list_tickets_resilient().map_err(|error| {
                    let error = ApiError::from(error);
                    ScanFailure {
                        status: error.status,
                        message: error.message,
                    }
                })?;
                Ok(Arc::new(HealthListing {
                    tickets: listing.tickets.len(),
                    corrupt: listing
                        .corrupt
                        .iter()
                        .map(|c| {
                            serde_json::json!({
                                "path": c.path.display().to_string(),
                                "id": c.id.map(|id| id.to_string()),
                                "slug": c.slug,
                                "error": c.error,
                                "error_code": c.error_code,
                            })
                        })
                        .collect(),
                }))
            })
            .await;
        match resolution {
            Resolution::Fresh(Ok(listing)) => Ok((listing, "fresh")),
            Resolution::Fresh(Err(failure)) => Err(ApiError::new(failure.status, failure.message)),
            Resolution::Cached(listing) => Ok((listing, "cached")),
            Resolution::Pending => {
                let tickets = self.index.lock_or_recover().ticket_count()?;
                Ok((
                    Arc::new(HealthListing {
                        tickets,
                        corrupt: Vec::new(),
                    }),
                    "index",
                ))
            }
        }
    }
}

pub(crate) struct ActiveMutation(pub(crate) Arc<LifecycleControl>);

impl Drop for ActiveMutation {
    fn drop(&mut self) {
        self.0.active_mutations.fetch_sub(1, Ordering::Release);
    }
}

impl AppState {
    pub(crate) fn begin_mutation(&self) -> Option<ActiveMutation> {
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            return None;
        }
        self.lifecycle
            .active_mutations
            .fetch_add(1, Ordering::AcqRel);
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            self.lifecycle
                .active_mutations
                .fetch_sub(1, Ordering::Release);
            None
        } else {
            Some(ActiveMutation(self.lifecycle.clone()))
        }
    }

    pub(crate) fn begin_background_work(&self) -> Option<ActiveBackgroundWork> {
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            return None;
        }
        self.lifecycle
            .active_background
            .fetch_add(1, Ordering::AcqRel);
        if self.lifecycle.quiescing.load(Ordering::Acquire) {
            self.lifecycle
                .active_background
                .fetch_sub(1, Ordering::Release);
            None
        } else {
            Some(ActiveBackgroundWork(self.lifecycle.clone()))
        }
    }
}

pub(crate) struct ActiveBackgroundWork(pub(crate) Arc<LifecycleControl>);

impl Drop for ActiveBackgroundWork {
    fn drop(&mut self) {
        self.0.active_background.fetch_sub(1, Ordering::Release);
    }
}

pub(crate) async fn lifecycle_quiescence(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "quiescing": state.lifecycle.quiescing.load(Ordering::Acquire),
        "report": state.quiescence_report(),
    }))
}

pub(crate) async fn lifecycle_restart(State(state): State<AppState>) -> Response {
    if state
        .lifecycle
        .quiescing
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "a safe restart is already being evaluated"
            })),
        )
            .into_response();
    }
    let report = state.quiescence_report();
    if !report.quiescent {
        state.lifecycle.quiescing.store(false, Ordering::Release);
        return (
            StatusCode::CONFLICT,
            Json(serde_json::json!({
                "error": "server is not quiescent; active work was preserved",
                "quiescence": report,
            })),
        )
            .into_response();
    }
    state.lifecycle.shutdown.notify_one();
    (
        StatusCode::ACCEPTED,
        Json(serde_json::json!({ "restarting": true })),
    )
        .into_response()
}

pub(crate) const API_PROTOCOL_MIN: u32 = 1;
pub(crate) const API_PROTOCOL_MAX: u32 = 1;

/// Authenticated build metadata and protocol compatibility. Exact application/revision
/// equality is informational; clients make hard decisions from the inclusive range.
pub(crate) async fn compatibility(State(state): State<AppState>) -> Json<serde_json::Value> {
    let started_at = state
        .instance
        .with_lock(|instance| instance.as_ref().map(|value| value.started_at.clone()));
    let source = state.source_revision.status();
    Json(serde_json::json!({
        "generation": "hs2",
        "application_version": env!("CARGO_PKG_VERSION"),
        "build_revision": source.build_revision,
        "source_revision": source.source_revision,
        "source_stale": source.source_stale,
        "protocol": { "min": API_PROTOCOL_MIN, "max": API_PROTOCOL_MAX },
        "store_schema": { "min": 1, "max": hotsheet_ticketing::STORE_SCHEMA_VERSION },
        "capabilities": {
            "lifecycle_restart": true,
            "lifecycle_quiescence": true
        },
        "started_at": started_at
    }))
}

pub(crate) async fn list_tickets(
    State(state): State<AppState>,
    Query(params): Query<ListParams>,
) -> Result<Response, ApiError> {
    let entry = state.default_entry();
    list_entry_tickets(&entry, &multistore::store_url_id(&state.store), params)
}

impl From<ops::StoreReadBoundError> for ApiError {
    fn from(error: ops::StoreReadBoundError) -> Self {
        ApiError::new(StatusCode::BAD_REQUEST, error.to_string())
    }
}

/// One store's unpaged list, served from its index under the bounded-response contract
/// (HS2-3JEFQT): at most `STORE_READ_MAX_ROWS` rows, an implicit overflow fails with 400,
/// and an explicit `limit` truncates with `x-hotsheet-truncated: true`. Larger reads page
/// with `limit` + `page_after`.
pub(crate) fn list_entry_tickets(
    entry: &StoreEntry,
    connection_id: &str,
    params: ListParams,
) -> Result<Response, ApiError> {
    let compact = params.compact.unwrap_or(true);
    let fields = parse_fields(&params.fields);
    let mut query = params.into_query(entry.store.root())?;
    let bound = ops::StoreReadBound::new(query.limit)?;
    query.limit = Some(bound.fetch_limit());
    let mut rows = entry.index.lock_or_recover().query(&query)?;
    let truncated = bound.finish(&mut rows)?;
    for row in &mut rows {
        row.set_connection(connection_id);
    }
    if compact {
        for row in &mut rows {
            row.make_compact();
        }
    }
    let contexts = auto_context::effective(&Settings::new(entry.store.root()))
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    for row in &mut rows {
        row.add_auto_context(&contexts);
    }
    let mut response = Json(rows_to_json(rows, &fields)).into_response();
    if truncated {
        response.headers_mut().insert(
            TRUNCATED_HEADER,
            axum::http::HeaderValue::from_static("true"),
        );
    }
    Ok(response)
}

pub(crate) async fn get_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ApiTicket>, ApiError> {
    let ticket = ops::resolve(&state.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(api_ticket(&state.default_entry(), &ticket)?))
}

pub(crate) fn api_ticket(entry: &StoreEntry, ticket: &Ticket) -> Result<ApiTicket, ApiError> {
    api_ticket_with_settings(entry, ticket, &Settings::new(entry.store.root()))
}

pub(crate) fn api_ticket_with_settings(
    entry: &StoreEntry,
    ticket: &Ticket,
    settings: &Settings,
) -> Result<ApiTicket, ApiError> {
    let contexts = auto_context::effective(settings)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(ApiTicket::with_provider_auto_context(
        ticket,
        &multistore::store_url_id(&entry.store),
        None,
        &contexts,
    ))
}

pub(crate) fn contextualize_api_ticket(
    mut ticket: ApiTicket,
    settings: &Settings,
) -> Result<ApiTicket, ApiError> {
    let contexts = auto_context::effective(settings)
        .map_err(|e| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    ticket.auto_context = auto_context::resolve_fields(&ticket.category, &ticket.tags, &contexts);
    Ok(ticket)
}
