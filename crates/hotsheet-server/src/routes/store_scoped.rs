//! Store-scoped write routes (`/stores/{id}/...`, HS2-87).

use crate::*;

// ---- store-scoped write routes (multi-store, HS2-87) -----------------------------

/// Look up a hosted store by URL id, 404 if not hosted.
pub(crate) fn scoped_entry(state: &AppState, store_id: &str) -> Result<StoreEntry, ApiError> {
    state
        .host
        .get(store_id)
        .ok_or_else(|| ApiError::not_found(store_id))
}

pub(crate) async fn create_store_ticket(
    State(state): State<AppState>,
    Path(store_id): Path<String>,
    Json(req): Json<CreateReq>,
) -> Result<(StatusCode, Json<ApiTicket>), ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    let ticket = do_create(&state, &entry, req)?;
    Ok((StatusCode::CREATED, Json(ticket)))
}

pub(crate) async fn update_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
    Json(req): Json<UpdateReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    Ok(Json(do_update(&state, &entry, &id, req)?))
}

pub(crate) async fn close_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
    Json(req): Json<CloseReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    Ok(Json(do_close(&state, &entry, &id, None, req)?))
}

pub(crate) async fn assign_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
    Json(req): Json<AssignReq>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    Ok(Json(do_assign(&state, &entry, &id, req)?))
}

pub(crate) async fn get_store_ticket(
    State(state): State<AppState>,
    Path((store_id, id)): Path<(String, String)>,
) -> Result<Json<ApiTicket>, ApiError> {
    let entry = scoped_entry(&state, &store_id)?;
    let ticket = ops::resolve(&entry.store, &id)?.ok_or_else(|| ApiError::not_found(&id))?;
    Ok(Json(api_ticket(&entry, &ticket)?))
}

/// A cross-store resolve result: the ticket + which hosted store it lives in.
#[derive(Serialize)]
pub(crate) struct ResolvedTicket {
    /// URL id of the store the live instance lives in.
    pub(crate) store: String,
    #[serde(flatten)]
    pub(crate) ticket: ApiTicket,
}

