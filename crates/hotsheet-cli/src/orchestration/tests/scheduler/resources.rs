//! HS2-2FG91X: scheduling around claimable resources through the shared runtime.
//! Real git ticket stores and the real local claim backend, a scripted claim source
//! for what the local backend cannot do (an unreachable backend, a chosen expiry),
//! and the recording session executor: placement decisions are asserted, no session
//! is started. Synthetic tickets, resources and identities only.
use std::collections::BTreeMap;
use std::sync::Condvar;

use hotsheet_resource_claims::{ClaimService, Grant, Namespace, ResourceRef};
use hotsheet_ticketing::orchestration::{
    HeldClaim, OfflinePolicy, ResourceNeed, ResourceState, TargetResources, UnreadableReason,
};

use super::super::tickets::RemotePeer;
use super::*;

const VM: &str = "vm/macos-vm";

fn need(text: &str) -> ResourceNeed {
    ResourceNeed::parse(text).unwrap()
}
/// What a session does for itself: claim the resource on its machine's backend.
fn acquire(home: &Path, name: &str) -> Grant {
    ClaimService::new(home)
        .acquire(
            &ResourceRef::parse(name).unwrap(),
            Duration::from_secs(30 * 60),
            None,
            &BTreeMap::new(),
        )
        .unwrap()
}
fn release(home: &Path, grant: &Grant) {
    ClaimService::new(home)
        .release(&grant.renewal_token, None)
        .unwrap();
}
fn live_claims(home: &Path, namespace: &str) -> usize {
    ClaimService::new(home)
        .list(&Namespace::parse(namespace).unwrap())
        .unwrap()
        .len()
}

/// A claim source the test scripts: what is held, whether the backend answers at
/// all, and when a waiter is let go.
#[derive(Default)]
struct Scripted {
    held: Mutex<Vec<HeldClaim>>,
    unreachable: Mutex<Option<OfflinePolicy>>,
    asked: AtomicUsize,
    waiting: AtomicUsize,
    freed: Mutex<bool>,
    signal: Condvar,
}
impl Scripted {
    fn hold(&self, name: &str, expires_at_ms: u64) {
        let ResourceNeed {
            namespace,
            resource,
        } = need(name);
        *self.held.lock().unwrap() = vec![HeldClaim {
            namespace,
            resource,
            expires_at_ms,
        }];
    }
    /// The holder releases: nothing is held and every waiter wakes.
    fn release(&self) {
        self.held.lock().unwrap().clear();
        *self.freed.lock().unwrap() = true;
        self.signal.notify_all();
    }
}
impl ClaimSource for Scripted {
    fn held(&self, namespace: &str) -> Result<Vec<HeldClaim>, ClaimsUnreachable> {
        self.asked.fetch_add(1, Ordering::SeqCst);
        if let Some(offline) = *self.unreachable.lock().unwrap() {
            return Err(ClaimsUnreachable { offline });
        }
        Ok(self
            .held
            .lock()
            .unwrap()
            .iter()
            .filter(|claim| claim.namespace == namespace)
            .cloned()
            .collect())
    }
    fn wait_free(&self, _: &str, _: &str, _: Duration) -> bool {
        self.waiting.fetch_add(1, Ordering::SeqCst);
        // Only the test ends this wait. Real time passing must never wake a loop
        // that runs on the test's own clock; the waiting thread is detached, so a
        // test that ends without releasing leaves nothing to join.
        let mut freed = self
            .signal
            .wait_while(self.freed.lock().unwrap(), |freed| !*freed)
            .unwrap();
        std::mem::take(&mut *freed)
    }
}

/// The fixed clock of these tests, moved by hand.
struct At(u64);
impl hotsheet_ticketing::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0
    }
}

