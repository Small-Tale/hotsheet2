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
    // An ordinary session is authenticated but lacks the admin grant (HS2-DXM0YN).
    assert_eq!(owner_from_session.status(), StatusCode::FORBIDDEN);
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

/// Pair one browser end to end through the owner secret and return its session cookie,
/// CSRF token and device JSON.
async fn paired(router: &axum::Router, label: &str, approval: Value) -> (String, String, Value) {
    let id = pending(router, label).await;
    let mut body = approval;
    body["id"] = json!(id.0);
    let approved = router
        .clone()
        .oneshot(request(
            "POST",
            "/remote-auth/owner/approve",
            Some(body),
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
            Some(json!({"id": id.0, "challenge": id.1})),
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
        .map(|value| {
            value
                .to_str()
                .unwrap()
                .split(';')
                .next()
                .unwrap()
                .to_owned()
        })
        .collect::<Vec<_>>();
    let csrf = cookies[1].split_once('=').unwrap().1.to_owned();
    (cookies[0].clone(), csrf, json_body(redeemed).await)
}

/// Request pairing and return the pending id and challenge.
async fn pending(router: &axum::Router, label: &str) -> (String, String) {
    let pair = json_body(
        router
            .clone()
            .oneshot(request(
                "POST",
                "/remote-auth/pair",
                Some(json!({"label": label})),
                None,
                None,
                false,
            ))
            .await
            .unwrap(),
    )
    .await;
    (
        pair["id"].as_str().unwrap().to_owned(),
        pair["challenge"].as_str().unwrap().to_owned(),
    )
}

async fn status(router: &axum::Router, request: Request<Body>) -> StatusCode {
    router.clone().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn hs2_85w6be_dxm0yn_admin_and_local_grants_gate_device_management() {
    let (_dir, state) = fixture();
    let router = app(state);
    let (plain, plain_csrf, plain_device) =
        paired(&router, "Synthetic phone", json!({"role": "read_write"})).await;
    assert_eq!(
        (plain_device["admin"].clone(), plain_device["local"].clone()),
        (json!(false), json!(false))
    );
    // A read-only admin still manages devices: admin is independent of the read role.
    let (admin, admin_csrf, admin_device) = paired(
        &router,
        "Synthetic tablet",
        json!({"role": "read_only", "admin": true}),
    )
    .await;
    let (host, host_csrf, host_device) = paired(
        &router,
        "Synthetic Mac",
        json!({"role": "read_write", "admin": true, "local": true}),
    )
    .await;
    assert_eq!(
        (host_device["admin"].clone(), host_device["local"].clone()),
        (json!(true), json!(true))
    );
    let me = json_body(
        router
            .clone()
            .oneshot(request(
                "GET",
                "/remote-auth/me",
                None,
                Some(&host),
                None,
                false,
            ))
            .await
            .unwrap(),
    )
    .await;
    assert_eq!(
        (me["admin"].clone(), me["local"].clone()),
        (json!(true), json!(true))
    );

    // An ordinary session can neither read nor change the device list.
    let target = pending(&router, "Synthetic laptop").await.0;
    let admin_path = format!(
        "/remote-auth/owner/devices/{}",
        admin_device["id"].as_str().unwrap()
    );
    for (method, path, body) in [
        ("GET", "/remote-auth/owner".to_owned(), None),
        ("GET", "/remote-auth/owner/changes?after=1".to_owned(), None),
        (
            "POST",
            "/remote-auth/owner/approve".to_owned(),
            Some(json!({"id": target, "role": "read_only"})),
        ),
        (
            "POST",
            "/remote-auth/owner/reject".to_owned(),
            Some(json!({"id": target})),
        ),
        ("PATCH", admin_path.clone(), Some(json!({"admin": false}))),
        ("DELETE", admin_path.clone(), None),
    ] {
        let code = status(
            &router,
            request(method, &path, body, Some(&plain), Some(&plain_csrf), false),
        )
        .await;
        assert_eq!(code, StatusCode::FORBIDDEN, "{method} {path}");
    }

    // An admin session lists devices and names itself as the manager.
    let listed = router
        .clone()
        .oneshot(request(
            "GET",
            "/remote-auth/owner",
            None,
            Some(&admin),
            None,
            false,
        ))
        .await
        .unwrap();
    assert_eq!(listed.status(), StatusCode::OK);
    let listed = json_body(listed).await;
    assert_eq!(listed["manager"]["device_id"], admin_device["id"]);
    assert_eq!(listed["manager"]["local"], json!(false));
    assert_eq!(listed["pair_ttl_seconds"], json!(900));
    assert_eq!(listed["devices"].as_array().unwrap().len(), 3);
    assert_eq!(listed["pending"].as_array().unwrap().len(), 1);

    // Changes need the paired Origin and the CSRF token.
    let reject_target = json!({"id": target});
    assert_eq!(
        status(
            &router,
            request(
                "POST",
                "/remote-auth/owner/reject",
                Some(reject_target.clone()),
                Some(&admin),
                None,
                false,
            ),
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let mut foreign = request(
        "POST",
        "/remote-auth/owner/reject",
        Some(reject_target),
        Some(&admin),
        Some(&admin_csrf),
        false,
    );
    foreign
        .headers_mut()
        .insert(header::ORIGIN, "https://attacker.invalid".parse().unwrap());
    assert_eq!(status(&router, foreign).await, StatusCode::UNAUTHORIZED);

    // A non-local admin approves and changes admin, but never grants host access.
    let admin_change = |method: &str, path: &str, body: Value| {
        request(
            method,
            path,
            Some(body),
            Some(&admin),
            Some(&admin_csrf),
            false,
        )
    };
    assert_eq!(
        status(
            &router,
            admin_change(
                "POST",
                "/remote-auth/owner/approve",
                json!({"id": target, "role": "read_write", "local": true}),
            ),
        )
        .await,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        status(
            &router,
            admin_change(
                "POST",
                "/remote-auth/owner/approve",
                json!({"id": target, "role": "read_write", "admin": true}),
            ),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(
            &router,
            admin_change("PATCH", &admin_path, json!({"local": true}))
        )
        .await,
        StatusCode::FORBIDDEN
    );
    let plain_path = format!(
        "/remote-auth/owner/devices/{}",
        plain_device["id"].as_str().unwrap()
    );
    let promoted = router
        .clone()
        .oneshot(admin_change("PATCH", &plain_path, json!({"admin": true})))
        .await
        .unwrap();
    assert_eq!(promoted.status(), StatusCode::OK);
    assert_eq!(json_body(promoted).await["admin"], json!(true));
    assert_eq!(
        status(
            &router,
            admin_change("PATCH", &plain_path, json!({"owner": true}))
        )
        .await,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        status(
            &router,
            admin_change(
                "PATCH",
                "/remote-auth/owner/devices/missing",
                json!({"admin": true})
            ),
        )
        .await,
        StatusCode::NOT_FOUND
    );
    // Revoking every session stays with the trusted host.
    assert_eq!(
        status(
            &router,
            request(
                "POST",
                "/remote-auth/owner/revoke-all",
                None,
                Some(&admin),
                Some(&admin_csrf),
                false,
            ),
        )
        .await,
        StatusCode::UNAUTHORIZED
    );

    // A host-trusted admin session may grant and remove host access.
    let granted = router
        .clone()
        .oneshot(request(
            "PATCH",
            &plain_path,
            Some(json!({"local": true, "role": "read_only"})),
            Some(&host),
            Some(&host_csrf),
            false,
        ))
        .await
        .unwrap();
    assert_eq!(granted.status(), StatusCode::OK);
    let granted = json_body(granted).await;
    assert_eq!(
        (granted["local"].clone(), granted["role"].clone()),
        (json!(true), json!("read_only"))
    );
    let stranger = json!({"id": pending(&router, "Synthetic stranger").await.0});
    let host_reject = || {
        request(
            "POST",
            "/remote-auth/owner/reject",
            Some(stranger.clone()),
            Some(&host),
            Some(&host_csrf),
            false,
        )
    };
    assert_eq!(status(&router, host_reject()).await, StatusCode::NO_CONTENT);
    assert_eq!(status(&router, host_reject()).await, StatusCode::NOT_FOUND);
    assert_eq!(
        status(
            &router,
            request(
                "DELETE",
                &plain_path,
                None,
                Some(&admin),
                Some(&admin_csrf),
                false
            ),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        status(
            &router,
            request("GET", "/remote-auth/me", None, Some(&plain), None, false)
        )
        .await,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn hs2_dxm0yn_device_list_long_poll_wakes_on_change_and_rechecks_access() {
    let (_dir, state) = fixture();
    let router = app(state);
    let (admin, _csrf, admin_device) = paired(
        &router,
        "Synthetic tablet",
        json!({"role": "read_write", "admin": true}),
    )
    .await;
    let changes = |after: u64| {
        request(
            "GET",
            &format!("/remote-auth/owner/changes?after={after}"),
            None,
            Some(&admin),
            None,
            false,
        )
    };
    let first = json_body(
        router
            .clone()
            .oneshot(request(
                "GET",
                "/remote-auth/owner",
                None,
                Some(&admin),
                None,
                false,
            ))
            .await
            .unwrap(),
    )
    .await;
    let revision = first["revision"].as_u64().unwrap();
    // A stale cursor (for example from before a server restart) answers at once.
    let stale = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        router.clone().oneshot(changes(revision.wrapping_sub(3))),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(stale.status(), StatusCode::OK);
    // A current cursor waits for a new pairing request, then reports it.
    let waiting = tokio::spawn(router.clone().oneshot(changes(revision)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    assert!(!waiting.is_finished());
    pending(&router, "Synthetic phone").await;
    let woke = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(woke.status(), StatusCode::OK);
    let woke = json_body(woke).await;
    let next = woke["revision"].as_u64().unwrap();
    assert_ne!(next, revision);
    assert_eq!(woke["pending"][0]["label"], "Synthetic phone");
    // A session revoked while it waits receives no list.
    let waiting = tokio::spawn(router.clone().oneshot(changes(next)));
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    let revoke = format!(
        "/remote-auth/owner/devices/{}",
        admin_device["id"].as_str().unwrap()
    );
    assert_eq!(
        status(&router, request("DELETE", &revoke, None, None, None, true)).await,
        StatusCode::NO_CONTENT
    );
    let denied = tokio::time::timeout(std::time::Duration::from_secs(5), waiting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
}
