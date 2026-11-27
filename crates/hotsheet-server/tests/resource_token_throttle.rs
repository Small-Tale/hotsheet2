//! HS2-JFEMAR: the unauthenticated renewal-token route bounds the failed attempts one peer may
//! make, answers the rest `429 rate_limited` before reading the token, never counts a valid
//! token, and recovers when the window ends. A real server socket with connect info, as the
//! real listener has. Synthetic names only.

use std::net::SocketAddr;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use hotsheet_resource_claims::ClaimService;
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use serde_json::{Value, json};
use tokio::net::TcpListener;

const WINDOW: Duration = Duration::from_millis(900);
const BUDGET: u32 = 5;

struct Server {
    _root: tempfile::TempDir,
    home: std::path::PathBuf,
    base: String,
}

async fn start() -> Server {
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    let store = FsStore::init_git(root.path().join("demo.hs2"), &StoreMetadata::new("HS")).unwrap();
    let state = AppState::new(store, "secret".into())
        .unwrap()
        .with_machine_home(&home)
        .with_default_codex_home(root.path().join("no-codex-home"))
        .with_token_failure_budget(BUDGET, WINDOW);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move {
        axum::serve(
            listener,
            app(state).into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await
        .unwrap();
    });
    Server {
        _root: root,
        home,
        base,
    }
}

fn forged(target: &str) -> String {
    format!(
        "hsrc1.{}.{}.{}",
        B64.encode(target),
        ulid::Ulid::new(),
        B64.encode([7u8; 32])
    )
}

struct Reply {
    status: u16,
    retry_after: Option<String>,
    body: Value,
}

fn renew(base: &str, authorization: Option<&str>) -> Reply {
    let mut request = hotsheet_http::build(hotsheet_http::config().http_status_as_error(false))
        .post(&format!("{base}/resource-claim-token/renew"))
        .header("Content-Type", "application/json");
    if let Some(value) = authorization {
        request = request.header("Authorization", value);
    }
    let response = request
        .send("{\"duration_ms\": 60000}")
        .unwrap_or_else(|error| panic!("{error}"));
    let status = response.status().as_u16();
    let retry_after = hotsheet_http::header(&response, "Retry-After").map(str::to_owned);
    let body = serde_json::from_str(&response.into_body().read_to_string().unwrap())
        .unwrap_or(Value::Null);
    Reply {
        status,
        retry_after,
        body,
    }
}

fn bearer(token: &str) -> String {
    format!("Bearer {token}")
}

fn acquire(home: &std::path::Path, seconds: u64) -> String {
    ClaimService::new(home.to_path_buf())
        .request(
            "POST",
            "/resource-claims/vms/one/acquire",
            &json!({ "duration_ms": seconds * 1000 }),
        )
        .unwrap()["renewal_token"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jfemar_a_flood_of_wrong_tokens_is_evaluated_only_up_to_the_budget_then_refused_cheaply()
 {
    let server = start().await;
    let base = server.base.clone();
    let replies = tokio::task::spawn_blocking(move || {
        (0..40)
            .map(|_| renew(&base, Some(&bearer(&forged("vms/one")))))
            .collect::<Vec<_>>()
    })
    .await
    .unwrap();
    let evaluated = replies.iter().filter(|r| r.status != 429).count();
    assert_eq!(
        evaluated, BUDGET as usize,
        "only the budget's worth of attempts reached the claim state"
    );
    for refused in replies.iter().filter(|r| r.status == 429) {
        assert_eq!(refused.body["code"], "rate_limited", "{}", refused.body);
        assert!(
            refused
                .retry_after
                .as_deref()
                .is_some_and(|s| s.parse::<u64>().is_ok_and(|n| (1..=1).contains(&n))),
            "{:?}",
            refused.retry_after
        );
        assert_eq!(refused.body["retry_after_seconds"], 1);
        assert!(
            !refused.body.to_string().contains("hsrc1"),
            "no token is ever echoed"
        );
    }
    assert!(
        replies[..BUDGET as usize]
            .iter()
            .all(|r| matches!(r.status, 403 | 409)),
        "the evaluated ones are the usual refusals"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jfemar_a_valid_token_is_never_counted_and_a_throttled_peer_recovers_when_the_window_ends()
 {
    let server = start().await;
    let token = acquire(&server.home, 600);
    let base = server.base.clone();
    let token_clone = token.clone();
    tokio::task::spawn_blocking(move || {
        // Valid renewals never consume the budget, however many there are.
        for _ in 0..(BUDGET * 4) {
            assert_eq!(renew(&base, Some(&bearer(&token_clone))).status, 200);
        }
        // This peer then burns its own budget with wrong tokens (and a missing header counts too).
        for n in 0..BUDGET {
            let reply = if n == 0 {
                renew(&base, None)
            } else {
                renew(&base, Some(&bearer(&forged("vms/one"))))
            };
            assert!(matches!(reply.status, 401 | 403 | 409), "{}", reply.status);
        }
        // Throttled: even its valid token waits (the work of checking is what is bounded).
        assert_eq!(renew(&base, Some(&bearer(&token_clone))).status, 429);
        // The window ends and the valid token works again: delayed, never lost.
        std::thread::sleep(WINDOW + Duration::from_millis(200));
        assert_eq!(renew(&base, Some(&bearer(&token_clone))).status, 200);
        // A fresh window starts from zero.
        assert!(matches!(
            renew(&base, Some(&bearer(&forged("vms/one")))).status,
            403 | 409
        ));
    })
    .await
    .unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn hs2_jfemar_an_expired_lease_proves_the_token_so_it_is_not_a_failed_guess() {
    let server = start().await;
    let token = acquire(&server.home, 1);
    std::thread::sleep(Duration::from_millis(1200));
    let base = server.base.clone();
    tokio::task::spawn_blocking(move || {
        for _ in 0..(BUDGET * 3) {
            let reply = renew(&base, Some(&bearer(&token)));
            assert_eq!(
                reply.status, 410,
                "an expired holder keeps hearing expired, never rate_limited: {}",
                reply.body
            );
        }
    })
    .await
    .unwrap();
}
