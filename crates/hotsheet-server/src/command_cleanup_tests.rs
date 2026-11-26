//! HS2-27C161 command ticket auto-cleanup: hosted change events, the per-run sweep, the change
//! bus, and an external provider write.

use super::*;
use hotsheet_model::{Priority, Status};
use hotsheet_ticketing::command_cleanup::CommandCleanup;
use hotsheet_ticketing::{FsStore, GitProvider, Scope, StoreMetadata, ops};
use serde_json::json;
use std::sync::Arc;

const COMMAND: &str = "daily-summary";
const TAG: &str = "hs2-command:daily-summary";

/// A hosted Markdown store linked to one checkout whose AI command has `cleanup`.
fn fixture(cleanup: &str) -> (tempfile::TempDir, AppState, FsStore, String) {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    let store_path = root.path().join("tickets");
    std::fs::create_dir(&project).unwrap();
    let store = FsStore::init_git(&store_path, &StoreMetadata::new("SY")).unwrap();
    let state = AppState::new(store.clone(), "synthetic-secret".into())
        .unwrap()
        .with_checkout_registry(root.path().join("checkouts.json"));
    let checkout = state
        .checkout_registry
        .register(&project, None, None, vec![store_path])
        .unwrap();
    set_cleanup(&checkout.settings(), cleanup);
    let connection = crate::multistore::store_url_id(&store);
    (root, state, store, connection)
}

fn set_cleanup(settings: &hotsheet_ticketing::Settings, cleanup: &str) {
    let mut command = json!({"id": COMMAND, "title": "Daily summary", "kind": "ai",
        "prompt": "Summarize synthetic work"});
    if cleanup != "none" {
        command["cleanup"] = cleanup.into();
    }
    settings
        .set("commands", json!([command]), Scope::Local)
        .unwrap();
}

fn command_ticket(store: &FsStore, tags: &[&str]) -> hotsheet_model::Ticket {
    ops::create(
        store,
        Ulid::new(),
        "SY",
        crate::now(),
        ops::NewTicket {
            title: "Daily summary".into(),
            category: "task".into(),
            priority: Priority::Highest,
            status: Status::NotStarted,
            details: "Summarize synthetic work".into(),
            tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
            up_next: true,
            blocked_by: Vec::new(),
            group: None,
        },
    )
    .unwrap()
}

fn set_status(store: &FsStore, ticket: &hotsheet_model::Ticket, status: Status) {
    ops::update(
        store,
        &ticket.id,
        crate::now(),
        ops::TicketPatch {
            status: Some(status),
            ..Default::default()
        },
    )
    .unwrap();
}

fn event(connection: &str, ticket: &hotsheet_model::Ticket, kind: &str) -> ChangeEvent {
    ChangeEvent {
        live: None,
        cursor: None,
        store: connection.into(),
        kind: kind.into(),
        id: ticket.id.to_string(),
        slug: ticket.slug.clone(),
        message: None,
        activity: None,
        assignment: None,
        turn: None,
    }
}

fn read(store: &FsStore, ticket: &hotsheet_model::Ticket) -> hotsheet_model::Ticket {
    store.read_ticket(&ticket.id).unwrap()
}

/// The cleanup notes by their text; the status transition note it causes carries the same
/// system actor.
fn cleanup_notes(ticket: &hotsheet_model::Ticket) -> usize {
    ticket
        .notes
        .iter()
        .filter(|note| note.text.starts_with(command_cleanup::CLEANUP_NOTE_PREFIX))
        .count()
}

/// Archive on completion, once: a ticket moved back to Completed is kept.
#[test]
fn hs2_27c161_archives_a_completed_command_ticket_once() {
    let (_root, state, store, connection) = fixture("archive");
    let ticket = command_ticket(&store, &[TAG]);
    // Open tickets are never touched.
    for status in [Status::NotStarted, Status::Started] {
        set_status(&store, &ticket, status);
        assert!(on_change_event(&state, &event(&connection, &ticket, "updated")).is_none());
    }
    set_status(&store, &ticket, Status::Completed);
    let cleaned = on_change_event(&state, &event(&connection, &ticket, "updated")).unwrap();
    assert_eq!(cleaned.status, Status::Archive);
    let archived = read(&store, &ticket);
    assert_eq!(archived.status, Status::Archive);
    assert_eq!(cleanup_notes(&archived), 1);
    let note = archived
        .notes
        .iter()
        .find(|note| note.text.starts_with(command_cleanup::CLEANUP_NOTE_PREFIX))
        .unwrap();
    assert_eq!(
        note.actor.as_ref().and_then(|actor| actor.id.as_deref()),
        Some(command_cleanup::CLEANUP_ACTOR_ID)
    );
    assert_eq!(
        note.summary.as_deref(),
        Some(command_cleanup::CLEANUP_NOTE_SUMMARY)
    );
    // Repeated events, and moving it back to Completed, keep it where the person put it.
    assert!(on_change_event(&state, &event(&connection, &ticket, "changed")).is_none());
    set_status(&store, &ticket, Status::Completed);
    assert!(on_change_event(&state, &event(&connection, &ticket, "updated")).is_none());
    assert_eq!(read(&store, &ticket).status, Status::Completed);
    assert_eq!(cleanup_notes(&read(&store, &ticket)), 1);
}