/// `GET /resolve/{ulid}` — resolve a **global ULID** to its single live instance across
/// every hosted store, following `moved_to_store` tombstones (HS2-87 / HS2-S4H2AM). By
/// ULID (not slug): slugs are per-store-prefix, but a ULID is global. 404 if unhosted.
pub(crate) async fn resolve_ticket(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<ResolvedTicket>, ApiError> {
    let ulid = Ulid::from_string(&id)
        .map_err(|_| ApiError::new(StatusCode::BAD_REQUEST, format!("not a ULID: {id}")))?;
    let (store, ticket) = state
        .host
        .resolve(&ulid)?
        .ok_or_else(|| ApiError::not_found(&id))?;
    let entry = scoped_entry(&state, &store)?;
    Ok(Json(ResolvedTicket {
        store,
        ticket: api_ticket(&entry, &ticket)?,
    }))
}

/// Prepare the served project for an AI tool — the same core setup the CLI runs headless
/// (`POST /setup/<tool>`, HS2-91). The server serves one store, so the project dir is the
/// store root; a single named tool doesn't need the enabled-plugin filter.
pub(crate) async fn setup_tool(
    State(state): State<AppState>,
    Path(tool): Path<String>,
) -> Result<Json<Vec<hotsheet_plugins::SetupReport>>, ApiError> {
    // Setup writes instruction/MCP/permission files and may probe the tool, so it runs on
    // the blocking pool instead of occupying an async request thread (HS2-9TV33W).
    let store = state.store.root().to_path_buf();
    let plugin_dirs = state.plugin_dirs.clone();
    let reports = tokio::task::spawn_blocking(move || {
        hotsheet_plugins::run_setup_in(&store, &store, Some(&tool), false, None, &plugin_dirs)
            .map_err(|e| ApiError::new(StatusCode::BAD_REQUEST, e.to_string()))
    })
    .await
    .map_err(|error| ApiError::new(StatusCode::INTERNAL_SERVER_ERROR, error.to_string()))??;
    // Setting a tool up changes its plugin state; the next discovery must rescan.
    state.invalidate_ai_tool_discovery();
    Ok(Json(reports))
}

pub(crate) async fn ws_sync(
    State(state): State<AppState>,
    Query(params): Query<WsParams>,
    ws: WebSocketUpgrade,
) -> Response {
    if params.secret.as_deref() != Some(state.secret.as_str()) {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    let rx = state.events.subscribe();
    let lease = begin_presence(
        &state,
        params.checkout.as_deref(),
        params.client.as_deref(),
        presence::Channel::Socket,
    );
    ws.on_upgrade(move |socket| async move {
        ws_loop(socket, rx).await;
        drop(lease);
    })
}

/// Start a change-stream lease. A lease on a checkout also brings its stores and repository
/// monitor back if an earlier sweep stopped them, off the request path.
pub(crate) fn begin_presence(
    state: &AppState,
    checkout: Option<&str>,
    client: Option<&str>,
    channel: presence::Channel,
) -> presence::PresenceGuard {
    let lease = state.presence.begin(checkout, client, channel);
    state.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
    if let Some(reference) = checkout.filter(|reference| !reference.is_empty()) {
        let state = state.clone();
        let reference = reference.to_string();
        tokio::task::spawn_blocking(move || {
            let Ok(checkout) = state.checkout_registry.resolve(&reference) else {
                return;
            };
            for source in checkout.sources.iter().filter(|s| s.provider == "git") {
                state.hosted_source(source);
            }
            state.watch_checkout_repository(&checkout);
        });
    }
    lease
}

/// `POST /checkouts/{reference}/close?client=<id>` — a client closed this project, so its
/// lease ends now and the project's stores are unhosted unless something still needs them
/// (HS2-ARJ9J1).
pub(crate) async fn close_checkout_session(
    State(state): State<AppState>,
    Path(reference): Path<String>,
    Query(params): Query<CloseSessionParams>,
) -> Result<StatusCode, ApiError> {
    let checkout = state
        .checkout_registry
        .resolve(&reference)
        .map_err(|e| ApiError::new(StatusCode::NOT_FOUND, e.to_string()))?;
    state.presence.close(&checkout.id, params.client.as_deref());
    state.request_unhost_sweep(Duration::ZERO);
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
pub(crate) struct CloseSessionParams {
    pub(crate) client: Option<String>,
}

/// When the next unhost sweep runs; a request for an earlier time wakes the sweeper.
#[derive(Default)]
pub(crate) struct UnhostSweep {
    pub(crate) due: Mutex<Option<tokio::time::Instant>>,
    pub(crate) wake: tokio::sync::Notify,
    pub(crate) started: std::sync::atomic::AtomicBool,
}

/// Run unhost sweeps as they come due (HS2-ARJ9J1). A local timer only: it issues no network
/// requests, and stays idle when no project store is hosted.
pub(crate) async fn run_unhost_sweeper(state: AppState) {
    loop {
        let due = state.unhost_sweep.due.with_lock(|due| *due);
        let Some(at) = due else {
            state.unhost_sweep.wake.notified().await;
            continue;
        };
        tokio::select! {
            () = tokio::time::sleep_until(at) => {
                state.unhost_sweep.due.with_lock(|due| {
                    if due.is_some_and(|due| due <= tokio::time::Instant::now()) {
                        *due = None;
                    }
                });
                sweep_unhosted(&state).await;
            }
            () = state.unhost_sweep.wake.notified() => {}
        }
    }
}

/// Unhost every project store that no open checkout references and no live work needs: a
/// drive on the store, a terminal inside a checkout that uses it, or a live claim in it.
/// Repository monitors of checkouts nobody has open stop too. While project stores remain
/// hosted, another sweep is scheduled for when their leases could have lapsed.
pub(crate) async fn sweep_unhosted(state: &AppState) {
    let live = state.presence.live();
    let terminal_dirs = live_terminal_dirs(state).await;
    let drive_sources: std::collections::HashSet<String> = state
        .client_drives
        .list()
        .into_iter()
        .map(|drive| drive.source)
        .collect();
    let sweeping = state.clone();
    let remaining = tokio::task::spawn_blocking(move || {
        let state = sweeping;
        let checkouts = state.checkout_registry.list().unwrap_or_default();
        // Checkout roots are canonical; compare terminal directories in the same form (for
        // example `/var` vs `/private/var` on macOS) or a terminal inside a checkout never matches.
        let terminal_dirs: Vec<std::path::PathBuf> = terminal_dirs
            .into_iter()
            .map(|dir| dir.canonicalize().unwrap_or(dir))
            .collect();
        let in_terminal = |checkout: &hotsheet_ticketing::checkouts::Checkout| {
            terminal_dirs
                .iter()
                .any(|dir| dir.starts_with(&checkout.root))
        };
        let hosted: Vec<String> = state
            .project_hosted
            .with_lock(|hosted| hosted.iter().cloned().collect());
        let mut eligible = Vec::new();
        let mut busy = std::collections::HashSet::new();
        for id in hosted {
            let referencing: Vec<&hotsheet_ticketing::checkouts::Checkout> = checkouts
                .iter()
                .filter(|checkout| {
                    checkout
                        .sources
                        .iter()
                        .any(|source| source.provider == "git" && source.connection_id == id)
                })
                .collect();
            let referenced_live = live.untagged
                || referencing
                    .iter()
                    .any(|checkout| live.checkouts.contains(&checkout.id));
            if !referenced_live
                && (drive_sources.contains(&id)
                    || referencing.iter().any(|checkout| in_terminal(checkout))
                    || store_has_live_claim(&state, &id))
            {
                busy.insert(id.clone());
            }
            eligible.push((id, referencing.iter().map(|c| c.id.clone()).collect()));
        }
        let candidates = presence::unhost_candidates(&eligible, &live, &busy);
        for id in &candidates {
            state.unhost_store(id);
        }
        if !live.untagged {
            let stopped: Vec<RepositoryWatchHandle> =
                state.repository_watchers.with_lock(|watchers| {
                    checkouts
                        .iter()
                        .filter(|checkout| {
                            !live.checkouts.contains(&checkout.id) && !in_terminal(checkout)
                        })
                        .filter_map(|checkout| watchers.remove(&checkout.id))
                        .collect()
                });
            drop(stopped);
        }
        eligible.len() - candidates.len()
    })
    .await
    .unwrap_or(0);
    if remaining > 0 {
        state.request_unhost_sweep(presence::POLL_RECONNECT_GAP + presence::UNHOST_GRACE);
    }
}

/// The working directories of live terminals, which keep their checkout's stores hosted.
pub(crate) async fn live_terminal_dirs(state: &AppState) -> Vec<std::path::PathBuf> {
    let infos: Vec<TerminalInfo> = if let Some(broker) = &state.terminal_broker {
        match broker.call(hotsheet_terminals::BrokerRequest::List).await {
            Ok(hotsheet_terminals::BrokerResponse::List { terminals }) => {
                terminals.into_iter().map(broker_info).collect()
            }
            _ => Vec::new(),
        }
    } else {
        state
            .terminals
            .list()
            .into_iter()
            .filter_map(|key| state.terminals.get(&key).map(|t| term_info(&t, &key.1)))
            .collect()
    };
    infos
        .into_iter()
        .filter(|info| info.alive)
        .filter_map(|info| info.cwd.map(std::path::PathBuf::from))
        .collect()
}

/// Whether any ticket in a hosted store is claimed under a lease that has not expired.
pub(crate) fn store_has_live_claim(state: &AppState, id: &str) -> bool {
    let Some(entry) = state.host.get(id) else {
        return false;
    };
    let now = now();
    entry.store.list_tickets().is_ok_and(|tickets| {
        tickets
            .iter()
            .any(|ticket| ticket.claimed_by.is_some() && !ops::claim_available(ticket, &now))
    })
}

pub(crate) async fn ws_loop(mut socket: WebSocket, mut rx: broadcast::Receiver<ChangeEvent>) {
    while let Ok(event) = rx.recv().await {
        let text = match serde_json::to_string(&event) {
            Ok(t) => t,
            Err(_) => continue,
        };
        if socket.send(Message::Text(text.into())).await.is_err() {
            break;
        }
    }
}

/// The default and max time a long-poll request will wait for a new event.
pub(crate) const POLL_DEFAULT_MS: u64 = 25_000;
pub(crate) const POLL_MAX_MS: u64 = 55_000;

#[derive(Debug, Deserialize)]
pub(crate) struct PollParams {
    pub(crate) secret: Option<String>,
    /// Return events with `seq > since`. Omit to just fetch the current cursor (no backlog),
    /// the way a fresh WebSocket only sees future events.
    pub(crate) since: Option<u64>,
    /// How long to block for the next event when none are newer than `since` (ms, capped).
    pub(crate) timeout_ms: Option<u64>,
    /// The checkout this subscription serves and the client holding it (HS2-ARJ9J1).
    pub(crate) checkout: Option<String>,
    pub(crate) client: Option<String>,
}

/// One long-poll response: the new cursor, any events since the requested one, and whether
/// the caller fell so far behind the ring that events were lost (→ re-sync via a full list).
#[derive(Debug, Serialize)]
pub(crate) struct PollResponse {
    pub(crate) cursor: u64,
    pub(crate) events: Vec<ChangeEvent>,
    pub(crate) overflow: bool,
}

/// `GET /ws/poll?secret=…&since=<seq>&timeout_ms=<n>` — the long-poll fallback to `/ws/sync`
/// (HS2-P3P3CC). Authentication accepts either the legacy query secret or the standard
/// `X-Hotsheet-Secret` header. Returns immediately with any events after `since`; otherwise
/// subscribes and waits up to `timeout_ms` for the next one, returning an empty list (with
/// the current cursor) on timeout. The client re-polls with the returned `cursor`.
pub(crate) async fn poll_events(
    State(state): State<AppState>,
    Query(params): Query<PollParams>,
    headers: HeaderMap,
) -> Response {
    let header_secret = headers
        .get("x-hotsheet-secret")
        .and_then(|value| value.to_str().ok());
    if params.secret.as_deref() != Some(state.secret.as_str())
        && header_secret != Some(state.secret.as_str())
    {
        return (StatusCode::UNAUTHORIZED, "missing or invalid secret").into_response();
    }
    let _lease = begin_presence(
        &state,
        params.checkout.as_deref(),
        params.client.as_deref(),
        presence::Channel::Poll,
    );
    // No `since` → hand back the current cursor with no backlog (initial handshake).
    let Some(since) = params.since else {
        return Json(PollResponse {
            cursor: state.event_cursor(),
            events: Vec::new(),
            overflow: false,
        })
        .into_response();
    };

    // Subscribe BEFORE reading the log, so an event emitted in the gap isn't missed: it
    // either lands in the log we read, or wakes the receiver below.
    let mut rx = state.events.subscribe();
    let (backlog, overflow, snapshot_cursor) = state.event_log.with_lock(|log| {
        let (events, overflow) = log.since(since);
        (events, overflow, log.seq)
    });
    if overflow || !backlog.is_empty() {
        return Json(PollResponse {
            cursor: snapshot_cursor,
            events: backlog,
            overflow,
        })
        .into_response();
    }

    // Caught up — wait for the next event (or time out with an empty list).
    let wait = Duration::from_millis(
        params
            .timeout_ms
            .unwrap_or(POLL_DEFAULT_MS)
            .min(POLL_MAX_MS),
    );
    // A stopping server ends the wait early with the ordinary empty "timeout" reply, so a
    // long poll never holds the shutdown drain open (HS2-W1KJR4).
    let next = tokio::select! {
        received = tokio::time::timeout(wait, rx.recv()) => received.map_err(|_| ()),
        () = state.stopping() => Err(()),
    };
    let (events, cursor, overflow) = match next {
        // Re-read the ring rather than returning only the wake-up event. This atomically
        // captures every event + the exact cursor through that span, so a burst racing the
        // response cannot advance the cursor past an event the client never received.
        Ok(Ok(_)) => state.event_log.with_lock(|log| {
            let (events, overflow) = log.since(since);
            (events, log.seq, overflow)
        }),
        // Lagged (fell behind the broadcast buffer) → signal overflow so the client re-syncs.
        Ok(Err(broadcast::error::RecvError::Lagged(_))) => {
            return Json(PollResponse {
                cursor: state.event_cursor(),
                events: Vec::new(),
                overflow: true,
            })
            .into_response();
        }
        Ok(Err(broadcast::error::RecvError::Closed)) | Err(_) => {
            (Vec::new(), state.event_cursor(), false)
        }
    };
    Json(PollResponse {
        cursor,
        events,
        overflow,
    })
    .into_response()
}
