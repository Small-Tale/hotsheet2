//! HS2-S8R3PD: the server refuses host-acting routes to a remote browser session without the
//! `local` grant, whatever path the request took (the bridge's project-API proxy forwards any
//! route). A `local` session and the machine-secret bearer keep them.
//!
//! No request here may start a host program: allowed calls name an unknown checkout, a
//! missing store, or use the attachment `path` action, which only reports the host path.
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:4175";
const SECRET: &str = "synthetic-local-secret";

struct Fixture {
    dir: tempfile::TempDir,
    router: axum::Router,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("store"), &StoreMetadata::new("SYN")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(dir.path().join("home"))
        .with_checkout_registry(dir.path().join("home/checkouts.json"))
        .with_remote_auth(dir.path().join("home/remote-auth.json"))
        .unwrap();
    Fixture {
        router: app(state),
        dir,
    }
}

/// Who sends a request: the trusted host's machine secret, or a paired browser session.
#[derive(Clone, Copy)]
enum As<'a> {
    Bearer,
    Session { cookie: &'a str, csrf: &'a str },
}

fn request(method: &str, path: &str, body: Option<Value>, caller: As<'_>) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN);
    req = match caller {
        As::Bearer => req.header("x-hotsheet-secret", SECRET),
        As::Session { cookie, csrf } => req
            .header(header::COOKIE, cookie)
            .header("x-hotsheet-csrf", csrf),
    };
    match body {
        Some(value) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    }
}

async fn send(router: &axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = router.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

/// Pair one read-write browser with the given grants and return its cookie and CSRF token.
async fn session(router: &axum::Router, label: &str, local: bool) -> (String, String) {
    let (_, pair) = send(
        router,
        Request::builder()
            .method("POST")
            .uri("/remote-auth/pair")
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(json!({ "label": label }).to_string()))
            .unwrap(),
    )
    .await;
    let id = pair["id"].as_str().unwrap();
    let challenge = pair["challenge"].as_str().unwrap();
    let (approved, _) = send(
        router,
        request(
            "POST",
            "/remote-auth/owner/approve",
            Some(json!({ "id": id, "role": "read_write", "local": local })),
            As::Bearer,
        ),
    )
    .await;
    assert_eq!(approved, StatusCode::NO_CONTENT);
    let redeemed = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/remote-auth/redeem")
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({ "id": id, "challenge": challenge }).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(redeemed.status(), StatusCode::OK);
    let cookies = redeemed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| {
            let text = value.to_str().unwrap();
            text.split(';').next().unwrap().to_owned()
        })
        .collect::<Vec<_>>();
    let csrf = cookies[1].split_once('=').unwrap().1.to_owned();
    (cookies[0].clone(), csrf)
}

fn assert_refused(status: StatusCode, body: &Value, route: &str) {
    assert_eq!(status, StatusCode::FORBIDDEN, "{route}: {body}");
    assert_eq!(body["code"], "host_access_required", "{route}: {body}");
}

fn assert_admitted(status: StatusCode, body: &Value, route: &str) {
    assert_ne!(body["code"], "host_access_required", "{route}: {body}");
    assert_ne!(status, StatusCode::UNAUTHORIZED, "{route}: {body}");
    assert_ne!(status, StatusCode::FORBIDDEN, "{route}: {body}");
}

/// Every host-acting route, with a body that would be accepted if the guard admitted it but
/// that cannot reach a host program (unknown checkout or a path that does not exist).
fn host_routes(missing: &str) -> Vec<(&'static str, String, Value)> {
    vec![
        (
            "POST",
            "/checkouts/missing-checkout/tickets/SYN-1/code-review".into(),
            json!({ "mode": "commit", "commit": "0000000" }),
        ),
        (
            "POST",
            "/checkouts/missing-checkout/repository/review".into(),
            json!({ "mode": "commit", "commit": "0000000" }),
        ),
        (
            "POST",
            "/checkouts/missing-checkout/repository/files/action".into(),
            json!({ "path": "README.md", "action": "open" }),
        ),
        (
            "POST",
            "/checkouts/missing-checkout/repository/files/action".into(),
            json!({ "path": "README.md", "action": "reveal" }),
        ),
        (
            "POST",
            "/checkouts/missing-checkout/tickets/SYN-1/attachments/01M79EQJ6QWJM8Y670YBP1X03H/action"
                .into(),
            json!({ "action": "open" }),
        ),
        (
            "POST",
            "/checkouts/missing-checkout/tickets/SYN-1/attachments/by-name/proof.txt/action".into(),
            json!({ "action": "reveal" }),
        ),
        (
            "POST",
            "/projects/open".into(),
            json!({ "root": missing, "stores": [] }),
        ),
        ("POST", "/checkouts".into(), json!({ "root": missing })),
        ("POST", "/stores".into(), json!({ "path": missing })),
        (
            "PUT",
            "/checkouts/missing-checkout/sources/synthetic".into(),
            json!({ "provider": "git", "locator": missing }),
        ),
        (
            "PATCH",
            "/checkouts/missing-checkout/sources/synthetic/relink".into(),
            json!({ "path": missing }),
        ),
    ]
}

