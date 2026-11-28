//! HS2-W3JD5K: the git provider checks `expected_token` against the ticket it reads
//! inside the ticket transaction, so a write landing between resolving the ticket and
//! taking the lock turns a same-token write into a conflict instead of a lost update.
//! Interleavings run deterministically through the provider's before-lock test hook.

use super::*;
use crate::StoreMetadata;
use hotsheet_model::{AiFeedback, AiFeedbackRating, AttachmentActorRole, NoteActor};
use std::sync::Barrier;

fn git_store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn create(provider: &GitProvider, title: &str, at: &str) -> ApiTicket {
    provider
        .create(
            MutationContext {
                now: Timestamp::new(at),
                generated_id: Ulid::new(),
            },
            ProviderDraft {
                group: None,
                title: title.into(),
                category: "task".into(),
                priority: Priority::Default,
                status: Status::NotStarted,
                details: String::new(),
                tags: vec![],
                up_next: false,
                blocked_by: vec![],
                transfer: None,
                created_by: None,
                due_on: None,
                milestone: None,
            },
        )
        .unwrap()
}

fn retitle(
    provider: &GitProvider,
    native_id: &str,
    title: &str,
    at: &str,
    token: Option<&str>,
) -> Result<ApiTicket, ProviderError> {
    provider.update(
        native_id,
        Timestamp::new(at),
        ProviderPatch {
            expected_token: token.map(str::to_owned),
            title: Some(title.into()),
            ..ProviderPatch::default()
        },
    )
}

fn token(ticket: &ApiTicket) -> String {
    ticket.concurrency_token.clone().unwrap()
}

fn assert_changed_since_read(result: Result<ApiTicket, ProviderError>) {
    let error = result.expect_err("a same-token write after a competing write must conflict");
    assert!(error.is_changed_since_read(), "{error:?}");
}

#[test]
fn an_interleaved_same_token_update_conflicts_instead_of_overwriting() {
    let (_dir, store) = git_store();
    let first = GitProvider::new("local", store.clone());
    let second = GitProvider::new("local", store);
    let ticket = create(&first, "original", "2026-11-26T01:00:00Z");
    let id = ticket.native_id.clone();
    let read_token = token(&ticket);

    // The competing writer read the same token and lands between the first writer's
    // resolve and its lock.
    let (competing_id, competing_token) = (id.clone(), read_token.clone());
    first.before_next_ticket_lock(move || {
        retitle(
            &second,
            &competing_id,
            "second wins",
            "2026-11-26T01:01:00Z",
            Some(&competing_token),
        )
        .expect("the competing write holds the current token");
    });

    assert_changed_since_read(retitle(
        &first,
        &id,
        "first loses",
        "2026-11-26T01:02:00Z",
        Some(&read_token),
    ));
    let stored = first.get(&id).unwrap();
    assert_eq!(stored.title, "second wins");
    assert_eq!(
        stored.concurrency_token.as_deref(),
        Some("2026-11-26T01:01:00Z")
    );
}

#[test]
fn an_interleaved_same_token_rating_conflicts_instead_of_overwriting() {
    let (_dir, store) = git_store();
    let first = GitProvider::new("local", store.clone()).with_default(true);
    let second = GitProvider::new("local", store);
    let ticket = create(&first, "rated", "2026-11-26T02:00:00Z");
    let id = ticket.native_id.clone();
    let read_token = token(&ticket);

    let (competing_id, competing_token) = (id.clone(), read_token.clone());
    first.before_next_ticket_lock(move || {
        retitle(
            &second,
            &competing_id,
            "retitled meanwhile",
            "2026-11-26T02:01:00Z",
            Some(&competing_token),
        )
        .unwrap();
    });

    assert_changed_since_read(first.rate_ai_content(
        &id,
        Timestamp::new("2026-11-26T02:02:00Z"),
        AiFeedback {
            target: "activity:run-1".into(),
            rating: Some(AiFeedbackRating::Helpful),
            explanation: None,
        },
        NoteActor {
            role: AttachmentActorRole::Human,
            id: Some("rater".into()),
        },
        Some(&read_token),
    ));
    let stored = first.get(&id).unwrap();
    assert_eq!(stored.title, "retitled meanwhile");
    assert!(stored.notes.iter().all(|note| note.ai_feedback.is_none()));
}

#[test]
fn repeated_updates_each_carrying_the_fresh_token_all_succeed() {
    let (_dir, store) = git_store();
    let provider = GitProvider::new("local", store);
    let ticket = create(&provider, "v0", "2026-11-26T03:00:00Z");
    let id = ticket.native_id.clone();
    let mut current = token(&ticket);
    let mut previous = Vec::new();
    for round in 1..=4 {
        let updated = retitle(
            &provider,
            &id,
            &format!("v{round}"),
            &format!("2026-11-26T03:0{round}:00Z"),
            Some(&current),
        )
        .unwrap();
        assert_eq!(updated.title, format!("v{round}"));
        previous.push(std::mem::replace(&mut current, token(&updated)));
    }
    // Every token handed out before the latest write is now stale.
    for stale in previous {
        assert_changed_since_read(retitle(
            &provider,
            &id,
            "stale",
            "2026-11-26T03:09:00Z",
            Some(&stale),
        ));
    }
    assert_eq!(provider.get(&id).unwrap().title, "v4");
}

