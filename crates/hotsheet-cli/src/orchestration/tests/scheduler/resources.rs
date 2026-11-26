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
    fn held(
        &self,
        namespace: &str,
        _: Option<&std::path::Path>,
    ) -> Result<Vec<HeldClaim>, ClaimsUnreachable> {
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
    fn wait_free(&self, _: &str, _: &str, _: Duration, _: Option<&std::path::Path>) -> bool {
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
    set_policy(&s.m, policy(true), &mut tx);
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
    set_policy(&s.m, policy(true), &mut tx);
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
    set_policy(&s.m, policy(true), &mut tx);
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
    set_policy(&s.m, policy(true), &mut tx);
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
    allow_on(
        &s.m.runtime,
        &s.m.owner.id,
        &id(1),
        &[TargetAction::SessionsManage],
        &["codex", "claude"],
        &mut tx,
    );
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
    set_policy(&s.m, policy(true), &mut tx);
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
    set_policy(&s.m, policy(false), &mut tx);
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
    let mut body = policy(true);
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
        let mut body = serde_json::to_value(policy(false)).unwrap();
        body["placement"]["resources"] = resources;
        let Ok(body) = serde_json::from_value::<PolicyBody>(body) else {
            return vec!["unreadable_body".to_owned()];
        };
        s.m.runtime
            .validate_policy(&owner, PolicyProposal { policy: Some(body) })
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

/// HS2-KCEHVR: one read lists eight namespaces, so a target's namespaces are read in
/// pages of eight, at most four per pass. Only a namespace beyond the fourth page is
/// unknown, never assumed free.
#[test]
fn hs2_2fg91x_a_namespace_one_read_has_no_room_for_is_unknown_not_free() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let mut body = policy(false);
    body.machine.max_sessions = 12;
    for tool in &mut body.tools {
        tool.max_sessions = Some(12);
    }
    body.targets[0].max_sessions = Some(12);
    set_policy(&s.m, body, &mut tx);
    // Thirty-three namespaces across nine steps; one pass reads thirty-two.
    let names: Vec<String> = (0..33).map(|n| format!("ns{n:02}/r")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let steps: Vec<String> = refs
        .chunks(4)
        .enumerate()
        .map(|(n, chunk)| s.ready_needing(&format!("Step {n}"), chunk, &mut tx).1)
        .collect();
    let read = s.probe.asked();
    let plan =
        s.m.runtime
            .scheduler_plan(&s.owner(), Some(&worker()), &mut tx)
            .unwrap();
    // Four reads of eight, one target.
    assert_eq!(s.probe.asked() - read, 4);
    // Every namespace in the four pages was read: only the 33rd is unknown. (Steps
    // the machine has no room for this pass wait for room, not for a resource.)
    let unknown: Vec<_> = plan
        .schedule
        .unplaced
        .iter()
        .filter(|item| item.reason.code() == "resource_unknown")
        .map(|item| (item.step_id.clone(), item.detail.clone()))
        .collect();
    assert_eq!(unknown, [(steps[8].clone(), Some("ns32/r".to_owned()))]);
    assert_eq!(plan.schedule.contention.len(), 1);
    assert_eq!(
        plan.schedule.contention[0].unreadable,
        Some(UnreadableReason::NamespaceLimit)
    );
    assert_eq!(plan.inputs.unreadable_namespaces, 1);
}

/// HS2-KCEHVR: a namespace on a later page is read, not assumed free: its claim
/// defers the work that needs it.
#[test]
fn hs2_kcehvr_a_claim_on_a_later_page_is_read_and_defers_its_work() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let mut body = policy(false);
    body.targets[0].max_sessions = Some(4);
    set_policy(&s.m, body, &mut tx);
    let names: Vec<String> = (0..9).map(|n| format!("ns{n}/r")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let (_, a) = s.ready_needing("A", &refs[0..4], &mut tx);
    let (_, b) = s.ready_needing("B", &refs[4..8], &mut tx);
    let (_, c) = s.ready_needing("C", &refs[8..9], &mut tx);
    let grant = acquire(&s.m.home, "ns8/r");
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
    assert_eq!(unplaced(&plan), [("resource_held", Some("ns8/r"))]);
    assert_eq!(plan.inputs.unreadable_namespaces, 0);
    release(&s.m.home, &grant);
    let plan =
        s.m.runtime
            .scheduler_plan(&s.owner(), Some(&worker()), &mut tx)
            .unwrap();
    assert!(
        plan.schedule.unplaced.is_empty(),
        "{:?}",
        plan.schedule.unplaced
    );
}

/// HS2-KCEHVR: the machine's own claim service reports each unreadable namespace with
/// the offline policy its configuration names, and the claims of the namespaces it
/// could read. The scheduler defers only the unreadable work, each by its own policy.
/// A configuration bound on the machine without a repository cannot be read; no
/// credential is configured, so nothing reaches a credential store.
#[test]
fn hs2_kcehvr_each_unreadable_namespace_reports_its_configured_policy() {
    use hotsheet_ticketing::claim_backends::{BackendConfiguration, BackendKind, Scope};
    tx!(tx);
    let s = setup(true, &mut tx);
    let mut body = policy(false);
    body.targets[0].max_sessions = Some(4);
    set_policy(&s.m, body, &mut tx);
    let configuration = |id: &str, offline| {
        let mut c = BackendConfiguration::new(id, BackendKind::GitRef).unwrap();
        c.offline = offline;
        c
    };
    Scope::Machine {
        home: s.m.home.clone(),
    }
    .update(|r| {
        r.upsert_configuration(configuration("open", OfflinePolicy::Provisional))?;
        r.upsert_configuration(configuration("strict", OfflinePolicy::FailClosed))?;
        r.bind_namespace("pool", "open");
        r.bind_namespace("lab", "strict");
        Ok(())
    })
    .unwrap();
    let grant = acquire(&s.m.home, VM);

    let snapshot =
        s.m.runtime
            .hosted_capacity(&s.m.target.id, &["pool".into(), "lab".into(), "vm".into()])
            .unwrap();
    // A reader that knows only the older members still sees every namespace unknown,
    // under the strictest policy.
    assert_eq!(
        (snapshot.claims.clone(), snapshot.claims_offline),
        (None, Some(OfflinePolicy::FailClosed))
    );
    let wire = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(wire["claims"], Value::Null);
    assert_eq!(
        wire["claims_unreadable"],
        json!([{"namespace": "pool", "offline": "provisional"},
            {"namespace": "lab", "offline": "fail_closed"}])
    );
    assert_eq!(wire["claims_readable"][0]["resource"], "macos-vm");
    assert_eq!(wire["claims_readable"].as_array().unwrap().len(), 1);

    let (_, open) = s.ready_needing("Open pool", &["pool/a"], &mut tx);
    let (_, strict) = s.ready_needing("Strict lab", &["lab/a"], &mut tx);
    let (_, held) = s.ready_needing("Held VM", &[VM], &mut tx);
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
    assert_eq!(started, [open]);
    let mut waiting: Vec<_> = plan
        .schedule
        .unplaced
        .iter()
        .map(|item| (item.step_id.clone(), item.reason.code()))
        .collect();
    waiting.sort();
    let mut expected = vec![(strict, "resource_unknown"), (held, "resource_held")];
    expected.sort();
    assert_eq!(waiting, expected);
    assert_eq!(plan.inputs.unreadable_namespaces, 2);
    release(&s.m.home, &grant);
}

/// HS2-KCEHVR: what an older peer answered, all unknown under one policy, still
/// makes every asked namespace unknown; a partial answer without its readable claims
/// is read the same way.
#[test]
fn hs2_kcehvr_an_answer_without_per_namespace_detail_keeps_every_namespace_unknown() {
    use hotsheet_ticketing::orchestration::{CapacitySnapshot, UnreadableNamespace};
    let mut snapshot = CapacitySnapshot::new(
        "s".into(),
        "c".into(),
        HostObservation::default(),
        vec!["a".into(), "b".into()],
        None,
    )
    .with_claims_offline(Some(OfflinePolicy::Provisional));
    assert!(snapshot.claims_unreadable.is_empty());
    let names = vec!["a".to_owned(), "b".to_owned()];
    let (mut held, mut unreadable) = (Vec::new(), Vec::new());
    crate::orchestration::scheduler::read_claims(&snapshot, &names, &mut held, &mut unreadable);
    assert!(held.is_empty());
    assert_eq!(
        unreadable
            .iter()
            .map(|entry| (entry.namespace.as_str(), entry.offline))
            .collect::<Vec<_>>(),
        [
            ("a", OfflinePolicy::Provisional),
            ("b", OfflinePolicy::Provisional)
        ]
    );
    // A partial answer: only "b" is unreadable, and "a" holds a claim.
    snapshot = snapshot.with_partial_claims(
        vec![HeldClaim {
            namespace: "a".into(),
            resource: "r".into(),
            expires_at_ms: 9,
        }],
        vec![UnreadableNamespace {
            namespace: "b".into(),
            offline: OfflinePolicy::FailClosed,
        }],
    );
    let (mut held, mut unreadable) = (Vec::new(), Vec::new());
    crate::orchestration::scheduler::read_claims(&snapshot, &names, &mut held, &mut unreadable);
    assert_eq!(held.len(), 1);
    assert_eq!(
        unreadable
            .iter()
            .map(|entry| (entry.namespace.as_str(), entry.offline))
            .collect::<Vec<_>>(),
        [("b", OfflinePolicy::FailClosed)]
    );
    // Known claims ignore any partial detail.
    let known = CapacitySnapshot::new(
        "s".into(),
        "c".into(),
        HostObservation::default(),
        names.clone(),
        Some(Vec::new()),
    )
    .with_partial_claims(
        Vec::new(),
        vec![UnreadableNamespace {
            namespace: "b".into(),
            offline: OfflinePolicy::FailClosed,
        }],
    );
    assert!(known.claims_unreadable.is_empty() && known.claims_readable.is_none());
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
        offline: Default::default(),
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
                    site_of: None,
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
    allow_on(
        &runtime,
        &target.owner_checkout_id,
        &target.target_id,
        &[TargetAction::WorkStart, TargetAction::SessionsManage],
        &["codex", "claude"],
        tx,
    );
    runtime
        .set_policy(
            &owner.id,
            SetPolicy {
                expected_revision: None,
                policy: policy(true),
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
    // A following loop waits on the peer's own backend for a claim there
    // (HS2-G1PKK2); the expiry it was told is still when it looks again.
    let mut state = SchedulerFollowState::default();
    let followed = r
        .runtime
        .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx);
    assert_eq!(
        (followed.trigger, followed.retry_at_ms),
        (Some("start"), Some(held.claim.expires_at_ms))
    );
    assert_eq!(
        r.runtime.claim_watches(),
        [("vm".to_owned(), "macos-vm".to_owned())]
    );
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

/// HS2-G1PKK2: remote work that needs the peer's VM, which the peer holds now.
fn remote_waiting(lease: Duration, tx: &mut Transaction<'_>) -> (Remote, Grant, String) {
    let r = remote(tx);
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
            tx,
        )
        .unwrap()
        .readiness
        .step
        .step_id;
    let held = ClaimService::new(&r.peer.hosted.home)
        .acquire(
            &ResourceRef::parse(VM).unwrap(),
            lease,
            None,
            &BTreeMap::new(),
        )
        .unwrap();
    (r, held, step)
}

fn wait_calls(r: &Remote) -> usize {
    r.peer
        .calls
        .lock()
        .unwrap()
        .iter()
        .filter(|(_, path)| path.contains("/target-resource-wait?"))
        .count()
}

/// Wait on state, never a fixed delay: until `done` or the bound passes.
fn until(bound: Duration, mut done: impl FnMut() -> bool) -> bool {
    let began = std::time::Instant::now();
    while began.elapsed() < bound {
        if done() {
            return true;
        }
        std::thread::yield_now();
        std::thread::sleep(Duration::from_millis(20));
    }
    done()
}

/// Follow steps until one is triggered, within `bound`.
fn follow_until_triggered(
    r: &Remote,
    state: &mut SchedulerFollowState,
    bound: Duration,
    tx: &mut Transaction<'_>,
) -> Option<&'static str> {
    let began = std::time::Instant::now();
    let mut trigger = None;
    while trigger.is_none() && began.elapsed() < bound {
        trigger = r
            .runtime
            .scheduler_follow_step(&r.owner.id, state, 1_000, tx)
            .trigger;
    }
    trigger
}

#[test]
fn hs2_g1pkk2_a_release_on_another_machine_wakes_the_follower() {
    tx!(tx);
    let (r, held, step) = remote_waiting(Duration::from_secs(30 * 60), &mut tx);
    let mut state = SchedulerFollowState::default();
    let first = r
        .runtime
        .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx);
    assert_eq!(first.trigger, Some("start"));
    assert!(r.sessions.calls().is_empty());
    // The watch long-polls the peer's own wait over the reviewed transport.
    assert!(until(Duration::from_secs(5), || wait_calls(&r) >= 1));
    let path = r
        .peer
        .calls
        .lock()
        .unwrap()
        .iter()
        .find(|(_, path)| path.contains("/target-resource-wait?"))
        .cloned()
        .unwrap();
    assert_eq!(
        path,
        (
            "GET".to_owned(),
            format!(
                "/checkouts/{}/target-resource-wait?namespace=vm&resource=macos-vm&wait_ms={MAX_RESOURCE_WAIT_MS}",
                r.peer.hosted.target.id
            )
        )
    );
    // Another pass that defers the same work arms no second watch.
    OrchestrationRuntime::scheduler_wake_at(&r.home);
    assert_eq!(
        r.runtime
            .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx)
            .trigger,
        Some("signal")
    );
    assert_eq!(r.runtime.claim_watches().len(), 1);
    // The holder releases on the peer: the peer's wait answers, the follower wakes
    // and the work starts, long before the claim's expiry.
    release(&r.peer.hosted.home, &held);
    assert_eq!(
        follow_until_triggered(&r, &mut state, Duration::from_secs(20), &mut tx),
        Some("signal")
    );
    let [SessionCall::Start(request)] = &r.sessions.calls()[..] else {
        panic!("one start");
    };
    assert_eq!(request.steps, [step]);
    assert!(until(Duration::from_secs(5), || r
        .runtime
        .claim_watches()
        .is_empty()));
}

