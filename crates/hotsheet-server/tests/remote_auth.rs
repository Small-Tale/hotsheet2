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
    // A write the session may make; `/stores` names a host path and needs host access since
    // HS2-S8R3PD, so it no longer proves CSRF acceptance for a plain session.
    let allowed = router
        .clone()
        .oneshot(request(
            "POST",
            "/checkouts/missing-checkout/close",
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

    // HS2-KTHQE5: any admin, with or without host access itself, approves with and changes
    // both grants.
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
                json!({"id": target, "role": "read_write", "admin": true, "local": true}),
            ),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let plain_path = format!(
        "/remote-auth/owner/devices/{}",
        plain_device["id"].as_str().unwrap()
    );
    for local in [true, false] {
        let changed = router
            .clone()
            .oneshot(admin_change("PATCH", &plain_path, json!({"local": local})))
            .await
            .unwrap();
        assert_eq!(changed.status(), StatusCode::OK);
        assert_eq!(json_body(changed).await["local"], json!(local));
    }
    // The admin may also grant host access to itself.
    let own = router
        .clone()
        .oneshot(admin_change("PATCH", &admin_path, json!({"local": true})))
        .await
        .unwrap();
    assert_eq!(own.status(), StatusCode::OK);
    assert_eq!(json_body(own).await["local"], json!(true));
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

/// The `browser_grants` events emitted after `since`, through the cursored long-poll channel
/// every open page already follows.
async fn grant_events_since(router: &axum::Router, since: u64) -> (u64, Vec<Value>) {
    let page = json_body(
        router
            .clone()
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={since}&timeout_ms=0"),
                None,
                None,
                None,
                true,
            ))
            .await
            .unwrap(),
    )
    .await;
    let events = page["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| event["kind"] == "browser_grants")
        .cloned()
        .collect();
    (page["cursor"].as_u64().unwrap(), events)
}

#[tokio::test]
async fn hs2_a3tdnh_grant_changes_push_a_cursored_event_without_identities() {
    let (_dir, state) = fixture();
    let router = app(state);
    let (_tablet, _csrf, tablet) =
        paired(&router, "Synthetic tablet", json!({"role": "read_write"})).await;
    let (phone_cookie, phone_csrf, _phone) =
        paired(&router, "Synthetic phone", json!({"role": "read_only"})).await;
    let tablet_path = format!(
        "/remote-auth/owner/devices/{}",
        tablet["id"].as_str().unwrap()
    );
    let (mut cursor, _) = grant_events_since(&router, 0).await;

    // A pairing request changes no open page's grants and pushes nothing.
    pending(&router, "Synthetic stranger").await;
    let (next, events) = grant_events_since(&router, cursor).await;
    assert!(events.is_empty(), "{events:?}");
    cursor = next;

    // Granting, then removing, each push one cursored event; repeats push again.
    for grant in [
        json!({"admin": true}),
        json!({"local": true}),
        json!({"admin": false, "local": false}),
        json!({"admin": true}),
    ] {
        assert_eq!(
            status(
                &router,
                request("PATCH", &tablet_path, Some(grant), None, None, true)
            )
            .await,
            StatusCode::OK
        );
        let (next, events) = grant_events_since(&router, cursor).await;
        assert_eq!(events.len(), 1, "{events:?}");
        let event = &events[0];
        assert!(event["cursor"].as_u64().unwrap() > cursor);
        // Machine-wide and anonymous: no store, device id, or grant values reach other sessions.
        assert_eq!(event["store"], "");
        assert_eq!(event["id"], "");
        assert_eq!(event["slug"], "");
        assert!(event.get("message").is_none());
        cursor = next;
    }

    // A change to an unknown device pushes nothing.
    assert_eq!(
        status(
            &router,
            request(
                "PATCH",
                "/remote-auth/owner/devices/missing",
                Some(json!({"admin": true})),
                None,
                None,
                true
            )
        )
        .await,
        StatusCode::NOT_FOUND
    );
    let (next, events) = grant_events_since(&router, cursor).await;
    assert!(events.is_empty(), "{events:?}");
    cursor = next;

    // Revoking a device pushes; revoking it again (already gone) does not.
    for expected in [1, 0] {
        assert_eq!(
            status(
                &router,
                request("DELETE", &tablet_path, None, None, None, true)
            )
            .await,
            StatusCode::NO_CONTENT
        );
        let (next, events) = grant_events_since(&router, cursor).await;
        assert_eq!(events.len(), expected, "{events:?}");
        cursor = next;
    }

    // A browser signing itself out pushes, so its other tabs learn the session is gone.
    assert_eq!(
        status(
            &router,
            request(
                "POST",
                "/remote-auth/logout",
                None,
                Some(&phone_cookie),
                Some(&phone_csrf),
                false
            )
        )
        .await,
        StatusCode::NO_CONTENT
    );
    let (next, events) = grant_events_since(&router, cursor).await;
    assert_eq!(events.len(), 1, "{events:?}");
    cursor = next;

    // Revoke-all pushes only when it removed a session.
    paired(&router, "Synthetic laptop", json!({"role": "read_only"})).await;
    let (next, _) = grant_events_since(&router, cursor).await;
    cursor = next;
    for expected in [1, 0] {
        assert_eq!(
            status(
                &router,
                request(
                    "POST",
                    "/remote-auth/owner/revoke-all",
                    None,
                    None,
                    None,
                    true
                )
            )
            .await,
            StatusCode::NO_CONTENT
        );
        let (next, events) = grant_events_since(&router, cursor).await;
        assert_eq!(events.len(), expected, "{events:?}");
        cursor = next;
    }
}

