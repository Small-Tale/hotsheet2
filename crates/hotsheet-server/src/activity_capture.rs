//! Activity capture: persisting live activity events and the optional deterministic
//! distillation into ticket notes (HS2-YMG48C split from lib.rs).

use crate::events::ChangeEvent;
use crate::{AppState, multistore};
use hotsheet_model::Timestamp;
use hotsheet_sync::LockExt;
use hotsheet_ticketing::{FsStore, GitProvider, Settings};

impl AppState {
    /// Persist one activity event and publish that exact event on the shared live bus.
    /// Unlike announcements this is also placed in the long-poll ring; the rolling
    /// activity store remains the authoritative reconnect/digest source.
    pub fn record_activity(
        &self,
        store: &FsStore,
        event: hotsheet_ticketing::ActivityEvent,
    ) -> std::io::Result<()> {
        let admitted = self.activity_volume.lock_or_recover().observe(event);
        for event in admitted {
            hotsheet_ticketing::activity::record(store, &event)?;
            self.emit(ChangeEvent {
                cursor: None,
                store: multistore::store_url_id(store),
                kind: "activity".to_string(),
                id: event.id.clone(),
                slug: String::new(),
                message: None,
                activity: Some(event.clone()),
                assignment: None,
                turn: None,
            });
            self.maybe_distill_activity(store, &event);
        }
        Ok(())
    }

    fn maybe_distill_activity(&self, store: &FsStore, event: &hotsheet_ticketing::ActivityEvent) {
        let store_id = multistore::store_url_id(store);
        let (pipeline_id, settings) = match event.project.as_deref() {
            Some(reference) => {
                let checkout = match self.checkout_registry.resolve(reference) {
                    Ok(checkout) => checkout,
                    Err(error) => {
                        tracing::warn!(
                            checkout = reference,
                            %error,
                            "activity distillation skipped for unresolved checkout"
                        );
                        return;
                    }
                };
                let canonical_store = store
                    .root()
                    .canonicalize()
                    .unwrap_or_else(|_| store.root().to_path_buf());
                let linked = checkout.sources.iter().any(|source| {
                    source.provider == "git"
                        && std::path::Path::new(&source.locator)
                            .canonicalize()
                            .unwrap_or_else(|_| source.locator.clone().into())
                            == canonical_store
                });
                if !linked {
                    tracing::warn!(
                        checkout = %checkout.id,
                        store = store_id,
                        "activity distillation skipped: checkout does not own store"
                    );
                    return;
                }
                (
                    format!("checkout:{}:store:{store_id}", checkout.id),
                    checkout.settings(),
                )
            }
            None => (
                format!("legacy-store:{store_id}"),
                Settings::new(store.root()),
            ),
        };
        let policy = match hotsheet_ticketing::DistillationPolicy::from_local_settings(&settings) {
            Ok(policy) => policy,
            Err(error) => {
                tracing::warn!(%error, "activity distillation policy ignored");
                return;
            }
        };
        // Named client adapters (including Apple Foundation Models) consume the same
        // normalized stream on-device. They are never loaded as server requirements.
        if !policy.enabled || policy.adapter != "deterministic" {
            {
                let mut pipelines = self.activity_distillation.lock_or_recover();
                pipelines.remove(&pipeline_id);
            }
            return;
        }
        let request = self.activity_distillation.with_lock(|pipelines| {
            pipelines
                .entry(pipeline_id)
                .or_default()
                .observe(event, &policy)
        });
        let Some(request) = request else {
            return;
        };
        let Some(note) = hotsheet_ticketing::activity_distillation::distill(
            &request,
            &policy,
            &hotsheet_ticketing::DeterministicActivitySummarizer,
        ) else {
            return;
        };
        // The hosted entry's index-backed resolver answers the note's ticket lookup, so
        // a distilled note never walks the store (HS2-2MJCMV). A store this server does
        // not host (or a path replaced since hosting) keeps the walking provider.
        let hosted = self
            .host
            .get(&store_id)
            .filter(|entry| entry.store.root() == store.root());
        let provider = match hosted {
            Some(entry) => {
                GitProvider::new(store_id, store.clone()).with_resolver(entry.resolver())
            }
            None => GitProvider::new(store_id, store.clone()),
        };
        if let Err(error) = hotsheet_ticketing::write_distilled_note(
            &provider,
            &request.provenance.ticket,
            Timestamp::new(event.ts.clone()),
            &note,
        ) {
            // Activity capture is authoritative and must not fail because an optional
            // distillation adapter or ticket mutation is temporarily unavailable.
            tracing::warn!(%error, "activity distillation note failed");
        }
    }
}

#[cfg(test)]
mod activity_distillation_tests {
    use super::AppState;
    use hotsheet_model::{NoteKind, Timestamp, Ulid};
    use hotsheet_ticketing::{
        ActivityEvent, ActivityKind, FsStore, NewTicket, Scope, Settings, StoreMetadata, ops,
    };
    use serde_json::json;

    #[test]
    fn enabled_deterministic_adapter_distills_live_activity_once() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        Settings::new(store.root())
            .set(
                "activity_distillation",
                json!({"enabled":true,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let mut event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"private decision text"}),
        );
        event.ticket = Some(ticket_id.to_string());
        event.session = Some("session-1".into());

        state.record_activity(&store, event.clone()).unwrap();
        state.record_activity(&store, event).unwrap();

