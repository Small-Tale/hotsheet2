//! HS2-KXKA7X: triggers through the shared runtime, with a scripted terminal host for
//! the planner, real git ticket stores, a clock a test moves, and, for branch watches,
//! the real GitHub Actions adapter over a Job store whose watch Jobs a test judges the
//! way the Job monitor would. Synthetic templates, tickets and commits only.
use std::sync::atomic::AtomicU64;

use hotsheet_jobs::JobStore;
use hotsheet_ticketing::actor::MutationActor;
use hotsheet_ticketing::orchestration::{
    ApplyState, AuditOutcome, DeliveryOutcome, GateKind, GateState, PermissionAnswer, TargetAction,
    Template, TemplateApplyStore, TriggerOp, TriggerState,
};
use hotsheet_ticketing::{NewTicket, TicketPatch, ops};
use serde_json::{Value, json};

use super::gates::{CI, Scripted, judge, link, sha, verdict, with_run};
use super::sessions::{Probe, ScriptedHost, roomy};
use super::tickets::{Machine, NOW, bind_and_allow, human, machine, unsupported, worker};
use super::*;

/// A clock a test moves.
struct At(AtomicU64);
impl hotsheet_ticketing::Clock for At {
    fn now_ms(&self) -> u64 {
        self.0.load(Ordering::SeqCst)
    }
}

/// 2026-11-26T10:00:00Z.
const T0: u64 = 1_795_687_200_000;
const MINUTE: u64 = 60_000;

/// Notifications a trigger raised.
#[derive(Default)]
struct Notices(Mutex<Vec<TriggerNotification>>);
impl HomeNotices for Notices {
    fn notify(&self, _: &HomeNotification) {}
    fn notify_trigger(&self, notification: &TriggerNotification) {
        self.0.lock().unwrap().push(notification.clone());
    }
}
impl Notices {
    fn titles(&self) -> Vec<String> {
        self.0
            .lock()
            .unwrap()
            .iter()
            .map(|notice| notice.title.clone())
            .collect()
    }
}

struct Fixture {
    m: Machine,
    host: Arc<ScriptedHost>,
    notices: Arc<Notices>,
    _probe: Arc<Probe>,
}

impl Fixture {
    /// One project target; `actions` remembered for it.
    fn new(actions: &[TargetAction], tx: &mut Transaction<'_>) -> Self {
        Self::from(machine(10, false, unsupported), actions, tx)
    }

    fn from(mut m: Machine, actions: &[TargetAction], tx: &mut Transaction<'_>) -> Self {
        let host = Arc::new(ScriptedHost::default());
        let probe = Arc::new(Probe::default());
        probe.set(|observation| *observation = roomy());
        let notices = Arc::new(Notices::default());
        m.runtime = m
            .runtime
            .clone()
            .with_capacity_probe(probe.clone())
            .with_session_host(host.clone())
            .with_home_notices(notices.clone());
        if m.runtime
            .declarations(&m.owner.id)
            .unwrap()
            .targets
            .is_empty()
        {
            let mut granted = vec![TargetAction::TicketsRead];
            granted.extend_from_slice(actions);
            bind_and_allow(&m, 1, &granted, tx);
        }
        Self {
            m,
            host,
            notices,
            _probe: probe,
        }
    }

    fn owner(&self) -> String {
        self.m.owner.id.clone()
    }

    fn template(&self, title: &str, tx: &mut Transaction<'_>) -> Template {
        let document = self
            .m
            .runtime
            .template_set(
                &self.owner(),
                serde_json::from_value(json!({"templates": [{
                    "name": "Repair failing CI",
                    "inputs": [
                        {"key": "commit", "label": "Commit", "kind": "text"},
                        {"key": "api", "label": "API Project", "kind": "target", "default": id(1)}
                    ],
                    "goal": {"title": title},
                    "steps": [{"key": "fix", "target": "{{api}}", "title": "Repair CI at {{commit}}"}]
                }]}))
                .unwrap(),
                Some(&human()),
                tx,
            )
            .unwrap();
        document.templates.into_iter().next().unwrap()
    }

    fn set(&self, definition: Value, tx: &mut Transaction<'_>) -> TriggerView {
        self.m
            .runtime
            .trigger_set(
                &self.owner(),
                serde_json::from_value(definition).unwrap(),
                Some(&human()),
                tx,
            )
            .unwrap()
    }

    fn reference(trigger: &TriggerView) -> TriggerRef {
        TriggerRef {
            trigger_id: trigger.trigger.trigger_id.clone(),
        }
    }

    fn show(&self, trigger: &TriggerView, tx: &Transaction<'_>) -> TriggerView {
        self.m
            .runtime
            .trigger_show(&self.owner(), &Self::reference(trigger), tx)
            .unwrap()
    }

