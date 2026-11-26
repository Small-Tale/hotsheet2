//! HS2-BWJWWX: the Ably adapter against a scripted fake Ably Realtime server (a blocking
//! WebSocket server on loopback that speaks the JSON protocol: CONNECTED, ATTACH/ATTACHED,
//! MESSAGE, ERROR, DISCONNECTED), alone and through the real hub with a real registry. Synthetic
//! names and keys only; GitHub payloads are generated, never copied.

use std::collections::{BTreeMap, VecDeque};
use std::io::ErrorKind;
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, accept_hdr};

use hotsheet_notify::remote_change::ably::{AblyAdapter, Timing, validate_endpoint};
use hotsheet_notify::remote_change::github::{
    MAX_PAYLOAD_BYTES, Mapped, Skip, map_push, map_value,
};
use hotsheet_notify::remote_change::hub::{
    Backoff, ChangeListener, Hub, RecheckReason, RemoteChange,
};
use hotsheet_notify::remote_change::model::{
    AdapterError, ChangeAdapter, ConnectionContext, Event, Health, RepoIdentity,
};
use hotsheet_notify::remote_change::workflow::Workflow;
use hotsheet_ticketing::secrets::{SecretError, SecretStore};

const KEY: &str = "appid.keyid:synthetic-secret-half";
const TIP_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const TIP_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

// ---- the fake service ---------------------------------------------------------------

enum Step {
    Frame(Value),
    Pause(Duration),
    /// Drop the socket without a close frame.
    Drop,
}

#[derive(Default)]
struct Script {
    steps: Vec<Step>,
    /// Channels the key may not attach: answered with an authorization ERROR.
    deny: Vec<String>,
    /// Refuse the WebSocket handshake with this status.
    refuse: Option<u16>,
    /// Answer the connection with an ERROR frame instead of CONNECTED.
    connect_error: Option<Value>,
    /// Never answer: the connection stays silent after the handshake.
    silent: bool,
}

#[derive(Default)]
struct Server {
    scripts: Mutex<VecDeque<Script>>,
    queries: Mutex<Vec<String>>,
    attached: Mutex<Vec<Vec<String>>>,
    closed_by_client: AtomicUsize,
    connections: AtomicUsize,
}

struct Fake {
    server: Arc<Server>,
    endpoint: String,
}

impl Fake {
    fn start(scripts: Vec<Script>) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = Arc::new(Server::default());
        server.scripts.lock().unwrap().extend(scripts);
        let shared = Arc::clone(&server);
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let server = Arc::clone(&shared);
                std::thread::spawn(move || serve(&server, stream));
            }
        });
        Fake {
            server,
            endpoint: format!("ws://127.0.0.1:{port}"),
        }
    }

    fn context(&self, key: Option<&str>) -> ConnectionContext {
        let mut settings = BTreeMap::new();
        settings.insert("endpoint".to_owned(), self.endpoint.clone());
        ConnectionContext {
            connection: "ably1".into(),
            settings,
            credential: key.map(hotsheet_sync::secret::SecretString::from),
        }
    }

    fn connections(&self) -> usize {
        self.server.connections.load(Ordering::SeqCst)
    }
}

fn frame(action: u8, channel: Option<&str>) -> Value {
    let mut v = json!({ "action": action });
    if let Some(c) = channel {
        v["channel"] = json!(c);
    }
    v
}

fn message(channel: &str, id: &str, data: Value) -> Step {
    Step::Frame(json!({
        "action": 15, "channel": channel, "id": id,
        "messages": [{ "id": id, "name": "push", "data": data }],
    }))
}

fn push(name: &str, reference: &str, tip: &str) -> Value {
    json!({ "ref": reference, "after": tip, "before": "0".repeat(40), "repository": { "id": 7, "full_name": name } })
}