impl Setup {
    /// Confirmed work on a new target ticket whose session must hold `resources`.
    fn ready_needing(
        &self,
        title: &str,
        resources: &[&str],
        tx: &mut Transaction<'_>,
    ) -> (Ticket, String) {
        let (ticket, plain) = self.ready(title, tx);
        if resources.is_empty() {
            return (ticket, plain);
        }
        let step = self
            .m
            .runtime
            .readiness_define(
                &self.owner(),
                serde_json::from_value(json!({
                    "ticket": {"connection_id": self.m.owner_connection(),
                        "native_id": self.orchestration.id.to_string()},
                    "target_id": id(1), "connection_id": self.m.target_connection(),
                    "native_id": ticket.slug,
                    "resources": resources.iter().map(|name| need(name)).collect::<Vec<_>>(),
                }))
                .unwrap(),
                Some(&worker()),
                tx,
            )
            .unwrap()
            .readiness
            .step;
        assert_eq!(step.step_id, plain);
        (ticket, plain)
    }
    /// The target's dispatcher assigned the ticket to the session started for it:
    /// a run exists, the step is in progress, and the session is busy on that run.
    fn working(&self, ticket: &Ticket, session: usize, tx: &mut Transaction<'_>) {
        let run = self
            .m
            .runtime
            .run_dispatch(
                &self.owner(),
                serde_json::from_value(json!({
                    "ticket": {"connection_id": self.m.owner_connection(),
                        "native_id": self.orchestration.id.to_string()},
                    "target_id": id(1), "connection_id": self.m.target_connection(),
                    "native_id": ticket.slug,
                    "operation_id": format!("op-{}", ticket.slug),
                    "worker_id": format!("worker-{session}"),
                }))
                .unwrap(),
                Some(&worker()),
                tx,
            )
            .unwrap()
            .run;
        self.sessions.sessions.lock().unwrap()[session].run_id = Some(run.run_id);
    }
    fn starts(&self) -> Vec<PlacementStart> {
        self.sessions
            .calls()
            .into_iter()
            .filter_map(|call| match call {
                SessionCall::Start(request) => Some(request),
                SessionCall::WindDown(_) => None,
            })
            .collect()
    }
    fn explain(&self, step: &str, tx: &mut Transaction<'_>) -> Value {
        serde_json::to_value(
            self.m
                .runtime
                .scheduler_explain(
                    &self.owner(),
                    &ExplainRequest {
                        step_id: Some(step.into()),
                        ..ExplainRequest::default()
                    },
                    Some(&worker()),
                    tx,
                )
                .unwrap(),
        )
        .unwrap()
    }
    fn scripted(&mut self) -> Arc<Scripted> {
        let source = Arc::new(Scripted::default());
        self.m.runtime = self.m.runtime.clone().with_claim_source(source.clone());
        source
    }
}

/// `(resource, state, taken by, waiting)` of each contention entry.
fn contention(view: &ScheduleView) -> Vec<(String, &'static str, Option<String>, Vec<String>)> {
    view.schedule
        .contention
        .iter()
        .map(|entry| {
            (
                format!("{}/{}", entry.namespace, entry.resource),
                entry.state.code(),
                entry.taken_by.clone(),
                entry.waiting.clone(),
            )
        })
        .collect()
}

#[test]
fn hs2_2fg91x_two_tickets_need_one_resource_one_starts_the_other_after_release() {
    tx!(tx);
    let s = setup(true, &mut tx);
    set_policy(&s.m, policy(true, Vec::new()), &mut tx);
    let (first_ticket, first) = s.ready_needing("First", &[VM], &mut tx);
    let (_, second) = s.ready_needing("Second", &[VM], &mut tx);

    // Both are ready and the target has room for two sessions, but the resource has
    // one holder: one session starts and is told what its work must hold.
    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(unplaced(&tick.view), [("resource_held", Some(VM))]);
    assert_eq!(tick.view.schedule.unplaced[0].step_id, second);
    assert_eq!(tick.view.schedule.unplaced[0].retry_at_ms, None);
    let starts = s.starts();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].steps, std::slice::from_ref(&first));
    assert_eq!(starts[0].resources, [need(VM)]);
    assert_eq!(
        contention(&tick.view),
        [(
            VM.to_owned(),
            "taken",
            Some(first.clone()),
            vec![second.clone()]
        )]
    );
    assert_eq!(
        tick.view.schedule.contention[0].server_id,
        Some(id(10)),
        "the machine that hosts the target"
    );
    // The scheduler decided; it claimed nothing.
    assert_eq!(live_claims(&s.m.home, "vm"), 0);

    // The session works its ticket and claims the resource itself.
    s.working(&first_ticket, 0, &mut tx);
    let grant = acquire(&s.m.home, VM);
    let expires = grant.claim.expires_at_ms;
    let waiting = s.tick(&mut tx);
    assert!(waiting.outcomes.is_empty());
    assert_eq!(unplaced(&waiting.view), [("resource_held", Some(VM))]);
    assert_eq!(waiting.view.schedule.retry_at_ms, Some(expires));
    assert_eq!(waiting.view.inputs.held_resources, 1);
    assert_eq!(
        contention(&waiting.view),
        [(VM.to_owned(), "held", None, vec![second.clone()])]
    );
    assert_eq!(
        waiting.view.schedule.contention[0].expires_at_ms,
        Some(expires)
    );
    // Why the second is not running, and when to expect a change.
    let why = s.explain(&second, &mut tx);
    assert_eq!(
        (&why["standing"], &why["reason"], &why["detail"]),
        (&json!("not_placed"), &json!("resource_held"), &json!(VM))
    );
    assert_eq!(why["retry_at_ms"], json!(expires));
    assert_eq!(
        why["resources"],
        json!([{"namespace": "vm", "resource": "macos-vm", "state": "held",
            "server_id": id(10), "expires_at_ms": expires, "declared_by": "step"}])
    );
    assert_eq!(s.explain(&first, &mut tx)["standing"], "running");
    // The deferral is in the decision log with the resource, and nothing else of it.
    let logged = s.log();
    let last = logged.last().unwrap();
    assert_eq!(
        (
            last.kind,
            last.reason.as_deref(),
            last.detail.as_deref(),
            last.retry_at_ms
        ),
        (
            DecisionKind::Unplaced,
            Some("resource_held"),
            Some(VM),
            Some(expires)
        )
    );

    // The same pass again starts nothing more, however often it runs.
    for _ in 0..2 {
        assert!(s.tick(&mut tx).outcomes.is_empty());
    }
    // The session renews with its own token: the time to look again moves.
    let renewed = ClaimService::new(&s.m.home)
        .renew(&grant.renewal_token, Duration::from_secs(60 * 60), None)
        .unwrap();
    assert!(renewed.expires_at_ms > expires);
    assert_eq!(
        s.tick(&mut tx).view.schedule.retry_at_ms,
        Some(renewed.expires_at_ms)
    );

    // Released: the deferred work starts in the next pass.
    release(&s.m.home, &grant);
    let after = s.tick(&mut tx);
    assert_eq!(outcomes(&after), [("started", None)]);
    assert!(after.view.schedule.contention.is_empty());
    let starts = s.starts();
    assert_eq!(starts.len(), 2);
    assert_eq!(starts[1].steps, std::slice::from_ref(&second));
    assert_eq!(starts[1].resources, [need(VM)]);
    assert_eq!(live_claims(&s.m.home, "vm"), 0);
    assert_eq!(s.explain(&second, &mut tx)["resources"][0]["state"], "free");
}

