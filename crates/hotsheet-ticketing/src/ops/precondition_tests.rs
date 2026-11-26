//! HS2-W3JD5K: `update_if` and `rate_ai_content_if` run the caller's precondition on the
//! ticket read inside the ticket transaction, after any earlier write, and a refused
//! precondition writes nothing.

use super::*;
use crate::store::StoreMetadata;
use hotsheet_model::{AiFeedbackRating, AttachmentActorRole, NoteActor};

#[derive(Debug, PartialEq, Eq)]
enum Refused {
    Changed(String),
    Store(String),
}

impl From<StoreError> for Refused {
    fn from(error: StoreError) -> Self {
        Self::Store(error.to_string())
    }
}

fn store() -> (tempfile::TempDir, FsStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = FsStore::init(dir.path(), &StoreMetadata::new("HS")).unwrap();
    (dir, store)
}

fn add(store: &FsStore) -> Ticket {
    create(
        store,
        Ulid::new(),
        "HS",
        Timestamp::new("2026-11-26T00:00:00Z"),
        NewTicket {
            title: "original".into(),
            ..Default::default()
        },
    )
    .unwrap()
}

fn retitle(title: &str) -> TicketPatch {
    TicketPatch {
        title: Some(title.into()),
        ..TicketPatch::default()
    }
}

fn token_is(expected: &str) -> impl FnOnce(&Ticket) -> Result<(), Refused> + '_ {
    move |current| {
        if current.updated_at.as_str() == expected {
            Ok(())
        } else {
            Err(Refused::Changed(current.updated_at.as_str().to_owned()))
        }
    }
}

fn rater() -> Option<NoteActor> {
    Some(NoteActor {
        role: AttachmentActorRole::Human,
        id: Some("rater".into()),
    })
}

#[test]
fn update_if_sees_the_latest_write_and_a_refusal_writes_nothing() {
    let (_dir, store) = store();
    let ticket = add(&store);
    let read = ticket.updated_at.as_str().to_owned();
    // A competing write lands after `read` was taken.
    update(
        &store,
        &ticket.id,
        Timestamp::new("2026-11-26T00:01:00Z"),
        retitle("competing"),
    )
    .unwrap();
    let refused = update_if(
        &store,
        &ticket.id,
        Timestamp::new("2026-11-26T00:02:00Z"),
        retitle("lost"),
        token_is(&read),
    );
    assert_eq!(
        refused.unwrap_err(),
        Refused::Changed("2026-11-26T00:01:00Z".into())
    );
    let stored = store.read_ticket(&ticket.id).unwrap();
    assert_eq!(stored.title, "competing");

    let accepted = update_if(
        &store,
        &ticket.id,
        Timestamp::new("2026-11-26T00:03:00Z"),
        retitle("fresh"),
        token_is("2026-11-26T00:01:00Z"),
    )
    .unwrap();
    assert_eq!(accepted.title, "fresh");
    assert_eq!(accepted.updated_at.as_str(), "2026-11-26T00:03:00Z");
}

#[test]
fn update_if_still_reports_store_errors_through_the_caller_error() {
    let (_dir, store) = store();
    let missing = update_if(
        &store,
        &Ulid::new(),
        Timestamp::new("2026-11-26T00:01:00Z"),
        retitle("nobody"),
        |_| Ok::<(), Refused>(()),
    );
    assert!(matches!(missing, Err(Refused::Store(_))));
}

#[test]
fn rate_ai_content_if_refuses_a_stale_precondition_before_rating() {
    let (_dir, store) = store();
    let ticket = add(&store);
    let read = ticket.updated_at.as_str().to_owned();
    update(
        &store,
        &ticket.id,
        Timestamp::new("2026-11-26T00:01:00Z"),
        retitle("competing"),
    )
    .unwrap();
    let rating = |at: &str, precondition| {
        rate_ai_content_if(
            &store,
            &ticket.id,
            Timestamp::new(at),
            AiContentRating {
                target: "activity:run-1",
                rating: Some(AiFeedbackRating::Helpful),
                explanation: None,
                actor: rater(),
            },
            precondition,
        )
    };
    assert_eq!(
        rating("2026-11-26T00:02:00Z", token_is(&read)).unwrap_err(),
        Refused::Changed("2026-11-26T00:01:00Z".into())
    );
    let stored = store.read_ticket(&ticket.id).unwrap();
    assert!(stored.notes.iter().all(|note| note.ai_feedback.is_none()));

    let rated = rating("2026-11-26T00:03:00Z", token_is("2026-11-26T00:01:00Z")).unwrap();
    assert_eq!(
        rated
            .notes
            .iter()
            .filter(|note| note.ai_feedback.is_some())
            .count(),
        1
    );
}