#[allow(
    clippy::result_large_err,
    reason = "tungstenite's handshake callback fixes its Err type (HS2-BWJWWX)"
)]
fn serve(server: &Server, stream: TcpStream) {
    server.connections.fetch_add(1, Ordering::SeqCst);
    let script = server
        .scripts
        .lock()
        .unwrap()
        .pop_front()
        .unwrap_or_default();
    let refuse = script.refuse;
    let callback = |request: &Request, response: Response| -> Result<Response, ErrorResponse> {
        server
            .queries
            .lock()
            .unwrap()
            .push(request.uri().query().unwrap_or_default().to_owned());
        match refuse {
            Some(status) => Err(tungstenite::http::Response::builder()
                .status(status)
                .body(None)
                .unwrap()),
            None => Ok(response),
        }
    };
    let Ok(mut ws) = accept_hdr(stream, callback) else {
        return;
    };
    if script.silent {
        std::thread::sleep(Duration::from_secs(5));
        return;
    }
    if let Some(error) = &script.connect_error {
        let _ = ws.send(Message::text(
            json!({ "action": 9, "error": error }).to_string(),
        ));
        return;
    }
    let _ = ws.send(Message::text(
        json!({ "action": 4, "connectionId": "synthetic" }).to_string(),
    ));
    let _ = ws
        .get_mut()
        .set_read_timeout(Some(Duration::from_millis(20)));
    let mut steps = script.steps.into_iter();
    let mut channels: Vec<String> = Vec::new();
    loop {
        match ws.read() {
            Ok(Message::Text(text)) => {
                let value: Value = serde_json::from_str(text.as_str()).unwrap();
                match value["action"].as_u64() {
                    Some(10) => {
                        let channel = value["channel"].as_str().unwrap().to_owned();
                        if script.deny.contains(&channel) {
                            let error = json!({ "code": 40160, "statusCode": 401, "message": "not permitted" });
                            let reply = json!({ "action": 9, "channel": channel, "error": error });
                            let _ = ws.send(Message::text(reply.to_string()));
                            continue;
                        }
                        channels.push(channel.clone());
                        server.attached.lock().unwrap().push(channels.clone());
                        let _ = ws.send(Message::text(frame(11, Some(&channel)).to_string()));
                    }
                    Some(7) => {
                        server.closed_by_client.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    _ => {}
                }
            }
            Ok(Message::Close(_)) => {
                server.closed_by_client.fetch_add(1, Ordering::SeqCst);
                return;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) =>
            {
                if channels.is_empty() {
                    continue;
                }
                match steps.next() {
                    Some(Step::Frame(v)) => {
                        let _ = ws.send(Message::text(v.to_string()));
                    }
                    Some(Step::Pause(d)) => std::thread::sleep(d),
                    Some(Step::Drop) => return,
                    None => {}
                }
            }
            Err(_) => return,
        }
    }
}

const FAST: Timing = Timing {
    tick: Duration::from_millis(20),
    handshake: Duration::from_secs(5),
    silence: Duration::from_millis(600),
};

fn adapter() -> AblyAdapter {
    AblyAdapter::with_timing(FAST)
}

/// Run the adapter on its own thread, collecting events until `done` holds, then stop it.
fn collect(
    fake: &Fake,
    adapter: &AblyAdapter,
    channels: &[&str],
    done: impl Fn(&[Event]) -> bool,
) -> (Vec<Event>, Result<(), AdapterError>) {
    let channels: Vec<String> = channels.iter().map(|c| (*c).to_owned()).collect();
    let context = fake.context(Some(KEY));
    let events = Mutex::new(Vec::new());
    let stop = AtomicBool::new(false);
    let result = std::thread::scope(|scope| {
        let handle = scope.spawn(|| {
            adapter.run(
                &context,
                &channels,
                &mut |e| events.lock().unwrap().push(e),
                &stop,
            )
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !handle.is_finished() && !done(&events.lock().unwrap()) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        stop.store(true, Ordering::SeqCst);
        handle.join().unwrap()
    });
    (events.into_inner().unwrap(), result)
}

fn hints(events: &[Event]) -> Vec<(&str, &hotsheet_notify::remote_change::ChangeHint)> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Hint { channel, hint } => Some((channel.as_str(), hint)),
            Event::Connected { .. } => None,
        })
        .collect()
}

