//! HS2-18QFJ5: creator filtering happens in the real HTTP index before the response bound.

use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use hotsheet_model::{AttachmentActorRole, NoteActor, Timestamp, Ulid};
use hotsheet_server::{AppState, app};
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

#[tokio::test]
async fn hs2_18qfj5_ai_creator_filter_precedes_bounded_http_list() {
    let directory = tempfile::tempdir().unwrap();
    let store = FsStore::init_git(directory.path(), &StoreMetadata::new("SY")).unwrap();
    let now = Timestamp::new("2026-11-09T00:00:00Z");
    for (title, role) in [
        ("Synthetic unknown creator", None),
        ("Synthetic person creator", Some(AttachmentActorRole::Human)),
        ("Synthetic AI creator", Some(AttachmentActorRole::Ai)),
    ] {
        ops::create_by(
            &store,
            Ulid::new(),
            "SY",
            now.clone(),
            NewTicket {
                title: title.into(),
                ..Default::default()
            },
            role.map(|role| NoteActor { role, id: None }),
        )
        .unwrap();
    }
    let router = app(AppState::new(store, SECRET.into()).unwrap());
    let request = |query: &str| {
        Request::builder()
            .uri(format!("/tickets?{query}"))
            .header("x-hotsheet-secret", SECRET)
            .body(Body::empty())
            .unwrap()
    };
    let ai = router
        .clone()
        .oneshot(request("ai_created=true&limit=1"))
        .await
        .unwrap();
    assert_eq!(ai.status(), StatusCode::OK);
    let ai: serde_json::Value =
        serde_json::from_slice(&to_bytes(ai.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(ai.as_array().unwrap().len(), 1);
    assert_eq!(ai[0]["title"], "Synthetic AI creator");
    assert_eq!(ai[0]["created_by"]["role"], "ai");

    let other = router
        .clone()
        .oneshot(request("ai_created=false&limit=3"))
        .await
        .unwrap();
    assert_eq!(other.status(), StatusCode::OK);
    let other: serde_json::Value =
        serde_json::from_slice(&to_bytes(other.into_body(), usize::MAX).await.unwrap()).unwrap();
    assert_eq!(other.as_array().unwrap().len(), 2);
    assert!(
        other
            .as_array()
            .unwrap()
            .iter()
            .all(|row| row["created_by"]["role"] != "ai")
    );
}
