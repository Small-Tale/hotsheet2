//! HS2-190RDX: Update Plan. The three-way merge a swap applies (table, conflicts,
//! invalid plans, order independence), the one-update-at-a-time decision, the draft
//! registry that never touches the live plan or its reads, and the locked swap.
//! Synthetic homes, ids and tickets only.

use std::path::{Path, PathBuf};

use super::*;

struct Clock(std::cell::Cell<u64>);
impl crate::Clock for Clock {
    fn now_ms(&self) -> u64 {
        self.0.get()
    }
}
struct Rng(u128);
impl crate::Rng for Rng {
    fn next_u128(&mut self) -> u128 {
        self.0 += 1;
        self.0
    }
}

const OWNER: &str = "launch-0123456789ab";

fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn ticket(native: &str) -> RunTarget {
    RunTarget {
        target_id: id(1),
        connection_id: "tickets".into(),
        native_id: native.into(),
    }
}
fn goal() -> RunTicket {
    RunTicket {
        connection_id: "plan".into(),
        native_id: id(900),
    }
}
fn step(n: u128, native: &str, revision: u32) -> SharedStep {
    SharedStep {
        requirements: Default::default(),
        step_id: id(n),
        ticket: goal(),
        target: ticket(native),
        prerequisites: Vec::new(),
        resources: Vec::new(),
        tool: None,
        model: None,
        revision,
        created_at_ms: 1,
        updated_at_ms: u64::from(revision),
        updated_by: None,
    }
}
fn ids(steps: &[SharedStep]) -> Vec<String> {
    steps.iter().map(|step| step.step_id.clone()).collect()
}

#[test]
fn hs2_190rdx_the_merge_keeps_live_changes_takes_draft_changes_and_refuses_both() {
    let (a, b, c) = (step(1, "a", 1), step(2, "b", 1), step(3, "c", 1));
    let base = vec![a.clone(), b.clone(), c.clone()];
    // Nothing changed anywhere: everything is kept.
    let (merged, swap) = merge_update(&base, &base, &base).unwrap();
    assert_eq!(merged, base);
    assert_eq!(swap.kept, ids(&base));
    assert!(swap.added.is_empty() && swap.changed.is_empty() && swap.removed.is_empty());

    // The draft changes b, removes c and adds d; the live plan is as it was.
    let b2 = step(2, "b", 2);
    let d = step(4, "d", 1);
    let draft = vec![a.clone(), b2.clone(), d.clone()];
    let (merged, swap) = merge_update(&base, &base, &draft).unwrap();
    assert_eq!(merged, draft);
    assert_eq!(swap.kept, vec![id(1)]);
    assert_eq!(swap.changed, vec![id(2)]);
    assert_eq!(swap.added, vec![id(4)]);
    assert_eq!(swap.removed, vec![id(3)]);
    assert_eq!(
        swap.withdrawn().cloned().collect::<Vec<_>>(),
        vec![id(2), id(3)]
    );

    // A step the draft left alone takes the live side: a person's change meanwhile, a
    // removal meanwhile, and a step added meanwhile all survive the swap.
    let a2 = step(1, "a", 2);
    let e = step(5, "e", 1);
    let live = vec![a2.clone(), b.clone(), e.clone()];
    let (merged, swap) = merge_update(&base, &live, &draft).unwrap();
    assert_eq!(ids(&merged), vec![id(1), id(2), id(4), id(5)]);
    assert_eq!(merged[0], a2);
    assert_eq!(merged[1], b2);
    assert_eq!(swap.kept, vec![id(1), id(5)]);
    assert!(swap.removed.is_empty(), "c was already gone");

    // Both sides made the same change: no conflict.
    let live = vec![a.clone(), b2.clone(), c.clone()];
    assert!(merge_update(&base, &live, &draft).is_ok());

    // Both changed b differently, or one changed what the other removed: refused,
    // naming the step, never guessed.
    for live in [
        vec![a.clone(), step(2, "b", 3), c.clone()],
        vec![a.clone(), c.clone()],
    ] {
        let refusal = merge_update(&base, &live, &draft).unwrap_err();
        assert_eq!(refusal.reason, plan_update_reason::CONFLICT);
        assert_eq!(refusal.conflicts, vec![id(2)]);
    }
    let refusal =
        merge_update(&base, &[a.clone(), b.clone(), step(3, "c", 2)], &draft).unwrap_err();
    assert_eq!(refusal.conflicts, vec![id(3)]);
}