// ---- payload mapping ----------------------------------------------------------------

#[test]
fn hs2_bwjwwx_a_push_payload_maps_to_a_hint_and_every_other_case_is_a_named_skip() {
    let text = push("acme/app", "refs/heads/main", TIP_A).to_string();
    match map_push(&text, Some("m1"), 42) {
        Mapped::Hint(hint) => {
            assert_eq!(
                hint.repository,
                RepoIdentity {
                    id: Some(7),
                    full_name: Some("acme/app".into())
                }
            );
            assert_eq!(
                (hint.ref_name.as_str(), hint.advertised_tip.as_deref()),
                ("refs/heads/main", Some(TIP_A))
            );
            assert_eq!((hint.event_id.as_deref(), hint.at_ms), (Some("m1"), 42));
        }
        other => panic!("{other:?}"),
    }
    let tag = push("acme/app", "refs/tags/v1", TIP_B);
    assert!(
        matches!(map_value(&tag, None, 1), Mapped::Hint(h) if h.ref_name == "refs/tags/v1"),
        "a tag is a hint; bindings decide whether it is followed"
    );

    let skip = |value: Value| match map_value(&value, None, 1) {
        Mapped::Skipped(skip) => skip,
        other => panic!("{other:?}"),
    };
    assert_eq!(
        skip(json!({ "zen": "Keep it logically awesome.", "hook_id": 1 })),
        Skip::Ping
    );
    assert_eq!(
        skip(json!({ "action": "opened", "issue": {} })),
        Skip::NotAPush
    );
    let mut deleted = push("acme/app", "refs/heads/gone", &"0".repeat(40));
    assert_eq!(skip(deleted.clone()), Skip::RefDeleted);
    deleted["after"] = json!(TIP_A);
    deleted["deleted"] = json!(true);
    assert_eq!(skip(deleted), Skip::RefDeleted);
    assert_eq!(skip(json!([1, 2])), Skip::Malformed);
    for (mutate, field) in [
        (
            json!({ "ref": 5, "after": TIP_A, "repository": { "id": 1 } }),
            "ref",
        ),
        (
            json!({ "ref": "main", "after": TIP_A, "repository": { "id": 1 } }),
            "ref",
        ),
        (
            json!({ "ref": "refs/heads/m", "repository": { "id": 1 } }),
            "after",
        ),
        (
            json!({ "ref": "refs/heads/m", "after": "not-a-sha", "repository": { "id": 1 } }),
            "after",
        ),
        (
            json!({ "ref": "refs/heads/m", "after": TIP_A }),
            "repository",
        ),
        (
            json!({ "ref": "refs/heads/m", "after": TIP_A, "repository": {} }),
            "repository",
        ),
    ] {
        assert_eq!(skip(mutate), Skip::MissingField(field));
    }
    assert!(matches!(
        map_push("not json at all", None, 1),
        Mapped::Skipped(Skip::Malformed)
    ));
    let huge = format!("{{\"pad\":\"{}\"}}", "x".repeat(MAX_PAYLOAD_BYTES));
    assert!(matches!(
        map_push(&huge, None, 1),
        Mapped::Skipped(Skip::Oversized)
    ));
}

// ---- the adapter against the fake service -------------------------------------------