#[test]
fn hs2_2fg91x_a_release_wakes_the_follower_through_the_claim_service_wait() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let owner = s.owner();
    set_policy(&s.m, policy(true, Vec::new()), &mut tx);
    // Someone else holds the resource on the real local backend.
    let grant = acquire(&s.m.home, VM);
    let (_, waits) = s.ready_needing("Waits", &[VM], &mut tx);
    let mut state = SchedulerFollowState::default();
    let step = |state: &mut SchedulerFollowState, wait_ms: u64, tx: &mut Transaction<'_>| {
        s.m.runtime
            .scheduler_follow_step(&owner, state, wait_ms, tx)
    };
    let watched = || s.m.runtime.claim_watches();

    assert!(watched().is_empty());
    let started = step(&mut state, 0, &mut tx);
    assert_eq!(started.trigger, Some("start"));
    assert_eq!(started.retry_at_ms, Some(grant.claim.expires_at_ms));
    assert!(s.starts().is_empty());
    assert_eq!(watched(), [("vm".to_owned(), "macos-vm".to_owned())]);

    // While it is held nothing reads, plans or asks: there is no interval.
    let asked = s.probe.asked();
    let logged = s.log().len();
    for _ in 0..3 {
        assert_eq!(step(&mut state, 0, &mut tx).trigger, None);
    }
    assert_eq!((s.probe.asked(), s.log().len()), (asked, logged));
    // Another pass that defers the same work waits on the resource once.
    OrchestrationRuntime::scheduler_wake_at(&s.m.home);
    assert_eq!(step(&mut state, 0, &mut tx).trigger, Some("signal"));
    assert_eq!(watched().len(), 1);
    assert!(s.starts().is_empty());

    // The holder releases. The claim service's own signal wakes the follower, which
    // was told nothing else.
    release(&s.m.home, &grant);
    let began = std::time::Instant::now();
    let mut woken = None;
    while woken.is_none() && began.elapsed() < Duration::from_secs(20) {
        woken = step(&mut state, 5_000, &mut tx).trigger;
    }
    assert_eq!(woken, Some("signal"));
    let starts = s.starts();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].steps, [waits]);
    assert!(watched().is_empty());
    // A one-shot plan or tick never waits on anything.
    let held_again = acquire(&s.m.home, VM);
    s.ready_needing("Later", &[VM], &mut tx);
    s.tick(&mut tx);
    assert!(watched().is_empty());
    release(&s.m.home, &held_again);
}