#[test]
fn hs2_190rdx_a_merged_plan_with_two_steps_for_one_ticket_is_invalid() {
    let base = vec![step(1, "a", 1)];
    // The planner and a person each planned ticket x under a new step id.
    let live = vec![step(1, "a", 1), step(2, "x", 1)];
    let draft = vec![step(1, "a", 1), step(3, "x", 1)];
    let refusal = merge_update(&base, &live, &draft).unwrap_err();
    assert_eq!(refusal.reason, plan_update_reason::INVALID);
    assert_eq!(refusal.conflicts, vec![id(2), id(3)]);
}

#[test]
fn hs2_190rdx_the_merge_is_the_same_in_any_input_order() {
    let base = vec![step(1, "a", 1), step(2, "b", 1), step(3, "c", 1)];
    let live = vec![
        step(3, "c", 1),
        step(2, "b", 1),
        step(1, "a", 2),
        step(5, "e", 1),
    ];
    let draft = vec![step(4, "d", 1), step(2, "b", 2), step(1, "a", 1)];
    let expected = merge_update(&base, &live, &draft).unwrap();
    let mut reversed = (base.clone(), live.clone(), draft.clone());
    reversed.0.reverse();
    reversed.1.reverse();
    reversed.2.reverse();
    assert_eq!(
        merge_update(&reversed.0, &reversed.1, &reversed.2).unwrap(),
        expected
    );
    // Empty on every side, and an empty draft that removes everything.
    assert_eq!(merge_update(&[], &[], &[]).unwrap().0, Vec::new());
    let (merged, swap) = merge_update(&base, &base, &[]).unwrap();
    assert!(merged.is_empty());
    assert_eq!(swap.removed, ids(&base));
}

#[test]
fn hs2_190rdx_only_one_update_drafts_at_a_time() {
    let update = |state: PlanUpdateState| PlanUpdate {
        update_id: id(10),
        state,
        planner: None,
        base_revision: None,
        base: Vec::new(),
        created_at_ms: 1,
        ended_at_ms: None,
        requested_by: None,
        reason: None,
        conflicts: Vec::new(),
        unwithdrawn: Vec::new(),
        swap: None,
        applied_revision: None,
    };
    let fresh = || id(20);
    assert_eq!(
        decide_update(None, None, fresh),
        UpdateDecision::Open(id(20))
    );
    assert_eq!(
        decide_update(None, Some(&id(30)), fresh),
        UpdateDecision::Open(id(30))
    );
    // A second request, under any id, answers with the drafting one.
    let drafting = update(PlanUpdateState::Drafting);
    for requested in [None, Some(id(10)), Some(id(30))] {
        assert_eq!(
            decide_update(Some(&drafting), requested.as_deref(), fresh),
            UpdateDecision::Existing(id(10))
        );
    }
    // A finished update is answered for its own id, and a new request opens another.
    for state in [
        PlanUpdateState::Applied,
        PlanUpdateState::Failed,
        PlanUpdateState::Cancelled,
    ] {
        let finished = update(state);
        assert!(!state.is_open());
        assert_eq!(
            decide_update(Some(&finished), Some(&id(10)), fresh),
            UpdateDecision::Existing(id(10))
        );
        assert_eq!(
            decide_update(Some(&finished), None, fresh),
            UpdateDecision::Open(id(20))
        );
    }
    let codes: Vec<&str> = PlanUpdateState::ALL.iter().map(|s| s.code()).collect();
    assert_eq!(codes, ["drafting", "applied", "failed", "cancelled"]);
}