#[test]
fn hs2_bwjwwx_a_connection_attaches_every_channel_and_turns_pushes_into_hints() {
    let fake = Fake::start(vec![Script {
        steps: vec![
            message(
                "code",
                "m1",
                json!(push("acme/app", "refs/heads/main", TIP_A).to_string()),
            ),
            // The same push as an already-decoded JSON object.
            message(
                "tickets",
                "m2",
                push("acme/tickets", "refs/heads/main", TIP_B),
            ),
            message(
                "code",
                "m3",
                json!({ "zen": "ping", "hook_id": 9 }).to_string().into(),
            ),
            message(
                "code",
                "m4",
                json!({ "action": "opened" }).to_string().into(),
            ),
            message(
                "code",
                "m5",
                json!(push("acme/app", "refs/heads/old", &"0".repeat(40)).to_string()),
            ),
            message("code", "m6", json!("{ not json")),
            message("code", "m7", json!(7)),
            message("code", "m8", json!("x".repeat(MAX_PAYLOAD_BYTES + 1))),
            message(
                "code",
                "m9",
                json!(json!({ "ref": "refs/heads/m", "after": TIP_A }).to_string()),
            ),
        ],
        ..Script::default()
    }]);
    let adapter = adapter();
    let (events, result) = collect(&fake, &adapter, &["code", "tickets"], |e| {
        let stats = adapter.stats.snapshot();
        e.len() >= 3 && stats.iter().map(|(_, n)| n).sum::<u64>() >= 9
    });
    assert_eq!(result, Ok(()));
    assert_eq!(
        events[0],
        Event::Connected { resumed: false },
        "announced once every channel is attached"
    );
    let seen = hints(&events);
    assert_eq!(seen.len(), 2, "{events:?}");
    assert_eq!(seen[0].0, "code");
    assert_eq!(seen[0].1.repository.full_name.as_deref(), Some("acme/app"));
    assert_eq!(seen[0].1.event_id.as_deref(), Some("m1"));
    assert_eq!(
        (seen[1].0, seen[1].1.advertised_tip.as_deref()),
        ("tickets", Some(TIP_B))
    );
    let stats: BTreeMap<_, _> = adapter.stats.snapshot().into_iter().collect();
    assert_eq!(stats["hints"], 2);
    assert_eq!(stats["pings"], 1);
    assert_eq!(stats["not_push"], 1);
    assert_eq!(stats["ref_deleted"], 1);
    assert_eq!(stats["malformed"], 2, "bad text and a non-text payload");
    assert_eq!(stats["oversized"], 1);
    assert_eq!(stats["missing_field"], 1);
    let attached = fake
        .server
        .attached
        .lock()
        .unwrap()
        .last()
        .cloned()
        .unwrap();
    assert_eq!(
        attached.len(),
        2,
        "both channels on one connection: {attached:?}"
    );
    assert_eq!(fake.connections(), 1);
    let query = fake.server.queries.lock().unwrap()[0].clone();
    assert!(
        query.contains("format=json") && query.contains("v=2"),
        "{query}"
    );
    // The server reads the CLOSE on its own thread, a moment after the adapter returned.
    let deadline = Instant::now() + Duration::from_secs(5);
    while fake.server.closed_by_client.load(Ordering::SeqCst) < 1 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        fake.server.closed_by_client.load(Ordering::SeqCst),
        1,
        "a stop closes politely"
    );
}

#[test]
fn hs2_bwjwwx_a_refused_handshake_connect_error_or_denied_channel_is_an_auth_failure_without_the_key()
 {
    let cases: Vec<(Script, &str)> = vec![
        (
            Script {
                refuse: Some(401),
                ..Script::default()
            },
            "HTTP 401",
        ),
        (
            Script {
                connect_error: Some(
                    json!({ "code": 40101, "statusCode": 401, "message": "Invalid key" }),
                ),
                ..Script::default()
            },
            "code 40101",
        ),
        (
            Script {
                deny: vec!["code".into()],
                ..Script::default()
            },
            "'code' failed",
        ),
    ];
    for (script, needle) in cases {
        let fake = Fake::start(vec![script]);
        let (_, result) = collect(&fake, &adapter(), &["code"], |_| false);
        match result {
            Err(AdapterError::Auth(message)) => {
                assert!(message.contains(needle), "{needle}: {message}");
                assert!(
                    !message.contains("synthetic-secret-half") && !message.contains(KEY),
                    "{message}"
                );
            }
            other => panic!("{needle}: {other:?}"),
        }
        assert!(
            fake.server
                .queries
                .lock()
                .unwrap()
                .iter()
                .all(|q| q.contains("key=")),
            "the key travels in the connect query only"
        );
    }
}