        let ticket = store.read_ticket(&ticket_id).unwrap();
        assert_eq!(ticket.notes.len(), 1);
        assert_eq!(ticket.notes[0].kind, NoteKind::Activity);
        assert!(ticket.notes[0].text.starts_with("Recorded a decision"));
        assert!(!ticket.notes[0].text.contains("private decision text"));
    }

    /// HS2-2MJCMV: a distilled note addressed by slug resolves through the hosted entry's
    /// index, so a corrupt decoy declaring the same slug (which fails the store walk) does
    /// not block the note.
    #[test]
    fn distilled_notes_resolve_their_ticket_through_the_hosted_index() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        let ticket = ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        Settings::new(store.root())
            .set(
                "activity_distillation",
                json!({"enabled":true,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let decoy = store.ticket_path(&Ulid::new());
        std::fs::create_dir_all(decoy.parent().unwrap()).unwrap();
        std::fs::write(&decoy, format!("---\nslug: {}\n---\nbroken", ticket.slug)).unwrap();
        assert!(
            ops::resolve(&store, &ticket.slug).is_err(),
            "the walk fails"
        );

        let mut event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"decision"}),
        );
        event.ticket = Some(ticket.slug.clone());
        event.session = Some("session-1".into());
        state.record_activity(&store, event).unwrap();

        let ticket = store.read_ticket(&ticket_id).unwrap();
        assert_eq!(ticket.notes.len(), 1, "the indexed lookup wrote the note");
        assert_eq!(ticket.notes[0].kind, NoteKind::Activity);
    }

    #[test]
    fn project_activity_distillation_uses_explicit_checkout_settings_for_a_shared_store() {
        let root = tempfile::tempdir().unwrap();
        let store_root = root.path().join("shared.hs2");
        let store = FsStore::init(&store_root, &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let first_root = root.path().join("first");
        let second_root = root.path().join("second");
        std::fs::create_dir(&first_root).unwrap();
        std::fs::create_dir(&second_root).unwrap();
        let registry_path = root.path().join("home/checkouts.json");
        let registry = hotsheet_ticketing::checkouts::CheckoutRegistry::new(&registry_path);
        let first = registry
            .register(&first_root, Some("first"), None, vec![store_root.clone()])
            .unwrap();
        let second = registry
            .register(&second_root, Some("second"), None, vec![store_root])
            .unwrap();
        first
            .settings()
            .set(
                "activity_distillation",
                json!({"enabled":true,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        second
            .settings()
            .set(
                "activity_distillation",
                json!({"enabled":false,"adapter":"deterministic"}),
                Scope::Local,
            )
            .unwrap();
        let state = AppState::new(store.clone(), "secret".into())
            .unwrap()
            .with_checkout_registry(registry_path);

        let mut first_event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"first private decision"}),
        );
        first_event.ticket = Some(ticket_id.to_string());
        first_event.session = Some("first-session".into());
        first_event.project = Some(first.id);
        state.record_activity(&store, first_event).unwrap();

        let mut second_event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAW",
            "2026-09-02T01:01:00Z",
            "codex",
            ActivityKind::Decision,
            json!({"text":"second private decision"}),
        );
        second_event.ticket = Some(ticket_id.to_string());
        second_event.session = Some("second-session".into());
        second_event.project = Some(second.id);
        state.record_activity(&store, second_event).unwrap();

        let ticket = store.read_ticket(&ticket_id).unwrap();
        assert_eq!(ticket.notes.len(), 1);
        assert!(ticket.notes[0].text.starts_with("Recorded a decision"));
    }

    #[test]
    fn distillation_is_disabled_by_default() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let ticket_id = Ulid::from_string("01ARZ3NDEKTSV4RRFFQ69G5FAA").unwrap();
        ops::create(
            &store,
            ticket_id,
            "HS",
            Timestamp::new("2026-09-02T00:00:00Z"),
            NewTicket::default(),
        )
        .unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let mut event = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "2026-09-02T01:00:00Z",
            "codex",
            ActivityKind::Decision,
            json!({}),
        );
        event.ticket = Some(ticket_id.to_string());
        event.session = Some("session-1".into());
        state.record_activity(&store, event).unwrap();
        assert!(store.read_ticket(&ticket_id).unwrap().notes.is_empty());
    }

    #[test]
    fn noisy_activity_is_bounded_before_persistence_and_broadcast() {
        let root = tempfile::tempdir().unwrap();
        let store = FsStore::init(root.path(), &StoreMetadata::new("HS")).unwrap();
        let state = AppState::new(store.clone(), "secret".into()).unwrap();
        let mut live = state.subscribe();

        for index in 0..140 {
            let mut event = ActivityEvent::new(
                format!("01ARZ3NDEKTSV4RRFFQ{index:06}"),
                "2026-09-02T01:00:00Z",
                "codex",
                ActivityKind::Command,
                json!({"command": "chatty"}),
            );
            event.session = Some("noisy-session".into());
            state.record_activity(&store, event).unwrap();
        }
        let mut done = ActivityEvent::new(
            "01ARZ3NDEKTSV4RRFFQ999999",
            "2026-09-02T01:01:00Z",
            "codex",
            ActivityKind::TurnEnd,
            json!({}),
        );
        done.session = Some("noisy-session".into());
        state.record_activity(&store, done).unwrap();

        let recorded = hotsheet_ticketing::activity::read_recent(&store).unwrap();
        assert_eq!(recorded.len(), 130);
        assert_eq!(recorded[128].kind, ActivityKind::Summary);
        assert_eq!(recorded[128].detail["coalesced"], 12);
        assert_eq!(recorded[129].kind, ActivityKind::TurnEnd);

        let broadcast = std::iter::from_fn(|| live.try_recv().ok()).collect::<Vec<_>>();
        assert_eq!(broadcast.len(), recorded.len());
        assert_eq!(broadcast[128].activity.as_ref(), Some(&recorded[128]));
    }
}