struct Machine {
    _home: tempfile::TempDir,
    _root: tempfile::TempDir,
    home: PathBuf,
    plan: PlanRegistry,
    clock: Clock,
    rng: Rng,
}

impl Machine {
    fn new() -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = tempfile::tempdir().unwrap();
        let checkout = root.path().to_path_buf();
        Self {
            home: home.path().to_path_buf(),
            plan: PlanRegistry::new(home.path(), move |_| Ok(checkout.clone())),
            _home: home,
            _root: root,
            clock: Clock(std::cell::Cell::new(1_000)),
            rng: Rng(100),
        }
    }
    fn tx(&mut self) -> Transaction<'_> {
        self.clock.0.set(self.clock.0.get() + 1);
        Transaction {
            clock: &self.clock,
            rng: &mut self.rng,
        }
    }
    fn define(&mut self, plan: &PlanRegistry, native: &str, tool: Option<&str>) -> PlanStep {
        let target = ticket(native);
        plan.define_observed(
            OWNER,
            NewPlanStep {
                requirements: Default::default(),
                ticket: goal(),
                target: target.clone(),
                prerequisites: Vec::new(),
                resources: Vec::new(),
                tool: tool.map(Into::into),
                model: None,
            },
            None,
            vec![(target, read(native))],
            Some("planner-1".into()),
            &mut self.tx(),
        )
        .unwrap()
        .0
    }
    fn natives(plan: &PlanRegistry) -> Vec<String> {
        plan.records(OWNER)
            .unwrap()
            .into_iter()
            .map(|record| record.step.target.native_id)
            .collect()
    }
}

fn read(status: &str) -> TicketUpdate {
    TicketUpdate {
        observed: Some(TicketSeen {
            at_ms: 5,
            exists: true,
            status: status.into(),
            priority: None,
            up_next: None,
            created_at_ms: None,
            opted_out: None,
            slug: None,
        }),
        unreachable: Some(None),
    }
}

fn checked_in(root: &Path) -> Option<SharedPlan> {
    SharedPlanStore::new(root).read().unwrap()
}