/// Every event one subscriber's long poll returns after `since`, polled as the trusted host
/// (`cookie` absent) or as one browser session, plus the page's new cursor (HS2-BZHMEV).
async fn poll_as(router: &axum::Router, cookie: Option<&str>, since: u64) -> (u64, Vec<Value>) {
    let page = json_body(
        router
            .clone()
            .oneshot(request(
                "GET",
                &format!("/ws/poll?since={since}&timeout_ms=0"),
                None,
                cookie,
                None,
                cookie.is_none(),
            ))
            .await
            .unwrap(),
    )
    .await;
    (
        page["cursor"].as_u64().unwrap(),
        page["events"].as_array().unwrap().clone(),
    )
}

fn pairing_events(events: &[Value]) -> Vec<&Value> {
    events
        .iter()
        .filter(|event| event["kind"] == "pairing_requests")
        .collect()
}

#[tokio::test]
async fn hs2_bzhmev_pairing_request_events_reach_only_device_managers_over_poll() {
    let (_dir, state) = fixture();
    let router = app(state);
    let (admin, _admin_csrf, admin_device) = paired(
        &router,
        "Synthetic tablet",
        json!({"role": "read_only", "admin": true}),
    )
    .await;
    let (reader, _reader_csrf, _reader_device) =
        paired(&router, "Synthetic phone", json!({"role": "read_write"})).await;
    let (start, _) = poll_as(&router, None, 0).await;

    // A new request pushes one anonymous event to the host and the admin session.
    let (request_id, challenge) = pending(&router, "Synthetic stranger").await;
    let (host_cursor, host_events) = poll_as(&router, None, start).await;
    let host_pairing = pairing_events(&host_events);
    assert_eq!(host_pairing.len(), 1, "{host_events:?}");
    let event = host_pairing[0];
    assert!(event["cursor"].as_u64().unwrap() > start);
    assert_eq!(event["store"], "");
    assert_eq!(event["id"], "");
    assert_eq!(event["slug"], "");
    assert!(event.get("message").is_none());
    let wire = event.to_string();
    for secret in [
        request_id.as_str(),
        challenge.as_str(),
        "Synthetic stranger",
        ORIGIN,
    ] {
        assert!(!wire.contains(secret), "event leaked {secret}: {wire}");
    }
    let (admin_cursor, admin_events) = poll_as(&router, Some(&admin), start).await;
    assert_eq!(pairing_events(&admin_events).len(), 1, "{admin_events:?}");
    assert_eq!(admin_cursor, host_cursor);

    // A session without the admin grant never sees it, but its cursor still moves past it.
    let (reader_cursor, reader_events) = poll_as(&router, Some(&reader), start).await;
    assert!(
        pairing_events(&reader_events).is_empty(),
        "{reader_events:?}"
    );
    assert_eq!(reader_cursor, host_cursor);

    // Approving and rejecting each push one event; a refused reject pushes none.
    let (second, _) = pending(&router, "Synthetic laptop").await;
    let (mut cursor, _) = poll_as(&router, None, host_cursor).await;
    for (path, body, expected) in [
        (
            "/remote-auth/owner/approve",
            json!({"id": request_id, "role": "read_only"}),
            StatusCode::NO_CONTENT,
        ),
        (
            "/remote-auth/owner/reject",
            json!({"id": second}),
            StatusCode::NO_CONTENT,
        ),
        (
            "/remote-auth/owner/reject",
            json!({"id": "missing"}),
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(
            status(&router, request("POST", path, Some(body), None, None, true)).await,
            expected
        );
        let (next, events) = poll_as(&router, None, cursor).await;
        let want = usize::from(expected == StatusCode::NO_CONTENT);
        assert_eq!(pairing_events(&events).len(), want, "{path}: {events:?}");
        cursor = next;
    }
    // A refused pairing request pushes nothing either.
    let invalid = status(
        &router,
        request(
            "POST",
            "/remote-auth/pair",
            Some(json!({"label": ""})),
            None,
            None,
            false,
        ),
    )
    .await;
    assert_eq!(invalid, StatusCode::UNAUTHORIZED);
    let (next, events) = poll_as(&router, None, cursor).await;
    assert!(pairing_events(&events).is_empty(), "{events:?}");
    cursor = next;

    // Removing the admin grant stops the events at once: the grant is checked per page.
    let admin_path = format!(
        "/remote-auth/owner/devices/{}",
        admin_device["id"].as_str().unwrap()
    );
    assert_eq!(
        status(
            &router,
            request(
                "PATCH",
                &admin_path,
                Some(json!({"admin": false})),
                None,
                None,
                true
            )
        )
        .await,
        StatusCode::OK
    );
    pending(&router, "Synthetic watch").await;
    let (_, demoted) = poll_as(&router, Some(&admin), cursor).await;
    assert!(pairing_events(&demoted).is_empty(), "{demoted:?}");
    assert!(
        demoted
            .iter()
            .any(|event| event["kind"] == "browser_grants"),
        "{demoted:?}"
    );

    // A waiting poll woken by a gated event answers without it.
    let (now, _) = poll_as(&router, None, 0).await;
    let waiting = router.clone().oneshot(request(
        "GET",
        &format!("/ws/poll?since={now}&timeout_ms=5000"),
        None,
        Some(&reader),
        None,
        false,
    ));
    let (woken, _) = tokio::join!(waiting, pending(&router, "Synthetic speaker"));
    let woken = json_body(woken.unwrap()).await;
    assert!(
        pairing_events(woken["events"].as_array().unwrap()).is_empty(),
        "{woken}"
    );
}

type SyncSocket =
    tokio_tungstenite::WebSocketStream<tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>>;

/// One remote session's `/ws/sync` socket, carrying the session and CSRF cookies the way a
/// browser's upgrade does.
async fn sync_socket(addr: std::net::SocketAddr, session: &str, csrf: &str) -> SyncSocket {
    use tokio_tungstenite::tungstenite::client::IntoClientRequest;
    let mut upgrade = format!("ws://{addr}/ws/sync")
        .into_client_request()
        .unwrap();
    let headers = upgrade.headers_mut();
    headers.insert("origin", ORIGIN.parse().unwrap());
    headers.insert(
        "cookie",
        format!("{session}; hs2_remote_csrf={csrf}")
            .parse()
            .unwrap(),
    );
    tokio_tungstenite::connect_async(upgrade).await.unwrap().0
}

/// The kind of the next text event a socket delivers.
async fn next_kind(socket: &mut SyncSocket) -> String {
    use futures_util::StreamExt;
    loop {
        let message = tokio::time::timeout(std::time::Duration::from_secs(10), socket.next())
            .await
            .expect("an event arrives")
            .expect("the socket stays open")
            .unwrap();
        if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
            let event: Value = serde_json::from_str(&text).unwrap();
            return event["kind"].as_str().unwrap().to_owned();
        }
    }
}