    fn fire(
        &self,
        trigger: &TriggerView,
        operation: &str,
        tx: &mut Transaction<'_>,
    ) -> DeliveryView {
        self.m
            .runtime
            .trigger_fire(
                &self.owner(),
                serde_json::from_value(json!({"trigger_id": trigger.trigger.trigger_id,
                    "operation_id": operation, "evidence": {"commit": "abc1234"}}))
                .unwrap(),
                Some(&human()),
                tx,
            )
            .unwrap()
    }

    fn goals(&self) -> Vec<hotsheet_model::Ticket> {
        ops::query(&self.m.owner_store, &ops::TicketQuery::default())
            .unwrap()
            .into_iter()
            .filter(|ticket| {
                ticket
                    .tags
                    .iter()
                    .any(|tag| tag == "orchestration-template")
            })
            .collect()
    }

    fn outcomes(&self, trigger: &TriggerView, tx: &Transaction<'_>) -> Vec<(DeliveryOutcome, u32)> {
        self.show(trigger, tx)
            .trigger
            .history
            .iter()
            .map(|entry| (entry.outcome, entry.count))
            .collect()
    }
}

fn template_trigger(template: &Template, overlap: &str) -> Value {
    json!({
        "name": "Repair API main CI",
        "source": {"kind": "gate_failed"},
        "action": {"template": {"template_id": template.template_id, "tool": "claude",
            "event_inputs": {"commit": "commit"}}},
        "overlap": overlap,
    })
}

fn schedule(every: u16) -> Value {
    json!({"kind": "schedule", "schedule": {"time_zone": "UTC", "local_time": "00:00",
        "recurrence": {"kind": "minutely", "start_date": "2026-11-01", "every": every}}})
}