#[test]
fn a_stale_token_conflicts_and_writes_nothing() {
    let (_dir, store) = git_store();
    let provider = GitProvider::new("local", store);
    let ticket = create(&provider, "kept", "2026-11-26T04:00:00Z");
    let id = ticket.native_id.clone();
    assert_changed_since_read(retitle(
        &provider,
        &id,
        "replaced",
        "2026-11-26T04:01:00Z",
        Some("2026-11-26T03:59:00Z"),
    ));
    let stored = provider.get(&id).unwrap();
    assert_eq!(stored.title, "kept");
    assert_eq!(token(&stored), token(&ticket));
}

#[test]
fn a_missing_token_writes_unconditionally_even_after_an_interleaved_write() {
    let (_dir, store) = git_store();
    let first = GitProvider::new("local", store.clone());
    let second = GitProvider::new("local", store);
    let ticket = create(&first, "original", "2026-11-26T05:00:00Z");
    let id = ticket.native_id.clone();
    let competing_id = id.clone();
    first.before_next_ticket_lock(move || {
        retitle(
            &second,
            &competing_id,
            "second",
            "2026-11-26T05:01:00Z",
            None,
        )
        .unwrap();
    });
    let updated = retitle(&first, &id, "first", "2026-11-26T05:02:00Z", None).unwrap();
    assert_eq!(updated.title, "first");
    assert_eq!(token(&updated), "2026-11-26T05:02:00Z");
}

#[test]
fn an_interleaved_write_to_another_ticket_does_not_conflict() {
    let (_dir, store) = git_store();
    let first = GitProvider::new("local", store.clone());
    let second = GitProvider::new("local", store);
    let one = create(&first, "one", "2026-11-26T06:00:00Z");
    let two = create(&first, "two", "2026-11-26T06:00:01Z");
    let (two_id, two_token) = (two.native_id.clone(), token(&two));
    first.before_next_ticket_lock(move || {
        retitle(
            &second,
            &two_id,
            "two updated",
            "2026-11-26T06:01:00Z",
            Some(&two_token),
        )
        .unwrap();
    });
    let updated = retitle(
        &first,
        &one.native_id,
        "one updated",
        "2026-11-26T06:02:00Z",
        Some(&token(&one)),
    )
    .unwrap();
    assert_eq!(updated.title, "one updated");
    assert_eq!(first.get(&two.native_id).unwrap().title, "two updated");
}

#[test]
fn concurrent_token_checked_updates_to_different_tickets_both_succeed() {
    let (_dir, store) = git_store();
    let seed = GitProvider::new("local", store.clone());
    let tickets: Vec<ApiTicket> = (0..4)
        .map(|n| create(&seed, &format!("t{n}"), &format!("2026-11-26T07:00:0{n}Z")))
        .collect();
    let barrier = Arc::new(Barrier::new(tickets.len()));
    let handles: Vec<_> = tickets
        .iter()
        .map(|ticket| {
            let provider = GitProvider::new("local", store.clone());
            let barrier = barrier.clone();
            let (id, read) = (ticket.native_id.clone(), token(ticket));
            std::thread::spawn(move || {
                barrier.wait();
                retitle(&provider, &id, "done", "2026-11-26T07:01:00Z", Some(&read))
            })
        })
        .collect();
    for handle in handles {
        assert_eq!(handle.join().unwrap().unwrap().title, "done");
    }
    for ticket in &tickets {
        assert_eq!(seed.get(&ticket.native_id).unwrap().title, "done");
    }
}

#[test]
fn concurrent_same_token_updates_to_one_ticket_let_exactly_one_win() {
    let (_dir, store) = git_store();
    let seed = GitProvider::new("local", store.clone());
    let ticket = create(&seed, "contested", "2026-11-26T08:00:00Z");
    let writers = 4;
    let barrier = Arc::new(Barrier::new(writers));
    let handles: Vec<_> = (0..writers)
        .map(|n| {
            let provider = GitProvider::new("local", store.clone());
            let barrier = barrier.clone();
            let (id, read) = (ticket.native_id.clone(), token(&ticket));
            std::thread::spawn(move || {
                barrier.wait();
                retitle(
                    &provider,
                    &id,
                    &format!("writer {n}"),
                    &format!("2026-11-26T08:01:0{n}Z"),
                    Some(&read),
                )
            })
        })
        .collect();
    let results: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let winners: Vec<_> = results.iter().filter_map(|r| r.as_ref().ok()).collect();
    assert_eq!(winners.len(), 1, "{results:?}");
    assert!(
        results
            .iter()
            .filter_map(|r| r.as_ref().err())
            .all(ProviderError::is_changed_since_read)
    );
    assert_eq!(seed.get(&ticket.native_id).unwrap().title, winners[0].title);
}
