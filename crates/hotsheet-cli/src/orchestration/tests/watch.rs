//! HS2-7QHWHJ: run events, status and following target changes without polling.
use hotsheet_model::{Status, Ticket};
use hotsheet_ticketing::checkouts::Checkout;
use hotsheet_ticketing::orchestration::{
    DenialReason, RunEventKind, RunEventSource, RunState, TargetAction, TargetOpError,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::{Value, json};

use super::tickets::{
    Machine, NOW, RemotePeer, bind_and_allow, human, machine, machine_with, op, unsupported, worker,
};
use super::*;

const LEASE: &str = "2030-01-01T00:00:00Z";

/// A host change stream the test drives by hand.
#[derive(Default)]
struct Feed {
    events: Mutex<Vec<TargetTicketEvent>>,
    sessions: Mutex<Vec<TargetSessionEvent>>,
    cursor: AtomicUsize,
    calls: Mutex<Vec<(Option<u64>, u64)>>,
    failing: std::sync::atomic::AtomicBool,
    overflow: std::sync::atomic::AtomicBool,
    /// HS2-VN8PDA: work done on the host while the next request waits.
    during: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl Feed {
    fn push(&self, connection: &str, ticket: &Ticket) {
        let cursor = self.cursor.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        self.events
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(TargetTicketEvent {
                cursor,
                connection_id: connection.into(),
                native_id: ticket.id.to_string(),
            });
    }
    /// HS2-257Q2Z: a hosted session ended or finished a turn.
    fn session(&self, session_id: &str) {
        let cursor = self.cursor.fetch_add(1, Ordering::SeqCst) as u64 + 1;
        self.sessions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push(TargetSessionEvent {
                cursor,
                session_id: session_id.into(),
            });
    }
    fn calls(&self) -> Vec<(Option<u64>, u64)> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}
impl TargetEvents for Feed {
    fn poll(
        &self,
        _: &Checkout,
        since: Option<u64>,
        wait_ms: u64,
    ) -> Result<TargetEventPage, OrchestrationError> {
        self.calls
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .push((since, wait_ms));
        if self.failing.load(Ordering::SeqCst) {
            return Err(OrchestrationError::TargetOperation(TargetOpError::Failed));
        }
        let during = self
            .during
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(during) = during {
            during();
        }
        let cursor = self.cursor.load(Ordering::SeqCst) as u64;
        Ok(TargetEventPage {
            cursor,
            events: since.map_or_else(Vec::new, |since| {
                self.events
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .filter(|event| event.cursor > since)
                    .cloned()
                    .collect()
            }),
            sessions: since.map_or_else(Vec::new, |since| {
                self.sessions
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .iter()
                    .filter(|event| event.cursor > since)
                    .cloned()
                    .collect()
            }),
            overflow: self.overflow.load(Ordering::SeqCst),
            speculation: None,
        })
    }
}

fn target_ticket(m: &Machine, title: &str) -> Ticket {
    ops::create(
        &m.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: title.into(),
            ..Default::default()
        },
    )
    .unwrap()
}
fn dispatch(
    m: &Machine,
    orchestration: &Ticket,
    target: &Ticket,
    operation: &str,
    worker_id: &str,
) -> String {
    let mut rng = Rng(u128::from(operation.len() as u64) * 1000 + 7);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    m.runtime
        .run_dispatch(
            &m.owner.id,
            serde_json::from_value(json!({
                "ticket": {"connection_id": m.owner_connection(), "native_id": orchestration.id.to_string()},
                "target_id": id(1), "connection_id": m.target_connection(),
                "native_id": target.id.to_string(), "operation_id": operation,
                "worker_id": worker_id,
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run
        .run_id
}
fn complete(m: &Machine, ticket: &Ticket) {
    ops::update(
        &m.target_store,
        &ticket.id,
        NOW.into(),
        TicketPatch {
            status: Some(Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
}
fn kinds(
    m: &Machine,
    run_id: &str,
) -> Vec<(
    RunEventKind,
    RunEventSource,
    Option<RunState>,
    Option<String>,
)> {
    m.runtime
        .run_events(&m.owner.id, 0, 500, Some(run_id), 0)
        .unwrap()
        .entries
        .into_iter()
        .map(|entry| {
            (
                entry.record.kind,
                entry.record.source,
                entry.record.state,
                entry.record.reason,
            )
        })
        .collect()
}

/// HS2-VN8PDA: a run dispatched while the follower's request waits is matched against
/// the page that request returns. Its claim arrives on that page and is not offered
/// again, so a run list read before the wait left the run queued until some unrelated
/// change; the two-server loop test stayed `queued` for that reason.
#[test]
fn hs2_vn8pda_a_run_dispatched_during_the_wait_is_synced_from_that_page() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let first = target_ticket(&m, "First");
    let second = target_ticket(&m, "Second");
    let run_one = dispatch(&m, &orchestration, &first, "op-1", "worker-a");
    let mut state = WatchState::default();
    let handshake = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(handshake.synced, std::slice::from_ref(&run_one));
    // While the next request waits, a second run is dispatched and its ticket changes.
    let (runtime, owner, owner_connection, target_connection) = (
        m.runtime.clone(),
        m.owner.id.clone(),
        m.owner_connection(),
        m.target_connection(),
    );
    let (during_feed, orchestration_id, second_ticket) =
        (feed.clone(), orchestration.id.to_string(), second.clone());
    let (dispatched, recorded) = std::sync::mpsc::channel();
    *feed
        .during
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(Box::new(move || {
        let mut rng = Rng(7_007);
        let mut tx = Transaction {
            clock: &Clock,
            rng: &mut rng,
        };
        let run = runtime
            .run_dispatch(
                &owner,
                serde_json::from_value(json!({
                    "ticket": {"connection_id": owner_connection, "native_id": orchestration_id},
                    "target_id": id(1), "connection_id": target_connection,
                    "native_id": second_ticket.id.to_string(), "operation_id": "op-during",
                    "worker_id": "worker-b",
                }))
                .unwrap(),
                Some(&worker()),
                &mut tx,
            )
            .unwrap()
            .run
            .run_id;
        during_feed.push(&target_connection, &second_ticket);
        dispatched.send(run).unwrap();
    }));
    let step = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    let run_two = recorded.try_recv().unwrap();
    assert_eq!(step.synced, std::slice::from_ref(&run_two));
    assert!(
        m.runtime
            .run_events(&m.owner.id, 0, 500, Some(&run_two), 0)
            .unwrap()
            .entries
            .iter()
            .any(|entry| entry.record.kind == RunEventKind::Observed),
        "the run dispatched during the wait was never observed"
    );
}

#[test]
fn hs2_7qhwhj_a_watch_is_idle_without_runs_and_syncs_only_changed_tickets() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let mut state = WatchState::default();
    // No active run: the target is not contacted at all, however often this is asked.
    for _ in 0..3 {
        assert_eq!(
            m.runtime
                .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx),
            WatchStep::default()
        );
    }
    assert!(feed.calls().is_empty());
    let orchestration = m.orchestration_ticket("Coordinate");
    let first = target_ticket(&m, "First");
    let second = target_ticket(&m, "Second");
    let unrelated = target_ticket(&m, "Not a run");
    let run_one = dispatch(&m, &orchestration, &first, "op-1", "worker-a");
    let run_two = dispatch(&m, &orchestration, &second, "op-22", "worker-b");
    // First contact is a handshake: no backlog, so every run is caught up once.
    let step = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(step.active && !step.failed);
    assert_eq!(step.synced, [run_one.clone(), run_two.clone()]);
    assert_eq!(feed.calls(), [(None, 0)]);
    assert_eq!(state.cursor(), Some(0));
    // Quiet target: one bounded request, nothing synced.
    let quiet = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 60_000, &mut tx);
    assert!(quiet.synced.is_empty());
    assert_eq!(feed.calls()[1], (Some(0), MAX_TARGET_EVENT_WAIT_MS));
    // A change to a ticket no run concerns syncs nothing.
    feed.push(&m.target_connection(), &unrelated);
    feed.push("another-source", &first);
    let other = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(other.synced.is_empty());
    assert_eq!(state.cursor(), Some(2));
    // The first run's ticket completes on the target: only that run is synced, and
    // it completes without anyone asking for a sync.
    complete(&m, &first);
    feed.push(&m.target_connection(), &first);
    let changed = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(changed.synced, std::slice::from_ref(&run_one));
    assert_eq!(
        m.runtime.run(&m.owner.id, &run_one).unwrap().state,
        RunState::Completed
    );
    assert_eq!(
        m.runtime.run(&m.owner.id, &run_two).unwrap().state,
        RunState::Dispatched
    );
    // The same change delivered again (a replay) is harmless and adds no event.
    let before = kinds(&m, &run_two).len();
    feed.push(&m.target_connection(), &second);
    feed.push(&m.target_connection(), &second);
    let replayed = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(replayed.synced, std::slice::from_ref(&run_two));
    assert_eq!(kinds(&m, &run_two).len(), before);
    // The stream of the completed run: dispatched, observed, then completed from the
    // target, each once.
    use RunEventKind::{Observed, State};
    use RunEventSource::{Orchestrator, Target};
    assert_eq!(
        kinds(&m, &run_one),
        [
            (
                State,
                Orchestrator,
                Some(RunState::Dispatched),
                Some("claimed".into())
            ),
            (Observed, Target, None, Some("started".into())),
            // HS2-WBREDM: connecting reconciles the run once; its outcome is one event.
            (
                RunEventKind::Reconciled,
                Orchestrator,
                None,
                Some("continue".into())
            ),
            (
                State,
                Target,
                Some(RunState::Completed),
                Some("target_completed".into())
            ),
            (Observed, Target, None, Some("completed".into())),
        ]
    );
    // The host lost its backlog: every remaining run is caught up.
    feed.overflow.store(true, Ordering::SeqCst);
    let overflow = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(overflow.synced, std::slice::from_ref(&run_two));
    feed.overflow.store(false, Ordering::SeqCst);
    // The last run ends: the watch goes idle again and stops asking.
    complete(&m, &second);
    feed.push(&m.target_connection(), &second);
    m.runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    let calls = feed.calls().len();
    let idle = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(idle, WatchStep::default());
    assert_eq!(feed.calls().len(), calls);
    assert_eq!(state.cursor(), None);
}

#[test]
fn hs2_7qhwhj_an_outage_marks_runs_stale_once_and_reconnect_replays_from_the_cursor() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Watched");
    let run_id = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a");
    let mut state = WatchState::default();
    // Never observed yet: the status says so rather than implying it is current.
    let unknown = m.runtime.run_status(&m.owner.id, &run_id).unwrap();
    assert_eq!(unknown.reachability, "unknown");
    assert!(unknown.stale);
    assert_eq!(unknown.last_confirmed_at_ms, None);
    m.runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    let confirmed = m.runtime.run_status(&m.owner.id, &run_id).unwrap();
    assert_eq!(confirmed.reachability, "reachable");
    assert!(!confirmed.stale);
    assert_eq!(confirmed.run.observed.as_ref().unwrap().status, "started");
    assert_eq!(
        confirmed
            .run
            .observed
            .as_ref()
            .unwrap()
            .claimed_by
            .as_deref(),
        Some("worker-a")
    );
    // The change stream fails but the target still answers reads: the watch backs off
    // and the run stays confirmed.
    feed.failing.store(true, Ordering::SeqCst);
    let flaky = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(flaky.active && flaky.failed);
    assert_eq!(flaky.synced, std::slice::from_ref(&run_id));
    assert_eq!(state.cursor(), Some(0));
    assert_eq!(
        m.runtime
            .run_status(&m.owner.id, &run_id)
            .unwrap()
            .reachability,
        "reachable"
    );
    // The target itself becomes unusable (its source is disabled): the run goes stale
    // and keeps its last confirmed state. The outage is one event, however long it lasts.
    let disabled = |disabled: bool| {
        // A git source cannot be disabled; unbinding is how this fixture loses the target.
        let _ = disabled;
    };
    disabled(true);
    let binding = m.runtime.resolve(&m.reference(1), &mut tx).unwrap().binding;
    let destination = binding.destination.clone();
    m.runtime
        .request(
            "DELETE",
            &format!(
                "/checkouts/{}/orchestration/targets/{}/binding",
                m.owner.id,
                id(1)
            ),
            &json!({"target":m.reference(1),"expected_binding_revision":binding.revision,
                "previous_destination":destination}),
            &mut tx,
        )
        .unwrap();
    // While the target was unreachable its ticket changed.
    feed.failing.store(false, Ordering::SeqCst);
    complete(&m, &ticket);
    feed.push(&m.target_connection(), &ticket);
    for _ in 0..3 {
        let down = m
            .runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
        assert!(down.active && down.failed);
        assert!(down.synced.is_empty());
    }
    let stale = m.runtime.run_status(&m.owner.id, &run_id).unwrap();
    assert_eq!(stale.reachability, "unreachable");
    assert!(stale.stale);
    assert_eq!(stale.run.state, RunState::Dispatched);
    assert_eq!(stale.run.observed.as_ref().unwrap().status, "started");
    assert_eq!(
        stale.run.unreachable.as_ref().unwrap().reason,
        "target_unavailable"
    );
    let outages = kinds(&m, &run_id)
        .into_iter()
        .filter(|event| event.0 == RunEventKind::TargetUnreachable)
        .count();
    assert_eq!(outages, 1);
    // The cursor was kept through the outage.
    assert_eq!(state.cursor(), Some(0));
    assert!(matches!(
        m.runtime
            .run_sync(&m.owner.id, &run_id, Some(&worker()), &mut tx),
        Err(OrchestrationError::Denied(DenialReason::TargetUnavailable))
    ));
    // Bound again: the next step replays the change that happened meanwhile.
    m.runtime
        .bind(
            BindTarget {
                target: m.reference(1),
                expected_manifest_revision: m
                    .runtime
                    .declarations(&m.owner.id)
                    .unwrap()
                    .revision
                    .unwrap(),
                expected_binding_revision: binding.revision + 1,
                previous_destination: None,
                destination: destination.unwrap(),
            },
            &mut tx,
        )
        .unwrap();
    let back = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(!back.failed);
    assert_eq!(back.synced, std::slice::from_ref(&run_id));
    assert_eq!(feed.calls().last().unwrap().0, Some(0));
    let done = m.runtime.run_status(&m.owner.id, &run_id).unwrap();
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(done.reachability, "reachable");
    let stream: Vec<_> = kinds(&m, &run_id).into_iter().map(|e| e.0).collect();
    use RunEventKind::{Observed, Reconciled, State, TargetReachable, TargetUnreachable};
    // HS2-WBREDM: each new reconciliation outcome is one event: confirmed on connect,
    // held while the target could not be verified. The finished run keeps none.
    assert_eq!(
        stream,
        [
            State,
            Observed,
            Reconciled,
            TargetUnreachable,
            Reconciled,
            State,
            Observed,
            TargetReachable
        ]
    );
}

#[test]
fn hs2_7qhwhj_events_replay_by_cursor_and_wait_for_the_next_one_without_polling() {
    let m = machine(10, false, unsupported);
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Watched");
    let events = format!("/checkouts/{}/orchestration/run-events", m.owner.id);
    // Nothing yet: an immediate read is empty and creates nothing.
    assert_eq!(
        m.runtime
            .request("GET", &events, &Value::Null, &mut tx)
            .unwrap(),
        json!({"entries":[],"cursor":0,"gap":false,"more":false})
    );
    // A waiting read ends at its deadline with the same cursor.
    let started = std::time::Instant::now();
    let empty = m.runtime.run_events(&m.owner.id, 0, 10, None, 300).unwrap();
    assert!(empty.entries.is_empty());
    assert_eq!(empty.cursor, 0);
    assert!(started.elapsed() >= std::time::Duration::from_millis(250));
    // A waiting read returns as soon as an event is recorded, long before its deadline.
    let run_id = std::thread::scope(|scope| {
        let waiter = scope.spawn(|| {
            let started = std::time::Instant::now();
            let page = m
                .runtime
                .run_events(&m.owner.id, 0, 10, None, 15_000)
                .unwrap();
            (page, started.elapsed())
        });
        std::thread::sleep(std::time::Duration::from_millis(200));
        let run_id = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a");
        let (page, waited) = waiter.join().unwrap();
        assert!(!page.entries.is_empty());
        assert_eq!(
            page.entries[0].record.run_id.as_deref(),
            Some(run_id.as_str())
        );
        assert!(waited < std::time::Duration::from_secs(10), "{waited:?}");
        run_id
    });
    // Cursor replay through the route, filtered to one run.
    m.runtime
        .run_renew(&m.owner.id, &run_id, Some(60), Some(&worker()), &mut tx)
        .unwrap();
    m.runtime
        .run_cancel(&m.owner.id, &run_id, Some(&worker()), &mut tx)
        .unwrap();
    let first = m
        .runtime
        .request(
            "GET",
            &format!("{events}?after=0&limit=1&run={run_id}"),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(first["entries"].as_array().unwrap().len(), 1);
    assert_eq!(first["entries"][0]["kind"], "state");
    assert_eq!(first["entries"][0]["state"], "dispatched");
    assert_eq!(first["entries"][0]["source"], "orchestrator");
    assert_eq!(first["more"], true);
    let rest = m
        .runtime
        .request(
            "GET",
            &format!("{events}?after={}&run={run_id}", first["cursor"]),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    let rest_kinds: Vec<_> = rest["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["kind"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(rest_kinds, ["lease", "state"]);
    assert_eq!(rest["entries"][1]["state"], "cancelled");
    // The status route answers from the record alone.
    let status = m
        .runtime
        .request(
            "GET",
            &format!(
                "/checkouts/{}/orchestration/runs/{run_id}/status",
                m.owner.id
            ),
            &Value::Null,
            &mut tx,
        )
        .unwrap();
    assert_eq!(status["state"], "cancelled");
    assert_eq!(status["reachability"], "unknown");
    assert_eq!(status["stale"], true);
    for bad in ["?after=x", "?run=nope", "?wait_ms=-1"] {
        assert!(
            m.runtime
                .request("GET", &format!("{events}{bad}"), &Value::Null, &mut tx)
                .is_err(),
            "{bad}"
        );
    }
    assert!(is_path(&events));
    assert!(is_machine_path(&events));
    // A host without a change stream says so; the hosted route is not machine-fenced.
    let hosted = format!("/checkouts/{}/target-events", m.target.id);
    assert!(is_path(&hosted));
    assert!(!is_machine_path(&hosted));
    assert_eq!(
        op(m.runtime.request("GET", &hosted, &Value::Null, &mut tx)),
        TargetOpError::PeerUnsupported
    );
    let _ = human();
}

#[test]
fn hs2_7qhwhj_a_remote_target_is_followed_through_the_reviewed_transport() {
    let feed = Arc::new(Feed::default());
    let peer = Arc::new(RemotePeer {
        hosted: machine_with(99, false, unsupported, Some(feed.clone())),
        calls: Mutex::new(Vec::new()),
        status_override: Mutex::new(None),
    });
    let root = tempfile::tempdir().unwrap();
    let home = root.path().join("home");
    std::fs::create_dir(&home).unwrap();
    let project = root.path().join("owner");
    std::fs::create_dir(&project).unwrap();
    let store_path = root.path().join("owner-store");
    let owner_store = hotsheet_ticketing::FsStore::init_git(
        &store_path,
        &hotsheet_ticketing::StoreMetadata::new("OWN"),
    )
    .unwrap();
    let checkouts = CheckoutRegistry::new(home.join("checkouts.json"));
    let owner = checkouts
        .register(&project, None, None, vec![store_path])
        .unwrap();
    let secrets = Arc::new(Memory::default());
    let runtime = OrchestrationRuntime::new(
        home,
        checkouts,
        ProviderConfigRegistry::new(root.path().join("providers.json")),
        secrets.clone(),
        peer.clone(),
        id(10),
    )
    .unwrap();
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    let target = TargetRef {
        owner_checkout_id: owner.id.clone(),
        target_id: id(1),
    };
    let manifest = runtime
        .replace(
            &owner.id,
            ReplaceTargets {
                expected_revision: None,
                targets: vec![TargetDeclaration {
                    target_id: id(1),
                    state: DeclarationState::Active,
                    label: "Remote".into(),
                    expected_repository_identity: None,
                    site_of: None,
                }],
            },
            &mut tx,
        )
        .unwrap();
    let registered = runtime
        .remotes
        .register(
            RegisterRemote {
                id: id(20),
                label: "Synthetic peer".into(),
                origin: "http://127.0.0.1:8765".into(),
                expected_server_id: id(99),
                credentials: credentials(),
            },
            secrets.as_ref(),
            peer.as_ref(),
            100,
        )
        .unwrap();
    let projection = peer
        .hosted
        .runtime
        .checkout_projection(&peer.hosted.target.id)
        .unwrap();
    runtime
        .bind(
            BindTarget {
                target: target.clone(),
                expected_manifest_revision: manifest.revision.unwrap(),
                expected_binding_revision: 0,
                previous_destination: None,
                destination: TargetDestination::Remote {
                    registration_id: id(20),
                    identity: registered.remote.identity,
                    checkout_id: projection.checkout_id,
                    checkout_authority: projection.checkout_authority,
                },
            },
            &mut tx,
        )
        .unwrap();
    allow_on(
        &runtime,
        &target.owner_checkout_id,
        &target.target_id,
        &[TargetAction::WorkStart],
        &[],
        &mut tx,
    );
    let orchestration = ops::create(
        &owner_store,
        ulid::Ulid::new(),
        "OWN",
        NOW.into(),
        NewTicket {
            title: "Coordinate remotely".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let ticket = target_ticket(&peer.hosted, "Remote work");
    let connection = peer.hosted.target_connection();
    let run_id = runtime
        .run_dispatch(
            &owner.id,
            serde_json::from_value(json!({
                "ticket": {"native_id": orchestration.slug},
                "target_id": id(1), "connection_id": connection,
                "native_id": ticket.id.to_string(), "operation_id": "op-1",
                "worker_id": "remote-worker",
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .run
        .run_id;
    let mut state = WatchState::default();
    runtime.watch_step(&owner.id, &id(1), &mut state, 8_000, &mut tx);
    // The peer's ticket is claimed by someone else there; the feed says it changed.
    ops::release(
        &peer.hosted.target_store,
        &ticket.id,
        NOW.into(),
        "remote-worker",
        false,
    )
    .unwrap();
    ops::claim_with_eta(
        &peer.hosted.target_store,
        &ticket.id,
        &NOW.into(),
        LEASE.into(),
        "peer-human",
        None,
        None,
    )
    .unwrap();
    feed.push(&connection, &ticket);
    let step = runtime.watch_step(&owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(step.synced, std::slice::from_ref(&run_id));
    let status = runtime.run_status(&owner.id, &run_id).unwrap();
    assert_eq!(status.run.state, RunState::Intervened);
    assert_eq!(
        status.run.observed.as_ref().unwrap().claimed_by.as_deref(),
        Some("peer-human")
    );
    // The watch reached the peer's hosted feed with its cursor, through the transport.
    let paths: Vec<_> = peer
        .calls
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .iter()
        .filter(|call| call.1.contains("/target-events"))
        .map(|call| call.1.clone())
        .collect();
    assert_eq!(
        paths,
        [
            format!("/checkouts/{}/target-events", peer.hosted.target.id),
            format!(
                "/checkouts/{}/target-events?since=0&wait_ms=8000",
                peer.hosted.target.id
            ),
        ]
    );
    // The peer goes away: the run is stale with its last confirmed state intact.
    *peer
        .status_override
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(503);
    let down = runtime.watch_step(&owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(down.failed);
    let stale = runtime.run_status(&owner.id, &run_id).unwrap();
    assert_eq!(stale.reachability, "unreachable");
    assert_eq!(
        stale.run.observed.as_ref().unwrap().claimed_by.as_deref(),
        Some("peer-human")
    );
    assert_eq!(state.cursor(), Some(1));
}

#[test]
fn hs2_wbredm_the_follower_reconciles_every_run_on_connect_and_on_reconnect() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let first = target_ticket(&m, "First");
    let second = target_ticket(&m, "Second");
    let run_one = dispatch(&m, &orchestration, &first, "op-1", "worker-a");
    let run_two = dispatch(&m, &orchestration, &second, "op-22", "worker-b");
    let outcome = |run_id: &str| {
        m.runtime
            .run(&m.owner.id, run_id)
            .unwrap()
            .reconciliation
            .map(|found| found.disposition.code())
    };
    assert_eq!(outcome(&run_one), None);
    // A restarted host starts a follower, and its first step reconciles every run.
    let mut state = WatchState::default();
    let connected = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(connected.synced, [run_one.clone(), run_two.clone()]);
    assert_eq!(outcome(&run_one), Some("continue"));
    assert_eq!(outcome(&run_two), Some("continue"));
    // A quiet step reconciles nothing and reports nothing.
    let before = kinds(&m, &run_two).len();
    let quiet = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(quiet.synced.is_empty() && !quiet.failed);
    assert_eq!(kinds(&m, &run_two).len(), before);
    // The change stream drops, and while it is down a claim is released without the
    // feed ever naming that ticket.
    feed.failing.store(true, Ordering::SeqCst);
    let down = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(down.failed);
    ops::release(&m.target_store, &second.id, NOW.into(), "worker-b", false).unwrap();
    feed.failing.store(false, Ordering::SeqCst);
    // Reconnecting reconciles every run, not only the tickets the feed replays.
    let back = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(!back.failed);
    assert_eq!(back.synced, [run_one.clone(), run_two.clone()]);
    let taken = m.runtime.run(&m.owner.id, &run_two).unwrap();
    assert_eq!(taken.state, RunState::Intervened);
    assert_eq!(outcome(&run_two), Some("needs_attention"));
    assert_eq!(outcome(&run_one), Some("continue"));
    // Reconnecting is one reconciliation: the following quiet step does nothing.
    let before = kinds(&m, &run_two).len();
    let quiet = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(quiet.synced.is_empty());
    assert_eq!(kinds(&m, &run_two).len(), before);
    assert_eq!(
        kinds(&m, &run_two)
            .into_iter()
            .filter(|event| event.0 == RunEventKind::Reconciled)
            .map(|event| event.3.unwrap())
            .collect::<Vec<_>>(),
        ["continue", "needs_attention"]
    );
}

/// HS2-257Q2Z: record a running session of the owner on target 1, hosted as
/// `hosted_id`, started by `worker-1`, optionally for a run.
fn hosted_session(
    m: &Machine,
    hosted_id: &str,
    run_id: Option<String>,
    tx: &mut Transaction<'_>,
) -> String {
    use hotsheet_ticketing::orchestration::{
        HostedRef, NewSession, SessionRegistry, SessionRole, SessionState, SessionUpdate,
    };
    let registry = SessionRegistry::new(&m.home);
    let (session, _) = registry
        .begin(
            &m.owner.id,
            NewSession {
                operation_id: format!("op-{}", &hosted_id[20..]),
                target_id: id(1),
                role: SessionRole::Worker,
                tool: "claude".into(),
                model: None,
                effort: None,
                account: None,
                run_id,
                replaces: None,
            },
            Some("worker-1".into()),
            tx,
        )
        .unwrap();
    registry
        .update(
            &m.owner.id,
            &session.session_id,
            SessionUpdate {
                to: Some(SessionState::Running),
                hosted: Some(HostedRef {
                    server_id: id(900),
                    session_id: hosted_id.into(),
                    worker_id: "worker-hosted".into(),
                }),
                ..SessionUpdate::default()
            },
            tx,
        )
        .unwrap();
    session.session_id
}

#[test]
fn hs2_257q2z_a_target_with_only_a_session_is_followed_and_its_changes_wake_the_scheduler() {
    use hotsheet_ticketing::orchestration::{SessionRegistry, SessionState, SessionUpdate};
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let mut state = WatchState::default();
    // Neither a run nor a session: nothing is asked.
    assert_eq!(
        m.runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx),
        WatchStep::default()
    );
    assert!(feed.calls().is_empty());
    let session = hosted_session(&m, &id(901), None, &mut tx);
    // A session alone is followed. The handshake cannot tell what happened before,
    // so the scheduler is woken once.
    let first = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(
        first.active && !first.failed && first.woke_scheduler,
        "{first:?}"
    );
    assert!(first.synced.is_empty());
    assert_eq!(feed.calls(), [(None, 0)]);
    // Quiet: nothing to wake for.
    let quiet = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(quiet.active && !quiet.woke_scheduler);
    // Another orchestrator's (or a person's) session on the host is not news here.
    feed.session(&id(902));
    let other = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(!other.woke_scheduler);
    // The owner's session finished a turn on its host: wake. Repeated and replayed
    // changes wake each time they arrive and never sync anything.
    for _ in 0..2 {
        feed.session(&id(901));
        feed.session(&id(901));
        let turn = m
            .runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
        assert!(turn.woke_scheduler && turn.synced.is_empty());
    }
    // A ticket changed (a claim taken or let go) while the session works no run.
    let ticket = target_ticket(&m, "Claimed by the session");
    feed.push(&m.target_connection(), &ticket);
    let claimed = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(claimed.woke_scheduler);
    // The session ends: the follower goes idle and stops asking.
    SessionRegistry::new(&m.home)
        .update(
            &m.owner.id,
            &session,
            SessionUpdate {
                to: Some(SessionState::Lost),
                ..SessionUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    let calls = feed.calls().len();
    assert_eq!(
        m.runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx),
        WatchStep::default()
    );
    assert_eq!(feed.calls().len(), calls);
    // Empty, then refilled: a new session is followed from a new handshake.
    hosted_session(&m, &id(903), None, &mut tx);
    let refilled = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(refilled.active && refilled.woke_scheduler);
    assert_eq!(feed.calls().last(), Some(&(None, 0)));
}

#[test]
fn hs2_257q2z_a_session_working_an_unfinished_run_learns_ticket_changes_from_the_run() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Worked by the session");
    let run_id = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a");
    hosted_session(&m, &id(901), Some(run_id.clone()), &mut tx);
    let mut state = WatchState::default();
    m.runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    // The run's ticket changed: the run is synced (its event wakes the loop), and the
    // follower does not wake it a second time.
    feed.push(&m.target_connection(), &ticket);
    let synced = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(synced.synced, std::slice::from_ref(&run_id));
    assert!(!synced.woke_scheduler);
    // HS2-XHQ1EH: reading sessions is consented to, so a run's sync also reads the
    // run's sessions from their host. No host here knows a scripted session, so it
    // reads as lost; each later step follows a fresh one.
    hosted_session(&m, &id(902), Some(run_id.clone()), &mut tx);
    // Its turn end has no run event, so it still wakes the loop.
    feed.session(&id(902));
    let turn = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(turn.woke_scheduler && turn.synced.is_empty(), "{turn:?}");
    // The run finishes: the session works no unfinished run, so a ticket change wakes.
    complete(&m, &ticket);
    feed.push(&m.target_connection(), &ticket);
    let finished = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(finished.synced, std::slice::from_ref(&run_id));
    assert_eq!(
        m.runtime.run(&m.owner.id, &run_id).unwrap().state,
        RunState::Completed
    );
    hosted_session(&m, &id(903), None, &mut tx);
    let other = target_ticket(&m, "Next");
    feed.push(&m.target_connection(), &other);
    let free = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(free.active && free.woke_scheduler && free.synced.is_empty());
}

#[test]
fn hs2_257q2z_a_page_without_sessions_reads_as_before() {
    let page: TargetEventPage =
        serde_json::from_value(json!({"cursor":3,"events":[],"overflow":false})).unwrap();
    assert!(page.sessions.is_empty());
    assert_eq!(
        serde_json::to_value(&page).unwrap(),
        json!({"cursor":3,"events":[],"overflow":false})
    );
    let with: TargetEventPage = serde_json::from_value(
        json!({"cursor":4,"events":[],"sessions":[{"cursor":4,"session_id":"t"}],"overflow":false}),
    )
    .unwrap();
    assert_eq!(with.sessions[0].session_id, "t");
}

/// HS2-TMA4YA: a prerequisite ticket no run stands on is followed on the target's
/// change feed, and its completion publishes the waiting step's new level.
#[test]
fn hs2_tma4ya_a_planned_ticket_without_a_run_is_followed_and_publishes_its_level() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::TicketsRead], &mut tx);
    let mut state = WatchState::default();
    let orchestration = m.orchestration_ticket("Coordinate");
    let before = target_ticket(&m, "Must finish first");
    let after = target_ticket(&m, "Then this");
    let unrelated = target_ticket(&m, "Not planned");
    // Nothing planned and no run: idle, nothing asked.
    assert_eq!(
        m.runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx),
        WatchStep::default()
    );
    assert!(feed.calls().is_empty());
    let step_id = m
        .runtime
        .readiness_define(
            &m.owner.id,
            serde_json::from_value(json!({
                "ticket": {"connection_id": m.owner_connection(),
                    "native_id": orchestration.id.to_string()},
                "target_id": id(1), "connection_id": m.target_connection(),
                "native_id": after.slug,
                "prerequisites": [{"kind": "ticket", "target_id": id(1),
                    "connection_id": m.target_connection(), "native_id": before.slug,
                    "gates": "none"}],
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step
        .step_id;
    assert_eq!(m.runtime.planned_targets(&m.owner.id), [id(1)]);
    let levels = |m: &Machine| -> Vec<String> {
        m.runtime
            .run_events(&m.owner.id, 0, 500, Some(&step_id), 0)
            .unwrap()
            .entries
            .into_iter()
            .filter_map(|entry| entry.record.reason)
            .collect()
    };
    assert_eq!(levels(&m).last().map(String::as_str), Some("blocked"));
    // First contact: a handshake, and both planned tickets are read once.
    let first = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(first.active && !first.failed);
    assert_eq!(first.refreshed, 2);
    assert!(first.synced.is_empty());
    // Quiet, or a change to a ticket the plan does not name: nothing is read.
    let quiet = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(quiet.refreshed, 0);
    feed.push(&m.target_connection(), &unrelated);
    assert_eq!(
        m.runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx)
            .refreshed,
        0
    );
    // The prerequisite completes on the target with no run on it: the change is read
    // and the step's new level is published, with nobody asking for a refresh.
    complete(&m, &before);
    feed.push(&m.target_connection(), &before);
    let changed = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(changed.refreshed, 1);
    assert_eq!(levels(&m).last().map(String::as_str), Some("confirmed"));
    // The same change again publishes nothing new.
    let count = levels(&m).len();
    feed.push(&m.target_connection(), &before);
    m.runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(levels(&m).len(), count);
    // An overflow means the follower cannot tell what it missed: every planned
    // ticket is read again.
    feed.overflow.store(true, Ordering::SeqCst);
    assert_eq!(
        m.runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx)
            .refreshed,
        2
    );
    feed.overflow.store(false, Ordering::SeqCst);
    // Once the step is removed nothing is followed and nothing is asked.
    m.runtime
        .readiness_remove(
            &m.owner.id,
            &PlanStepRef {
                step_id: step_id.clone(),
            },
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert!(m.runtime.planned_targets(&m.owner.id).is_empty());
    let asked = feed.calls().len();
    assert!(
        !m.runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx)
            .active
    );
    assert_eq!(feed.calls().len(), asked);
}

/// HS2-TMA4YA: the follower's read is opportunistic. A planned ticket it cannot read
/// (here the target is unbound) does not make the step stale; an explicit refresh would.
#[test]
fn hs2_tma4ya_a_follower_read_that_fails_changes_no_level() {
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(&m, 1, &[TargetAction::TicketsRead], &mut tx);
    let orchestration = m.orchestration_ticket("Coordinate");
    let before = target_ticket(&m, "First");
    let after = target_ticket(&m, "Then");
    let step_id = m
        .runtime
        .readiness_define(
            &m.owner.id,
            serde_json::from_value(json!({
                "ticket": {"connection_id": m.owner_connection(),
                    "native_id": orchestration.id.to_string()},
                "target_id": id(1), "connection_id": m.target_connection(),
                "native_id": after.slug,
                "prerequisites": [{"kind": "ticket", "target_id": id(1),
                    "connection_id": m.target_connection(), "native_id": before.slug,
                    "gates": "none"}],
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step
        .step_id;
    let mut state = WatchState::default();
    m.runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    let published = |m: &Machine| {
        m.runtime
            .run_events(&m.owner.id, 0, 500, Some(&step_id), 0)
            .unwrap()
            .entries
            .len()
    };
    let count = published(&m);
    // The target becomes unreadable for the gate; a change arrives anyway.
    super::tickets::unbind(&m, 1, &mut tx);
    feed.push(&m.target_connection(), &before);
    let step = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert_eq!(step.refreshed, 1);
    assert_eq!(
        published(&m),
        count,
        "no stale level from a failed follower read"
    );
}

/// HS2-RQDH0Q: once a run's site handoff is no longer asked about on a timer, a change
/// to its ticket (a note, a claim) wakes the scheduler, whose relay asks the home
/// again, although the run's sync has nothing new to report. Without that outcome the
/// same change wakes nothing beyond the run's own event.
#[test]
fn hs2_rqdh0q_a_ticket_change_wakes_a_run_whose_site_handoff_waits_on_a_person() {
    use hotsheet_ticketing::orchestration::{RunRegistry, RunUpdate};
    let feed = Arc::new(Feed::default());
    let m = machine_with(10, false, unsupported, Some(feed.clone()));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &Clock,
        rng: &mut rng,
    };
    bind_and_allow(
        &m,
        1,
        &[TargetAction::TicketsRead, TargetAction::WorkStart],
        &mut tx,
    );
    let orchestration = m.orchestration_ticket("Coordinate");
    let ticket = target_ticket(&m, "Worked on a site");
    let run_id = dispatch(&m, &orchestration, &ticket, "op-1", "worker-a");
    hosted_session(&m, &id(901), Some(run_id.clone()), &mut tx);
    let mut state = WatchState::default();
    m.runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    feed.push(&m.target_connection(), &ticket);
    let plain = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(!plain.woke_scheduler);
    // The timed asks gave up: the run's outcome names the home's waiting code.
    RunRegistry::new(&m.home)
        .update(
            &m.owner.id,
            &run_id,
            RunUpdate {
                reconciliation: Some(crate::orchestration::assignment::site_handoff_attention(
                    1_000,
                    "handoff_note_missing",
                )),
                ..RunUpdate::default()
            },
            &mut tx,
        )
        .unwrap();
    // Repeated changes wake each time; a change to another ticket does not. No host
    // here knows a scripted session, so the run's sync above read it as lost; the
    // site's worker session is followed afresh (as above).
    hosted_session(&m, &id(910), Some(run_id.clone()), &mut tx);
    for _ in 0..2 {
        feed.push(&m.target_connection(), &ticket);
        let woken = m
            .runtime
            .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
        assert!(woken.woke_scheduler, "{woken:?}");
    }
    let other = target_ticket(&m, "Unrelated");
    feed.push(&m.target_connection(), &other);
    let unrelated = m
        .runtime
        .watch_step(&m.owner.id, &id(1), &mut state, 8_000, &mut tx);
    assert!(!unrelated.woke_scheduler, "{unrelated:?}");
}