#[test]
fn hs2_bwjwwx_dropped_closed_silent_and_unreachable_connections_are_transient() {
    let cases: Vec<(Script, &str)> = vec![
        (
            Script {
                steps: vec![Step::Drop],
                ..Script::default()
            },
            "",
        ),
        (
            Script {
                steps: vec![Step::Frame(
                    json!({ "action": 6, "error": { "code": 80003, "message": "retry" } }),
                )],
                ..Script::default()
            },
            "code 80003",
        ),
        (
            Script {
                steps: vec![Step::Frame(json!({ "action": 13, "channel": "code" }))],
                ..Script::default()
            },
            "was detached",
        ),
        (
            Script {
                steps: vec![Step::Pause(Duration::from_secs(5))],
                ..Script::default()
            },
            "no traffic",
        ),
        (
            Script {
                silent: true,
                ..Script::default()
            },
            "did not confirm",
        ),
    ];
    for (script, needle) in cases {
        let fake = Fake::start(vec![script]);
        let adapter = AblyAdapter::with_timing(Timing {
            handshake: Duration::from_millis(500),
            ..FAST
        });
        let (_, result) = collect(&fake, &adapter, &["code"], |_| false);
        match result {
            Err(AdapterError::Transient(message)) => {
                assert!(message.contains(needle), "{needle}: {message}")
            }
            other => panic!("{needle}: {other:?}"),
        }
    }
    // Nothing is listening at all.
    let dead = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = dead.local_addr().unwrap().port();
    drop(dead);
    let mut context = ConnectionContext {
        connection: "c".into(),
        settings: BTreeMap::new(),
        credential: Some(KEY.into()),
    };
    context
        .settings
        .insert("endpoint".into(), format!("ws://127.0.0.1:{port}"));
    let result = adapter().run(
        &context,
        &["code".into()],
        &mut |_| {},
        &AtomicBool::new(false),
    );
    assert!(
        matches!(result, Err(AdapterError::Transient(m)) if !m.contains("synthetic-secret-half"))
    );
}

#[test]
fn hs2_bwjwwx_a_test_connects_and_closes_without_attaching_and_reports_failures() {
    let fake = Fake::start(vec![
        Script::default(),
        Script {
            refuse: Some(401),
            ..Script::default()
        },
    ]);
    assert_eq!(adapter().test(&fake.context(Some(KEY))), Ok(()));
    assert!(
        fake.server.attached.lock().unwrap().is_empty(),
        "a test attaches nothing"
    );
    assert!(matches!(
        adapter().test(&fake.context(Some(KEY))),
        Err(AdapterError::Auth(_))
    ));
    assert!(matches!(
        adapter().test(&fake.context(None)),
        Err(AdapterError::Config(_))
    ));
}

#[test]
fn hs2_bwjwwx_endpoints_are_checked_and_tls_is_built_with_an_explicit_provider() {
    for good in [
        "wss://main.realtime.ably.net",
        "wss://eu.example.net:8443",
        "ws://127.0.0.1:9",
        "ws://localhost",
    ] {
        assert!(validate_endpoint(good).is_ok(), "{good}");
    }
    for bad in [
        "ws://example.net",
        "https://main.realtime.ably.net",
        "wss://user:pw@host",
        "wss://host/path",
        "wss://host?key=x",
        "wss://host#f",
        "wss://",
        "wss://ho st",
    ] {
        assert!(validate_endpoint(bad).is_err(), "{bad}");
    }
    let mut context = ConnectionContext {
        connection: "c".into(),
        settings: BTreeMap::new(),
        credential: Some(KEY.into()),
    };
    context
        .settings
        .insert("endpoint".into(), "ws://example.net".into());
    let refused = adapter().test(&context).unwrap_err();
    assert!(matches!(refused, AdapterError::Config(_)));
    // Building the wss configuration must not panic for want of a process-wide crypto provider.
    assert!(AblyAdapter::tls_config().is_ok());
}