#[tokio::test]
async fn hs2_bzhmev_pairing_request_events_reach_only_device_managers_over_websocket() {
    let (_dir, state) = fixture();
    let router = app(state.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let served = app(state);
    tokio::spawn(async move { axum::serve(listener, served).await.unwrap() });
    let (admin, admin_csrf, _admin_device) = paired(
        &router,
        "Synthetic tablet",
        json!({"role": "read_only", "admin": true}),
    )
    .await;
    let (reader, reader_csrf, reader_device) =
        paired(&router, "Synthetic phone", json!({"role": "read_only"})).await;
    let mut admin_socket = sync_socket(addr, &admin, &admin_csrf).await;
    let mut reader_socket = sync_socket(addr, &reader, &reader_csrf).await;

    pending(&router, "Synthetic stranger").await;
    assert_eq!(next_kind(&mut admin_socket).await, "pairing_requests");

    // The reader's socket skips the pairing event: the next one it delivers is a later grant
    // change, which every session receives.
    let reader_path = format!(
        "/remote-auth/owner/devices/{}",
        reader_device["id"].as_str().unwrap()
    );
    assert_eq!(
        status(
            &router,
            request(
                "PATCH",
                &reader_path,
                Some(json!({"role": "read_write"})),
                None,
                None,
                true
            )
        )
        .await,
        StatusCode::OK
    );
    assert_eq!(next_kind(&mut reader_socket).await, "browser_grants");
    assert_eq!(next_kind(&mut admin_socket).await, "browser_grants");
}

/// HS2-YDDPN4: a paired device authorizes the machine's server, not one primary store. A
/// device paired while the server ran on store A under the per-store catalog layout keeps
/// working after the server restarts on store B, and a device paired on B then works when the
/// server returns to A. Revoking on B holds on A.
#[tokio::test]
async fn hs2_yddpn4_restart_on_another_primary_store_keeps_paired_devices() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let store_a = FsStore::init_git(dir.path().join("a.hs2"), &StoreMetadata::new("SYN")).unwrap();
    let store_b = FsStore::init_git(dir.path().join("b.hs2"), &StoreMetadata::new("SYN")).unwrap();
    let legacy = home.join("remote-auth").join(format!(
        "{}.json",
        hotsheet_server::multistore::store_url_id(&store_a)
    ));
    let old_layout = app(AppState::new(store_a.clone(), SECRET.into())
        .unwrap()
        .with_machine_home(&home)
        .with_remote_auth(&legacy)
        .unwrap());
    let (tablet, _, tablet_device) = paired(
        &old_layout,
        "Synthetic tablet",
        json!({"role": "read_write"}),
    )
    .await;
    drop(old_layout);
    let legacy_bytes = std::fs::read(&legacy).unwrap();

    let on_b = AppState::new(store_b, SECRET.into())
        .unwrap()
        .with_machine_home(&home)
        .with_machine_remote_auth()
        .unwrap();
    assert_eq!(
        on_b.remote_auth().unwrap().migration().merged,
        vec![legacy.file_name().unwrap().to_string_lossy().into_owned()]
    );
    let on_b = app(on_b);
    let me = |cookie: &str| request("GET", "/remote-auth/me", None, Some(cookie), None, false);
    assert_eq!(status(&on_b, me(&tablet)).await, StatusCode::OK);
    let (phone, _, phone_device) =
        paired(&on_b, "Synthetic phone", json!({"role": "read_only"})).await;
    drop(on_b);

    let on_a = AppState::new(store_a, SECRET.into())
        .unwrap()
        .with_machine_home(&home)
        .with_machine_remote_auth()
        .unwrap();
    assert!(on_a.remote_auth().unwrap().migration().merged.is_empty());
    let on_a = app(on_a);
    assert_eq!(status(&on_a, me(&tablet)).await, StatusCode::OK);
    assert_eq!(status(&on_a, me(&phone)).await, StatusCode::OK);
    let listed = json_body(
        on_a.clone()
            .oneshot(request("GET", "/remote-auth/owner", None, None, None, true))
            .await
            .unwrap(),
    )
    .await;
    let ids: Vec<_> = listed["devices"]
        .as_array()
        .unwrap()
        .iter()
        .map(|device| device["id"].clone())
        .collect();
    assert!(ids.contains(&tablet_device["id"]) && ids.contains(&phone_device["id"]));
    // The per-store file stays untouched for an older server.
    assert_eq!(std::fs::read(&legacy).unwrap(), legacy_bytes);

    // Two servers on different primary stores share the catalog: a revocation through one
    // holds in the other at its next request.
    let store_b = FsStore::open(dir.path().join("b.hs2")).unwrap();
    let on_b = app(AppState::new(store_b, SECRET.into())
        .unwrap()
        .with_machine_home(&home)
        .with_machine_remote_auth()
        .unwrap());
    let revoked = on_b
        .clone()
        .oneshot(request(
            "DELETE",
            &format!(
                "/remote-auth/owner/devices/{}",
                tablet_device["id"].as_str().unwrap()
            ),
            None,
            None,
            None,
            true,
        ))
        .await
        .unwrap();
    assert!(revoked.status().is_success(), "{}", revoked.status());
    let refused = on_a.clone().oneshot(me(&tablet)).await.unwrap();
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(json_body(refused).await["code"], "session_revoked");
    assert_eq!(status(&on_a, me(&phone)).await, StatusCode::OK);
}

