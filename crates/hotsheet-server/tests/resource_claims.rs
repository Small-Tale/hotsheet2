//! HS2-FMHH9Y: the resource-claim routes on a real server socket, the renewal-token
//! route's isolation from every other authority, long-poll waiting, and the MCP shim's
//! HTTP backend as a waiting client. Synthetic names only.

use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use axum::extract::Request;
use axum::middleware::{self, Next};
use hotsheet_mcp::{Backend, HttpBackend, handle_message};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const SECRET: &str = "test-secret";
const PROMPT: Duration = Duration::from_secs(30);

struct Server {
    _root: tempfile::TempDir,
    home: std::path::PathBuf,
    base: String,
    /// Every request the server received, as `METHOD path`.
    seen: Arc<Mutex<Vec<String>>>,
}

async fn server() -> Server {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = FsStore::init_git(root.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(&home)
        .with_default_codex_home(root.path().join("no-codex-home"));
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    let router = app(state).layer(middleware::from_fn(move |request: Request, next: Next| {
        record
            .lock()
            .unwrap()
            .push(format!("{} {}", request.method(), request.uri().path()));
        next.run(request)
    }));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    Server {
        _root: root,
        home,
        base,
        seen,
    }
}

/// One request with exactly the given headers; no credential is added implicitly.
fn http(method: &str, url: &str, headers: &[(&str, &str)], body: Option<Value>) -> (u16, Value) {
    let mut request = ureq::request(method, url);
    for (name, value) in headers {
        request = request.set(name, value);
    }
    let response = match body {
        Some(body) => request
            .set("content-type", "application/json")
            .send_string(&body.to_string()),
        None => request.call(),
    };
    let response = match response {
        Ok(response) => response,
        Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("{error}"),
    };
    let status = response.status();
    let text = response.into_string().unwrap();
    (status, serde_json::from_str(&text).unwrap_or(Value::Null))
}

fn with_secret(method: &str, url: &str, body: Option<Value>) -> (u16, Value) {
    http(method, url, &[("x-hotsheet-secret", SECRET)], body)
}

fn acquire(base: &str, target: &str, body: Value) -> Value {
    let url = format!("{base}/resource-claims/{target}/acquire");
    let (status, answer) = with_secret("POST", &url, Some(body));
    assert_eq!(status, 200, "{answer}");
    answer
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn until_waiters(home: &Path, count: usize) {
    let waiters = home.join("resource-claims/namespaces/vms/waiters");
    let started = Instant::now();
    while std::fs::read_dir(&waiters).map_or(0, Iterator::count) != count {
        assert!(started.elapsed() < PROMPT, "never saw {count} waiters");
        std::thread::yield_now();
    }
}

fn tool(backend: &dyn Backend, arguments: Value) -> Result<Value, String> {
    let reply = handle_message(
        &json!({"jsonrpc":"2.0","id":1,"method":"tools/call",
                "params":{"name":"hotsheet_resource_claim","arguments":arguments}}),
        backend,
    )
    .unwrap();
    let text = reply["result"]["content"][0]["text"].as_str().unwrap();
    if reply["result"]["isError"] == true {
        Err(text.to_owned())
    } else {
        Ok(serde_json::from_str(text).unwrap())
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_claim_routes_need_the_server_secret() {
    let server = server().await;
    let base = server.base.clone();
    tokio::task::spawn_blocking(move || {
        let grant = acquire(
            &base,
            "vms/mac-1",
            json!({"duration_ms": 600_000, "metadata": {"note": "ui"}}),
        );
        assert_eq!(grant["acquired"], true);
        assert_eq!(grant["fencing_token"], 1);
        assert_eq!(grant["metadata"]["note"], "plain:ui");
        let token = grant["renewal_token"].as_str().unwrap().to_owned();

        let refused = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000}));
        assert_eq!(refused["acquired"], false);
        assert_eq!(refused["code"], "held_by");
        assert_eq!(refused["holder"]["claim_id"], grant["claim_id"]);

        let (status, shown) =
            with_secret("GET", &format!("{base}/resource-claims/vms/mac-1"), None);
        assert_eq!(status, 200);
        assert_eq!(shown["held"], true);
        let (status, listed) = with_secret("GET", &format!("{base}/resource-claims/vms"), None);
        assert_eq!(status, 200);
        assert_eq!(listed["claims"].as_array().unwrap().len(), 1);
        assert!(!shown.to_string().contains(&token) && !listed.to_string().contains(&token));

        // No credential, and a renewal token offered as the credential: both refused on
        // every secret-authenticated route, claim routes and ticket routes alike.
        let token_header = bearer(&token);
        for headers in [
            vec![],
            vec![("authorization", token_header.as_str())],
            vec![("x-hotsheet-secret", token.as_str())],
        ] {
            for (method, path, body) in [
                ("GET", "/resource-claims/vms", None),
                ("GET", "/resource-claims/vms/mac-1", None),
                (
                    "POST",
                    "/resource-claims/vms/mac-2/acquire",
                    Some(json!({"duration_ms": 1000})),
                ),
                (
                    "POST",
                    "/resource-claims/vms/mac-1/wait",
                    Some(json!({"wait_ms": 1})),
                ),
                (
                    "POST",
                    "/resource-claims/vms/mac-1/renew",
                    Some(json!({"renewal_token": token, "duration_ms": 1000})),
                ),
                (
                    "POST",
                    "/resource-claims/vms/mac-1/release",
                    Some(json!({"renewal_token": token})),
                ),
                ("GET", "/tickets", None),
                ("POST", "/tickets", Some(json!({"title": "x"}))),
                ("GET", "/checkouts", None),
            ] {
                let (status, _) = http(method, &format!("{base}{path}"), &headers, body);
                assert_eq!(
                    status,
                    401,
                    "{method} {path} with {} header(s)",
                    headers.len()
                );
            }
        }
        // Nothing above changed the claim or created another.
        let (_, listed) = with_secret("GET", &format!("{base}/resource-claims/vms"), None);
        assert_eq!(listed["claims"].as_array().unwrap().len(), 1);
        assert_eq!(listed["claims"][0]["expires_at_ms"], grant["expires_at_ms"]);

        // With the secret, the targeted routes check the token against the path.
        let renew = format!("{base}/resource-claims/vms/mac-1/renew");
        let (status, renewed) = with_secret(
            "POST",
            &renew,
            Some(json!({"renewal_token": token, "duration_ms": 900_000})),
        );
        assert_eq!(status, 200, "{renewed}");
        assert!(renewed["claim"]["expires_at_ms"].as_u64() > grant["expires_at_ms"].as_u64());
        let other = format!("{base}/resource-claims/vms/mac-2/release");
        let (status, wrong) = with_secret("POST", &other, Some(json!({"renewal_token": token})));
        assert_eq!(status, 403);
        assert_eq!(wrong["code"], "token_invalid");
        let (status, bad) = with_secret("POST", &renew, Some(json!({"renewal_token": token})));
        assert_eq!((status, bad["code"].as_str()), (400, Some("invalid")));
        let bad_name = format!("{base}/resource-claims/VMS/mac-1");
        assert_eq!(with_secret("GET", &bad_name, None).0, 400);
        let release = format!("{base}/resource-claims/vms/mac-1/release");
        let (status, released) =
            with_secret("POST", &release, Some(json!({"renewal_token": token})));
        assert_eq!((status, released["released"].as_bool()), (200, Some(true)));
        let (status, revoked) =
            with_secret("POST", &release, Some(json!({"renewal_token": token})));
        assert_eq!(
            (status, revoked["code"].as_str()),
            (403, Some("token_invalid"))
        );
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_a_renewal_token_alone_renews_and_releases_only_its_own_claim() {
    let server = server().await;
    let base = server.base.clone();
    tokio::task::spawn_blocking(move || {
        let one = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000}));
        let two = acquire(&base, "vms/mac-2", json!({"duration_ms": 600_000}));
        let token_one = one["renewal_token"].as_str().unwrap().to_owned();
        let token_two = two["renewal_token"].as_str().unwrap().to_owned();
        let renew = format!("{base}/resource-claim-token/renew");
        let release = format!("{base}/resource-claim-token/release");
        let auth_one = bearer(&token_one);

        // A guest with only the token: no server secret anywhere in the request.
        let (status, renewed) = http(
            "POST",
            &renew,
            &[("authorization", &auth_one)],
            Some(json!({"duration_ms": 1_200_000})),
        );
        assert_eq!(status, 200, "{renewed}");
        assert_eq!(renewed["claim"]["claim_id"], one["claim_id"]);
        assert_eq!(renewed["claim"]["fencing_token"], 1);
        assert!(renewed["claim"]["expires_at_ms"].as_u64() > one["expires_at_ms"].as_u64());
        assert!(!renewed.to_string().contains(&token_one), "the token is never echoed");

        // The body cannot name a different token or target: only the bearer counts.
        let (status, still_one) = http(
            "POST",
            &renew,
            &[("authorization", &auth_one)],
            Some(json!({"duration_ms": 1_200_000, "renewal_token": token_two, "target": "vms/mac-2"})),
        );
        assert_eq!(status, 200);
        assert_eq!(still_one["claim"]["claim_id"], one["claim_id"]);

        // Missing, malformed, or wrong credentials.
        let parts_one: Vec<&str> = token_one.split('.').collect();
        let parts_two: Vec<&str> = token_two.split('.').collect();
        let forged = bearer(&[parts_two[0], parts_two[1], parts_two[2], parts_one[3]].join("."));
        let long = bearer(&"a".repeat(2_000));
        let cases: Vec<(Vec<(&str, &str)>, u16)> = vec![
            (vec![], 401),
            (vec![("x-hotsheet-secret", SECRET)], 401),
            (vec![("authorization", "Bearer ")], 401),
            (vec![("authorization", token_one.as_str())], 401),
            (vec![("authorization", long.as_str())], 401),
            (vec![("authorization", "Bearer test-secret")], 403),
            (vec![("authorization", "Bearer hsrc1.a.b.c")], 403),
            (vec![("authorization", forged.as_str())], 403),
        ];
        for (headers, expected) in cases {
            for url in [&renew, &release] {
                let (status, answer) = http("POST", url, &headers, Some(json!({"duration_ms": 1000})));
                assert_eq!(status, expected, "{headers:?} {answer}");
                assert_eq!(answer["code"], "token_invalid");
            }
        }
        // The token route offers nothing but renew and release.
        for action in ["acquire", "show", "list", "wait"] {
            let url = format!("{base}/resource-claim-token/{action}");
            let (status, answer) = http(
                "POST",
                &url,
                &[("authorization", &auth_one)],
                Some(json!({"duration_ms": 1000, "wait_ms": 1})),
            );
            assert_eq!((status, answer["code"].as_str()), (501, Some("unsupported")), "{action}");
        }
        assert_eq!(http("GET", &renew, &[("authorization", &auth_one)], None).0, 405);
        let (status, bad) = http("POST", &renew, &[("authorization", &auth_one)], None);
        assert_eq!((status, bad["code"].as_str()), (400, Some("invalid")), "renew needs a duration");

        // Through all of that, claim two was never touched.
        let (_, listed) = with_secret("GET", &format!("{base}/resource-claims/vms"), None);
        let claims = listed["claims"].as_array().unwrap();
        assert_eq!(claims.len(), 2);
        assert_eq!(claims[1]["claim_id"], two["claim_id"]);
        assert_eq!(claims[1]["expires_at_ms"], two["expires_at_ms"]);

        // Release with an empty body, then the token is revoked.
        let (status, released) = http("POST", &release, &[("authorization", &auth_one)], None);
        assert_eq!(status, 200, "{released}");
        assert_eq!(released["released"], true);
        for url in [&renew, &release] {
            let (status, revoked) =
                http("POST", url, &[("authorization", &auth_one)], Some(json!({"duration_ms": 1000})));
            assert_eq!((status, revoked["code"].as_str()), (403, Some("token_invalid")));
        }
        // After someone else takes the resource, the old token is a stale holder.
        let next = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000}));
        assert_eq!(next["fencing_token"], 2);
        let (status, stale) = http("POST", &release, &[("authorization", &auth_one)], None);
        assert_eq!((status, stale["code"].as_str()), (409, Some("not_holder")));
        let (_, listed) = with_secret("GET", &format!("{base}/resource-claims/vms"), None);
        assert_eq!(listed["claims"].as_array().unwrap().len(), 2);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_one_long_poll_request_waits_for_release_expiry_or_timeout() {
    let server = server().await;
    let (base, home, seen) = (
        server.base.clone(),
        server.home.clone(),
        server.seen.clone(),
    );
    tokio::task::spawn_blocking(move || {
        let acquires = || {
            seen.lock()
                .unwrap()
                .iter()
                .filter(|line| line.ends_with("/acquire") || line.ends_with("/wait"))
                .count()
        };
        // Satisfied by a release.
        let held = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000}));
        let before = acquires();
        let waiter_base = base.clone();
        let started = Instant::now();
        let waiter = std::thread::spawn(move || {
            acquire(
                &waiter_base,
                "vms/mac-1",
                json!({"duration_ms": 1_500, "wait_ms": 50_000}),
            )
        });
        until_waiters(&home, 1);
        let release = format!("{base}/resource-claim-token/release");
        let auth = bearer(held["renewal_token"].as_str().unwrap());
        assert_eq!(
            http("POST", &release, &[("authorization", &auth)], None).0,
            200
        );
        let second = waiter.join().unwrap();
        assert!(started.elapsed() < PROMPT, "woken by the release");
        assert_eq!(second["acquired"], true);
        assert_eq!(second["fencing_token"], 2);
        assert_eq!(acquires() - before, 1, "one request for the whole wait");

        // Satisfied by expiry of that 1.5 s lease.
        let before = acquires();
        let third = acquire(
            &base,
            "vms/mac-1",
            json!({"duration_ms": 600_000, "wait_ms": 50_000}),
        );
        assert_eq!(third["acquired"], true);
        assert_eq!(third["fencing_token"], 3);
        assert!(third["acquired_at_ms"].as_u64() >= second["expires_at_ms"].as_u64());
        assert_eq!(acquires() - before, 1);

        // Times out as an answer naming the holder.
        let started = Instant::now();
        let refused = acquire(
            &base,
            "vms/mac-1",
            json!({"duration_ms": 1000, "wait_ms": 700}),
        );
        assert!(started.elapsed() >= Duration::from_millis(700));
        assert_eq!(refused["acquired"], false);
        assert_eq!(refused["code"], "wait_timeout");
        assert_eq!(refused["holder"]["claim_id"], third["claim_id"]);

        // The wait route reports the holder on timeout and free after a release.
        let wait = format!("{base}/resource-claims/vms/mac-1/wait");
        let (status, still) = with_secret("POST", &wait, Some(json!({"wait_ms": 300})));
        assert_eq!((status, still["held"].as_bool()), (200, Some(true)));
        assert_eq!(still["claim"]["claim_id"], third["claim_id"]);
        let wait_url = wait.clone();
        let watcher = std::thread::spawn(move || {
            with_secret("POST", &wait_url, Some(json!({"wait_ms": 50_000})))
        });
        until_waiters(&home, 1);
        let auth = bearer(third["renewal_token"].as_str().unwrap());
        assert_eq!(
            http("POST", &release, &[("authorization", &auth)], None).0,
            200
        );
        let (status, freed) = watcher.join().unwrap();
        assert_eq!((status, freed["held"].as_bool()), (200, Some(false)));
        let (status, bad) = with_secret("POST", &wait, Some(json!({})));
        assert_eq!((status, bad["code"].as_str()), (400, Some("invalid")));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_a_waiting_mcp_client_sends_one_request_and_none_while_idle() {
    let server = server().await;
    let (base, home, seen) = (
        server.base.clone(),
        server.home.clone(),
        server.seen.clone(),
    );
    tokio::task::spawn_blocking(move || {
        let backend = HttpBackend::new(&base, SECRET);
        let held = tool(
            &backend,
            json!({"action":"acquire","target":"vms/mac-1","duration":"10m",
            "metadata":{"worker":"hmac-sha256:agent-1"},"actor_role":"ai","actor_id":"agent-1"}),
        )
        .unwrap();
        assert_eq!(held["acquired"], true);
        let token = held["renewal_token"].as_str().unwrap().to_owned();
        let refused = tool(
            &backend,
            json!({"action":"acquire","target":"vms/mac-1","duration":"10m"}),
        )
        .unwrap();
        assert_eq!(refused["code"], "held_by");
        assert_eq!(refused["holder"]["claim_id"], held["claim_id"]);
        let listed = tool(&backend, json!({"action":"list","namespace":"vms"})).unwrap();
        assert_eq!(listed["claims"].as_array().unwrap().len(), 1);

        // Idle wait: for 1.5 s nothing changes. The waiting client must not poll.
        seen.lock().unwrap().clear();
        let started = Instant::now();
        let timed_out = tool(
            &backend,
            json!({"action":"acquire","target":"vms/mac-1","duration":"10m","wait":"1500ms"}),
        )
        .unwrap();
        assert!(started.elapsed() >= Duration::from_millis(1_500));
        assert_eq!(timed_out["code"], "wait_timeout");
        assert_eq!(
            *seen.lock().unwrap(),
            ["POST /resource-claims/vms/mac-1/acquire"],
            "one request for the whole idle wait, none in between"
        );

        // A wait ended by a release is still one request from the waiter.
        seen.lock().unwrap().clear();
        let waiter_base = base.clone();
        let waiter = std::thread::spawn(move || {
            tool(
                &HttpBackend::new(&waiter_base, SECRET),
                json!({"action":"acquire","target":"vms/mac-1","duration":"10m","wait":"5m"}),
            )
        });
        until_waiters(&home, 1);
        let renewed = tool(
            &backend,
            json!({"action":"renew","renewal_token":token,"duration":"20m"}),
        )
        .unwrap();
        assert_eq!(renewed["claim"]["claim_id"], held["claim_id"]);
        let wrong = tool(
            &backend,
            json!({"action":"release","target":"vms/mac-2","renewal_token":token}),
        )
        .unwrap_err();
        assert!(wrong.contains("token_invalid"), "{wrong}");
        tool(&backend, json!({"action":"release","renewal_token":token})).unwrap();
        let granted = waiter.join().unwrap().unwrap();
        assert_eq!(granted["acquired"], true);
        assert_eq!(granted["fencing_token"], 2);
        let requests = seen.lock().unwrap().clone();
        assert_eq!(
            requests,
            [
                "POST /resource-claims/vms/mac-1/acquire",
                "POST /resource-claims/vms/mac-1/renew",
                "POST /resource-claims/vms/mac-1/release",
            ]
        );
        // The waiter holds the resource now, so the released token is a stale holder.
        let stale = tool(&backend, json!({"action":"release","renewal_token":token})).unwrap_err();
        assert!(
            stale.contains("409") && stale.contains("not_holder"),
            "{stale}"
        );
        let shown = tool(&backend, json!({"action":"show","target":"vms/mac-1"})).unwrap();
        assert_eq!(shown["claim"]["claim_id"], granted["claim_id"]);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_a_grant_for_a_client_that_left_is_released_at_once() {
    let server = server().await;
    let (base, home) = (server.base.clone(), server.home.clone());
    tokio::task::spawn_blocking(move || {
        let held = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000}));

        // A waiting client that disconnects mid-wait, asking for a ten-minute lease.
        let address = base.trim_start_matches("http://");
        let mut socket = std::net::TcpStream::connect(address).unwrap();
        let body = json!({"duration_ms": 600_000, "wait_ms": 50_000}).to_string();
        write!(
            socket,
            "POST /resource-claims/vms/mac-1/acquire HTTP/1.1\r\nHost: {address}\r\nX-Hotsheet-Secret: {SECRET}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
            body.len()
        )
        .unwrap();
        until_waiters(&home, 1);
        socket.shutdown(std::net::Shutdown::Both).unwrap();
        let mut rest = Vec::new();
        let _ = socket.read_to_end(&mut rest);
        drop(socket);
        // `/health` counts parked long waits; zero means the server dropped the request.
        let started = Instant::now();
        while http("GET", &format!("{base}/health"), &[], None).1["api_offload"]["long_waits"] != 0 {
            assert!(started.elapsed() < PROMPT, "server never noticed the disconnect");
            std::thread::yield_now();
        }

        let release = format!("{base}/resource-claim-token/release");
        let auth = bearer(held["renewal_token"].as_str().unwrap());
        assert_eq!(http("POST", &release, &[("authorization", &auth)], None).0, 200);

        // The departed request may still win the resource, but must not keep it: a
        // live client gets a turn long before that ten-minute lease could lapse.
        let mine = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000, "wait_ms": 20_000}));
        assert_eq!(mine["acquired"], true, "{mine}");
        let auth = bearer(mine["renewal_token"].as_str().unwrap());
        assert_eq!(http("POST", &release, &[("authorization", &auth)], None).0, 200);
        until_waiters(&home, 0);
        let last = acquire(&base, "vms/mac-1", json!({"duration_ms": 1_000, "wait_ms": 20_000}));
        assert_eq!(last["acquired"], true, "{last}");
        // Four grants in all: the holder, the departed request, and the two above.
        assert_eq!(last["fencing_token"], 4);
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_parked_requests_are_bounded() {
    let server = server().await;
    let (base, home) = (server.base.clone(), server.home.clone());
    tokio::task::spawn_blocking(move || {
        let held = acquire(&base, "vms/mac-1", json!({"duration_ms": 600_000}));
        let parked: Vec<_> = (0..64)
            .map(|_| {
                let base = base.clone();
                std::thread::spawn(move || {
                    acquire(
                        &base,
                        "vms/mac-1",
                        json!({"duration_ms": 600_000, "wait_ms": 4_000}),
                    )
                })
            })
            .collect();
        until_waiters(&home, 64);

        let url = format!("{base}/resource-claims/vms/mac-1/acquire");
        let (status, full) = with_secret(
            "POST",
            &url,
            Some(json!({"duration_ms": 1000, "wait_ms": 1000})),
        );
        assert_eq!((status, full["code"].as_str()), (503, Some("capacity")));
        // A request that does not wait is unaffected by the bound.
        let refused = acquire(&base, "vms/mac-1", json!({"duration_ms": 1000}));
        assert_eq!(refused["code"], "held_by");

        let release = format!("{base}/resource-claim-token/release");
        let auth = bearer(held["renewal_token"].as_str().unwrap());
        assert_eq!(
            http("POST", &release, &[("authorization", &auth)], None).0,
            200
        );
        let answers: Vec<Value> = parked
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        let granted = answers
            .iter()
            .filter(|answer| answer["acquired"] == true)
            .count();
        let timed_out = answers
            .iter()
            .filter(|answer| answer["code"] == "wait_timeout")
            .count();
        assert_eq!(
            (granted, timed_out),
            (1, 63),
            "exactly one waiter takes the release"
        );
    })
    .await
    .unwrap();
}

/// On a listener exposed to a guest network, a non-loopback peer cannot use the server
/// secret, but its renewal token still renews and releases its own claim.
#[tokio::test(flavor = "multi_thread")]
async fn hs2_fmhh9y_a_guest_peer_on_a_remote_listener_has_only_its_token() {
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::Request as HttpRequest;
    use tower::ServiceExt;

    let root = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(root.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(root.path().join("home"))
        .with_default_codex_home(root.path().join("no-codex-home"))
        .with_remote_listener();
    let router = app(state);
    let call = |peer: &str, path: &str, headers: Vec<(&str, String)>, body: Value| {
        let peer: std::net::SocketAddr = peer.parse().unwrap();
        let mut request = HttpRequest::builder()
            .method("POST")
            .uri(path)
            .header("content-type", "application/json");
        for (name, value) in headers {
            request = request.header(name, value);
        }
        let mut request = request.body(Body::from(body.to_string())).unwrap();
        request.extensions_mut().insert(ConnectInfo(peer));
        let router = router.clone();
        async move {
            let response = router.oneshot(request).await.unwrap();
            let status = response.status().as_u16();
            let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap();
            (
                status,
                serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null),
            )
        }
    };
    const HOST: &str = "127.0.0.1:50000";
    const GUEST: &str = "192.168.64.2:50000";
    let secret = || vec![("x-hotsheet-secret", SECRET.to_owned())];
    let acquire = "/resource-claims/vms/mac-1/acquire";

    let (status, grant) = call(HOST, acquire, secret(), json!({"duration_ms": 600_000})).await;
    assert_eq!(
        (status, grant["acquired"].as_bool()),
        (200, Some(true)),
        "{grant}"
    );
    let token = grant["renewal_token"].as_str().unwrap().to_owned();
    let auth = || vec![("authorization", bearer(&token))];

    // The guest cannot use the secret-authenticated routes, even knowing the secret.
    let (status, _) = call(GUEST, acquire, secret(), json!({"duration_ms": 600_000})).await;
    assert_eq!(status, 401);
    let release = "/resource-claims/vms/mac-1/release";
    let (status, _) = call(GUEST, release, secret(), json!({"renewal_token": token})).await;
    assert_eq!(status, 401);
    let (status, _) = call(GUEST, acquire, auth(), json!({"duration_ms": 600_000})).await;
    assert_eq!(status, 401, "the token is no credential for acquire");

    // Its token alone keeps and ends its own lease.
    let (status, renewed) = call(
        GUEST,
        "/resource-claim-token/renew",
        auth(),
        json!({"duration_ms": 900_000}),
    )
    .await;
    assert_eq!(status, 200, "{renewed}");
    assert_eq!(renewed["claim"]["claim_id"], grant["claim_id"]);
    let (status, released) = call(GUEST, "/resource-claim-token/release", auth(), json!({})).await;
    assert_eq!((status, released["released"].as_bool()), (200, Some(true)));
    let (status, revoked) = call(GUEST, "/resource-claim-token/release", auth(), json!({})).await;
    assert_eq!(
        (status, revoked["code"].as_str()),
        (403, Some("token_invalid"))
    );
}