/// Set up, review and turn On as a person; a template edit sends the On trigger to
/// Review (it then ignores deliveries) and tells a person once; turning it On again
/// records the new review.
#[test]
fn hs2_kxka7x_review_turn_on_and_a_template_change_needs_a_review() {
    let clock = At(AtomicU64::new(T0));
    let mut rng = Rng(100);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[TargetAction::TicketsWrite], &mut tx);
    let template = f.template("Repair CI at {{commit}}", &mut tx);
    let definition = template_trigger(&template, "skip");
    // An unsaved definition reviews without writing anything.
    let review =
        f.m.runtime
            .trigger_review(
                &f.owner(),
                &serde_json::from_value(json!({"trigger": definition})).unwrap(),
                Some(&human()),
                &tx,
            )
            .unwrap();
    assert!(review.problems.is_empty(), "{:?}", review.problems);
    assert_eq!(review.scope.watches.kind, "gate_failed");
    assert_eq!(review.scope.projects.len(), 1);
    assert_eq!(review.scope.projects[0].target_id, id(1));
    assert_eq!(review.scope.per_delivery.goals, 1);
    assert_eq!(review.scope.per_delivery.steps, 1);
    let needs: Vec<(String, ReviewNeed)> = review
        .permissions
        .iter()
        .map(|row| (row.action.clone(), row.need))
        .collect();
    assert_eq!(
        needs,
        [
            ("watch".into(), ReviewNeed::NotNeeded),
            ("tickets_write".into(), ReviewNeed::Remembered),
            ("session_start".into(), ReviewNeed::Asks),
            ("push_deploy".into(), ReviewNeed::Unchanged),
        ]
    );
    assert_eq!(review.turn_on, AccessNeed::Person);
    assert!(
        f.m.runtime
            .trigger_list(&f.owner(), &tx)
            .unwrap()
            .triggers
            .is_empty()
    );

    // Saved: Dry Run by default, never On before a review.
    let saved = f.set(definition.clone(), &mut tx);
    assert_eq!(saved.trigger.state, TriggerState::DryRun);
    assert!(!saved.reviewed);
    let on =
        f.m.runtime
            .trigger_on(
                &f.owner(),
                &Fixture::reference(&saved),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(on.trigger.state, TriggerState::On);
    assert!(on.reviewed);
    let recorded = on.trigger.review.clone().unwrap();
    assert_eq!(
        (recorded.role.as_str(), recorded.template_version),
        ("human", Some(1))
    );
    // Turning it On again changes nothing.
    let again =
        f.m.runtime
            .trigger_on(
                &f.owner(),
                &Fixture::reference(&saved),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(again.trigger.review, on.trigger.review);

    // The template changes: the next pass moves it to Review, once, and tells a person.
    let mut edited = template.clone();
    edited.goal.title = "Repair {{commit}} quickly".into();
    f.m.runtime
        .template_set(
            &f.owner(),
            serde_json::from_value(json!({"templates": [edited]})).unwrap(),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    for _ in 0..3 {
        f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    }
    let review_state = f.show(&saved, &tx);
    assert_eq!(review_state.trigger.state, TriggerState::NeedsReview);
    assert!(!review_state.reviewed);
    assert_eq!(
        f.notices.titles(),
        ["Trigger Repair API main CI Needs Review"]
    );
    assert_eq!(f.outcomes(&saved, &tx), [(DeliveryOutcome::NeedsReview, 1)]);
    // A delivery now is not taken.
    assert!(matches!(
        f.m.runtime.trigger_fire(
            &f.owner(),
            serde_json::from_value(json!({"trigger_id": saved.trigger.trigger_id,
                "operation_id": "by-hand"}))
            .unwrap(),
            Some(&human()),
            &mut tx,
        ),
        Err(OrchestrationError::Trigger(
            hotsheet_ticketing::orchestration::TriggerError::NotFiring
        ))
    ));
    // Reviewed again: On, for version 2.
    let reviewed =
        f.m.runtime
            .trigger_on(
                &f.owner(),
                &Fixture::reference(&saved),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(reviewed.trigger.state, TriggerState::On);
    assert_eq!(reviewed.trigger.review.unwrap().template_version, Some(2));
    // Editing the definition of an On trigger sends it to Review too.
    let mut changed = definition;
    changed["trigger_id"] = json!(saved.trigger.trigger_id);
    changed["overlap"] = json!("queue_latest");
    assert_eq!(
        f.set(changed, &mut tx).trigger.state,
        TriggerState::NeedsReview
    );
    let actions: Vec<String> =
        f.m.runtime
            .audit(&f.owner(), 0, 500)
            .unwrap()
            .entries
            .into_iter()
            .map(|entry| entry.record.action)
            .filter(|action| action.starts_with("trigger_"))
            .collect();
    assert_eq!(
        actions,
        [
            "trigger_set",
            "trigger_on",
            "trigger_on",
            "trigger_needs_review",
            "trigger_on",
            "trigger_set"
        ]
    );
}

/// A schedule fires once per slot at its instant (the pass names the next slot as the
/// wake, never an interval); slots missed while the machine was off run once; a
/// replay of the same slot does nothing; Pause ignores slots and Resume never
/// backfills. The action here starts placement.
#[test]
fn hs2_kxka7x_a_schedule_fires_once_per_slot_and_coalesces_missed_slots() {
    let clock = At(AtomicU64::new(T0 + 30_000));
    let mut rng = Rng(200);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[], &mut tx);
    let trigger = f.set(
        json!({"name": "Start every five minutes", "source": schedule(5),
            "action": {"placement": "start"}}),
        &mut tx,
    );
    assert_eq!(
        trigger.next_runs_ms,
        [T0 + 5 * MINUTE, T0 + 10 * MINUTE, T0 + 15 * MINUTE]
    );
    let on =
        f.m.runtime
            .trigger_on(
                &f.owner(),
                &Fixture::reference(&trigger),
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(on.trigger.state, TriggerState::On);
    let automatic = || {
        f.m.runtime
            .policy(&f.owner())
            .unwrap()
            .policy
            .and_then(|policy| policy.placement.automatic)
    };
    assert_eq!(automatic(), None);
    // Before the slot: nothing, and the wake is the slot itself.
    clock.0.store(T0 + 4 * MINUTE, Ordering::SeqCst);
    let pass = f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert!(pass.deliveries.is_empty());
    assert_eq!(pass.next_wake_ms, Some(T0 + 5 * MINUTE));
    // At the slot: one delivery; again at the same instant and a little later, none.
    clock.0.store(T0 + 5 * MINUTE, Ordering::SeqCst);
    let pass = f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert_eq!(pass.deliveries.len(), 1);
    assert_eq!(pass.deliveries[0].delivery_key, "schedule:2026-11-26T10:05");
    assert_eq!(pass.deliveries[0].outcome, DeliveryOutcome::Applied);
    assert_eq!(pass.next_wake_ms, Some(T0 + 10 * MINUTE));
    assert_eq!(
        automatic(),
        Some(true),
        "the placement action started placement"
    );
    for at in [T0 + 5 * MINUTE, T0 + 6 * MINUTE, T0 + 9 * MINUTE] {
        clock.0.store(at, Ordering::SeqCst);
        assert!(
            f.m.runtime
                .trigger_pass(&f.owner(), &mut tx)
                .unwrap()
                .deliveries
                .is_empty()
        );
    }
    // Off for an hour: the twelve slots missed run once, as the latest.
    clock.0.store(T0 + 65 * MINUTE + 1, Ordering::SeqCst);
    let pass = f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert_eq!(pass.deliveries.len(), 1);
    assert_eq!(pass.deliveries[0].delivery_key, "schedule:2026-11-26T11:05");
    let shown = f.show(&trigger, &tx);
    let last = shown.trigger.history.last().unwrap();
    assert_eq!((last.outcome, last.missed), (DeliveryOutcome::Applied, 11));
    assert_eq!(
        last.placement,
        Some(hotsheet_ticketing::orchestration::TriggerPlacement::Start)
    );
    // Paused: slots are ignored. Resumed later: nothing from the pause is backfilled.
    f.m.runtime
        .trigger_state(
            &f.owner(),
            &Fixture::reference(&trigger),
            TriggerOp::Pause,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    clock.0.store(T0 + 95 * MINUTE + 1, Ordering::SeqCst);
    assert!(
        f.m.runtime
            .trigger_pass(&f.owner(), &mut tx)
            .unwrap()
            .deliveries
            .is_empty()
    );
    let resumed =
        f.m.runtime
            .trigger_state(
                &f.owner(),
                &Fixture::reference(&trigger),
                TriggerOp::Resume,
                Some(&human()),
                &mut tx,
            )
            .unwrap();
    assert_eq!(resumed.trigger.state, TriggerState::On);
    assert!(
        f.m.runtime
            .trigger_pass(&f.owner(), &mut tx)
            .unwrap()
            .deliveries
            .is_empty()
    );
    clock.0.store(T0 + 100 * MINUTE, Ordering::SeqCst);
    let pass = f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert_eq!(pass.deliveries.len(), 1);
    assert_eq!(pass.deliveries[0].delivery_key, "schedule:2026-11-26T11:40");
    assert_eq!(f.show(&trigger, &tx).trigger.history.len(), 3);
    // A placement-only trigger opened nothing.
    assert!(f.goals().is_empty() && f.host.launches().is_empty());
}

/// Dry Run records what each delivery would do (the goal, the steps, the projects
/// that would ask, the placement change) and writes nothing: no goal ticket, no
/// permission request, no apply record, no policy change. A redelivery is already
/// handled, coalesced.
#[test]
fn hs2_kxka7x_dry_run_records_what_would_run_and_writes_nothing() {
    let clock = At(AtomicU64::new(T0));
    let mut rng = Rng(300);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[], &mut tx);
    let template = f.template("Repair CI at {{commit}}", &mut tx);
    let mut definition = template_trigger(&template, "skip");
    definition["action"]["placement"] = json!("start");
    let trigger = f.set(definition, &mut tx);
    assert_eq!(trigger.trigger.state, TriggerState::DryRun);
    let first = f.fire(&trigger, "event-1", &mut tx);
    assert_eq!(first.outcome, DeliveryOutcome::DryRun);
    let shown = f.show(&trigger, &tx);
    let would = shown.trigger.history[0].would.clone().unwrap();
    assert_eq!(would.goal_title.as_deref(), Some("Repair CI at abc1234"));
    assert_eq!(would.steps, 1);
    assert_eq!(
        would.asks,
        [id(1)],
        "the system actor would ask in the project"
    );
    assert_eq!(
        would.placement,
        Some(hotsheet_ticketing::orchestration::TriggerPlacement::Start)
    );
    // The same event again: already handled, coalesced; nothing more.
    for _ in 0..2 {
        assert_eq!(
            f.fire(&trigger, "event-1", &mut tx).outcome,
            DeliveryOutcome::AlreadyHandled
        );
    }
    assert_eq!(
        f.outcomes(&trigger, &tx),
        [
            (DeliveryOutcome::DryRun, 1),
            (DeliveryOutcome::AlreadyHandled, 2)
        ]
    );
    // Nothing was written anywhere.
    assert!(f.goals().is_empty());
    assert!(f.host.launches().is_empty());
    assert!(
        f.m.runtime
            .permission_list(&f.owner(), &mut tx)
            .unwrap()
            .requests
            .is_empty()
    );
    assert!(
        TemplateApplyStore::new(&f.m.home, &f.owner())
            .unwrap()
            .applies()
            .unwrap()
            .is_empty()
    );
    assert!(f.m.runtime.policy(&f.owner()).unwrap().policy.is_none());
    assert_eq!(
        f.m.last_audit(),
        (
            "trigger_delivery".into(),
            AuditOutcome::Succeeded,
            Some("already_handled".into())
        )
    );
}

/// On: a delivery applies the template under the key's operation id as the trigger's
/// system actor. A second delivery while that run's goal is open is skipped (and
/// consecutive skips coalesce); with `queue_latest` the latest is kept and runs once
/// the goal closes. A redelivery is the same apply: no second goal.
#[test]
fn hs2_kxka7x_overlap_skips_by_default_and_queue_latest_runs_after_the_run_ends() {
    let clock = At(AtomicU64::new(T0));
    let mut rng = Rng(400);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[TargetAction::TicketsWrite], &mut tx);
    let template = f.template("Repair CI at {{commit}}", &mut tx);
    let trigger = f.set(template_trigger(&template, "skip"), &mut tx);
    f.m.runtime
        .trigger_on(
            &f.owner(),
            &Fixture::reference(&trigger),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let first = f.fire(&trigger, "event-1", &mut tx);
    assert_eq!(first.outcome, DeliveryOutcome::Applied);
    let operation = first.operation_id.clone().unwrap();
    assert!(operation.starts_with("trg-"));
    let apply = TemplateApplyStore::new(&f.m.home, &f.owner())
        .unwrap()
        .find(&operation)
        .unwrap()
        .unwrap();
    assert_eq!(apply.state, ApplyState::Planning);
    assert_eq!(apply.actor_role.as_deref(), Some("system"));
    assert_eq!(
        apply.actor_id,
        Some(format!("trigger-{}", trigger.trigger.trigger_id))
    );
    assert_eq!(f.goals().len(), 1);
    assert!(f.show(&trigger, &tx).running);
    // While the goal is open: skipped, coalesced into one entry.
    assert_eq!(
        f.fire(&trigger, "event-2", &mut tx).outcome,
        DeliveryOutcome::SkippedOverlap
    );
    assert_eq!(
        f.fire(&trigger, "event-3", &mut tx).outcome,
        DeliveryOutcome::SkippedOverlap
    );
    // The first event again: the same apply, nothing new.
    assert_eq!(
        f.fire(&trigger, "event-1", &mut tx).outcome,
        DeliveryOutcome::AlreadyHandled
    );
    assert_eq!(
        f.outcomes(&trigger, &tx),
        [
            (DeliveryOutcome::Applied, 1),
            (DeliveryOutcome::SkippedOverlap, 2),
            (DeliveryOutcome::AlreadyHandled, 1)
        ]
    );
    assert_eq!(f.goals().len(), 1);
    assert_eq!(f.host.launches().len(), 1);

    // Run the latest instead: kept while the goal is open, replaced by a newer one,
    // and run once the goal closes.
    let mut latest = template_trigger(&template, "queue_latest");
    latest["trigger_id"] = json!(trigger.trigger.trigger_id);
    f.set(latest, &mut tx);
    f.m.runtime
        .trigger_on(
            &f.owner(),
            &Fixture::reference(&trigger),
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    // Turning On again re-armed it; the run it started earlier still counts.
    let shown = f.show(&trigger, &tx);
    assert!(shown.running);
    assert_eq!(
        f.fire(&trigger, "event-4", &mut tx).outcome,
        DeliveryOutcome::Queued
    );
    assert_eq!(
        f.fire(&trigger, "event-5", &mut tx).outcome,
        DeliveryOutcome::Queued
    );
    assert_eq!(
        f.show(&trigger, &tx).trigger.queued.unwrap().key,
        "manual:event-5"
    );
    assert!(
        f.m.runtime
            .trigger_pass(&f.owner(), &mut tx)
            .unwrap()
            .deliveries
            .is_empty()
    );
    let goal = f.goals().pop().unwrap();
    ops::update(
        &f.m.owner_store,
        &goal.id,
        NOW.into(),
        TicketPatch {
            status: Some(hotsheet_model::Status::Completed),
            ..Default::default()
        },
    )
    .unwrap();
    f.host.crash(&f.host.launches()[0].session_id);
    let pass = f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert_eq!(pass.deliveries.len(), 1);
    assert_eq!(pass.deliveries[0].delivery_key, "manual:event-5");
    assert_eq!(pass.deliveries[0].outcome, DeliveryOutcome::Applied);
    assert!(f.show(&trigger, &tx).trigger.queued.is_none());
    assert_eq!(f.goals().len(), 2);
    // Done: a further pass runs nothing more.
    assert!(
        f.m.runtime
            .trigger_pass(&f.owner(), &mut tx)
            .unwrap()
            .deliveries
            .is_empty()
    );
}

/// An AI session may turn a trigger On only through the permission card: it asks for
/// what the trigger's deliveries do and waits; once a person allows it, the pass turns
/// it On (as reviewed by the AI session after the person's answer). A Deny leaves it
/// as it was.
#[test]
fn hs2_kxka7x_an_ai_session_turns_a_trigger_on_through_the_permission_card() {
    let clock = At(AtomicU64::new(T0));
    let mut rng = Rng(500);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[], &mut tx);
    let template = f.template("Repair CI at {{commit}}", &mut tx);
    let trigger = f.set(template_trigger(&template, "skip"), &mut tx);
    let review =
        f.m.runtime
            .trigger_review(
                &f.owner(),
                &serde_json::from_value(json!({"trigger_id": trigger.trigger.trigger_id})).unwrap(),
                Some(&worker()),
                &tx,
            )
            .unwrap();
    assert_eq!(review.turn_on, AccessNeed::Asks);
    let asked = f.m.runtime.trigger_on(
        &f.owner(),
        &Fixture::reference(&trigger),
        Some(&worker()),
        &mut tx,
    );
    let Err(OrchestrationError::PermissionRequired { request_id }) = asked else {
        panic!("expected a permission request, got {asked:?}");
    };
    let shown = f.show(&trigger, &tx);
    assert_eq!(shown.trigger.state, TriggerState::DryRun);
    assert_eq!(
        shown.trigger.pending_on.clone().unwrap().requests,
        std::slice::from_ref(&request_id)
    );
    // Unanswered: the pass changes nothing.
    f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert_eq!(f.show(&trigger, &tx).trigger.state, TriggerState::DryRun);
    f.m.runtime
        .permission_answer(
            &f.owner(),
            &request_id,
            PermissionAnswer::AllowOnce,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    let on = f.show(&trigger, &tx);
    assert_eq!(on.trigger.state, TriggerState::On);
    assert!(on.trigger.pending_on.is_none());
    let review = on.trigger.review.unwrap();
    assert_eq!(
        (review.role.as_str(), review.actor_id.as_str()),
        ("ai", "worker-1")
    );
    assert_eq!(review.permission_requests, [request_id]);

    // Denied: it stays as it was and asks no more.
    f.m.runtime
        .trigger_state(
            &f.owner(),
            &Fixture::reference(&trigger),
            TriggerOp::DryRun,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let Err(OrchestrationError::PermissionRequired { request_id }) = f.m.runtime.trigger_on(
        &f.owner(),
        &Fixture::reference(&trigger),
        Some(&worker()),
        &mut tx,
    ) else {
        panic!("expected a second permission request");
    };
    f.m.runtime
        .permission_answer(
            &f.owner(),
            &request_id,
            PermissionAnswer::Deny,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    let denied = f.show(&trigger, &tx);
    assert_eq!(denied.trigger.state, TriggerState::DryRun);
    assert!(denied.trigger.pending_on.is_none());
    f.m.runtime.trigger_pass(&f.owner(), &mut tx).unwrap();
    assert_eq!(
        f.m.runtime
            .permission_list(&f.owner(), &mut tx)
            .unwrap()
            .requests
            .len(),
        2,
        "no new request after the Deny"
    );
}

/// A run's CI gate failing is one delivery per failure, read from the run event
/// journal after the trigger started firing; a replayed pass delivers nothing more.
#[test]
fn hs2_kxka7x_a_failed_run_gate_fires_once_per_failure() {
    let clock = At(AtomicU64::new(T0));
    let mut rng = Rng(600);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let (mut m, run) = with_run(&[TargetAction::GatesObserve], &mut tx);
    let ci = Scripted::new(CI, GateKind::Ci);
    m.runtime = m.runtime.clone().with_gate_provider(ci.clone());
    let f = Fixture::from(m, &[], &mut tx);
    let owner = f.owner();
    let gate =
        f.m.runtime
            .gate_link(&owner, &run, link(&sha('a'), CI), Some(&worker()), &mut tx)
            .unwrap()
            .gate;
    let trigger = f.set(
        json!({"name": "Note failed gates", "source": {"kind": "gate_failed"},
            "action": {"placement": "pause"}}),
        &mut tx,
    );
    // Nothing failed yet.
    assert!(
        f.m.runtime
            .trigger_pass(&owner, &mut tx)
            .unwrap()
            .deliveries
            .is_empty()
    );
    ci.set(
        &sha('a'),
        verdict(
            CI,
            GateState::Failed,
            7,
            &sha('a'),
            hotsheet_ticketing::orchestration::Coverage::Exact,
        ),
    );
    f.m.runtime
        .gate_sync(
            &owner,
            &run,
            &SyncGates::default(),
            Some(&worker()),
            &mut tx,
        )
        .unwrap();
    assert_eq!(
        f.m.runtime.gate(&owner, &gate.gate_id).unwrap().state,
        GateState::Failed
    );
    let pass = f.m.runtime.trigger_pass(&owner, &mut tx).unwrap();
    assert_eq!(pass.deliveries.len(), 1, "{:?}", pass.deliveries);
    assert_eq!(pass.deliveries[0].outcome, DeliveryOutcome::DryRun);
    assert!(
        pass.deliveries[0]
            .delivery_key
            .starts_with(&format!("gate:{run}:{}:", gate.gate_id))
    );
    let entry = f.show(&trigger, &tx).trigger.history[0].clone();
    assert_eq!(entry.evidence["commit"], sha('a'));
    assert_eq!(entry.evidence["gate"], gate.gate_id);
    // Replayed passes deliver nothing more.
    for _ in 0..3 {
        assert!(
            f.m.runtime
                .trigger_pass(&owner, &mut tx)
                .unwrap()
                .deliveries
                .is_empty()
        );
    }
    assert_eq!(f.show(&trigger, &tx).trigger.history.len(), 1);
}

fn git(root: &std::path::Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::command_in(root)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

/// A branch watch over the real GitHub Actions adapter and Job store (a test judges
/// the watch Jobs as the Job monitor would): the first step learns the branch's head
/// and delivers nothing; a red run is one `ci_failed` delivery; a newer commit the
/// provider built is one `commit_landed` delivery; the watch then follows that commit.
#[test]
fn hs2_kxka7x_a_branch_watch_delivers_red_ci_and_landed_commits_once() {
    let clock = At(AtomicU64::new(T0));
    let mut rng = Rng(700);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[TargetAction::GatesObserve], &mut tx);
    let owner = f.owner();
    let root = std::path::PathBuf::from(&f.m.target.root);
    git(&root, &["init", "-q", "-b", "main"]);
    git(
        &root,
        &[
            "remote",
            "add",
            "origin",
            "https://github.com/example/widgets.git",
        ],
    );
    let head = sha('a');
    git(
        &root,
        &["update-ref", "refs/remotes/origin/main", &{
            // A real commit object is needed for rev-parse to resolve.
            git(
                &root,
                &[
                    "-c",
                    "user.email=t@example.invalid",
                    "-c",
                    "user.name=t",
                    "commit",
                    "-q",
                    "--allow-empty",
                    "-m",
                    "base",
                ],
            );
            git(&root, &["rev-parse", "HEAD"])
        }],
    );
    let base = git(&root, &["rev-parse", "refs/remotes/origin/main"]);
    assert_ne!(base, head);
    let watch = json!({"repository": "example/widgets", "workflow": "ci.yml", "branch": "main"});
    let red = f.set(
        json!({"name": "Repair API main CI",
            "source": {"kind": "branch_ci_failed", "target_id": id(1), "watch": watch},
            "action": {"placement": "pause"}}),
        &mut tx,
    );
    let landed = f.set(
        json!({"name": "Validate merged commits",
            "source": {"kind": "commit_landed", "target_id": id(1), "watch": watch},
            "action": {"placement": "start"}}),
        &mut tx,
    );
    let mut red_state = BranchFollowState::default();
    let mut landed_state = BranchFollowState::default();
    let step = |trigger: &TriggerView, state: &mut BranchFollowState, tx: &mut Transaction<'_>| {
        f.m.runtime
            .trigger_branch_step(&owner, &trigger.trigger.trigger_id, state, 0, tx)
    };
    // Seed: the head, nothing delivered.
    for (trigger, state) in [(&red, &mut red_state), (&landed, &mut landed_state)] {
        let first = step(trigger, state, &mut tx);
        assert!(
            first.active && !first.failed && first.deliveries.is_empty(),
            "{first:?}"
        );
        assert_eq!(
            f.show(trigger, &tx).trigger.cursor.branch_commit.as_deref(),
            Some(base.as_str())
        );
        // Start the watch Job of the head.
        let started = step(trigger, state, &mut tx);
        assert!(
            started.deliveries.is_empty() && !started.failed,
            "{started:?}"
        );
    }
    let jobs = JobStore::at(&f.m.home);
    let job = |commit: &str| {
        jobs.list(&f.m.target.id)
            .unwrap()
            .jobs
            .into_iter()
            .rfind(|job| job.title.contains(&commit[..7]))
            .unwrap()
    };
    // One watch Job for the head, shared by both triggers.
    assert_eq!(jobs.list(&f.m.target.id).unwrap().jobs.len(), 1);
    // A newer commit's run fails: one ci_failed for the red trigger, one landed commit
    // for the other.
    let newer = sha('b');
    judge(
        &f.m.home,
        &f.m.target.id,
        &job(&base).id,
        "red",
        &newer,
        9182,
    );
    let red_step = step(&red, &mut red_state, &mut tx);
    assert_eq!(red_step.deliveries.len(), 1, "{red_step:?}");
    assert_eq!(
        red_step.deliveries[0].delivery_key,
        "ci:github-actions:9182:1"
    );
    assert_eq!(red_step.deliveries[0].outcome, DeliveryOutcome::DryRun);
    let landed_step = step(&landed, &mut landed_state, &mut tx);
    assert_eq!(landed_step.deliveries.len(), 1, "{landed_step:?}");
    assert_eq!(
        landed_step.deliveries[0].delivery_key,
        format!("commit:main:{newer}")
    );
    // The watches moved to the newer commit; a further step starts its watch and
    // delivers nothing again.
    for (trigger, state) in [(&red, &mut red_state), (&landed, &mut landed_state)] {
        assert_eq!(
            f.show(trigger, &tx).trigger.cursor.branch_commit.as_deref(),
            Some(newer.as_str())
        );
        let again = step(trigger, state, &mut tx);
        assert!(again.deliveries.is_empty(), "{again:?}");
    }
    let entry = f.show(&red, &tx).trigger.history[0].clone();
    assert_eq!(entry.evidence["commit"], newer);
    assert_eq!(entry.evidence["ci_run"], "9182.1");
    // A paused trigger's watch ends.
    f.m.runtime
        .trigger_state(
            &owner,
            &Fixture::reference(&red),
            TriggerOp::Pause,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    assert!(!step(&red, &mut red_state, &mut tx).active);
    // A repository the project's checkout does not name is refused there: a warning,
    // announced once, and no watch Job.
    let stranger = f.set(
        json!({"name": "Elsewhere",
            "source": {"kind": "branch_ci_failed", "target_id": id(1),
                "watch": {"repository": "intruder/elsewhere", "workflow": "ci.yml", "branch": "main"}},
            "action": {"placement": "pause"}}),
        &mut tx,
    );
    let mut state = BranchFollowState::default();
    step(&stranger, &mut state, &mut tx);
    for _ in 0..3 {
        assert!(step(&stranger, &mut state, &mut tx).failed);
    }
    let warned = f.show(&stranger, &tx).trigger.warning.unwrap();
    assert_eq!(warned.code, "gate_repository_mismatch");
    assert_eq!(
        f.notices
            .titles()
            .iter()
            .filter(|title| title.contains("Elsewhere"))
            .count(),
        1
    );
}

/// A ticket opened in the project is one delivery when it is open, carries the
/// trigger's tag and was created after the trigger started firing; an edit of it later
/// records nothing; a ticket without the tag, or one closed, fires nothing.
#[test]
fn hs2_kxka7x_a_ticket_opened_in_a_project_fires_once() {
    let clock = At(AtomicU64::new(1));
    let mut rng = Rng(800);
    let mut tx = Transaction {
        clock: &clock,
        rng: &mut rng,
    };
    let f = Fixture::new(&[], &mut tx);
    let owner = f.owner();
    let trigger = f.set(
        json!({"name": "Triage new bugs",
            "source": {"kind": "ticket_opened", "target_id": id(1), "tag": "triage"},
            "action": {"placement": "start"}}),
        &mut tx,
    );
    assert_eq!(f.m.runtime.trigger_targets(&owner), [id(1)]);
    let reader = MutationActor::parse(
        "system",
        Some(format!("trigger-{}", trigger.trigger.trigger_id)),
    )
    .unwrap();
    let authorized =
        f.m.runtime
            .authorize(
                &OrchestrationRuntime::target_ref(&owner, &id(1)).unwrap(),
                Some(&reader),
                ActionRequest::new(TargetAction::TicketsRead),
                &mut tx,
            )
            .unwrap();
    let create = |title: &str, tags: &[&str]| {
        ops::create(
            &f.m.target_store,
            ulid::Ulid::new(),
            "TGT",
            NOW.into(),
            NewTicket {
                title: title.into(),
                tags: tags.iter().map(|tag| (*tag).to_owned()).collect(),
                ..Default::default()
            },
        )
        .unwrap()
    };
    let tagged = create("Crash on start", &["triage"]);
    let plain = create("Typo", &[]);
    let event = |ticket: &hotsheet_model::Ticket| TargetTicketEvent {
        cursor: 1,
        connection_id: f.m.target_connection(),
        native_id: ticket.id.to_string(),
    };
    let events = [event(&tagged), event(&plain)];
    let fired =
        f.m.runtime
            .trigger_ticket_events((&owner, &id(1)), Some(&events), &authorized, &mut tx);
    assert_eq!(fired.len(), 1, "{fired:?}");
    assert_eq!(
        fired[0].delivery_key,
        format!("ticket:{}:{}", f.m.target_connection(), tagged.id)
    );
    // Edited later (the same ticket again): nothing more is recorded.
    for _ in 0..2 {
        assert!(
            f.m.runtime
                .trigger_ticket_events((&owner, &id(1)), Some(&events[..1]), &authorized, &mut tx)
                .is_empty()
        );
    }
    // A reconnect lists the open tickets: nothing new either.
    assert!(
        f.m.runtime
            .trigger_ticket_events((&owner, &id(1)), None, &authorized, &mut tx)
            .is_empty()
    );
    assert_eq!(f.show(&trigger, &tx).trigger.history.len(), 1);
    // A tagged ticket created before the trigger started firing is not new.
    clock.0.store(u64::MAX / 4, Ordering::SeqCst);
    f.m.runtime
        .trigger_state(
            &owner,
            &Fixture::reference(&trigger),
            TriggerOp::Pause,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    f.m.runtime
        .trigger_state(
            &owner,
            &Fixture::reference(&trigger),
            TriggerOp::Resume,
            Some(&human()),
            &mut tx,
        )
        .unwrap();
    let old = create("Old crash", &["triage"]);
    assert!(
        f.m.runtime
            .trigger_ticket_events((&owner, &id(1)), Some(&[event(&old)]), &authorized, &mut tx)
            .is_empty()
    );
}
