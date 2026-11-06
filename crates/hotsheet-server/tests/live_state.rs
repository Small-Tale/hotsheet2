//! HS2-JTTQMY E2E: the per-store live state follows real server writes through the running
//! notification router, is pushed as cursor-replayable `live_state` change events, and is
//! read back through `GET /live-state` and `GET /stores/{store_id}/live-state`. No
//! notification rule is configured: the claim tracker feeds the live state regardless.

use std::time::Duration;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, ChangeEvent, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "test-secret";

struct Harness {
    _dir: tempfile::TempDir,
    store_id: String,
    state: AppState,
}

fn harness() -> Harness {
    hotsheet_aitools::enable_test_permission_vault();
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let store_id = hotsheet_server::multistore::store_url_id(&store);
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_default_codex_home(dir.path().join("no-codex-home"))
        .with_machine_home(dir.path().join("machine-home"))
        .with_project_idle_grace(Duration::from_millis(300));
    Harness {
        _dir: dir,
        store_id,
        state,
    }
}

async fn call(
    router: &axum::Router,
    method: &str,
    uri: &str,
    body: Option<Value>,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header("X-Hotsheet-Secret", SECRET);
    let body = match body {
        Some(body) => {
            request = request.header(header::CONTENT_TYPE, "application/json");
            Body::from(body.to_string())
        }
        None => Body::empty(),
    };
    let response = router
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// The next pushed `live_state` for the store that satisfies `want`.
async fn next_live(
    events: &mut tokio::sync::broadcast::Receiver<ChangeEvent>,
    store: &str,
    want: impl Fn(&Value) -> bool,
) -> Value {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = events.recv().await.expect("change bus open");
            if event.kind != "live_state" || event.store != store {
                continue;
            }
            let live =
                serde_json::to_value(event.live.expect("live_state carries a snapshot")).unwrap();
            if want(&live) {
                return live;
            }
        }
    })
    .await
    .expect("the expected live state was pushed")
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jttqmy_live_state_follows_claims_release_and_feedback_without_rules() {
    let h = harness();
    let router = app(h.state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;
    let mut events = h.state.subscribe();
    let (_, start) = call(&router, "GET", "/ws/poll?timeout_ms=0", None).await;
    let since = start["cursor"].as_u64().unwrap();

    let (status, created) = call(
        &router,
        "POST",
        "/tickets",
        Some(json!({"title":"Glance work"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let id = created["id"].as_str().unwrap().to_string();
    let slug = created["slug"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":30,"eta":"45m"})),
    )
    .await;
    let active = next_live(&mut events, &h.store_id, |live| live["state"] == "active").await;
    assert_eq!(active["claims"][0]["slug"], slug.as_str());
    assert_eq!(active["claims"][0]["worker"], "w1");
    assert!(
        active["claims"][0]["eta_at_ms"].as_i64().unwrap()
            > active["claims"][0]["claimed_at_ms"].as_i64().unwrap()
    );
    assert_eq!(active["workers"], json!({"live":1,"expected":1}));
    assert_eq!(active["rates"]["tokens"]["window_minutes"], 5);
    assert_eq!(active["rates"]["tickets_updated"]["window_minutes"], 60);
    assert!(
        !active["rates"]["tickets_updated"]["at_ms"]
            .as_array()
            .unwrap()
            .is_empty(),
        "creating the ticket is a progress write"
    );

    // The REST reads return the pushed snapshot and revision.
    let (status, one) = call(
        &router,
        "GET",
        &format!("/stores/{}/live-state", h.store_id),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(one["state"], "active");
    assert!(one["revision"].as_u64().unwrap() >= active["revision"].as_u64().unwrap());
    let (_, all) = call(&router, "GET", "/live-state", None).await;
    assert_eq!(all["stores"].as_array().unwrap().len(), 1);
    assert_eq!(all["stores"][0]["store"], h.store_id.as_str());
    assert_eq!(all["epoch_ms"], one["epoch_ms"]);
    let (status, _) = call(&router, "GET", "/stores/no-such-store/live-state", None).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    // A feedback request outranks Active: Waiting on you.
    call(
        &router,
        "PATCH",
        &format!("/tickets/{id}"),
        Some(json!({"note":"Pick A or B?","note_kind":"feedback_needed"})),
    )
    .await;
    let waiting = next_live(&mut events, &h.store_id, |live| {
        live["state"] == "waiting_on_you"
    })
    .await;
    assert_eq!(waiting["activity"]["word"], "active");
    assert_eq!(waiting["waiting"]["count"], 1);
    assert_eq!(waiting["waiting"]["tickets"][0]["reason"], "feedback");

    // Release: the grace still reads Active underneath, then the tracker stops. The open
    // feedback request still outranks Stopped.
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/release"),
        Some(json!({"worker":"w1"})),
    )
    .await;
    let stopped = next_live(&mut events, &h.store_id, |live| {
        live["activity"]["word"] == "stopped"
    })
    .await;
    assert_eq!(stopped["state"], "waiting_on_you");
    assert!(stopped["claims"].as_array().unwrap().is_empty());
    assert_eq!(stopped["workers"], json!({"live":0,"expected":0}));
    assert!(stopped["activity"]["stopped_at_ms"].is_i64());
    // Closing the ticket ends the wait: Stopped.
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/close"),
        Some(json!({"reason":"not_planned"})),
    )
    .await;
    let stopped = next_live(&mut events, &h.store_id, |live| live["state"] == "stopped").await;
    assert_eq!(stopped["waiting"]["count"], 0);

    // Cursor replay: a client reconnecting from before the claim sees every pushed snapshot.
    let (_, replay) = call(
        &router,
        "GET",
        &format!("/ws/poll?since={since}&timeout_ms=0"),
        None,
    )
    .await;
    let states: Vec<&str> = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "live_state")
        .map(|event| event["live"]["state"].as_str().unwrap())
        .collect();
    assert!(
        states.contains(&"active") && states.contains(&"waiting_on_you"),
        "{states:?}"
    );
    assert_eq!(states.last(), Some(&"stopped"));
    let revisions: Vec<u64> = replay["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "live_state")
        .map(|event| event["live"]["revision"].as_u64().unwrap())
        .collect();
    assert!(
        revisions.windows(2).all(|pair| pair[0] < pair[1]),
        "{revisions:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jttqmy_an_unrenewed_lease_reads_stalled_and_an_unchanged_state_pushes_nothing() {
    let h = harness();
    let router = app(h.state.clone());
    let (_, created) = call(&router, "POST", "/tickets", Some(json!({"title":"Stalls"}))).await;
    let id = created["id"].as_str().unwrap().to_string();
    call(
        &router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":"w1","lease_minutes":1})),
    )
    .await;
    let state = h.state.clone();
    let store = h.store_id.clone();
    let (claimed, again, stalled, quiet) = tokio::task::spawn_blocking(move || {
        // Drive the router synchronously, as the bus would.
        let claim = ChangeEvent {
            live: None,
            cursor: None,
            store: store.clone(),
            kind: "claimed".into(),
            id: id.clone(),
            slug: String::new(),
            message: None,
            activity: None,
            assignment: None,
            turn: None,
        };
        hotsheet_server::notification_router::route_change_event(&state, &claim);
        let now = time::OffsetDateTime::now_utc();
        let claimed = hotsheet_server::live_state::refresh_at(&state, &store, now).unwrap();
        let mut bus = state.subscribe();
        let again = hotsheet_server::live_state::refresh_at(&state, &store, now).unwrap();
        let quiet_after_repeat = bus.try_recv().is_err();
        let later = now + time::Duration::minutes(2);
        hotsheet_server::notification_router::check_project_activity(&state, &store, later);
        let stalled = hotsheet_server::live_state::refresh_at(&state, &store, later).unwrap();
        (claimed, again, stalled, quiet_after_repeat)
    })
    .await
    .unwrap();
    assert_eq!(serde_json::to_value(claimed.state).unwrap(), "active");
    assert_eq!(
        again.revision, claimed.revision,
        "nothing changed, nothing pushed"
    );
    assert!(quiet, "an unchanged rebuild emits no event");
    assert_eq!(serde_json::to_value(stalled.state).unwrap(), "stalled");
    assert!(stalled.revision > claimed.revision);
    assert_eq!(stalled.activity.stalled[0].worker, "w1");
    assert_eq!(stalled.workers.expected, 1);
    assert_eq!(stalled.workers.live, 0);
}

/// A ticket claimed by the session of a live `cat` terminal `terminal` (worker
/// `terminal-<terminal>`, the id that terminal's session claims with).
async fn claimed_terminal(router: &axum::Router, terminal: &str) -> String {
    let (status, _) = call(
        router,
        "POST",
        "/terminals",
        Some(json!({"command":"cat","id":terminal})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (_, created) = call(
        router,
        "POST",
        "/tickets",
        Some(json!({"title":format!("Work in {terminal}")})),
    )
    .await;
    let id = created["id"].as_str().unwrap().to_string();
    call(
        router,
        "POST",
        &format!("/tickets/{id}/claim"),
        Some(json!({"worker":format!("terminal-{terminal}"),"lease_minutes":60})),
    )
    .await;
    id
}

fn causes(live: &Value) -> Vec<Value> {
    live["activity"]["causes"].as_array().unwrap().clone()
}

/// HS2-EGKKA0 E2E: a usage-limit halt reported by the terminal's hook becomes a cause with
/// the reset time the message states, an unstated reset is explicitly null, an overload is
/// no cause, and resuming clears it, all through the running router without rules.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_egkka0_usage_limit_pause_carries_its_reset_and_clears_on_resume() {
    let h = harness();
    let router = app(h.state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;
    let mut events = h.state.subscribe();
    claimed_terminal(&router, "lim").await;
    next_live(&mut events, &h.store_id, |live| live["state"] == "active").await;

    let (status, _) = call(
        &router,
        "POST",
        "/terminals/lim/halt",
        Some(json!({"error_type":"rate_limit","message":"Usage limit reached. Resets in 2h 15m.","agent":"claude"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let paused = next_live(&mut events, &h.store_id, |live| !causes(live).is_empty()).await;
    assert_eq!(paused["state"], "halted", "the word is unchanged");
    let cause = &causes(&paused)[0];
    assert_eq!(cause["kind"], "usage_limit");
    assert_eq!(cause["worker"], "terminal-lim");
    assert_eq!(cause["terminal"], "lim");
    assert_eq!(cause["prompt"], Value::Null);
    let since = cause["since_ms"].as_i64().unwrap();
    let resets = cause["resets_at_ms"].as_i64().unwrap();
    assert!(
        (resets - since - 135 * 60_000).abs() < 5_000,
        "2h 15m after the report: {since} -> {resets}"
    );

    // A reset stated only as a local time is unknown, never guessed.
    call(
        &router,
        "POST",
        "/terminals/lim/halt",
        Some(json!({"error_type":"usage_limit_exceeded","message":"You've hit your usage limit. Try again at 3:10 PM.","agent":"codex"})),
    )
    .await;
    let unknown = next_live(&mut events, &h.store_id, |live| {
        causes(live)
            .first()
            .is_some_and(|cause| cause["resets_at_ms"].is_null())
    })
    .await;
    assert_eq!(causes(&unknown)[0]["kind"], "usage_limit");

    // An overload is a halt but no usage-limit cause.
    call(
        &router,
        "POST",
        "/terminals/lim/halt",
        Some(json!({"error_type":"overloaded","message":"Selected model is at capacity."})),
    )
    .await;
    let overloaded = next_live(&mut events, &h.store_id, |live| causes(live).is_empty()).await;
    assert_eq!(overloaded["state"], "halted");

    // Usage limit again, then the user prompts again (resume): the cause clears.
    call(
        &router,
        "POST",
        "/terminals/lim/halt",
        Some(json!({"error_type":"rate_limit","message":"Usage limit reached."})),
    )
    .await;
    next_live(&mut events, &h.store_id, |live| !causes(live).is_empty()).await;
    call(&router, "DELETE", "/terminals/lim/halt", None).await;
    let resumed = next_live(&mut events, &h.store_id, |live| live["state"] == "active").await;
    assert!(causes(&resumed).is_empty());
    let (_, read) = call(
        &router,
        "GET",
        &format!("/stores/{}/live-state", h.store_id),
        None,
    )
    .await;
    assert_eq!(read["activity"]["causes"], json!([]));
    call(&router, "DELETE", "/terminals/lim", None).await;
}

/// HS2-EGKKA0 E2E: a terminal session waiting at an `AskUserQuestion` or a bridged permission
/// prompt marks its worker's project awaiting input; answering each clears it.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_egkka0_terminal_prompts_mark_awaiting_input_until_answered() {
    let h = harness();
    let router = app(h.state.clone());
    let _router_task =
        hotsheet_server::notification_router::spawn_notification_router(h.state.clone()).await;
    let mut events = h.state.subscribe();
    claimed_terminal(&router, "ask").await;
    // An unclaimed terminal's prompt never shows on the store.
    let (status, _) = call(
        &router,
        "POST",
        "/terminals",
        Some(json!({"command":"cat","id":"idle"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    next_live(&mut events, &h.store_id, |live| live["state"] == "active").await;

    // A question.
    let (status, _) = call(
        &router,
        "POST",
        "/terminals/ask/question",
        Some(json!({"question":"Which?","tool_use_id":"tu-1","session_id":"s-ask","questions":[]})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let asked = next_live(&mut events, &h.store_id, |live| !causes(live).is_empty()).await;
    let cause = &causes(&asked)[0];
    assert_eq!(cause["kind"], "awaiting_input");
    assert_eq!(cause["prompt"], "question");
    assert_eq!(cause["terminal"], "ask");
    assert_eq!(cause["worker"], "terminal-ask");
    assert_eq!(cause["resets_at_ms"], Value::Null);
    assert_eq!(
        asked["state"], "active",
        "a prompt is a cause, not a new word"
    );
    call(
        &router,
        "POST",
        "/terminals/idle/question",
        Some(json!({"question":"Idle?","tool_use_id":"tu-9","questions":[]})),
    )
    .await;
    call(
        &router,
        "DELETE",
        "/terminals/ask/question?tool_use_id=tu-1",
        None,
    )
    .await;
    let answered = next_live(&mut events, &h.store_id, |live| causes(live).is_empty()).await;
    assert_eq!(answered["state"], "active");

    // A permission prompt raised by the terminal's hook, matched by its session id.
    let ask_router = router.clone();
    let ask = tokio::spawn(async move {
        call(
            &ask_router,
            "POST",
            "/permissions/ask",
            Some(
                json!({"connection":"s-ask","tool":"Bash","action":"rm x","agent":"claude",
                "terminal_id":"ask","session_id":"s-ask"}),
            ),
        )
        .await
    });
    let waiting = next_live(&mut events, &h.store_id, |live| !causes(live).is_empty()).await;
    assert_eq!(causes(&waiting)[0]["prompt"], "permission");
    assert_eq!(causes(&waiting)[0]["terminal"], "ask");
    let (_, pending) = call(&router, "GET", "/permissions", None).await;
    let request = pending[0]["id"].as_u64().unwrap();
    let (status, _) = call(
        &router,
        "POST",
        &format!("/permissions/{request}"),
        Some(json!({"decision":"allow","scope":"once"})),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(ask.await.unwrap().1["decision"], "allow");
    let resolved = next_live(&mut events, &h.store_id, |live| causes(live).is_empty()).await;
    assert_eq!(resolved["state"], "active");
    for terminal in ["ask", "idle"] {
        call(&router, "DELETE", &format!("/terminals/{terminal}"), None).await;
    }
}

/// A machine server whose primary store is `demo.hs2` (A), hosting project B's checkout
/// (`project-b`, linked to `project-b.hs2`), with a `cat` terminal opened in a subdirectory
/// of that checkout (HS2-B4EQ87).
struct ProjectB {
    state: AppState,
    router: axum::Router,
    primary: FsStore,
    store: FsStore,
    id: String,
    checkout: String,
}

impl ProjectB {
    async fn open(h: &Harness, terminal: &str) -> Self {
        let primary = FsStore::open(h._dir.path().join("demo.hs2")).unwrap();
        let project = h._dir.path().join("project-b");
        std::fs::create_dir_all(project.join("src")).unwrap();
        let path = h._dir.path().join("project-b.hs2");
        let store = FsStore::init_git(&path, &StoreMetadata::new("PB")).unwrap();
        let id = hotsheet_server::multistore::store_url_id(&store);
        let state = h
            .state
            .clone()
            .with_checkout_registry(h._dir.path().join("checkouts.json"));
        let router = app(state.clone());
        let (status, opened) = call(
            &router,
            "POST",
            "/projects/open",
            Some(json!({"root": project, "stores": [path]})),
        )
        .await;
        assert!(status.is_success(), "{status} {opened}");
        let checkout = opened["checkout"]["id"].as_str().unwrap().to_string();
        // The hook that reports the AI session names no checkout: the directory decides.
        let (status, _) = call(
            &router,
            "POST",
            "/terminals",
            Some(json!({"command":"cat","id":terminal,"cwd":project.join("src")})),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        Self {
            state,
            router,
            primary,
            store,
            id,
            checkout,
        }
    }
}

/// One Claude turn in `terminal` for the session its transcript names: prompt, a transcript
/// row with one input and `tokens` output tokens, then stop.
async fn claude_turn(
    router: &axum::Router,
    terminal: &str,
    transcript: &std::path::Path,
    tokens: u64,
) {
    use std::io::Write as _;
    let session = transcript
        .file_stem()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let path = transcript.to_string_lossy().to_string();
    let endpoint = format!("/terminals/{terminal}/claude-turn");
    let turn =
        |event: &str| json!({"event": event, "session_id": session, "transcript_path": path});
    let (status, _) = call(router, "POST", &endpoint, Some(turn("start"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let row = json!({
        "type":"assistant", "timestamp":"2026-11-06T00:00:00Z",
        "message":{"id":format!("message-{tokens}"),"model":"claude-opus-4-8",
            "usage":{"input_tokens":1,"output_tokens":tokens}}
    });
    writeln!(
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(transcript)
            .unwrap(),
        "{row}"
    )
    .unwrap();
    let (status, _) = call(router, "POST", &endpoint, Some(turn("stop"))).await;
    assert_eq!(status, StatusCode::NO_CONTENT);
}

/// HS2-B4EQ87 regression: a machine server whose primary store is A hosts project B. A
/// Claude terminal opened in B's checkout records every turn in B's store (attributed to B's
/// claim when the terminal holds one), never in A, and B's live tokens per minute and the
/// project's Stats usage both show it.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_b4eq87_terminal_usage_records_in_its_project_store_not_the_primary() {
    let h = harness();
    let b = ProjectB::open(&h, "b-term").await;
    let (router, b_id, checkout) = (&b.router, b.id.clone(), b.checkout.clone());
    let mut events = b.state.subscribe();
    let (status, _) = call(
        router,
        "POST",
        "/terminals/b-term/ai-connection",
        Some(json!({"agent":"claude","session_id":"b-session"})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let transcripts = h._dir.path().join("claude").join("projects").join("b");
    std::fs::create_dir_all(&transcripts).unwrap();
    let transcript = transcripts.join("b-session.jsonl");

    // Without a claim the turn still belongs to B, unattributed.
    claude_turn(router, "b-term", &transcript, 40).await;
    let recorded = hotsheet_ticketing::metrics::read_raw(&b.store).unwrap();
    assert_eq!(recorded.len(), 1);
    assert_eq!((recorded[0].tokens_in, recorded[0].tokens_out), (1, 40));
    assert_eq!(recorded[0].ticket, None);
    let live = next_live(&mut events, &b_id, |live| {
        live["rates"]["tokens"]["minutes"]
            .as_array()
            .is_some_and(|minutes| minutes.iter().any(|minute| minute["tokens"] == 41))
    })
    .await;
    assert_eq!(live["rates"]["tokens"]["window_minutes"], 5);

    // A claim in B attributes the next turn to B's ticket.
    let (status, created) = call(
        router,
        "POST",
        &format!("/stores/{b_id}/tickets"),
        Some(json!({"title":"Project B work"})),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{created}");
    let ticket = created["id"].as_str().unwrap().to_string();
    let (status, claimed) = call(
        router,
        "POST",
        &format!("/checkouts/{checkout}/tickets/{ticket}/claim"),
        Some(json!({"worker":"terminal-b-term","lease_minutes":60})),
    )
    .await;
    assert!(status.is_success(), "{status} {claimed}");
    claude_turn(router, "b-term", &transcript, 7).await;
    let recorded = hotsheet_ticketing::metrics::read_raw(&b.store).unwrap();
    assert_eq!(recorded.len(), 2);
    assert_eq!(recorded[1].ticket.as_deref(), Some(ticket.as_str()));

    assert!(
        hotsheet_ticketing::metrics::read_raw(&b.primary)
            .unwrap()
            .is_empty(),
        "the primary store records none of B's usage"
    );
    let (status, analytics) = call(
        router,
        "GET",
        &format!("/checkouts/{checkout}/analytics"),
        None,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(analytics["stores"][0]["store_id"], b_id.as_str());
    assert_eq!(analytics["stores"][0]["usage"]["events"], 2);
    assert_eq!(analytics["stores"][0]["usage"]["tokens_out"], 47);
    call(router, "DELETE", "/terminals/b-term", None).await;
}

/// HS2-B4EQ87: a Codex terminal in project B records each rollout response in B's store,
/// attributed to the claim its session holds there, and moves B's live tokens per minute.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_b4eq87_codex_terminal_usage_records_in_its_project_store() {
    const SESSION: &str = "00000000-0000-4000-8000-0000000000b4";
    let h = harness();
    let b = ProjectB::open(&h, "codex-b").await;
    let router = &b.router;
    let mut events = b.state.subscribe();
    let (_, created) = call(
        router,
        "POST",
        &format!("/stores/{}/tickets", b.id),
        Some(json!({"title":"Codex work in B"})),
    )
    .await;
    let ticket = created["id"].as_str().unwrap().to_string();
    let (status, _) = call(
        router,
        "POST",
        &format!("/checkouts/{}/tickets/{ticket}/claim", b.checkout),
        Some(
            json!({"worker":hotsheet_aitools::session_worker_id("codex", "codex-b"),
            "lease_minutes":60}),
        ),
    )
    .await;
    assert!(status.is_success(), "{status}");
    let home = h._dir.path().join("codex-home");
    std::fs::create_dir_all(&home).unwrap();
    let (status, _) = call(
        router,
        "POST",
        "/terminals/codex-b/ai-connection",
        Some(json!({"agent":"codex","session_id":SESSION,"codex_home":home})),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let at = (time::OffsetDateTime::now_utc() + time::Duration::seconds(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let day = home.join("sessions/2026/11/06");
    std::fs::create_dir_all(&day).unwrap();
    let rows = [
        json!({"timestamp": "2026-11-06T00:00:00Z", "type": "session_meta",
            "payload": {"id": SESSION, "originator": "codex-tui"}}),
        json!({"timestamp": at, "type": "event_msg",
            "payload": {"type": "task_started", "turn_id": "t1"}}),
        json!({"timestamp": at, "type": "token_usage_record", "payload": {
            "response_id": "resp_b", "turn_id": "t1", "root_turn_id": "t1",
            "usage": {"input_tokens": 30, "output_tokens": 3}}}),
    ];
    let text: String = rows.iter().map(|row| format!("{row}\n")).collect();
    std::fs::write(
        day.join(format!("rollout-2026-11-06T00-00-00-{SESSION}.jsonl")),
        text,
    )
    .unwrap();
    next_live(&mut events, &b.id, |live| {
        live["rates"]["tokens"]["minutes"]
            .as_array()
            .is_some_and(|minutes| minutes.iter().any(|minute| minute["tokens"] == 33))
    })
    .await;
    let recorded = hotsheet_ticketing::metrics::read_raw(&b.store).unwrap();
    assert_eq!(recorded.len(), 1, "{recorded:?}");
    assert_eq!(recorded[0].ticket.as_deref(), Some(ticket.as_str()));
    assert!(
        hotsheet_ticketing::metrics::read_raw(&b.primary)
            .unwrap()
            .is_empty()
    );
    call(router, "DELETE", "/terminals/codex-b", None).await;
}