#[test]
fn hs2_g1pkk2_an_expiry_on_another_machine_wakes_the_follower() {
    tx!(tx);
    // Nobody releases; the lease lapses on the peer's clock.
    let (r, _held, step) = remote_waiting(Duration::from_millis(1_500), &mut tx);
    let mut state = SchedulerFollowState::default();
    assert_eq!(
        r.runtime
            .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx)
            .trigger,
        Some("start")
    );
    assert!(r.sessions.calls().is_empty());
    // The test's own clock never reaches the expiry, so only the peer's wait can
    // wake the loop.
    assert_eq!(
        follow_until_triggered(&r, &mut state, Duration::from_secs(20), &mut tx),
        Some("signal")
    );
    let [SessionCall::Start(request)] = &r.sessions.calls()[..] else {
        panic!("one start");
    };
    assert_eq!(request.steps, [step]);
}

#[test]
fn hs2_g1pkk2_a_renewal_on_another_machine_keeps_the_watch_waiting() {
    tx!(tx);
    let (r, held, _) = remote_waiting(Duration::from_millis(1_500), &mut tx);
    let mut state = SchedulerFollowState::default();
    r.runtime
        .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx);
    assert!(until(Duration::from_secs(5), || wait_calls(&r) >= 1));
    // The holder renews before the lease lapses.
    ClaimService::new(&r.peer.hosted.home)
        .renew(&held.renewal_token, Duration::from_secs(30 * 60), None)
        .unwrap();
    // Past the old expiry nothing wakes the loop and the watch still waits.
    let quiet = std::time::Instant::now();
    while quiet.elapsed() < Duration::from_millis(2_500) {
        assert_eq!(
            r.runtime
                .scheduler_follow_step(&r.owner.id, &mut state, 500, &mut tx)
                .trigger,
            None
        );
    }
    assert_eq!(r.runtime.claim_watches().len(), 1);
    assert!(r.sessions.calls().is_empty());
    release(&r.peer.hosted.home, &held);
    assert_eq!(
        follow_until_triggered(&r, &mut state, Duration::from_secs(20), &mut tx),
        Some("signal")
    );
    assert_eq!(r.sessions.calls().len(), 1);
}