#[test]
fn hs2_2fg91x_an_expiry_is_a_named_retry_time_and_a_renewal_moves_it() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    let owner = s.owner();
    let source = s.scripted();
    set_policy(&s.m, policy(true, Vec::new()), &mut tx);
    source.hold(VM, 5_000);
    let (_, waits) = s.ready_needing("Waits", &[VM], &mut tx);
    let mut state = SchedulerFollowState::default();
    let mut rng = Rng(900);
    let mut at = |now_ms: u64, state: &mut SchedulerFollowState, wait_ms: u64| {
        let clock = At(now_ms);
        s.m.runtime.scheduler_follow_step(
            &owner,
            state,
            wait_ms,
            &mut Transaction {
                clock: &clock,
                rng: &mut rng,
            },
        )
    };

    let first = at(200, &mut state, 0);
    assert_eq!(
        (first.trigger, first.retry_at_ms),
        (Some("start"), Some(5_000))
    );
    // Before the expiry nothing happens; at it the pass runs because of the time.
    assert_eq!(at(4_999, &mut state, 0).trigger, None);
    // The holder renewed meanwhile: still held, and the retry time is the new one.
    source.hold(VM, 9_000);
    let renewed = at(5_000, &mut state, 0);
    assert_eq!(
        (renewed.trigger, renewed.retry_at_ms),
        (Some("retry"), Some(9_000))
    );
    assert!(s.starts().is_empty());
    assert_eq!(at(8_999, &mut state, 0).trigger, None);
    // Nobody renews this time. The backend still lists the claim at that instant;
    // a claim is over at its expiry, so the work starts.
    let lapsed = at(9_000, &mut state, 0);
    assert_eq!((lapsed.trigger, lapsed.retry_at_ms), (Some("retry"), None));
    let starts = s.starts();
    assert_eq!(starts.len(), 1);
    assert_eq!(starts[0].steps, [waits]);
    // One waiter was armed for the resource across all those passes.
    assert_eq!(source.waiting.load(Ordering::SeqCst), 1);
    assert_eq!(s.m.runtime.claim_watches().len(), 1);

    // Let the waiter go. Its wake is a signal, and the pass it causes changes nothing.
    source.release();
    let began = std::time::Instant::now();
    let mut woken = None;
    while woken.is_none() && began.elapsed() < Duration::from_secs(20) {
        woken = at(9_001, &mut state, 5_000).trigger;
    }
    assert_eq!(woken, Some("signal"));
    assert!(s.m.runtime.claim_watches().is_empty());
    assert_eq!(s.starts().len(), 1);
}

#[test]
fn hs2_2fg91x_an_unreachable_backend_that_fails_closed_defers_the_work() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    let source = s.scripted();
    set_policy(&s.m, policy(true, Vec::new()), &mut tx);
    *source.unreachable.lock().unwrap() = Some(OfflinePolicy::FailClosed);
    let (_, needs) = s.ready_needing("Needs the VM", &[VM], &mut tx);
    let (_, plain) = s.ready("Needs nothing", &mut tx);

    // The machine says so itself: its claims are unknown and the backend fails closed.
    let snapshot =
        s.m.runtime
            .hosted_capacity(&s.m.target.id, &["vm".into()])
            .unwrap();
    assert_eq!(
        (snapshot.claims.clone(), snapshot.claims_offline),
        (None, Some(OfflinePolicy::FailClosed))
    );
    assert_eq!(
        serde_json::to_value(&snapshot).unwrap()["claims_offline"],
        "fail_closed"
    );

    let tick = s.tick(&mut tx);
    // Work that needs nothing is untouched; the other is not started on a guess.
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(s.starts()[0].steps, [plain]);
    assert!(s.starts()[0].resources.is_empty());
    assert_eq!(unplaced(&tick.view), [("resource_unknown", Some(VM))]);
    assert_eq!(
        contention(&tick.view),
        [(VM.to_owned(), "unreadable", None, vec![needs.clone()])]
    );
    assert_eq!(
        tick.view.schedule.contention[0].unreadable,
        Some(UnreadableReason::BackendUnreachable)
    );
    assert_eq!(
        (
            tick.view.inputs.held_resources,
            tick.view.inputs.unreadable_namespaces
        ),
        (0, 1)
    );

    // A start outranks a refusal on the same machine in one pass, so nothing is
    // backed off yet. The next pass starts nothing and finds the claims unreadable
    // again: from then the machine is left alone for a bounded, growing time
    // instead of being asked on every event.
    assert!(s.memory().iter().all(|entry| entry.refusals == 0));
    let again = s.tick(&mut tx);
    assert!(again.outcomes.is_empty());
    assert_eq!(unplaced(&again.view), [("resource_unknown", Some(VM))]);
    let memory = s.memory();
    assert_eq!(
        (memory[0].refusals, memory[0].refusal_reason.as_deref()),
        (1, Some("resource_unknown"))
    );
    let until = memory[0].refused_until_ms().unwrap();
    let (asked, read) = (source.asked.load(Ordering::SeqCst), s.probe.asked());
    let backing_off = s.tick(&mut tx);
    assert_eq!(
        unplaced(&backing_off.view),
        [("refusal_backoff", Some("resource_unknown"))]
    );
    assert_eq!(backing_off.view.schedule.retry_at_ms, Some(until));
    assert_eq!(
        (source.asked.load(Ordering::SeqCst), s.probe.asked()),
        (asked, read)
    );
    let why = s.explain(&needs, &mut tx);
    assert_eq!(why["resources"][0]["state"], "not_read");

    // The backend answers again and the backoff has passed: the work is placed.
    *source.unreachable.lock().unwrap() = None;
    for (actions, session_scope) in [
        (
            vec![TargetAction::TicketsRead, TargetAction::GatesObserve],
            None,
        ),
        (
            vec![TargetAction::SessionsManage],
            Some(SessionScope {
                tools: vec![scope("codex"), scope("claude")],
                max_concurrency: 4,
            }),
        ),
    ] {
        s.m.runtime
            .grant(
                &s.m.reference(1),
                CreateGrant {
                    grant_id: None,
                    actor_id: "worker-1".into(),
                    actions,
                    session_scope,
                    expires_at_ms: 10_000_000,
                },
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    }
    let clock = At(until);
    let mut rng = Rng(700);
    let mut later = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let recovered = s.tick(&mut later);
    assert!(
        recovered.view.schedule.unplaced.is_empty(),
        "{:?}",
        recovered.view.schedule.unplaced
    );
    assert!(recovered.view.schedule.contention.is_empty());
    assert!(
        recovered
            .view
            .schedule
            .placements
            .iter()
            .any(|placement| placement.steps == [needs.clone()])
    );
}

