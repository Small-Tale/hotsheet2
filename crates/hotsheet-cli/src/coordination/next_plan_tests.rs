use super::*;
use hotsheet_model::{Timestamp, Ulid};
use hotsheet_ticketing::NewTicket;

fn planned(
    ticket: &hotsheet_model::Ticket,
    source: &str,
    worker: &str,
    operation: &str,
) -> NextPlanEntry {
    NextPlanEntry {
        operation: operation.into(),
        worker: worker.into(),
        generation: "generation-1".into(),
        ticket: TicketRef {
            connection_id: source.into(),
            native_id: ticket.id.to_string(),
        },
        slug: ticket.slug.clone(),
        priority_at_plan: None,
        updated_at_plan: None,
        scope: Vec::new(),
    }
}

#[test]
fn hs2_xcmavt_revision_generation_and_manual_assignment_reconcile_without_claim() {
    let (_root, coordinator) = tests::fixture();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let at = Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(now).unwrap());
    let store = FsStore::open(&coordinator.source).unwrap();
    let source = git_connection_id(&store);
    let ticket = ops::create(
        &store,
        Ulid::new(),
        "HS",
        at,
        NewTicket {
            title: "Synthetic planned task".into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap();
    tests::register(
        &coordinator,
        Role::Integrator,
        "integrator",
        "generation-1",
        now,
    );
    tests::register(&coordinator, Role::Worker, "worker-a", "generation-1", now);
    tests::register(&coordinator, Role::Worker, "worker-b", "generation-1", now);
    let entry = planned(&ticket, &source, "worker-a", "plan-one");
    assert!(
        coordinator
            .replace_next_plan(
                "integrator",
                "generation-1",
                &|| now,
                0,
                vec![entry.clone()],
                false
            )
            .is_err()
    );
    assert_eq!(coordinator.state().unwrap().version, 3);
    assert!(
        coordinator
            .replace_next_plan(
                "integrator",
                "generation-1",
                &|| now,
                0,
                vec![
                    entry.clone(),
                    planned(&ticket, &source, "worker-b", "plan-two")
                ],
                true
            )
            .is_err()
    );
    assert_eq!(
        coordinator
            .replace_next_plan(
                "integrator",
                "generation-1",
                &|| now,
                0,
                vec![entry.clone()],
                true
            )
            .unwrap(),
        1
    );
    assert_eq!(coordinator.state().unwrap().version, 7);
    assert!(
        coordinator
            .replace_next_plan("integrator", "generation-1", &|| now, 0, vec![], false)
            .is_err()
    );
    let snapshot = coordinator.snapshot(now).unwrap();
    assert!(
        snapshot.planned[0].reasons.is_empty(),
        "{:?}",
        snapshot.planned[0].reasons
    );
    assert_eq!(store.read_ticket(&ticket.id).unwrap().claimed_by, None);
    assert_eq!(coordinator.next_plan_events(0).unwrap().len(), 1);
    let mut assignment = tests::assignment("manual", "worker-b", &ticket.id.to_string());
    assignment.ticket.connection_id = source.clone();
    assignment.slug = ticket.slug.clone();
    coordinator
        .assign("integrator", "generation-1", now, assignment)
        .unwrap();
    let state = coordinator.state().unwrap();
    assert!(state.next_plan.is_empty());
    assert_eq!(state.plan_revision, 2);
    assert_eq!(state.plan_events[1].kind, PlanEventKind::Assigned);
    assert_eq!(store.read_ticket(&ticket.id).unwrap().claimed_by, None);
    assert_eq!(coordinator.next_plan_events(1).unwrap().len(), 1);
}

#[test]
fn hs2_xcmavt_restart_replacement_and_dispatch_preserve_exact_plan() {
    let (root, coordinator) = tests::fixture();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let at = Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(now).unwrap());
    let store = FsStore::open(&coordinator.source).unwrap();
    let source = git_connection_id(&store);
    let ticket = ops::create(
        &store,
        Ulid::new(),
        "HS",
        at,
        NewTicket {
            title: "Synthetic dispatch".into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap();
    tests::register(
        &coordinator,
        Role::Integrator,
        "integrator",
        "generation-1",
        now,
    );
    tests::register(&coordinator, Role::Worker, "worker", "generation-1", now);
    let entry = planned(&ticket, &source, "worker", "planned-op");
    coordinator
        .replace_next_plan("integrator", "generation-1", &|| now, 0, vec![entry], true)
        .unwrap();
    let reopened = Coordinator::open(&root.path().join("project"), &coordinator.source).unwrap();
    assert_eq!(reopened.state().unwrap().next_plan.len(), 1);
    assert!(
        reopened
            .dispatch_next_plan(
                "integrator",
                "generation-1",
                &|| now,
                0,
                "planned-op",
                tests::SHA
            )
            .is_err()
    );
    let assigned = reopened
        .dispatch_next_plan(
            "integrator",
            "generation-1",
            &|| now,
            1,
            "planned-op",
            tests::SHA,
        )
        .unwrap();
    assert_eq!(assigned.id, "planned-op");
    assert_eq!(reopened.state().unwrap().plan_revision, 2);
    assert!(reopened.state().unwrap().next_plan.is_empty());
    assert_eq!(
        reopened
            .dispatch_next_plan(
                "integrator",
                "generation-1",
                &|| now,
                1,
                "planned-op",
                tests::SHA
            )
            .unwrap()
            .id,
        "planned-op"
    );
    assert_eq!(store.read_ticket(&ticket.id).unwrap().claimed_by, None);
}

#[test]
fn hs2_xcmavt_disconnected_and_replaced_worker_retain_blocked_intent() {
    let (_root, coordinator) = tests::fixture();
    let now = time::OffsetDateTime::now_utc().unix_timestamp();
    let at = Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(now).unwrap());
    let store = FsStore::open(&coordinator.source).unwrap();
    let source = git_connection_id(&store);
    let ticket = ops::create(
        &store,
        Ulid::new(),
        "HS",
        at,
        NewTicket {
            title: "Synthetic retained plan".into(),
            up_next: true,
            ..NewTicket::default()
        },
    )
    .unwrap();
    tests::register(
        &coordinator,
        Role::Integrator,
        "integrator",
        "generation-1",
        now,
    );
    tests::register(&coordinator, Role::Worker, "worker", "generation-1", now);
    coordinator
        .replace_next_plan(
            "integrator",
            "generation-1",
            &|| now,
            0,
            vec![planned(&ticket, &source, "worker", "retained-op")],
            true,
        )
        .unwrap();
    let mut reprioritized = store.read_ticket(&ticket.id).unwrap();
    reprioritized.priority = hotsheet_model::Priority::High;
    store.write_ticket(&reprioritized).unwrap();
    assert!(
        coordinator.snapshot(now).unwrap().planned[0]
            .reasons
            .contains(&"priority_changed".to_string())
    );
    assert!(
        coordinator
            .dispatch_next_plan(
                "integrator",
                "generation-1",
                &|| now,
                1,
                "retained-op",
                tests::SHA
            )
            .is_err()
    );
    let disconnected = coordinator.snapshot(now + 121).unwrap();
    assert!(
        disconnected.planned[0]
            .reasons
            .contains(&"worker_disconnected".to_string())
    );
    tests::register(
        &coordinator,
        Role::Worker,
        "worker",
        "generation-2",
        now + 121,
    );
    let replaced = coordinator.snapshot(now + 121).unwrap();
    assert!(
        replaced.planned[0]
            .reasons
            .contains(&"worker_replaced".to_string())
    );
    assert_eq!(
        coordinator.state().unwrap().next_plan[0].operation,
        "retained-op"
    );
    assert_eq!(coordinator.state().unwrap().plan_revision, 1);
}