#[test]
fn hs2_g1pkk2_an_unreachable_peer_ends_the_watch_without_a_wake() {
    tx!(tx);
    let (r, held, _) = remote_waiting(Duration::from_secs(30 * 60), &mut tx);
    let mut state = SchedulerFollowState::default();
    r.runtime
        .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx);
    assert!(until(Duration::from_secs(5), || wait_calls(&r) >= 1));
    // The peer stops answering. The wait in flight ends held; the next request
    // fails, and the watch ends without waking anyone or asking again.
    *r.peer.status_override.lock().unwrap() = Some(503);
    assert!(until(
        Duration::from_millis(MAX_RESOURCE_WAIT_MS + 10_000),
        || r.runtime.claim_watches().is_empty()
    ));
    let asked = wait_calls(&r);
    assert_eq!(asked, 2);
    assert_eq!(
        r.runtime
            .scheduler_follow_step(&r.owner.id, &mut state, 0, &mut tx)
            .trigger,
        None
    );
    assert!(r.sessions.calls().is_empty());
    *r.peer.status_override.lock().unwrap() = None;
    release(&r.peer.hosted.home, &held);
}

#[test]
fn hs2_g1pkk2_a_follow_ends_on_deadline_refusal_or_a_peer_that_does_not_wait() {
    tx!(tx);
    let (r, held, _) = remote_waiting(Duration::from_secs(30 * 60), &mut tx);
    let target = TargetRef {
        owner_checkout_id: r.owner.id.clone(),
        target_id: id(1),
    };
    let follow = |target: &TargetRef, deadline: std::time::Instant, tx: &mut Transaction<'_>| {
        r.runtime
            .follow_remote_release(&r.owner.id, target, &worker(), &need(VM), deadline, tx)
    };
    let soon = || std::time::Instant::now() + Duration::from_secs(30);
    // A deadline already passed asks nothing.
    let before = wait_calls(&r);
    assert_eq!(
        follow(&target, std::time::Instant::now(), &mut tx),
        RemoteRelease::Ended("deadline".into())
    );
    assert_eq!(wait_calls(&r), before);
    // An undeclared target is refused by the gate before the peer is asked.
    let undeclared = TargetRef {
        owner_checkout_id: r.owner.id.clone(),
        target_id: id(7),
    };
    assert!(matches!(
        follow(&undeclared, soon(), &mut tx),
        RemoteRelease::Ended(code) if code != "deadline"
    ));
    assert_eq!(wait_calls(&r), before);
    // Free: the follow answers released and writes the scheduler's wake marker.
    release(&r.peer.hosted.home, &held);
    let marker = r.home.join("orchestration-scheduler-wake").join("token");
    let _ = std::fs::remove_file(&marker);
    assert_eq!(follow(&target, soon(), &mut tx), RemoteRelease::Released);
    assert!(marker.is_file());
    assert_eq!(wait_calls(&r), before + 1);
    // A peer whose backend cannot wait answers held at once: the follow ends rather
    // than asking again in a tight loop.
    let state = find_state(&r.peer.hosted.home).expect("the peer's claim state");
    acquire(&r.peer.hosted.home, VM);
    std::fs::write(&state, b"{").unwrap();
    assert_eq!(
        follow(&target, soon(), &mut tx),
        RemoteRelease::Ended("peer_did_not_wait".into())
    );
    assert_eq!(wait_calls(&r), before + 2);
}

