//! HS2-YDY5HJ: the checked-in plan. Merge by step id (table and order), two machines
//! that share one checkout's plan but keep their own reads and levels, concurrent
//! edits, the step revision CAS, migration of a machine-local plan (idempotent,
//! refused on conflict, a person's discard), removal seen by the other machine,
//! damaged shared data and empty-then-refill. Synthetic homes, ids and tickets only.

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

/// The same orchestration project on two machines: one checkout directory (the
/// shared, checked-in files), two homes, and each machine's own checkout id.
const OWNER_A: &str = "launch-0123456789ab";
const OWNER_B: &str = "launch-ba9876543210";

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
fn requires(target: &RunTarget) -> Prerequisite {
    Prerequisite::Ticket {
        target_id: target.target_id.clone(),
        connection_id: target.connection_id.clone(),
        native_id: target.native_id.clone(),
        gates: GateRequirement::None,
        commit: None,
    }
}
fn new(target: &RunTarget, prerequisites: Vec<Prerequisite>) -> NewPlanStep {
    NewPlanStep {
        requirements: Default::default(),
        ticket: goal(),
        target: target.clone(),
        prerequisites,
        resources: Vec::new(),
        tool: None,
        model: None,
    }
}
fn read(status: &str, at_ms: u64) -> TicketUpdate {
    TicketUpdate {
        observed: Some(TicketSeen {
            at_ms,
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

struct Machine {
    _home: tempfile::TempDir,
    home: PathBuf,
    owner: &'static str,
    plan: PlanRegistry,
    clock: Clock,
    rng: Rng,
}

impl Machine {
    fn new(owner: &'static str, root: &Path, seed: u128) -> Self {
        let home = tempfile::tempdir().unwrap();
        let root = root.to_path_buf();
        Self {
            home: home.path().to_path_buf(),
            plan: PlanRegistry::new(home.path(), move |_| Ok(root.clone())),
            _home: home,
            owner,
            clock: Clock(std::cell::Cell::new(1_000)),
            rng: Rng(seed),
        }
    }
    fn tx(&mut self) -> Transaction<'_> {
        self.clock.0.set(self.clock.0.get() + 1);
        Transaction {
            clock: &self.clock,
            rng: &mut self.rng,
        }
    }
    fn define(
        &mut self,
        target: &RunTarget,
        prerequisites: Vec<Prerequisite>,
        expected: Option<u32>,
    ) -> Result<(PlanStep, bool), OrchestrationError> {
        let (owner, plan) = (self.owner, self.plan.clone());
        plan.define_observed(
            owner,
            new(target, prerequisites),
            expected,
            Vec::new(),
            Some("worker-1".into()),
            &mut self.tx(),
        )
    }
    fn remove(&mut self, step_id: &str) -> Result<PlanStep, OrchestrationError> {
        let (owner, plan) = (self.owner, self.plan.clone());
        plan.remove(owner, step_id, &mut self.tx())
    }
    fn migrate(&mut self, discard: bool) -> Result<PlanMigration, OrchestrationError> {
        let (owner, plan) = (self.owner, self.plan.clone());
        plan.migrate(owner, discard, &mut self.tx())
    }
    fn steps(&self) -> Vec<(String, String, u32)> {
        self.plan
            .records(self.owner)
            .unwrap()
            .into_iter()
            .map(|record| {
                assert_eq!(record.step.owner_checkout_id, self.owner);
                (
                    record.step.step_id,
                    record.step.target.native_id,
                    record.step.revision,
                )
            })
            .collect()
    }
    /// The levels this machine computes from its own reads alone.
    fn levels(&self) -> Vec<(String, ReadinessLevel)> {
        let steps: Vec<PlanStep> = self
            .plan
            .records(self.owner)
            .unwrap()
            .into_iter()
            .map(|record| record.step)
            .collect();
        let mut facts = PlanFacts::default();
        for record in self.plan.tickets(self.owner).unwrap() {
            facts
                .tickets
                .insert(ticket_key(&record.target), record.facts);
        }
        evaluate_plan(&steps, &facts)
            .into_iter()
            .map(|readiness| (readiness.step.target.native_id, readiness.level))
            .collect()
    }
}

fn shared(id_number: u128, native: &str, revision: u32) -> SharedStep {
    SharedStep {
        requirements: Default::default(),
        step_id: id(id_number),
        ticket: goal(),
        target: ticket(native),
        prerequisites: Vec::new(),
        resources: Vec::new(),
        tool: None,
        model: None,
        revision,
        created_at_ms: 1,
        updated_at_ms: 1,
        updated_by: None,
    }
}

#[test]
fn hs2_ydy5hj_steps_merge_by_id_and_a_conflict_is_refused_not_guessed() {
    let (a, b, c) = (shared(1, "a", 1), shared(2, "b", 1), shared(3, "c", 1));
    let ids = |steps: Vec<SharedStep>| {
        steps
            .into_iter()
            .map(|step| step.step_id)
            .collect::<Vec<_>>()
    };
    // Disjoint, identical, empty and repeated sides merge, in any order.
    for (ours, theirs) in [
        (vec![a.clone(), b.clone()], vec![c.clone()]),
        (vec![c.clone()], vec![b.clone(), a.clone()]),
        (
            vec![a.clone(), b.clone(), c.clone()],
            vec![a.clone(), c.clone()],
        ),
        (
            vec![a.clone(), a.clone()],
            vec![b.clone(), c.clone(), c.clone()],
        ),
    ] {
        assert_eq!(
            ids(merge_steps(&ours, &theirs).unwrap()),
            [id(1), id(2), id(3)]
        );
    }
    assert!(merge_steps(&[], &[]).unwrap().is_empty());
    // The same step changed on both sides, at the same or another revision.
    let mut revised = a.clone();
    revised.revision = 2;
    revised.prerequisites = vec![requires(&ticket("b"))];
    let mut retitled = a.clone();
    retitled.tool = Some("claude".into());
    for other in [revised, retitled] {
        assert_eq!(
            merge_steps(&[a.clone(), b.clone()], std::slice::from_ref(&other)),
            Err(vec![id(1)])
        );
        assert_eq!(
            merge_steps(&[other], std::slice::from_ref(&a)),
            Err(vec![id(1)])
        );
    }
    // One target ticket under two step ids.
    let twin = shared(4, "a", 1);
    assert_eq!(
        merge_steps(std::slice::from_ref(&a), std::slice::from_ref(&twin)),
        Err(vec![id(1), id(4)])
    );
    assert_eq!(merge_steps(&[twin, b], &[a]), Err(vec![id(1), id(4)]));
}

#[test]
fn hs2_ydy5hj_a_second_machine_shares_the_steps_and_computes_readiness_from_its_own_reads() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    let mut b = Machine::new(OWNER_B, root.path(), 200);
    let (first, second) = (ticket("first"), ticket("second"));
    // Nothing is checked in yet, and a read creates nothing on either side.
    assert!(b.plan.is_empty(OWNER_B).unwrap());
    assert_eq!(b.plan.revision(OWNER_B).unwrap(), None);
    assert!(!root.path().join(SHARED_PLAN_PATH).exists());

    let (step_one, _) = a.define(&first, Vec::new(), Some(0)).unwrap();
    let (step_two, _) = a.define(&second, vec![requires(&first)], None).unwrap();
    // The definitions are checked in beside the manifest; no read or level is.
    let checked_in = std::fs::read_to_string(root.path().join(SHARED_PLAN_PATH)).unwrap();
    for absent in [OWNER_A, "observed", "published", "facts"] {
        assert!(!checked_in.contains(absent), "{absent}");
    }
    let revision = a.plan.revision(OWNER_A).unwrap().unwrap();
    assert_eq!(
        b.plan.revision(OWNER_B).unwrap().as_deref(),
        Some(revision.as_str())
    );
    // Machine B sees the same steps as its own checkout's, with nothing published.
    assert_eq!(b.steps(), a.steps());
    assert!(
        b.plan
            .records(OWNER_B)
            .unwrap()
            .iter()
            .all(|record| record.published.is_none())
    );
    assert!(b.plan.tickets(OWNER_B).unwrap().is_empty());

    // Only A read the tickets: A's first step is confirmed, B's is not.
    a.plan
        .observe(OWNER_A, &first, read("not_started", 10))
        .unwrap();
    a.plan
        .observe(OWNER_A, &second, read("not_started", 10))
        .unwrap();
    assert_eq!(
        a.levels(),
        [
            ("first".into(), ReadinessLevel::Confirmed),
            ("second".into(), ReadinessLevel::Blocked)
        ]
    );
    assert!(
        b.levels()
            .iter()
            .all(|(_, level)| *level == ReadinessLevel::Stale)
    );
    assert!(
        !std::fs::read_to_string(b.home.join("orchestration-plan.json"))
            .unwrap_or_default()
            .contains("not_started")
    );
    // B's own read confirms it for B, and each machine publishes its own levels.
    b.plan
        .observe(OWNER_B, &first, read("started", 20))
        .unwrap();
    assert_eq!(b.levels()[0], ("first".into(), ReadinessLevel::Confirmed));
    let published = |m: &Machine, level| {
        m.plan
            .publish(m.owner, &[(step_one.step_id.clone(), level)], 30)
            .unwrap()
    };
    assert_eq!(published(&a, ReadinessLevel::Confirmed).len(), 1);
    assert_eq!(published(&b, ReadinessLevel::Stale).len(), 1);
    assert_eq!(published(&a, ReadinessLevel::Confirmed).len(), 0);
    let level_of = |m: &Machine| {
        m.plan.records(m.owner).unwrap()[0]
            .published
            .as_ref()
            .map(|published| published.level)
    };
    assert_eq!(level_of(&a), Some(ReadinessLevel::Confirmed));
    assert_eq!(level_of(&b), Some(ReadinessLevel::Stale));
    // A reading the other machine's checkout id never sees A's records.
    assert!(a.plan.tickets(OWNER_B).unwrap().is_empty());

    // B withdraws the second step: A no longer plans it, and the next write on A
    // forgets A's read of a ticket nothing names any more.
    b.remove(&step_two.step_id).unwrap();
    assert_eq!(a.steps(), [(step_one.step_id.clone(), "first".into(), 1)]);
    a.plan
        .observe(OWNER_A, &first, read("started", 40))
        .unwrap();
    assert!(
        a.plan
            .tickets(OWNER_A)
            .unwrap()
            .iter()
            .all(|record| record.target == first)
    );
    // Empty, then refill: a step planned again is a new step, on both machines.
    b.remove(&step_one.step_id).unwrap();
    assert!(a.plan.is_empty(OWNER_A).unwrap() && b.plan.is_empty(OWNER_B).unwrap());
    assert!(published(&a, ReadinessLevel::Confirmed).is_empty());
    let (again, created) = a.define(&first, Vec::new(), Some(0)).unwrap();
    assert!(created && again.step_id != step_one.step_id);
    assert_eq!(b.steps(), [(again.step_id, "first".into(), 1)]);
}

#[test]
fn hs2_ydy5hj_the_step_revision_refuses_a_change_made_since_it_was_read() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    let mut b = Machine::new(OWNER_B, root.path(), 200);
    let (first, other) = (ticket("first"), ticket("other"));
    // 0 means "nothing planned for this ticket yet".
    let (step, created) = a.define(&first, Vec::new(), Some(0)).unwrap();
    assert!(created && step.revision == 1);
    let refused = |result: Result<(PlanStep, bool), OrchestrationError>| {
        matches!(
            result,
            Err(OrchestrationError::Readiness(ReadinessError::PlanConflict))
        )
    };
    assert!(refused(b.define(&first, Vec::new(), Some(0))));
    // Both machines read revision 1; A changes it first, so B's change is refused
    // and the checked-in step is A's, untouched.
    let before = a.plan.revision(OWNER_A).unwrap();
    let (revised, changed) = a.define(&first, vec![requires(&other)], Some(1)).unwrap();
    assert!(changed && revised.revision == 2);
    let file = std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap();
    assert!(refused(b.define(&first, Vec::new(), Some(1))));
    assert_eq!(
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        file
    );
    assert_ne!(a.plan.revision(OWNER_A).unwrap(), before);
    // Repeating A's change, with or without the new revision, changes nothing.
    for expected in [Some(2), None] {
        let (same, changed) = a.define(&first, vec![requires(&other)], expected).unwrap();
        assert!(!changed && same == revised);
    }
    assert_eq!(
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        file
    );
    // B, having read revision 2, may change it.
    let (taken, changed) = b.define(&first, Vec::new(), Some(2)).unwrap();
    assert!(changed && taken.revision == 3);
    assert_eq!(a.steps(), [(step.step_id, "first".into(), 3)]);
}

#[test]
fn hs2_ydy5hj_concurrent_edits_of_different_steps_both_survive() {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().to_path_buf();
    let workers: Vec<_> = (0..4u128)
        .map(|worker| {
            let path = path.clone();
            std::thread::spawn(move || {
                let owner = if worker % 2 == 0 { OWNER_A } else { OWNER_B };
                let mut machine = Machine::new(owner, &path, 1_000 * (worker + 1));
                for n in 0..5 {
                    machine
                        .define(&ticket(&format!("w{worker}-{n}")), Vec::new(), Some(0))
                        .unwrap();
                }
                machine
            })
        })
        .collect();
    let machines: Vec<Machine> = workers
        .into_iter()
        .map(|worker| worker.join().unwrap())
        .collect();
    for machine in &machines {
        assert_eq!(machine.steps().len(), 20);
    }
    // Racing changes of one step from one revision: exactly one wins.
    let target = ticket("w0-0");
    let racers: Vec<_> = (0..4u128)
        .map(|n| {
            let path = path.clone();
            let target = target.clone();
            std::thread::spawn(move || {
                let mut machine = Machine::new(OWNER_A, &path, 50_000 * (n + 1));
                machine.define(
                    &target,
                    vec![requires(&ticket(&format!("dep-{n}")))],
                    Some(1),
                )
            })
        })
        .collect();
    let results: Vec<_> = racers
        .into_iter()
        .map(|racer| racer.join().unwrap())
        .collect();
    assert_eq!(results.iter().filter(|result| result.is_ok()).count(), 1);
    assert!(
        results
            .iter()
            .filter(|result| result.is_err())
            .all(|result| matches!(
                result,
                Err(OrchestrationError::Readiness(ReadinessError::PlanConflict))
            ))
    );
}

fn conflict<T>(result: Result<T, OrchestrationError>) -> bool {
    matches!(
        result,
        Err(OrchestrationError::Readiness(ReadinessError::PlanConflict))
    )
}

/// Writes a plan as this machine kept it before HS2-YDY5HJ.
fn legacy_plan(home: &Path, owner: &str, steps: &[(u128, &str, Option<ReadinessLevel>)]) {
    let records: Vec<serde_json::Value> = steps
        .iter()
        .map(|(number, native, level)| {
            let mut record = serde_json::json!({
                "step": shared(*number, native, 1).step(owner),
            });
            if let Some(level) = level {
                record["published"] = serde_json::json!({"level": level, "since_ms": 5, "seq": 2});
            }
            record
        })
        .collect();
    std::fs::write(
        home.join("orchestration-plan.json"),
        serde_json::to_vec(
            &serde_json::json!({"schema_version": 1, "steps": records, "tickets": []}),
        )
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn hs2_ydy5hj_a_machine_local_plan_moves_in_once_and_a_conflict_moves_nothing() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    legacy_plan(
        &a.home,
        OWNER_A,
        &[
            (1, "first", Some(ReadinessLevel::Confirmed)),
            (2, "second", None),
        ],
    );
    // Before it moves, the local definitions are read as the plan.
    assert_eq!(a.steps().len(), 2);
    assert!(!root.path().join(SHARED_PLAN_PATH).exists());
    let moved = a.migrate(false).unwrap();
    assert_eq!(moved.migrated, [id(1), id(2)]);
    assert!(moved.already_shared.is_empty() && moved.discarded.is_empty());
    assert_eq!(moved.revision, a.plan.revision(OWNER_A).unwrap());
    // The definitions left the machine-local file; the published level stayed.
    let local = std::fs::read_to_string(a.home.join("orchestration-plan.json")).unwrap();
    assert!(!local.contains("\"steps\"") && local.contains("confirmed"));
    assert_eq!(
        a.plan.records(OWNER_A).unwrap()[0]
            .published
            .as_ref()
            .map(|published| published.level),
        Some(ReadinessLevel::Confirmed)
    );
    // Repeating it changes nothing.
    let file = std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap();
    let again = a.migrate(false).unwrap();
    assert!(again.migrated.is_empty() && again.already_shared.is_empty());
    assert_eq!(again.revision, moved.revision);
    assert_eq!(
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        file
    );

    // Machine B kept the same step identically and one of its own: the first is
    // already shared, the other moves in; A sees B's.
    let mut b = Machine::new(OWNER_B, root.path(), 200);
    legacy_plan(&b.home, OWNER_B, &[(1, "first", None), (3, "third", None)]);
    let merged = b.migrate(false).unwrap();
    assert_eq!(
        (merged.migrated, merged.already_shared),
        (vec![id(3)], vec![id(1)])
    );
    assert_eq!(a.steps().len(), 3);

    // Machine C kept step 2 differently, and step 5 for a ticket step 1 works: the
    // reads refuse, a write refuses, and migrating refuses; nothing is written.
    let mut c = Machine::new(OWNER_B, root.path(), 300);
    let mut changed = shared(2, "second", 1);
    changed.tool = Some("codex".into());
    std::fs::write(
        c.home.join("orchestration-plan.json"),
        serde_json::to_vec(&serde_json::json!({"schema_version": 1, "tickets": [],
            "steps": [{"step": changed.step(OWNER_B)}, {"step": shared(5, "first", 1).step(OWNER_B)},
                      {"step": shared(6, "sixth", 1).step(OWNER_B)}]}))
        .unwrap(),
    )
    .unwrap();
    let (checked_in, local) = (
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        std::fs::read(c.home.join("orchestration-plan.json")).unwrap(),
    );
    assert!(conflict(c.plan.records(OWNER_B)));
    assert!(conflict(c.define(&ticket("seventh"), Vec::new(), None)));
    assert!(conflict(c.migrate(false)));
    assert_eq!(
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        checked_in
    );
    assert_eq!(
        std::fs::read(c.home.join("orchestration-plan.json")).unwrap(),
        local
    );
    // A person's discard drops exactly the conflicting local definitions.
    let resolved = c.migrate(true).unwrap();
    assert_eq!(resolved.discarded, [id(2), id(5)]);
    assert_eq!(resolved.migrated, [id(6)]);
    assert_eq!(c.steps().len(), 4);
    assert!(a.steps().iter().any(|(step, _, _)| *step == id(6)));
}

#[test]
fn hs2_ydy5hj_damaged_shared_data_is_an_error_and_never_overwritten() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    a.define(&ticket("first"), Vec::new(), None).unwrap();
    let path = root.path().join(SHARED_PLAN_PATH);
    let good = std::fs::read_to_string(&path).unwrap();
    let revision = a.plan.revision(OWNER_A).unwrap().unwrap();
    let twin = serde_json::to_string(&shared(7, "first", 1)).unwrap();
    for (bytes, unsupported) in [
        // A merge left its markers in the file.
        (
            format!("<<<<<<< ours\n{good}=======\n{good}>>>>>>> theirs\n"),
            false,
        ),
        (
            good.replace("\"schema_version\": 1", "\"schema_version\": 3"),
            true,
        ),
        // HS2-38MFXT: version 2 always carries a hold.
        (
            good.replace("\"schema_version\": 1", "\"schema_version\": 2"),
            false,
        ),
        (good.replace(&revision, "not-a-revision"), false),
        // Two steps for one ticket, as a careless text merge could leave.
        (
            good.replacen("\"steps\": [", &format!("\"steps\": [{twin},"), 1),
            false,
        ),
    ] {
        std::fs::write(&path, &bytes).unwrap();
        let read = a.plan.records(OWNER_A);
        if unsupported {
            assert!(matches!(
                read,
                Err(OrchestrationError::UnsupportedVersion { .. })
            ));
        } else {
            assert!(matches!(read, Err(OrchestrationError::Corrupt)), "{bytes}");
        }
        assert!(a.define(&ticket("second"), Vec::new(), None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }
}

/// A level publication or a ticket read racing a definition must not prune the reads
/// the definition stored against the steps as they were before it (found by
/// `hs2_j89jc7_http_mcp_and_cli_read_the_same_timeline_and_wait_by_version`).
#[test]
fn hs2_ydy5hj_a_racing_publish_never_prunes_a_new_steps_reads() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    let first = ticket("first");
    a.define(&first, Vec::new(), None).unwrap();
    for n in 0..40 {
        let target = ticket(&format!("racing-{n}"));
        let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let racer = {
            let (plan, stop, first) = (a.plan.clone(), stop.clone(), first.clone());
            std::thread::spawn(move || {
                while !stop.load(std::sync::atomic::Ordering::Relaxed) {
                    plan.publish(OWNER_A, &[], 1).unwrap();
                    plan.observe(OWNER_A, &first, read("not_started", 1))
                        .unwrap();
                }
            })
        };
        let (owner, plan) = (a.owner, a.plan.clone());
        plan.define_observed(
            owner,
            new(&target, vec![requires(&first)]),
            Some(0),
            vec![(target.clone(), read("not_started", 5))],
            Some("worker-1".into()),
            &mut a.tx(),
        )
        .unwrap();
        stop.store(true, std::sync::atomic::Ordering::Relaxed);
        racer.join().unwrap();
        assert!(
            a.plan
                .tickets(OWNER_A)
                .unwrap()
                .iter()
                .any(|record| record.target == target && record.facts.observed.is_some()),
            "round {n}"
        );
    }
}

/// HS2-F1BKQ0: the plan's state as a value where every read fails: ok, a local conflict a
/// person's discard resolves, a conflict it does not, damaged checked-in data and damaged
/// local data. It reads only, so the files are unchanged.
#[test]
fn hs2_f1bkq0_health_names_the_conflict_or_the_damaged_file_and_writes_nothing() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    // Empty, then a plan: ok both times.
    assert_eq!(a.plan.health(OWNER_A).unwrap().state, "ok");
    a.define(&ticket("first"), Vec::new(), None).unwrap();
    assert_eq!(
        a.plan.health(OWNER_A).unwrap(),
        PlanHealth {
            state: "ok".into(),
            ..PlanHealth::default()
        }
    );
    let first = a.steps()[0].0.clone();

    // B kept a different step 8 for the ticket A's step works, and an unrelated step 9.
    let b = Machine::new(OWNER_A, root.path(), 200);
    std::fs::write(
        b.home.join("orchestration-plan.json"),
        serde_json::to_vec(&serde_json::json!({"schema_version": 1, "tickets": [],
            "steps": [{"step": shared(8, "first", 1).step(OWNER_A)},
                      {"step": shared(9, "ninth", 1).step(OWNER_A)}]}))
        .unwrap(),
    )
    .unwrap();
    let before = (
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        std::fs::read(b.home.join("orchestration-plan.json")).unwrap(),
    );
    assert!(conflict(b.plan.records(OWNER_A)));
    let health = b.plan.health(OWNER_A).unwrap();
    assert_eq!(health.state, "conflict");
    assert_eq!(health.conflicts, {
        let mut ids = vec![first.clone(), id(8)];
        ids.sort();
        ids
    });
    assert_eq!(health.local, [id(8)]);
    assert!(health.discard_resolves);
    // Reading twice changes nothing and answers the same.
    assert_eq!(b.plan.health(OWNER_A).unwrap(), health);
    assert_eq!(
        std::fs::read(root.path().join(SHARED_PLAN_PATH)).unwrap(),
        before.0
    );
    assert_eq!(
        std::fs::read(b.home.join("orchestration-plan.json")).unwrap(),
        before.1
    );

    // Damaged checked-in data: merge markers, then a newer version.
    let path = root.path().join(SHARED_PLAN_PATH);
    let good = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        &path,
        format!("<<<<<<< ours\n{good}=======\n{good}>>>>>>> theirs\n"),
    )
    .unwrap();
    assert_eq!(
        b.plan.health(OWNER_A).unwrap(),
        PlanHealth {
            state: "damaged".into(),
            file: Some("checked_in".into()),
            reason: Some("corrupt".into()),
            ..PlanHealth::default()
        }
    );
    std::fs::write(
        &path,
        good.replace("\"schema_version\": 1", "\"schema_version\": 3"),
    )
    .unwrap();
    assert_eq!(
        b.plan.health(OWNER_A).unwrap().reason.as_deref(),
        Some("unsupported_version")
    );
    std::fs::write(&path, &good).unwrap();
    // Damaged local data.
    std::fs::write(b.home.join("orchestration-plan.json"), b"{ not json").unwrap();
    let local = b.plan.health(OWNER_A).unwrap();
    assert_eq!(
        (local.state.as_str(), local.file.as_deref()),
        ("damaged", Some("local"))
    );
    // A person's discard resolves the first case, and the plan reads ok again.
    std::fs::write(b.home.join("orchestration-plan.json"), &before.1).unwrap();
    let mut b = b;
    b.migrate(true).unwrap();
    assert_eq!(b.plan.health(OWNER_A).unwrap().state, "ok");
}