/// One write to the primary store with exactly the given credential headers.
fn write_with(headers: &[(&str, &str)]) -> Request<Body> {
    let mut req = Request::builder()
        .method("POST")
        .uri("/tickets")
        .header(header::CONTENT_TYPE, "application/json");
    for (name, value) in headers {
        req = req.header(*name, *value);
    }
    req.body(Body::from(
        json!({"title": "Synthetic bearer write"}).to_string(),
    ))
    .unwrap()
}

/// HS2-AW2AKF: a paired machine presents its session as an `Authorization: Bearer` header
/// and writes without browser Origin and CSRF proof; the cookie form still needs both, so a
/// cross-site browser request carrying the cookie stays refused.
#[tokio::test]
async fn hs2_aw2akf_bearer_writes_without_browser_proof_while_cookie_writes_need_it() {
    let (_dir, state) = fixture();
    let router = app(state);
    let (cookie, csrf, _) =
        paired(&router, "Synthetic machine", json!({"role": "read_write"})).await;
    let token = cookie.split_once('=').unwrap().1.to_owned();
    let bearer = format!("Bearer {token}");
    let refusal = |response: axum::response::Response| async move {
        let status = response.status();
        (status, json_body(response).await["code"].clone())
    };
    let send = |request| router.clone().oneshot(request);
    // The browser form: the cookie alone, or with Origin but no CSRF, writes nothing.
    assert_eq!(
        refusal(send(write_with(&[("cookie", &cookie)])).await.unwrap()).await,
        (StatusCode::UNAUTHORIZED, json!("session_invalid"))
    );
    assert_eq!(
        refusal(
            send(write_with(&[("cookie", &cookie), ("origin", ORIGIN)]))
                .await
                .unwrap()
        )
        .await,
        (StatusCode::FORBIDDEN, json!("csrf_required"))
    );
    assert_eq!(
        refusal(
            send(write_with(&[
                ("cookie", &cookie),
                ("origin", "http://elsewhere.example"),
                ("x-hotsheet-csrf", &csrf),
            ]))
            .await
            .unwrap()
        )
        .await,
        (StatusCode::UNAUTHORIZED, json!("session_invalid"))
    );
    // The CSRF token presented as a bearer is no session: the cookie form decides, and
    // without its proof it refuses.
    let csrf_as_bearer = format!("Bearer {csrf}");
    assert_eq!(
        refusal(
            send(write_with(&[
                ("cookie", &cookie),
                ("origin", ORIGIN),
                ("authorization", &csrf_as_bearer),
            ]))
            .await
            .unwrap()
        )
        .await,
        (StatusCode::FORBIDDEN, json!("csrf_required"))
    );
    assert_eq!(
        refusal(
            send(write_with(&[("authorization", "Bearer not-a-session")]))
                .await
                .unwrap()
        )
        .await,
        (StatusCode::UNAUTHORIZED, json!("session_invalid"))
    );
    // The full browser proof and the bearer each write.
    let browser = send(write_with(&[
        ("cookie", &cookie),
        ("origin", ORIGIN),
        ("x-hotsheet-csrf", &csrf),
    ]))
    .await
    .unwrap();
    assert!(browser.status().is_success(), "{}", browser.status());
    let machine = send(write_with(&[("authorization", &bearer)]))
        .await
        .unwrap();
    assert!(machine.status().is_success(), "{}", machine.status());
    // The peer narrows the device to read-only: the bearer still reads and is refused
    // writes with the read-only code.
    let device = json_body(
        send(request("GET", "/remote-auth/owner", None, None, None, true))
            .await
            .unwrap(),
    )
    .await;
    let id = device["devices"][0]["id"].as_str().unwrap().to_owned();
    assert_eq!(
        status(
            &router,
            request(
                "PATCH",
                &format!("/remote-auth/owner/devices/{id}"),
                Some(json!({"role": "read_only"})),
                None,
                None,
                true,
            ),
        )
        .await,
        StatusCode::OK
    );
    let read = Request::builder()
        .uri("/tickets")
        .header("authorization", &bearer)
        .body(Body::empty())
        .unwrap();
    assert_eq!(send(read).await.unwrap().status(), StatusCode::OK);
    assert_eq!(
        refusal(
            send(write_with(&[("authorization", &bearer)]))
                .await
                .unwrap()
        )
        .await,
        (StatusCode::FORBIDDEN, json!("read_only"))
    );
    // Revocation ends the bearer as it ends the cookie.
    assert_eq!(
        status(
            &router,
            request(
                "DELETE",
                &format!("/remote-auth/owner/devices/{id}"),
                None,
                None,
                None,
                true,
            ),
        )
        .await,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        refusal(
            send(write_with(&[("authorization", &bearer)]))
                .await
                .unwrap()
        )
        .await,
        (StatusCode::UNAUTHORIZED, json!("session_revoked"))
    );
}