/// The local backend's state file of the `vm` namespace under a machine home.
fn find_state(dir: &Path) -> Option<PathBuf> {
    for entry in std::fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(found) = find_state(&path) {
                return Some(found);
            }
        } else if path.ends_with("namespaces/vm/state.json") {
            return Some(path);
        }
    }
    None
}

#[test]
fn hs2_g1pkk2_the_hosted_wait_answers_from_this_machines_backend() {
    let m = machine(10, false, unsupported);
    let base = format!("/checkouts/{}/target-resource-wait", m.target.id);
    let wait = |query: &str| {
        m.runtime
            .request_now("GET", &format!("{base}{query}"), &Value::Null)
    };
    // Nothing held: free at once.
    assert_eq!(
        wait("?namespace=vm&resource=macos-vm&wait_ms=0").unwrap(),
        json!({"free": true})
    );
    // Held: still held when a short wait ends; free once released.
    let grant = acquire(&m.home, VM);
    assert_eq!(
        wait("?namespace=vm&resource=macos-vm&wait_ms=50").unwrap(),
        json!({"free": false})
    );
    release(&m.home, &grant);
    assert_eq!(
        wait("?namespace=vm&resource=macos-vm&wait_ms=50").unwrap(),
        json!({"free": true})
    );
    for query in [
        "",
        "?namespace=vm",
        "?resource=macos-vm",
        "?namespace=Not%20One&resource=macos-vm",
        "?namespace=vm&resource=macos-vm&wait_ms=x",
        "?namespace=vm&namespace=vm&resource=macos-vm",
    ] {
        assert!(
            matches!(wait(query), Err(OrchestrationError::Invalid(_))),
            "{query}"
        );
    }
    // A checkout this machine does not host is not found.
    assert!(
        m.runtime
            .request_now(
                "GET",
                "/checkouts/elsewhere-0123456789ab/target-resource-wait?namespace=vm&resource=macos-vm",
                &Value::Null,
            )
            .is_err()
    );
    // Ordinary authenticated project data, not machine-fenced, so a remote
    // orchestrator can reach it.
    assert!(is_path(&base));
    assert!(!is_machine_path(&base));
}