#[test]
fn hs2_190rdx_a_draft_never_touches_the_live_plan_until_it_is_applied() {
    let mut machine = Machine::new();
    let live = machine.plan.clone();
    let a = machine.define(&live, "a", None);
    let b = machine.define(&live, "b", None);
    let c = machine.define(&live, "c", None);
    let (base, revision) = live.checked_in_steps(OWNER).unwrap();
    assert_eq!(
        ids(&base),
        vec![a.step_id.clone(), b.step_id.clone(), c.step_id.clone()]
    );
    let store = PlanUpdateStore::new(&machine.home, OWNER).unwrap();
    store.seed_draft(base.clone(), &mut machine.tx()).unwrap();
    let draft = live.for_update(&machine.home, OWNER).unwrap();
    assert_eq!(Machine::natives(&draft), ["a", "b", "c"]);

    // The planner changes b, removes c and adds d in the draft only.
    machine.define(&draft, "b", Some("codex"));
    draft.remove(OWNER, &c.step_id, &mut machine.tx()).unwrap();
    let d = machine.define(&draft, "d", None);
    assert_eq!(Machine::natives(&draft), ["a", "b", "d"]);
    // The live plan, its revision and its reads are as they were.
    assert_eq!(Machine::natives(&live), ["a", "b", "c"]);
    assert_eq!(live.revision(OWNER).unwrap(), revision);
    assert_eq!(live.tickets(OWNER).unwrap().len(), 3);
    assert!(live.find_target(OWNER, &ticket("d")).unwrap().is_none());

    // Meanwhile a person changes a on the live plan.
    machine.define(&live, "a", Some("claude"));

    let observed = draft.tickets(OWNER).unwrap();
    let (swap, applied) = live
        .apply_update(
            OWNER,
            &base,
            &store.draft_steps().unwrap(),
            observed,
            &mut machine.tx(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(swap.kept, vec![a.step_id.clone()]);
    assert_eq!(swap.changed, vec![b.step_id.clone()]);
    assert_eq!(swap.removed, vec![c.step_id.clone()]);
    assert_eq!(swap.added, vec![d.step_id.clone()]);
    assert_eq!(Machine::natives(&live), ["a", "b", "d"]);
    let records = live.records(OWNER).unwrap();
    assert_eq!(
        records[0].step.tool.as_deref(),
        Some("claude"),
        "the live change survives"
    );
    assert_eq!(
        records[1].step.tool.as_deref(),
        Some("codex"),
        "the draft change lands"
    );
    assert_eq!(
        applied,
        checked_in(machine._root.path()).map(|doc| doc.revision)
    );
    // The new step starts from the planner's read; c's read is forgotten.
    let natives: Vec<String> = live
        .tickets(OWNER)
        .unwrap()
        .into_iter()
        .map(|record| record.target.native_id)
        .collect();
    assert_eq!(natives, ["a", "b", "d"]);

    // Discarding the draft forgets it, and a new seed starts empty-then-refilled.
    store.discard_draft().unwrap();
    assert!(store.draft_steps().unwrap().is_empty());
    store.discard_draft().unwrap();
    store.seed_draft(Vec::new(), &mut machine.tx()).unwrap();
    assert!(Machine::natives(&draft).is_empty());
    machine.define(&draft, "z", None);
    assert_eq!(Machine::natives(&draft), ["z"]);
}

#[test]
fn hs2_190rdx_a_refused_swap_writes_nothing() {
    let mut machine = Machine::new();
    let live = machine.plan.clone();
    machine.define(&live, "a", None);
    let (base, revision) = live.checked_in_steps(OWNER).unwrap();
    let store = PlanUpdateStore::new(&machine.home, OWNER).unwrap();
    store.seed_draft(base.clone(), &mut machine.tx()).unwrap();
    let draft = live.for_update(&machine.home, OWNER).unwrap();
    machine.define(&draft, "a", Some("codex"));
    machine.define(&live, "a", Some("claude"));
    let revision_now = live.revision(OWNER).unwrap();
    assert_ne!(revision_now, revision);
    let refusal = live
        .apply_update(
            OWNER,
            &base,
            &store.draft_steps().unwrap(),
            Vec::new(),
            &mut machine.tx(),
        )
        .unwrap()
        .unwrap_err();
    assert_eq!(refusal.reason, plan_update_reason::CONFLICT);
    assert_eq!(live.revision(OWNER).unwrap(), revision_now);
    assert_eq!(
        live.records(OWNER).unwrap()[0].step.tool.as_deref(),
        Some("claude")
    );
}

#[test]
fn hs2_190rdx_the_update_record_round_trips_and_is_never_created_by_a_read() {
    let mut machine = Machine::new();
    let store = PlanUpdateStore::new(&machine.home, OWNER).unwrap();
    assert_eq!(store.latest().unwrap(), None);
    assert!(!machine.home.join(PLAN_UPDATE_DIR).exists());
    let update = PlanUpdate {
        update_id: id(10),
        state: PlanUpdateState::Drafting,
        planner: Some(id(11)),
        base_revision: None,
        base: vec![step(1, "a", 1)],
        created_at_ms: machine.tx().clock.now_ms(),
        ended_at_ms: None,
        requested_by: Some("person-1".into()),
        reason: None,
        conflicts: Vec::new(),
        unwithdrawn: Vec::new(),
        swap: None,
        applied_revision: None,
    };
    {
        let _lock = store.lock().unwrap();
        store.write(&update).unwrap();
    }
    assert_eq!(store.latest().unwrap(), Some(update));
    assert!(PlanUpdateStore::new(&machine.home, "../escape").is_err());
}
