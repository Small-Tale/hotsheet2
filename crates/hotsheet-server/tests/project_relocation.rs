//! HS2-86ES0B: a client that remembered a checkout's old root (a restored project tab)
//! reopens the checkout at the root an explicit relocate moved it to.
use axum::body::Body;
use axum::http::{Request, StatusCode};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::checkouts::CheckoutRegistry;
use hotsheet_ticketing::{FsStore, StoreMetadata};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

const SECRET: &str = "synthetic-relocation-secret";

async fn post(router: &axum::Router, path: &str, body: Value) -> (StatusCode, Value) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(path)
                .header("x-hotsheet-secret", SECRET)
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}

#[tokio::test]
async fn hs2_86es0b_open_and_follow_reach_a_relocated_checkout_from_its_old_root() {
    let dir = tempfile::tempdir().unwrap();
    let base = hotsheet_sync::paths::canonicalize(dir.path()).unwrap();
    let primary = FsStore::init_git(base.join("bootstrap"), &StoreMetadata::new("BS")).unwrap();
    let registry_path = base.join("home/checkouts.json");
    let router = app(AppState::new(primary, SECRET.into())
        .unwrap()
        .with_machine_home(base.join("home"))
        .with_checkout_registry(&registry_path));
    let (original, relocated) = (base.join("app"), base.join("moved/app"));
    let store = base.join("app.hs2");
    std::fs::create_dir(&original).unwrap();
    std::fs::create_dir(base.join("moved")).unwrap();
    FsStore::init_git(&store, &StoreMetadata::new("SY")).unwrap();

    let (status, opened) = post(
        &router,
        "/projects/open",
        json!({ "root": original, "stores": [store] }),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{opened}");
    let id = opened["checkout"]["id"].as_str().unwrap().to_owned();
    let (_, unmoved) = post(
        &router,
        "/projects/follow-relocation",
        json!({ "root": original }),
    )
    .await;
    assert_eq!(unmoved, json!({ "checkout": null }), "an existing folder");

    std::fs::rename(&original, &relocated).unwrap();
    let (_, untold) = post(
        &router,
        "/projects/follow-relocation",
        json!({ "root": original }),
    )
    .await;
    assert_eq!(
        untold,
        json!({ "checkout": null }),
        "a move the registry was not told about stays unavailable"
    );
    let (status, refused) = post(&router, "/projects/open", json!({ "root": original })).await;
    assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");

    // The supported relocate path (`hotsheet-cli checkout relocate` calls this).
    CheckoutRegistry::new(&registry_path)
        .relocate(&id, &relocated, None)
        .unwrap();
    let (status, followed) = post(
        &router,
        "/projects/follow-relocation",
        json!({ "root": original }),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(followed["checkout"]["id"], id.as_str());
    assert_eq!(
        followed["checkout"]["root"],
        relocated.to_string_lossy().as_ref()
    );

    let (status, reopened) = post(&router, "/projects/open", json!({ "root": original })).await;
    assert_eq!(status, StatusCode::CREATED, "{reopened}");
    assert_eq!(reopened["checkout"]["id"], id.as_str());
    assert_eq!(
        reopened["checkout"]["root"],
        relocated.to_string_lossy().as_ref()
    );
    assert_eq!(
        reopened["checkout"]["stores"],
        json!([store]),
        "the relocated checkout keeps its ticket store"
    );
    assert!(!original.exists(), "nothing was created at the old root");
    assert_eq!(
        CheckoutRegistry::new(&registry_path).list().unwrap().len(),
        1,
        "the old root was not registered as a new checkout"
    );
}