/// HS2-AWB7BX: a capacity follow of a remote target long-polls the peer's own
/// capacity wait over the reviewed transport; a peer without the route ends it.
#[test]
fn hs2_awb7bx_a_remote_capacity_follow_uses_the_peers_wait() {
    tx!(tx);
    let r = remote(&mut tx);
    let target = TargetRef {
        owner_checkout_id: r.owner.id.clone(),
        target_id: id(1),
    };
    let follow = |tx: &mut Transaction<'_>| {
        r.runtime.follow_capacity_change(
            &target,
            &worker(),
            &["vm".to_owned()],
            "0000000000000000",
            std::time::Instant::now() + Duration::from_secs(30),
            tx,
        )
    };
    assert!(matches!(follow(&mut tx), CapacityFollow::Changed(_)));
    let last = r.peer.calls.lock().unwrap().last().cloned().unwrap();
    assert_eq!(
        last,
        (
            "GET".to_owned(),
            format!(
                "/checkouts/{}/target-capacity?claim_namespaces=vm&after=0000000000000000&wait_ms={MAX_CAPACITY_WAIT_MS}",
                r.peer.hosted.target.id
            )
        )
    );
    // An older peer without the route, or one that cannot be reached, ends it.
    *r.peer.status_override.lock().unwrap() = Some(404);
    assert_eq!(
        follow(&mut tx),
        CapacityFollow::Ended("target_peer_unsupported".into())
    );
    *r.peer.status_override.lock().unwrap() = None;
}