#[test]
fn hs2_2fg91x_an_unreachable_backend_that_allows_provisional_claims_starts_one() {
    tx!(tx);
    let mut s = setup(true, &mut tx);
    let source = s.scripted();
    set_policy(&s.m, policy(true, Vec::new()), &mut tx);
    *source.unreachable.lock().unwrap() = Some(OfflinePolicy::Provisional);
    let (first_ticket, first) = s.ready_needing("First", &[VM], &mut tx);
    let (_, second) = s.ready_needing("Second", &[VM], &mut tx);
    let snapshot =
        s.m.runtime
            .hosted_capacity(&s.m.target.id, &["vm".into()])
            .unwrap();
    assert_eq!(
        (snapshot.claims.clone(), snapshot.claims_offline),
        (None, Some(OfflinePolicy::Provisional))
    );

    // One piece of work starts, marked unverified; a second is never started on the
    // same unverified resource in the same pass.
    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    let starts = s.starts();
    assert_eq!((starts.len(), &starts[0].steps), (1, &vec![first.clone()]));
    assert_eq!(starts[0].resources, [need(VM)]);
    assert_eq!(unplaced(&tick.view), [("resource_held", Some(VM))]);
    assert_eq!(
        contention(&tick.view),
        [(
            VM.to_owned(),
            "unverified",
            Some(first.clone()),
            vec![second.clone()]
        )]
    );
    assert_eq!(tick.view.inputs.unreadable_namespaces, 1);
    // Nothing was refused, so nothing is backed off.
    assert!(s.memory().iter().all(|entry| entry.refusals == 0));
    let why = s.explain(&second, &mut tx);
    assert_eq!(
        (&why["reason"], &why["resources"][0]["state"]),
        (&json!("resource_held"), &json!("taken"))
    );
    assert_eq!(why["resources"][0]["taken_by"], json!(first));

    // The backend returns and lists the claim the first session took: the second
    // waits on a known expiry, and starts when it is released.
    s.working(&first_ticket, 0, &mut tx);
    *source.unreachable.lock().unwrap() = None;
    source.hold(VM, 7_000);
    let reconciled = s.tick(&mut tx);
    assert_eq!(unplaced(&reconciled.view), [("resource_held", Some(VM))]);
    assert_eq!(reconciled.view.schedule.retry_at_ms, Some(7_000));
    source.release();
    assert_eq!(outcomes(&s.tick(&mut tx)), [("started", None)]);
    assert_eq!(s.starts()[1].steps, [second]);
}