/// Delete moves the ticket to the restorable Trash; Restore keeps it afterwards.
#[test]
fn hs2_27c161_delete_moves_to_trash_and_a_restore_keeps_it() {
    let (_root, state, store, connection) = fixture("delete");
    let ticket = command_ticket(&store, &[TAG]);
    set_status(&store, &ticket, Status::Verified);
    let cleaned = on_change_event(&state, &event(&connection, &ticket, "updated")).unwrap();
    assert_eq!(cleaned.status, Status::Deleted);
    assert!(store.ticket_path(&ticket.id).exists(), "Trash, not a purge");
    let restored = ops::restore(&store, &ticket.id, crate::now()).unwrap();
    assert_eq!(restored.status, Status::Verified);
    assert!(on_change_event(&state, &event(&connection, &ticket, "updated")).is_none());
    assert_eq!(read(&store, &ticket).status, Status::Verified);
}

/// No policy, no command tag, another command, or a non-ticket event leaves tickets alone.
#[test]
fn hs2_27c161_leaves_other_tickets_alone() {
    let (_root, state, store, connection) = fixture("none");
    let ticket = command_ticket(&store, &[TAG]);
    set_status(&store, &ticket, Status::Completed);
    assert!(on_change_event(&state, &event(&connection, &ticket, "updated")).is_none());
    assert_eq!(read(&store, &ticket).status, Status::Completed);

    let (_root, state, store, connection) = fixture("delete");
    for tags in [&["bug"][..], &["hs2-command:other-command"][..]] {
        let ticket = command_ticket(&store, tags);
        set_status(&store, &ticket, Status::Completed);
        assert!(on_change_event(&state, &event(&connection, &ticket, "updated")).is_none());
        assert_eq!(read(&store, &ticket).status, Status::Completed);
    }
    let ticket = command_ticket(&store, &[TAG]);
    set_status(&store, &ticket, Status::Completed);
    assert!(on_change_event(&state, &event(&connection, &ticket, "note_added")).is_none());
    assert!(on_change_event(&state, &event("unknown-store", &ticket, "updated")).is_none());
    assert_eq!(read(&store, &ticket).status, Status::Completed);
}

/// A command run tidies its completed tickets, skipping open and already cleaned ones.
#[test]
fn hs2_27c161_sweep_cleans_completed_tickets_once() {
    let (_root, state, store, connection) = fixture("archive");
    let done: Vec<_> = (0..2)
        .map(|_| {
            let ticket = command_ticket(&store, &[TAG]);
            set_status(&store, &ticket, Status::Completed);
            ticket
        })
        .collect();
    let open = command_ticket(&store, &[TAG]);
    let command = commands::from_settings(&state.checkout_registry.list().unwrap()[0].settings())
        .unwrap()
        .remove(0);
    assert_eq!(command.cleanup, CommandCleanup::Archive);
    assert_eq!(sweep(&state, &connection, &command), 2);
    for ticket in &done {
        assert_eq!(read(&store, ticket).status, Status::Archive);
    }
    assert_eq!(read(&store, &open).status, Status::NotStarted);
    assert_eq!(sweep(&state, &connection, &command), 0);
    let none = CommandDefinition {
        cleanup: CommandCleanup::None,
        ..command
    };
    set_status(&store, &open, Status::Completed);
    assert_eq!(sweep(&state, &connection, &none), 0);
    assert_eq!(read(&store, &open).status, Status::Completed);
}

/// The change bus applies cleanup for a write made anywhere, and stops with the server.
#[tokio::test]
async fn hs2_27c161_bus_cleans_up_a_hosted_completion() {
    let (_root, state, store, connection) = fixture("delete");
    let ticket = command_ticket(&store, &[TAG]);
    let mut events = state.subscribe();
    let task = spawn_command_cleanup(state.clone());
    let entry = state.host.get(&connection).unwrap();
    set_status(&store, &ticket, Status::Completed);
    state.changed_in(&entry, "updated", &read(&store, &ticket));
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        loop {
            let event = events.recv().await.unwrap();
            if event.id == ticket.id.to_string() && read(&store, &ticket).status == Status::Deleted
            {
                break;
            }
        }
    })
    .await
    .expect("the cleanup write is announced");
    state.begin_stopping();
    tokio::time::timeout(std::time::Duration::from_secs(5), task)
        .await
        .expect("the bus task ends when the server stops")
        .unwrap();
}