// ---- through the real hub -----------------------------------------------------------

#[derive(Default)]
struct Memory(Mutex<BTreeMap<String, String>>);

impl SecretStore for Memory {
    fn set(&self, account: &str, secret: &str) -> Result<(), SecretError> {
        self.0.lock().unwrap().insert(account.into(), secret.into());
        Ok(())
    }
    fn get(&self, account: &str) -> Result<Option<String>, SecretError> {
        Ok(self.0.lock().unwrap().get(account).cloned())
    }
    fn delete(&self, account: &str) -> Result<bool, SecretError> {
        Ok(self.0.lock().unwrap().remove(account).is_some())
    }
}

#[derive(Default)]
struct Recorder {
    changes: Mutex<Vec<RemoteChange>>,
    rechecks: Mutex<Vec<String>>,
    health: Mutex<Vec<Health>>,
}

impl ChangeListener for Recorder {
    fn changed(&self, change: &RemoteChange) {
        self.changes.lock().unwrap().push(change.clone());
    }
    fn recheck(&self, source: &str, _: RecheckReason) {
        self.rechecks.lock().unwrap().push(source.to_owned());
    }
    fn health(&self, _: &str, health: &Health) {
        self.health.lock().unwrap().push(health.clone());
    }
}

fn wait_for(what: &str, condition: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(15);
    while !condition() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn hs2_bwjwwx_one_connection_serves_the_code_checkout_and_two_ticket_stores_and_survives_a_disconnect()
 {
    let fake = Fake::start(vec![
        // First connection: two pushes, a duplicate of the first, a push for another repository,
        // then the socket drops without a close.
        Script {
            steps: vec![
                message(
                    "repo-events",
                    "m1",
                    json!(push("acme/app", "refs/heads/main", TIP_A).to_string()),
                ),
                message(
                    "repo-events",
                    "m2",
                    json!(push("acme/tickets", "refs/heads/main", TIP_B).to_string()),
                ),
                message(
                    "repo-events",
                    "m3",
                    json!(push("acme/app", "refs/heads/main", TIP_A).to_string()),
                ),
                message(
                    "repo-events",
                    "m4",
                    json!(push("evil/app", "refs/heads/main", TIP_B).to_string()),
                ),
                Step::Pause(Duration::from_millis(100)),
                Step::Drop,
            ],
            ..Script::default()
        },
        // The reconnection: a fresh push.
        Script {
            steps: vec![message(
                "repo-events",
                "m5",
                json!(
                    push(
                        "acme/app",
                        "refs/heads/main",
                        "cccccccccccccccccccccccccccccccccccccccc"
                    )
                    .to_string()
                ),
            )],
            ..Script::default()
        },
    ]);
    let home = tempfile::tempdir().unwrap();
    let secrets = Arc::new(Memory::default());
    let recorder = Arc::new(Recorder::default());
    let adapters: Vec<Arc<dyn ChangeAdapter>> = vec![Arc::new(adapter())];
    let wf = Workflow {
        home: home.path(),
        adapters: &adapters,
        secrets: secrets.as_ref(),
    };
    let mut settings = BTreeMap::new();
    settings.insert("endpoint".to_owned(), fake.endpoint.clone());
    let (_, next) = wf.add_connection("shared", "ably", settings).unwrap();
    assert!(next[0].contains("connection secret shared"), "{next:?}");
    assert_eq!(wf.test("shared").unwrap().code, Some("missing_credential"));
    wf.set_secret("shared", Some(KEY)).unwrap();
    for (source, repository) in [
        ("checkout:code", "acme/app"),
        ("store:tickets-a", "acme/tickets"),
        ("store:tickets-b", "acme/tickets"),
    ] {
        let identity = RepoIdentity {
            id: None,
            full_name: Some(repository.into()),
        };
        wf.bind(source, "shared", "repo-events", identity, vec![])
            .unwrap();
    }
    let hub = Hub::start(
        home.path().to_path_buf(),
        adapters.clone(),
        secrets.clone(),
        recorder.clone(),
        Backoff {
            start: Duration::from_millis(20),
            cap: Duration::from_millis(100),
        },
        Duration::ZERO,
        Arc::new(|| {
            u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis(),
            )
            .unwrap()
        }),
    );
    hub.reload().unwrap();
    // Three sources, one channel: one attachment per connection, and the second connection is
    // the reconnect after the drop.
    wait_for("the push after the reconnect", || {
        recorder
            .changes
            .lock()
            .unwrap()
            .iter()
            .any(|c| c.event_id.as_deref() == Some("m5"))
    });
    let changes = recorder.changes.lock().unwrap().clone();
    let by = |source: &str| changes.iter().filter(|c| c.source == source).count();
    assert_eq!(
        by("checkout:code"),
        2,
        "m1, then m5 after the reconnect; the duplicate m3 and the foreign m4 never arrive: {changes:?}"
    );
    assert_eq!(by("store:tickets-a"), 1);
    assert_eq!(by("store:tickets-b"), 1);
    assert_eq!(
        fake.server
            .attached
            .lock()
            .unwrap()
            .iter()
            .filter(|c| c.len() == 1)
            .count(),
        2,
        "the same single channel, attached on each connection"
    );
    let mut rechecks = recorder.rechecks.lock().unwrap().clone();
    rechecks.sort();
    // Every source is checked when the connection first comes up and again after the drop.
    assert_eq!(rechecks.len(), 6, "{rechecks:?}");
    assert_eq!(rechecks.iter().filter(|s| *s == "checkout:code").count(), 2);
    let health = recorder.health.lock().unwrap().clone();
    assert!(
        health
            .iter()
            .any(|h| matches!(h, Health::Reconnecting { .. })),
        "{health:?}"
    );
    assert!(
        health.iter().any(|h| matches!(h, Health::Connected { .. })),
        "{health:?}"
    );
    assert_eq!(hub.ignored()["wrong_repository"], 1);
    assert!(hub.ignored()["duplicate"] >= 1);
    hub.shutdown();
}