#[test]
fn hs2_2fg91x_the_local_backend_fails_closed_when_its_state_cannot_be_read() {
    tx!(tx);
    let s = setup(true, &mut tx);
    set_policy(&s.m, policy(false, Vec::new()), &mut tx);
    let grant = acquire(&s.m.home, VM);
    s.ready_needing("Needs the VM", &[VM], &mut tx);
    let state = s.m.home.join("resource-claims/namespaces/vm/state.json");
    let good = std::fs::read(&state).unwrap();
    std::fs::write(&state, b"{not json").unwrap();
    let plan =
        s.m.runtime
            .scheduler_plan(&s.owner(), Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(unplaced(&plan), [("resource_unknown", Some(VM))]);
    assert_eq!(plan.schedule.contention[0].state, ResourceState::Unreadable);
    // The damaged state was not overwritten by reading it, and a dry run backs
    // nothing off.
    assert_eq!(std::fs::read(&state).unwrap(), b"{not json");
    assert!(s.memory().is_empty());
    std::fs::write(&state, good).unwrap();
    let plan =
        s.m.runtime
            .scheduler_plan(&s.owner(), Some(&worker()), &mut tx)
            .unwrap();
    assert_eq!(unplaced(&plan), [("resource_held", Some(VM))]);
    release(&s.m.home, &grant);
}

#[test]
fn hs2_2fg91x_the_policy_requires_a_resource_of_every_piece_of_work_on_a_target() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let mut body = policy(true, Vec::new());
    body.placement.resources = vec![TargetResources {
        target_id: id(1),
        resources: vec![need(VM)],
    }];
    set_policy(&s.m, body.clone(), &mut tx);
    // One step names nothing, one names the same resource and another of its own.
    let (_, plain) = s.ready("Names nothing", &mut tx);
    let (_, both) = s.ready_needing("Names both", &[VM, "ports/8080"], &mut tx);

    let grant = acquire(&s.m.home, VM);
    let tick = s.tick(&mut tx);
    assert!(tick.outcomes.is_empty());
    assert_eq!(unplaced(&tick.view), [("resource_held", Some(VM)); 2]);
    let why = s.explain(&plain, &mut tx);
    assert_eq!(
        why["resources"],
        json!([{"namespace": "vm", "resource": "macos-vm", "state": "held",
            "server_id": id(10), "expires_at_ms": grant.claim.expires_at_ms,
            "declared_by": "target"}])
    );
    // Named by the step and required of the target: listed once, as the step's.
    let why = s.explain(&both, &mut tx);
    assert_eq!(
        why["resources"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| (
                entry["resource"].as_str().unwrap(),
                entry["state"].as_str().unwrap(),
                entry["declared_by"].as_str().unwrap()
            ))
            .collect::<Vec<_>>(),
        [("8080", "free", "step"), ("macos-vm", "held", "step")]
    );

    // Released: one of them starts with everything it must hold, and the other
    // waits for it.
    release(&s.m.home, &grant);
    let after = s.tick(&mut tx);
    assert_eq!(outcomes(&after), [("started", None)]);
    assert_eq!(unplaced(&after.view), [("resource_held", Some(VM))]);
    let started = &s.starts()[0];
    let expected: Vec<ResourceNeed> = if started.steps == [plain.clone()] {
        vec![need(VM)]
    } else {
        vec![need("ports/8080"), need(VM)]
    };
    assert_eq!(started.resources, expected);

    // The requirement is the policy's: removed there, nothing is required.
    body.placement.resources.clear();
    set_policy(&s.m, body, &mut tx);
    assert!(s.explain(&plain, &mut tx).get("resources").is_none());
}