// HS2-38MFXT: the Wait for Accept hold travels with the checked-in plan.

fn hold(operation: &str, created_at_ms: u64) -> PlanHold {
    PlanHold {
        operation_id: operation.into(),
        template_id: id(800),
        template_name: "Repair the release".into(),
        goal: goal(),
        goal_slug: Some("PLN-1".into()),
        accepted_steps: Vec::new(),
        accepted_at_ms: None,
        accepted_by: None,
        created_at_ms,
    }
}

/// The goal a template apply opened, and its hold.
fn template_goal() -> RunTicket {
    RunTicket {
        connection_id: "plan".into(),
        native_id: id(901),
    }
}
fn template_hold(operation: &str, created_at_ms: u64) -> PlanHold {
    PlanHold {
        goal: template_goal(),
        ..hold(operation, created_at_ms)
    }
}

impl Machine {
    fn define_for(
        &mut self,
        goal: RunTicket,
        target: &RunTarget,
    ) -> Result<(PlanStep, bool), OrchestrationError> {
        let (owner, plan) = (self.owner, self.plan.clone());
        let mut step = new(target, Vec::new());
        step.ticket = goal;
        plan.define_observed(
            owner,
            step,
            None,
            Vec::new(),
            Some("worker-1".into()),
            &mut self.tx(),
        )
    }
    fn add_hold(&mut self, new: PlanHold) {
        let (owner, plan) = (self.owner, self.plan.clone());
        plan.update_holds(owner, &mut self.tx(), |holds, _| {
            if !holds
                .iter()
                .any(|known| known.operation_id == new.operation_id)
            {
                holds.push(new);
            }
            Ok(())
        })
        .unwrap();
    }
    /// A person's accept of the goal's live steps, as `templates/accept` records it.
    fn accept(&mut self, operation: &str) -> Vec<String> {
        let (owner, plan) = (self.owner, self.plan.clone());
        let now = self.clock.0.get();
        plan.update_holds(owner, &mut self.tx(), |holds, steps| {
            let hold = holds
                .iter_mut()
                .find(|hold| hold.operation_id == operation)
                .unwrap();
            let goal_steps: Vec<String> = steps
                .iter()
                .filter(|step| hold.names_goal(&step.ticket))
                .map(|step| step.step_id.clone())
                .collect();
            hold.accept(&goal_steps, Some("maintainer"), now);
            Ok(goal_steps)
        })
        .unwrap()
    }
    fn held(&self) -> Vec<String> {
        self.plan
            .held_steps(self.owner)
            .unwrap()
            .into_keys()
            .collect()
    }
    fn schema_version(&self, root: &Path) -> u64 {
        let text = std::fs::read_to_string(root.join(SHARED_PLAN_PATH)).unwrap();
        serde_json::from_str::<serde_json::Value>(&text).unwrap()["schema_version"]
            .as_u64()
            .unwrap()
    }
}

