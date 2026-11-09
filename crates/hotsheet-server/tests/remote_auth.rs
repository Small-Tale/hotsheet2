//! HS2-7DBJMS: real router coverage for the local-owner / remote-browser boundary.
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const ORIGIN: &str = "http://localhost:4175";
const SECRET: &str = "synthetic-local-secret";

fn fixture() -> (tempfile::TempDir, AppState) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(dir.path().join("store"), &StoreMetadata::new("SYN")).unwrap();
    let state = AppState::new(store, SECRET.into())
        .unwrap()
        .with_machine_home(dir.path().join("home"))
        .with_remote_auth(dir.path().join("home/remote-auth.json"))
        .unwrap();
    (dir, state)
}

fn request(
    method: &str,
    path: &str,
    body: Option<Value>,
    cookie: Option<&str>,
    csrf: Option<&str>,
    owner: bool,
) -> Request<Body> {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header(header::ORIGIN, ORIGIN);
    if let Some(cookie) = cookie {
        req = req.header(header::COOKIE, cookie);
    }
    if let Some(csrf) = csrf {
        req = req.header("x-hotsheet-csrf", csrf);
    }
    if owner {
        req = req.header("x-hotsheet-secret", SECRET);
    }
    match body {
        Some(value) => req
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(value.to_string()))
            .unwrap(),
        None => req.body(Body::empty()).unwrap(),
    }
}

async fn json_body(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}

#[tokio::test]
async fn hs2_7dbjms_pair_approve_redeem_role_replay_revoke() {
    let (_dir, state) = fixture();
    let router = app(state);
    let denied = router
        .clone()
        .oneshot(request("GET", "/checkouts", None, None, None, false))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);

    let pair = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/pair",
            Some(json!({"label":"Synthetic tablet"})),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(pair.status(), StatusCode::CREATED);
    let pair = json_body(pair).await;
    let id = pair["id"].as_str().unwrap();
    let challenge = pair["challenge"].as_str().unwrap();
    let redeem_body = json!({"id":id,"challenge":challenge});
    let early = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/redeem",
            Some(redeem_body.clone()),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(early.status(), StatusCode::CONFLICT);
    let unauthorized_approve = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/owner/approve",
            Some(json!({"id":id,"role":"read_only"})),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(unauthorized_approve.status(), StatusCode::UNAUTHORIZED);
    let approved = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/owner/approve",
            Some(json!({"id":id,"role":"read_only"})),
            None,
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(approved.status(), StatusCode::NO_CONTENT);
    let redeemed = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/redeem",
            Some(redeem_body.clone()),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(redeemed.status(), StatusCode::OK);
    let cookies = redeemed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    assert!(cookies[0].contains("HttpOnly; SameSite=Strict"));
    assert!(!cookies[0].contains("Domain="));
    assert!(!cookies[0].contains("Secure"));
    let session = cookies[0].split(';').next().unwrap();
    let csrf = cookies[1]
        .split('=')
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap();
    let device = json_body(redeemed).await;
    assert_eq!(device["role"], "read_only");

    let replay = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/redeem",
            Some(redeem_body),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    let read = router
        .clone()
        .oneshot(request(
            "GET",
            "/checkouts",
            None,
            Some(session),
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(read.status(), StatusCode::OK);
    let write = router
        .clone()
        .oneshot(request(
            "POST",
            "/stores",
            Some(json!({})),
            Some(session),
            Some(csrf),
            false,
        ))
        .await
        .unwrap();
    assert_eq!(write.status(), StatusCode::FORBIDDEN);
    let owner_from_session = router
        .clone()
        .oneshot(request(
            "GET",
            "/remote-auth/owner",
            None,
            Some(session),
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(owner_from_session.status(), StatusCode::UNAUTHORIZED);
    let revoked = router
        .clone()
        .oneshot(request(
            "DELETE",
            &format!(
                "/remote-auth/owner/devices/{}",
                device["id"].as_str().unwrap()
            ),
            None,
            None,
            None,
            true,
        ))
        .await
        .unwrap();
    assert_eq!(revoked.status(), StatusCode::NO_CONTENT);
    let after = router
        .oneshot(request(
            "GET",
            "/checkouts",
            None,
            Some(session),
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(after.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn hs2_7dbjms_origin_csrf_rotation_logout_and_restart() {
    let (dir, state) = fixture();
    let router = app(state);
    let pair = json_body(
        router
            .clone()
            .oneshot(request(
                "POST",
                "/remote-auth/pair",
                Some(json!({"label":"Synthetic laptop"})),
                None,
                None,
                false,
            ))
            .await
            .unwrap(),
    )
    .await;
    let id = pair["id"].as_str().unwrap();
    let challenge = pair["challenge"].as_str().unwrap();
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "POST",
                "/remote-auth/owner/approve",
                Some(json!({"id":id,"role":"read_write"})),
                None,
                None,
                true
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    let redeemed = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/redeem",
            Some(json!({"id":id,"challenge":challenge})),
            None,
            None,
            false,
        ))
        .await
        .unwrap();
    let cookies = redeemed
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let session = cookies[0].split(';').next().unwrap().to_owned();
    let csrf = cookies[1]
        .split('=')
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();

    let mut foreign = request("GET", "/checkouts", None, Some(&session), None, false);
    foreign
        .headers_mut()
        .insert(header::ORIGIN, "http://foreign.invalid".parse().unwrap());
    assert_eq!(
        router.clone().oneshot(foreign).await.unwrap().status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "POST",
                "/stores",
                Some(json!({})),
                Some(&session),
                None,
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        router
            .clone()
            .oneshot(request(
                "POST",
                "/stores",
                Some(json!({})),
                Some(&session),
                Some("wrong"),
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
    let allowed = router
        .clone()
        .oneshot(request(
            "POST",
            "/stores",
            Some(json!({})),
            Some(&session),
            Some(&csrf),
            false,
        ))
        .await
        .unwrap();
    assert_ne!(allowed.status(), StatusCode::UNAUTHORIZED);
    assert_ne!(allowed.status(), StatusCode::FORBIDDEN);

    let reopened = AppState::new(
        FsStore::open(dir.path().join("store")).unwrap(),
        SECRET.into(),
    )
    .unwrap()
    .with_machine_home(dir.path().join("home"))
    .with_remote_auth(dir.path().join("home/remote-auth.json"))
    .unwrap();
    let restarted = app(reopened);
    assert_eq!(
        restarted
            .clone()
            .oneshot(request(
                "GET",
                "/checkouts",
                None,
                Some(&session),
                None,
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );

    let rotated = restarted
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/rotate",
            None,
            Some(&session),
            Some(&csrf),
            false,
        ))
        .await
        .unwrap();
    assert_eq!(rotated.status(), StatusCode::OK);
    let cookies = rotated
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_owned())
        .collect::<Vec<_>>();
    let next_session = cookies[0].split(';').next().unwrap().to_owned();
    let next_csrf = cookies[1]
        .split('=')
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    assert_eq!(
        restarted
            .clone()
            .oneshot(request(
                "GET",
                "/checkouts",
                None,
                Some(&session),
                None,
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        restarted
            .clone()
            .oneshot(request(
                "GET",
                "/checkouts",
                None,
                Some(&next_session),
                None,
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        restarted
            .clone()
            .oneshot(request(
                "POST",
                "/remote-auth/logout",
                None,
                Some(&next_session),
                Some(&next_csrf),
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        restarted
            .oneshot(request(
                "GET",
                "/checkouts",
                None,
                Some(&next_session),
                None,
                false
            ))
            .await
            .unwrap()
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