#[test]
fn hs2_bwjwwx_a_refused_key_through_the_hub_stops_without_a_retry_loop() {
    let fake = Fake::start(vec![
        Script {
            refuse: Some(401),
            ..Script::default()
        },
        Script::default(),
    ]);
    let home = tempfile::tempdir().unwrap();
    let secrets = Arc::new(Memory::default());
    let recorder = Arc::new(Recorder::default());
    let adapters: Vec<Arc<dyn ChangeAdapter>> = vec![Arc::new(adapter())];
    let wf = Workflow {
        home: home.path(),
        adapters: &adapters,
        secrets: secrets.as_ref(),
    };
    wf.add_connection(
        "shared",
        "ably",
        [("endpoint".to_owned(), fake.endpoint.clone())].into(),
    )
    .unwrap();
    wf.set_secret("shared", Some(KEY)).unwrap();
    wf.bind(
        "checkout:code",
        "shared",
        "ch",
        RepoIdentity {
            id: None,
            full_name: Some("acme/app".into()),
        },
        vec![],
    )
    .unwrap();
    let hub = Hub::start(
        home.path().to_path_buf(),
        adapters.clone(),
        secrets.clone(),
        recorder.clone(),
        Backoff {
            start: Duration::from_millis(20),
            cap: Duration::from_millis(50),
        },
        Duration::ZERO,
        Arc::new(|| 0),
    );
    hub.reload().unwrap();
    wait_for("auth failure", || {
        matches!(hub.health().get("shared"), Some(Health::AuthFailed { .. }))
    });
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(fake.connections(), 1, "a refused key is not retried");
    let Some(Health::AuthFailed { error }) = hub.health().get("shared").cloned() else {
        unreachable!()
    };
    assert!(!error.contains("synthetic-secret-half"), "{error}");
    hub.shutdown();
}