#[test]
fn hs2_38mfxt_a_hold_holds_on_every_machine_and_an_accept_anywhere_releases_it() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    let mut b = Machine::new(OWNER_B, root.path(), 200);
    // A step of another goal first: the plan is version 1, byte for byte as before.
    let (other, _) = a.define(&ticket("other"), Vec::new(), None).unwrap();
    let before = std::fs::read_to_string(root.path().join(SHARED_PLAN_PATH)).unwrap();
    assert!(!before.contains("holds"));
    assert_eq!(a.schema_version(root.path()), 1);
    // Machine A applies a waiting template: the hold is checked in before any step.
    a.add_hold(template_hold("tpl-wait", 5));
    assert_eq!(a.schema_version(root.path()), 2);
    assert!(a.held().is_empty() && b.held().is_empty());
    // The planner defines a step of the template's goal: held on both machines.
    let (first, _) = a.define_for(template_goal(), &ticket("first")).unwrap();
    assert_eq!(a.held(), std::slice::from_ref(&first.step_id));
    assert_eq!(b.held(), std::slice::from_ref(&first.step_id));
    assert!(!b.held().contains(&other.step_id));
    // An accept on B releases it on both machines.
    let released = b.accept("tpl-wait");
    assert_eq!(released, std::slice::from_ref(&first.step_id));
    assert!(a.held().is_empty() && b.held().is_empty());
    let holds = a.plan.holds(OWNER_A).unwrap();
    assert_eq!(holds[0].accepted_by.as_deref(), Some("maintainer"));
    // A step planned later (on B this time) is held again, until the next accept on A.
    let (later, _) = b.define_for(template_goal(), &ticket("later")).unwrap();
    assert_eq!(a.held(), std::slice::from_ref(&later.step_id));
    assert_eq!(b.held(), std::slice::from_ref(&later.step_id));
    a.accept("tpl-wait");
    assert!(a.held().is_empty() && b.held().is_empty());
    // A repeat accept changes nothing, not even the revision.
    let revision = a.plan.revision(OWNER_A).unwrap();
    b.accept("tpl-wait");
    assert_eq!(a.plan.revision(OWNER_A).unwrap(), revision);
    // Step writes keep the hold: removing a step keeps it, at version 2.
    a.remove(&later.step_id).unwrap();
    assert_eq!(a.plan.holds(OWNER_A).unwrap().len(), 1);
    assert_eq!(a.schema_version(root.path()), 2);
    // The same hold added again (a repeated apply) changes nothing.
    let revision = a.plan.revision(OWNER_A).unwrap();
    b.add_hold(template_hold("tpl-wait", 9));
    assert_eq!(a.plan.revision(OWNER_A).unwrap(), revision);
    // Re-planned after the removal: held again, the accepted one stays released.
    let (again, _) = a.define_for(template_goal(), &ticket("again")).unwrap();
    assert_eq!(b.held(), [again.step_id]);
}