#[test]
fn hs2_2fg91x_requirements_are_validated_where_they_are_declared() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let (ticket, step) = s.ready("Work", &mut tx);
    let owner = s.owner();
    let define = |resources: Value, tx: &mut Transaction<'_>| {
        serde_json::from_value::<DefinePlanStep>(json!({
            "ticket": {"connection_id": s.m.owner_connection(),
                "native_id": s.orchestration.id.to_string()},
            "target_id": id(1), "connection_id": s.m.target_connection(),
            "native_id": ticket.slug, "resources": resources,
        }))
        .map_err(|_| OrchestrationError::Invalid("body"))
        .and_then(|request| {
            s.m.runtime
                .readiness_define(&owner, request, Some(&worker()), tx)
        })
    };
    // Refused before any target is read: the audit journal does not grow.
    let audited = s.m.runtime.audit(&owner, 0, 500).unwrap().entries.len();
    for invalid in [
        json!([{"namespace": "VM", "resource": "macos-vm"}]),
        json!([{"namespace": "vm", "resource": "has space"}]),
        json!([{"namespace": "vm", "resource": "a"}, {"namespace": "vm", "resource": "a"}]),
        json!([{"namespace": "vm", "resource": "a"}, {"namespace": "vm", "resource": "b"},
            {"namespace": "vm", "resource": "c"}, {"namespace": "vm", "resource": "d"},
            {"namespace": "vm", "resource": "e"}]),
        json!([{"namespace": "vm", "resource": "a", "holder": "someone"}]),
        json!(["vm/a"]),
    ] {
        assert!(
            matches!(
                define(invalid.clone(), &mut tx),
                Err(OrchestrationError::Invalid(_))
            ),
            "{invalid}"
        );
    }
    assert_eq!(
        s.m.runtime.audit(&owner, 0, 500).unwrap().entries.len(),
        audited
    );
    // Valid: stored sorted, shown by the plan and the ready list, audited once.
    let defined = s.audited("readiness_define").len();
    let view = define(
        json!([{"namespace": "vm", "resource": "macos-vm"},
            {"namespace": "ports", "resource": "8080"}]),
        &mut tx,
    )
    .unwrap();
    assert_eq!(view.readiness.step.step_id, step);
    assert_eq!(view.readiness.step.revision, 2);
    assert_eq!(
        view.readiness.step.resources,
        [need("ports/8080"), need(VM)]
    );
    let ready = s.m.runtime.ready_work(&owner, &mut tx).unwrap();
    assert_eq!(ready[0].resources, [need("ports/8080"), need(VM)]);
    assert_eq!(s.audited("readiness_define").len(), defined + 1);

    // The policy's requirements are refused with a stable code each.
    let problems = |resources: Value| {
        let mut body = serde_json::to_value(policy(false, Vec::new())).unwrap();
        body["placement"]["resources"] = resources;
        let Ok(body) = serde_json::from_value::<PolicyBody>(body) else {
            return vec!["unreadable_body".to_owned()];
        };
        s.m.runtime
            .validate_policy(
                &owner,
                PolicyProposal {
                    policy: Some(body),
                    actor_id: None,
                },
            )
            .unwrap()
            .problems
            .into_iter()
            .map(|problem| problem.code.to_owned())
            .collect::<Vec<_>>()
    };
    let entry =
        |target: String, resources: Value| json!([{"target_id": target, "resources": resources}]);
    assert_eq!(
        problems(entry(
            id(1),
            json!([{"namespace": "vm", "resource": "macos-vm"}])
        )),
        [""; 0]
    );
    assert_eq!(
        problems(entry(id(1), json!([]))),
        ["invalid_resource_count"]
    );
    assert_eq!(
        problems(entry(id(2), json!([{"namespace": "vm", "resource": "a"}]))),
        ["resources_without_target"]
    );
    assert_eq!(
        problems(entry(id(1), json!([{"namespace": "v m", "resource": "a"}]))),
        ["invalid_resource"]
    );
    assert_eq!(
        problems(json!([{"target_id": id(1), "resources": [], "extra": 1}])),
        ["unreadable_body"]
    );

    // The names accepted here are exactly the claim service's.
    for name in [
        "vm/macos-vm",
        "a/B",
        "0.a_b-c/Sim_1.2-x",
        "VM/a",
        "vm/a b",
        "vm/.a",
        "-vm/a",
        "v..m/a",
        "vm/",
        "/a",
        "vm",
        "vm/a/b",
        "vm/\u{e9}",
        &format!("{}/a", "n".repeat(64)),
        &format!("{}/a", "n".repeat(65)),
        &format!("a/{}", "r".repeat(128)),
        &format!("a/{}", "r".repeat(129)),
    ] {
        assert_eq!(
            ResourceNeed::parse(name).is_ok(),
            ResourceRef::parse(name).is_ok(),
            "{name:?}"
        );
    }
}

#[test]
fn hs2_2fg91x_a_namespace_one_read_has_no_room_for_is_unknown_not_free() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let mut body = policy(false, Vec::new());
    body.targets[0].max_sessions = 4;
    set_policy(&s.m, body, &mut tx);
    // Nine namespaces across three steps; one capacity read lists eight.
    let names: Vec<String> = (0..9).map(|n| format!("ns{n}/r")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_, a) = s.ready_needing("A", &refs[0..4], &mut tx);
    let (_, b) = s.ready_needing("B", &refs[4..8], &mut tx);
    let (_, c) = s.ready_needing("C", &refs[8..9], &mut tx);
    let plan =
        s.m.runtime
            .scheduler_plan(&s.owner(), Some(&worker()), &mut tx)
            .unwrap();
    let started: Vec<_> = plan
        .schedule
        .placements
        .iter()
        .flat_map(|placement| placement.steps.clone())
        .collect();
    assert!(started.contains(&a) && started.contains(&b));
    assert_eq!(plan.schedule.unplaced[0].step_id, c);
    assert_eq!(unplaced(&plan), [("resource_unknown", Some("ns8/r"))]);
    assert_eq!(
        plan.schedule.contention[0].unreadable,
        Some(UnreadableReason::NamespaceLimit)
    );
    assert_eq!(plan.inputs.unreadable_namespaces, 1);
}

/// A controller whose only target is hosted by a peer machine, reached over the
/// reviewed transport, with one orchestration ticket of its own.
struct Remote {
    _root: tempfile::TempDir,
    home: PathBuf,
    runtime: OrchestrationRuntime,
    owner: Checkout,
    peer: Arc<RemotePeer>,
    sessions: Arc<RecordingSessions>,
    orchestration: Ticket,
}