/// An external provider's status write through the server returns the cleaned-up ticket.
#[test]
fn hs2_27c161_external_provider_write_cleans_up_through_the_provider() {
    let (root, state, _hosted, _connection) = fixture("archive");
    let external =
        FsStore::init_git(root.path().join("external"), &StoreMetadata::new("EX")).unwrap();
    let state = state.with_ticket_provider(Arc::new(GitProvider::new(
        "external-test",
        external.clone(),
    )));
    // Link the external connection to the checkout, as a provider connection would be.
    let registry = root.path().join("checkouts.json");
    let mut file: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&registry).unwrap()).unwrap();
    file["checkouts"][0]["sources"]
        .as_array_mut()
        .unwrap()
        .push(
            json!({"connection_id": "external-test", "provider": "github",
            "locator": "synthetic/repo"}),
        );
    std::fs::write(&registry, file.to_string()).unwrap();
    assert_eq!(
        policy(&state, "external-test", COMMAND).cleanup,
        CommandCleanup::Archive
    );
    let ticket = command_ticket(&external, &[TAG]);
    let request = serde_json::from_value(json!({"status": "completed"})).unwrap();
    let (returned, _) = do_provider_update(
        &state,
        "external-test",
        None,
        &ticket.id.to_string(),
        request,
    )
    .unwrap();
    assert_eq!(returned.status, Status::Archive);
    assert_eq!(read(&external, &ticket).status, Status::Archive);
    // A write that sets no status never triggers it.
    let other = command_ticket(&external, &[TAG]);
    set_status(&external, &other, Status::Completed);
    let request = serde_json::from_value(json!({"title": "Renamed"})).unwrap();
    let (returned, _) = do_provider_update(
        &state,
        "external-test",
        None,
        &other.id.to_string(),
        request,
    )
    .unwrap();
    assert_eq!(returned.status, Status::Completed);
}

fn checkout_settings(state: &AppState) -> hotsheet_ticketing::Settings {
    state.checkout_registry.list().unwrap()[0].settings()
}

/// Tickets completed before cleanup was turned on stay put on the bus and in the sweep;
/// tickets completed afterwards are cleaned.
#[test]
fn hs2_27c161_only_completions_after_enabling_are_cleaned() {
    let (_root, state, store, connection) = fixture("none");
    let before = command_ticket(&store, &[TAG]);
    set_status(&store, &before, Status::Completed);
    set_cleanup(&checkout_settings(&state), "archive");
    let command = commands::from_settings(&checkout_settings(&state))
        .unwrap()
        .remove(0);
    assert!(command.cleanup_since.is_some());
    assert!(on_change_event(&state, &event(&connection, &before, "updated")).is_none());
    assert_eq!(sweep(&state, &connection, &command), 0);
    assert_eq!(read(&store, &before).status, Status::Completed);

    let after = command_ticket(&store, &[TAG]);
    set_status(&store, &after, Status::Completed);
    assert!(on_change_event(&state, &event(&connection, &after, "updated")).is_some());
    assert_eq!(read(&store, &after).status, Status::Archive);
    assert_eq!(read(&store, &before).status, Status::Completed);
}

/// Archive to Delete keeps the stamp, so an earlier completion still counts; None and back
/// restamps, so a completion from the first period no longer does.
#[test]
fn hs2_27c161_switching_policies_keeps_or_resets_the_stamp() {
    let (_root, state, store, connection) = fixture("archive");
    let settings = checkout_settings(&state);
    let since = |settings: &hotsheet_ticketing::Settings| {
        commands::from_settings(settings).unwrap()[0]
            .cleanup_since
            .clone()
    };
    let first = since(&settings).expect("stamped");
    let completed = command_ticket(&store, &[TAG]);
    set_status(&store, &completed, Status::Completed);
    set_cleanup(&settings, "delete");
    assert_eq!(since(&settings).as_deref(), Some(first.as_str()));
    assert_eq!(
        on_change_event(&state, &event(&connection, &completed, "updated"))
            .unwrap()
            .status,
        Status::Deleted
    );

    let earlier = command_ticket(&store, &[TAG]);
    set_status(&store, &earlier, Status::Completed);
    set_cleanup(&settings, "none");
    assert_eq!(since(&settings), None);
    set_cleanup(&settings, "archive");
    let second = since(&settings).expect("restamped");
    assert!(
        command_cleanup::parse_instant(&second).unwrap()
            >= command_cleanup::parse_instant(&first).unwrap()
    );
    assert!(on_change_event(&state, &event(&connection, &earlier, "updated")).is_none());
    assert_eq!(read(&store, &earlier).status, Status::Completed);
}

/// A done ticket with no completion or verification time is never cleaned up.
#[test]
fn hs2_27c161_missing_completion_time_is_left_alone() {
    let (_root, state, store, connection) = fixture("delete");
    let ticket = command_ticket(&store, &[TAG]);
    set_status(&store, &ticket, Status::Completed);
    let mut stripped = read(&store, &ticket);
    stripped.completed_at = None;
    stripped.verified_at = None;
    store.write_ticket_committing(&stripped).unwrap();
    assert!(on_change_event(&state, &event(&connection, &ticket, "updated")).is_none());
    let command = commands::from_settings(&checkout_settings(&state))
        .unwrap()
        .remove(0);
    assert_eq!(sweep(&state, &connection, &command), 0);
    assert_eq!(read(&store, &ticket).status, Status::Completed);
}