#[test]
fn hs2_38mfxt_a_step_of_another_goal_is_never_held_and_the_goal_matches_by_native_id() {
    let mut waiting = hold("tpl-wait", 1);
    let mut step = shared(1, "a", 1);
    assert!(waiting.holds(&step));
    // Another machine names the same goal through its own connection id.
    step.ticket.connection_id = "another-machine".into();
    assert!(waiting.holds(&step));
    // Another goal is not held.
    step.ticket.native_id = id(901);
    assert!(!waiting.holds(&step));
    // An accepted step is not held; the accept is idempotent.
    let step = shared(1, "a", 1);
    assert!(waiting.accept(std::slice::from_ref(&step.step_id), Some("maintainer"), 7));
    assert!(!waiting.holds(&step));
    assert!(!waiting.accept(std::slice::from_ref(&step.step_id), Some("someone"), 8));
    assert_eq!(waiting.accepted_at_ms, Some(7));
    assert_eq!(waiting.accepted_by.as_deref(), Some("maintainer"));
    // The pure selection: empty holds hold nothing; each held step names its hold.
    let (one, two) = (shared(1, "a", 1), shared(2, "b", 1));
    assert!(held_plan_steps(&[], &[one.clone(), two.clone()]).is_empty());
    let holds = [hold("tpl-a", 1)];
    let held = held_plan_steps(&holds, &[one, two]);
    assert_eq!(held.len(), 2);
    assert!(held.values().all(|hold| hold.operation_id == "tpl-a"));
    assert_eq!(plan_version(&[]), 1);
    assert_eq!(plan_version(&holds), PLAN_HOLD_VERSION);
}

