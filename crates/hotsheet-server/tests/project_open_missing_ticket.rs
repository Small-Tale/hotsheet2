//! HS2-KJ9WSC: a Git checkout can remove one indexed Up Next file after open.

use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, Ordering},
};

use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use hotsheet_server::{AppState, OpenBackgroundWork, app};
use hotsheet_ticketing::{FsStore, NewTicket, StoreMetadata, ops};
use tower::ServiceExt;

const SECRET: &str = "synthetic-secret";

#[derive(Clone, Copy)]
enum Mutation {
    Remove,
    ReplaceWithDirectory,
}

async fn open_and_mutate_indexed_ticket(mutation: Mutation) -> Option<String> {
    let workspace = tempfile::tempdir().unwrap();
    let bootstrap = tempfile::tempdir().unwrap();
    let primary = FsStore::init_git(bootstrap.path(), &StoreMetadata::new("BS")).unwrap();
    let checkout = workspace.path().join("app");
    let source = workspace.path().join("app.hs2");
    std::fs::create_dir(&checkout).unwrap();
    let store = FsStore::init_git(&source, &StoreMetadata::new("SY")).unwrap();
    let now = hotsheet_model::Timestamp::new("2026-11-09T00:00:00Z");
    let healthy = ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "SY",
        now.clone(),
        NewTicket {
            title: "Synthetic healthy worklist item".into(),
            category: "task".into(),
            up_next: true,
            ..Default::default()
        },
    )
    .unwrap();
    let vanished = ops::create(
        &store,
        hotsheet_model::Ulid::new(),
        "SY",
        now,
        NewTicket {
            title: "Synthetic vanished worklist item".into(),
            category: "task".into(),
            up_next: true,
            ..Default::default()
        },
    )
    .unwrap();
    let missing_path = store.ticket_path(&vanished.id);
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let removed = Arc::new(AtomicBool::new(false));
    let (finished, mut completed) = tokio::sync::mpsc::unbounded_channel();
    let hook_gate = gate.clone();
    let hook_removed = removed.clone();
    let state = AppState::new(primary, SECRET.into())
        .unwrap()
        .with_machine_home(workspace.path().join("machine"))
        .with_checkout_registry(workspace.path().join("checkouts.json"))
        .with_open_background_hook(move |work| match work {
            OpenBackgroundWork::WorklistRegenerationStarted => {
                let (lock, ready) = &*hook_gate;
                let mut released = lock.lock().unwrap();
                while !*released {
                    released = ready.wait(released).unwrap();
                }
                if !hook_removed.swap(true, Ordering::SeqCst) {
                    std::fs::remove_file(&missing_path).unwrap();
                    if matches!(mutation, Mutation::ReplaceWithDirectory) {
                        std::fs::create_dir(&missing_path).unwrap();
                    }
                }
            }
            OpenBackgroundWork::WorklistRegenerationFinished => {
                let _ = finished.send(());
            }
            _ => {}
        });
    let router = app(state);
    let open = || {
        Request::builder()
            .method("POST")
            .uri("/projects/open")
            .header("x-hotsheet-secret", SECRET)
            .header("content-type", "application/json")
            .body(Body::from(
                serde_json::json!({ "root": checkout, "stores": [source] }).to_string(),
            ))
            .unwrap()
    };
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        router.clone().oneshot(open()),
    )
    .await
    .expect("project open waited on background worklist regeneration")
    .unwrap();
    assert_eq!(response.status(), StatusCode::CREATED);
    assert!(!removed.load(Ordering::SeqCst));

    // The hook resumes only after the successful open response and removes a file
    // before the queued worklist reads its indexed Up Next rows.
    {
        let (lock, ready) = &*gate;
        *lock.lock().unwrap() = true;
        ready.notify_all();
    }
    let completion = tokio::time::timeout(std::time::Duration::from_secs(5), completed.recv())
        .await
        .expect("background worklist regeneration did not finish");
    assert_eq!(completion, Some(()));
    assert!(removed.load(Ordering::SeqCst));
    if matches!(mutation, Mutation::ReplaceWithDirectory) {
        let error = store.read_ticket(&vanished.id).unwrap_err();
        assert!(!error.is_io_kind(std::io::ErrorKind::NotFound));
    }
    let worklist =
        std::fs::read_to_string(checkout.join(hotsheet_ticketing::worklist::CHECKOUT_WORKLIST))
            .ok();
    if let Some(ref worklist) = worklist {
        assert!(!worklist.contains(&vanished.title));
    }
    if matches!(mutation, Mutation::Remove) {
        assert!(
            worklist
                .as_ref()
                .is_some_and(|text| text.contains(&healthy.title))
        );
    }
    worklist
}

#[tokio::test]
async fn hs2_kj9wsc_missing_indexed_ticket_does_not_drop_healthy_worklist() {
    let worklist = open_and_mutate_indexed_ticket(Mutation::Remove)
        .await
        .expect("the healthy ticket should still produce a worklist");
    assert!(worklist.contains("Synthetic healthy worklist item"));
}

#[tokio::test]
async fn hs2_kj9wsc_other_read_error_still_fails_worklist_refresh() {
    let worklist = open_and_mutate_indexed_ticket(Mutation::ReplaceWithDirectory).await;
    assert!(worklist.is_none(), "other read errors must not be skipped");
}