#[tokio::test]
async fn hs2_s8r3pd_host_routes_need_the_local_grant_or_the_local_bearer() {
    let fixture = fixture();
    let router = &fixture.router;
    let missing = fixture.dir.path().join("does-not-exist");
    let missing = missing.to_str().unwrap();
    let (plain, plain_csrf) = session(router, "Synthetic phone", false).await;
    let (local, local_csrf) = session(router, "Synthetic Mac", true).await;
    let plain = As::Session {
        cookie: &plain,
        csrf: &plain_csrf,
    };
    let local = As::Session {
        cookie: &local,
        csrf: &local_csrf,
    };
    for (method, path, body) in host_routes(missing) {
        let route = format!("{method} {path} {body}");
        let (status, refused) =
            send(router, request(method, &path, Some(body.clone()), plain)).await;
        assert_refused(status, &refused, &route);
        let (session_status, session_body) =
            send(router, request(method, &path, Some(body.clone()), local)).await;
        assert_admitted(session_status, &session_body, &route);
        let (bearer_status, bearer_body) =
            send(router, request(method, &path, Some(body), As::Bearer)).await;
        assert_admitted(bearer_status, &bearer_body, &route);
        // The guard only decides admission: past it, a local session and the host bearer
        // reach the same downstream answer.
        assert_eq!(session_status, bearer_status, "{route}");
    }
    // The refusal does not depend on the body: a malformed one is still refused first.
    let (status, body) = send(
        router,
        request(
            "POST",
            "/checkouts/missing-checkout/repository/files/action",
            Some(json!({ "unexpected": true })),
            plain,
        ),
    )
    .await;
    assert_refused(status, &body, "malformed file action");
    // Nothing above registered a checkout or hosted a store at the missing path.
    assert!(!fixture.dir.path().join("does-not-exist").exists());
}

#[tokio::test]
async fn hs2_s8r3pd_project_routes_that_stay_on_the_server_remain_remote_features() {
    let fixture = fixture();
    let router = &fixture.router;
    let (plain, plain_csrf) = session(router, "Synthetic phone", false).await;
    let plain = As::Session {
        cookie: &plain,
        csrf: &plain_csrf,
    };
    // Reads, code-review discovery and a provider (non-Git) source link are not host
    // actions: the guard must not refuse them.
    for (method, path, body) in [
        ("GET", "/checkouts", None),
        ("GET", "/stores", None),
        (
            "GET",
            "/checkouts/missing-checkout/tickets/SYN-1/code-review",
            None,
        ),
        (
            "PUT",
            "/checkouts/missing-checkout/sources/synthetic",
            Some(json!({ "provider": "github", "locator": "synthetic/repository" })),
        ),
    ] {
        let (status, response) = send(router, request(method, path, body, plain)).await;
        assert_ne!(
            status,
            StatusCode::UNAUTHORIZED,
            "{method} {path}: {response}"
        );
        assert_ne!(
            response["code"], "host_access_required",
            "{method} {path}: {response}"
        );
    }
}

#[tokio::test]
async fn hs2_s8r3pd_attachment_path_action_end_to_end_per_caller() {
    let fixture = fixture();
    let router = &fixture.router;
    let checkout = fixture.dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).unwrap();
    let store = fixture.dir.path().join("store");
    let (registered, registration) = send(
        router,
        request(
            "POST",
            "/checkouts",
            Some(json!({ "root": checkout, "alias": "demo", "stores": [store] })),
            As::Bearer,
        ),
    )
    .await;
    assert_eq!(registered, StatusCode::CREATED, "{registration}");
    let (_, created) = send(
        router,
        request(
            "POST",
            "/checkouts/demo/tickets",
            Some(json!({ "title": "Synthetic attachment holder" })),
            As::Bearer,
        ),
    )
    .await;
    let slug = created["slug"].as_str().unwrap().to_owned();
    let (attached, _) = send(
        router,
        Request::builder()
            .method("POST")
            .uri(format!("/checkouts/demo/tickets/{slug}/attachments"))
            .header("x-hotsheet-secret", SECRET)
            .header("x-hotsheet-filename", "proof.txt")
            .body(Body::from("synthetic evidence"))
            .unwrap(),
    )
    .await;
    assert!(attached.is_success());
    let (plain, plain_csrf) = session(router, "Synthetic phone", false).await;
    let (local, local_csrf) = session(router, "Synthetic Mac", true).await;
    let path = format!("/checkouts/demo/tickets/{slug}/attachments/by-name/proof.txt/action");
    let action = json!({ "action": "path" });
    let (status, body) = send(
        router,
        request(
            "POST",
            &path,
            Some(action.clone()),
            As::Session {
                cookie: &plain,
                csrf: &plain_csrf,
            },
        ),
    )
    .await;
    assert_refused(status, &body, "plain session attachment path");
    assert!(body.get("path").is_none(), "{body}");
    for caller in [
        As::Session {
            cookie: &local,
            csrf: &local_csrf,
        },
        As::Bearer,
    ] {
        let (status, body) =
            send(router, request("POST", &path, Some(action.clone()), caller)).await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert!(
            body["path"].as_str().unwrap().ends_with("proof.txt"),
            "{body}"
        );
    }
    // A plain session still reads the attachment itself over HTTP; only the host action is refused.
    let (read, _) = send(
        router,
        request(
            "GET",
            &format!("/checkouts/demo/tickets/{slug}/attachments/by-name/proof.txt"),
            None,
            As::Session {
                cookie: &plain,
                csrf: &plain_csrf,
            },
        ),
    )
    .await;
    assert_eq!(read, StatusCode::OK);
}