#[test]
fn hs2_38mfxt_an_older_reader_refuses_a_plan_with_a_hold_and_damage_is_refused() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    a.define(&ticket("first"), Vec::new(), None).unwrap();
    a.add_hold(hold("tpl-wait", 1));
    let path = root.path().join(SHARED_PLAN_PATH);
    let good = std::fs::read_to_string(&path).unwrap();
    // An older Hot Sheet reads every orchestration file through the version-1 decoder:
    // it answers upgrade_required for the plan, so it places nothing and writes nothing.
    assert!(matches!(
        super::storage::decode::<serde_json::Value>(good.as_bytes()),
        Err(OrchestrationError::UnsupportedVersion { found: 2 })
    ));
    let second = serde_json::to_string(&hold("tpl-b", 2)).unwrap();
    let first_hold = serde_json::to_string(&a.plan.holds(OWNER_A).unwrap()[0]).unwrap();
    for bytes in [
        // A hold in a version-1 plan: an older binary would have ignored it.
        good.replace("\"schema_version\": 2", "\"schema_version\": 1"),
        // Holds out of order, or twice.
        good.replacen("\"holds\": [", &format!("\"holds\": [{second},"), 1)
            .replace("tpl-b", "tpl-z"),
        good.replacen("\"holds\": [", &format!("\"holds\": [{first_hold},"), 1),
        // A hold that names no goal id, or an unknown field.
        good.replace(&id(900), "not-a-ulid"),
        good.replacen("\"operation_id\"", "\"unknown\": 1, \"operation_id\"", 1),
    ] {
        std::fs::write(&path, &bytes).unwrap();
        assert!(
            matches!(a.plan.holds(OWNER_A), Err(OrchestrationError::Corrupt)),
            "{bytes}"
        );
        // The scheduler's read fails rather than place a step nobody accepted.
        assert!(a.plan.held_steps(OWNER_A).is_err());
        assert!(a.define(&ticket("second"), Vec::new(), None).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_38mfxt_the_holds_are_bounded_and_a_hold_on_a_live_step_goes_last() {
    let root = tempfile::tempdir().unwrap();
    let mut a = Machine::new(OWNER_A, root.path(), 100);
    // The oldest hold names the goal every planned step belongs to.
    a.add_hold(hold("tpl-0000", 0));
    let (first, _) = a.define(&ticket("first"), Vec::new(), None).unwrap();
    // Fill to the bound with holds of other goals, none holding a live step.
    let (owner, plan) = (a.owner, a.plan.clone());
    plan.update_holds(owner, &mut a.tx(), |holds, _| {
        for n in 1..MAX_PLAN_HOLDS {
            let mut other = hold(&format!("tpl-{n:04}"), n as u64);
            other.goal.native_id = id(10_000 + n as u128);
            holds.push(other);
        }
        Ok(())
    })
    .unwrap();
    assert_eq!(a.plan.holds(OWNER_A).unwrap().len(), MAX_PLAN_HOLDS);
    // One more: the oldest idle hold is forgotten, never the one holding `first`.
    let mut newest = hold("tpl-9999", 9_999);
    newest.goal.native_id = id(20_000);
    a.add_hold(newest);
    let kept: Vec<String> = a
        .plan
        .holds(OWNER_A)
        .unwrap()
        .into_iter()
        .map(|hold| hold.operation_id)
        .collect();
    assert_eq!(kept.len(), MAX_PLAN_HOLDS);
    assert!(kept.contains(&"tpl-0000".to_owned()));
    assert!(!kept.contains(&"tpl-0001".to_owned()));
    assert!(kept.contains(&"tpl-9999".to_owned()));
    assert_eq!(a.held(), [first.step_id]);
}