fn remote(tx: &mut Transaction<'_>) -> Remote {
    let mut hosted = machine(99, false, unsupported);
    let probe = Arc::new(Probe::default());
    probe.set(|observation| *observation = roomy());
    hosted.runtime = hosted.runtime.clone().with_capacity_probe(probe);
    let peer = Arc::new(RemotePeer {
        hosted,
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
    let sessions = Arc::new(RecordingSessions::default());
    let runtime = OrchestrationRuntime::new(
        home.clone(),
        checkouts,
        ProviderConfigRegistry::new(root.path().join("providers.json")),
        secrets.clone(),
        peer.clone(),
        id(10),
    )
    .unwrap()
    .with_session_executor(sessions.clone());
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
                    label: "Remote target".into(),
                    expected_repository_identity: None,
                }],
            },
            tx,
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
    assert!(
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
                tx
            )
            .unwrap()
            .usable
    );
    for (actions, session_scope) in [
        (
            vec![
                TargetAction::TicketsRead,
                TargetAction::GatesObserve,
                TargetAction::WorkStart,
            ],
            None,
        ),
        (
            vec![TargetAction::SessionsManage],
            Some(SessionScope {
                tools: vec![scope("codex"), scope("claude")],
                max_concurrency: 4,
            }),
        ),
    ] {
        runtime
            .grant(
                &target,
                CreateGrant {
                    grant_id: None,
                    actor_id: "worker-1".into(),
                    actions,
                    session_scope,
                    expires_at_ms: 10_000,
                },
                Some(&human()),
                tx,
            )
            .unwrap();
    }
    runtime
        .set_policy(
            &owner.id,
            SetPolicy {
                expected_revision: None,
                policy: policy(true, Vec::new()),
            },
            Some(&human()),
            tx,
        )
        .unwrap();
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
    Remote {
        _root: root,
        home,
        runtime,
        owner,
        peer,
        sessions,
        orchestration,
    }
}

#[test]
fn hs2_2fg91x_a_resource_on_another_machine_is_read_from_that_machines_backend() {
    tx!(tx);
    let r = remote(&mut tx);
    let peer_home = r.peer.hosted.home.clone();
    let ticket = ops::create(
        &r.peer.hosted.target_store,
        ulid::Ulid::new(),
        "TGT",
        NOW.into(),
        NewTicket {
            title: "Remote work".into(),
            ..Default::default()
        },
    )
    .unwrap();
    let step = r
        .runtime
        .readiness_define(
            &r.owner.id,
            serde_json::from_value(json!({
                "ticket": {"native_id": r.orchestration.slug},
                "target_id": id(1), "connection_id": r.peer.hosted.target_connection(),
                "native_id": ticket.slug, "resources": [need(VM)],
            }))
            .unwrap(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap()
        .readiness
        .step
        .step_id;
    let tick = |tx: &mut Transaction<'_>| {
        r.runtime
            .scheduler_tick(&r.owner.id, &TickRequest::default(), Some(&worker()), tx)
            .unwrap()
    };
    let calls = || std::mem::take(&mut *r.peer.calls.lock().unwrap());

    // The peer's VM is held on the peer's backend. The controller's own machine has
    // nothing of that name, and a resource of the same name there is another one.
    let held = acquire(&peer_home, VM);
    assert_eq!(live_claims(&r.home, "vm"), 0);
    calls();
    let waiting = tick(&mut tx);
    assert!(waiting.outcomes.is_empty());
    assert_eq!(unplaced(&waiting.view), [("resource_held", Some(VM))]);
    let entry = &waiting.view.schedule.contention[0];
    assert_eq!(
        (entry.state, entry.server_id.clone(), entry.expires_at_ms),
        (
            ResourceState::Held,
            Some(id(99)),
            Some(held.claim.expires_at_ms)
        )
    );
    assert_eq!(
        waiting.view.schedule.retry_at_ms,
        Some(held.claim.expires_at_ms)
    );
    // It was read from the peer, over the reviewed transport, once.
    assert_eq!(
        calls(),
        [(
            "GET".to_owned(),
            format!(
                "/checkouts/{}/target-capacity?claim_namespaces=vm",
                r.peer.hosted.target.id
            )
        )]
    );
    // A following loop waits on nothing here for a claim on another machine: the
    // expiry it was told is when it looks again.
    let mut state = SchedulerFollowState::default();
    let followed = r
        .runtime
        .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx);
    assert_eq!(
        (followed.trigger, followed.retry_at_ms),
        (Some("start"), Some(held.claim.expires_at_ms))
    );
    assert!(r.runtime.claim_watches().is_empty());
    assert!(r.sessions.calls().is_empty());

    // Released on the peer while the same name is held on the controller's own
    // machine: the work starts, because that claim is not the target's.
    release(&peer_home, &held);
    let local = acquire(&r.home, VM);
    let after = tick(&mut tx);
    assert_eq!(outcomes(&after), [("started", None)]);
    assert!(after.view.schedule.contention.is_empty());
    let [SessionCall::Start(request)] = &r.sessions.calls()[..] else {
        panic!("one start");
    };
    assert_eq!(request.steps, [step]);
    assert_eq!(request.resources, [need(VM)]);
    // Neither machine's backend was claimed on by the scheduler.
    assert_eq!(
        (live_claims(&peer_home, "vm"), live_claims(&r.home, "vm")),
        (0, 1)
    );
    release(&r.home, &local);
}