/// HS2-NPAZQW: a pattern the policy requires of a target is resolved by the pass to
/// one free declared resource, and the started session is told to acquire that exact
/// name, never the pattern. An exact resource the work names that matches satisfies it.
#[test]
fn hs2_npazqw_a_pattern_requirement_starts_a_session_holding_one_exact_match() {
    tx!(tx);
    let s = setup(true, &mut tx);
    let mut body = policy(true);
    body.placement.resources = vec![TargetResources {
        target_id: id(1),
        resources: vec![ResourceNeed {
            namespace: "vm".into(),
            resource: "mac-*".into(),
        }],
    }];
    set_policy(&s.m, body, &mut tx);
    // One step names nothing; the other names an exact match, which declares it.
    let (_, plain) = s.ready("Names nothing", &mut tx);
    let (_, named) = s.ready_needing("Names a match", &["vm/mac-b"], &mut tx);

    let tick = s.tick(&mut tx);
    assert_eq!(outcomes(&tick), [("started", None)]);
    assert_eq!(unplaced(&tick.view), [("resource_held", Some("vm/mac-b"))]);
    let started = &s.starts()[0];
    assert!(started.steps == [plain.clone()] || started.steps == [named.clone()]);
    // Either way the session holds the one exact resource, never the pattern.
    assert_eq!(started.resources, [need("vm/mac-b")]);
}
