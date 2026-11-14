//! Provisional assignment transitions (HS2-2SJPCH): the ledger matrix, red with parked
//! and integrated dependents, rebase after the fix, repeated red, a result before the
//! claim, a restart in the middle, the integration rule, the unchanged default, and an
//! idle wait that reads once per wake.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::time::Duration;

use hotsheet_model::{NoteKind, StartedPhase, Status, Ticket, Timestamp, Ulid};
use hotsheet_ticketing::NewTicket;
use hotsheet_ticketing::provisional::{
    GateObservation, GateProbe, GateResult, IntegrationRule, SpeculationPolicy,
};

use super::speculation::{
    LEDGER_FILE, PendingGate, Rebase, Recovery, SpeculationLedger, SpeculationState,
    SpeculativeRecord, UNNAMED_RUN, follow,
};
use super::speculation_runtime::{BRANCH_BASE_KEY, BRANCH_GATE_KEY, BRANCH_STATE_KEY, BaseProbe};
use super::tests::{NOW, assignment, fixture, register};
use super::*;

const GEN: &str = "generation-1";
const BASE: &str = "1111111111111111111111111111111111111111";
const GATE_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const GATE_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const FIX: &str = "cccccccccccccccccccccccccccccccccccccccc";
const BASE_2: &str = "2222222222222222222222222222222222222222";
const OTHER: &str = "dddddddddddddddddddddddddddddddddddddddd";

/// Injected facts: which commits are pushed, what each gate reads, which base contains
/// which commit. Counts gate reads.
#[derive(Default)]
struct Facts {
    pushed: RefCell<Vec<String>>,
    gates: RefCell<BTreeMap<String, GateObservation>>,
    contained: RefCell<Vec<(String, String)>>,
    reads: Cell<u32>,
}

impl Facts {
    fn push(&self, commit: &str, base: &str, result: GateResult, run: Option<&str>) -> &Self {
        self.pushed.borrow_mut().push(commit.into());
        self.contained
            .borrow_mut()
            .push((commit.into(), base.into()));
        self.set(commit, result, run)
    }

    fn set(&self, commit: &str, result: GateResult, run: Option<&str>) -> &Self {
        self.gates.borrow_mut().insert(
            commit.into(),
            GateObservation {
                result,
                run: run.map(str::to_owned),
            },
        );
        self
    }

    fn observed(&self, commit: &str) -> GateObservation {
        self.gates.borrow().get(commit).cloned().unwrap_or_default()
    }
}

impl GateProbe for Facts {
    fn reachable(&self, commit: &str) -> Option<bool> {
        Some(self.pushed.borrow().iter().any(|found| found == commit))
    }

    fn gate(&self, commit: &str) -> GateObservation {
        self.reads.set(self.reads.get() + 1);
        self.observed(commit)
    }
}

impl BaseProbe for Facts {
    fn contains(&self, ancestor: &str, descendant: &str) -> Option<bool> {
        Some(
            self.contained
                .borrow()
                .iter()
                .any(|(a, d)| a == ancestor && d == descendant),
        )
    }
}

fn enabled(integration: IntegrationRule) -> SpeculationPolicy {
    SpeculationPolicy {
        enabled: true,
        integration,
        ..SpeculationPolicy::default()
    }
}

fn store_of(c: &Coordinator) -> FsStore {
    FsStore::open_without_maintenance(&c.source).unwrap()
}

fn at() -> Timestamp {
    Timestamp::from_datetime(time::OffsetDateTime::from_unix_timestamp(NOW).unwrap())
}

fn ticket(store: &FsStore, title: &str, blocked_by: Vec<Ulid>) -> Ticket {
    ops::create(
        store,
        Ulid::new(),
        "HS",
        at(),
        NewTicket {
            title: title.into(),
            up_next: true,
            blocked_by,
            ..NewTicket::default()
        },
    )
    .unwrap()
}

/// A prerequisite pushed and waiting on CI with `commit` recorded.
fn awaiting_ci(store: &FsStore, title: &str, commit: &str) -> Ticket {
    let created = ticket(store, title, Vec::new());
    let mut found = store.read_ticket(&created.id).unwrap();
    found.status = Status::Started;
    found.started_phase = Some(StartedPhase::FinalTesting);
    store.write_ticket(&found).unwrap();
    ops::add_note(
        store,
        &created.id,
        Ulid::new(),
        at(),
        NoteKind::Regular,
        format!("Pushed and waiting on CI.\n\nCI gate commit: {commit}\n"),
    )
    .unwrap()
}

/// Reserve `op` for a fresh worker on `t` at `base`, as `coordination assign` does: the
/// rule decides, the pool reserves, the ledger records.
fn reserve(
    c: &Coordinator,
    store: &FsStore,
    policy: &SpeculationPolicy,
    facts: &Facts,
    op: &str,
    t: &Ticket,
    base: &str,
) -> Result<Option<SpeculativeRecord>> {
    let worker = format!("worker-{op}");
    register(c, Role::Integrator, "i", GEN, NOW);
    register(c, Role::Worker, &worker, GEN, NOW);
    let tickets = store.list_tickets()?;
    let fresh = store.read_ticket(&t.id)?;
    let reservation = c.speculation_reservation(policy, facts, &fresh, &tickets, base)?;
    let mut a = assignment(op, &worker, &t.id.to_string());
    a.ticket.connection_id = git_connection_id(store);
    a.slug = t.slug.clone();
    a.base = base.into();
    c.assign("i", GEN, NOW, a)?;
    reservation
        .map(|found| c.speculation_record(op, found.depth, policy.integration, found.gates, NOW))
        .transpose()
}

fn to_handoff(c: &Coordinator, op: &str) {
    let worker = format!("worker-{op}");
    c.worker_transition(&worker, GEN, op, Phase::Accepted, None)
        .unwrap();
    c.worker_transition(&worker, GEN, op, Phase::Handoff, Some(BASE.into()))
        .unwrap();
}

fn state_of(c: &Coordinator, op: &str) -> SpeculationState {
    c.speculation_ledger().unwrap().unwrap().records[op].state
}

fn notes_with(store: &FsStore, t: &Ticket, heading: &str) -> usize {
    store
        .read_ticket(&t.id)
        .unwrap()
        .notes
        .iter()
        .filter(|note| note.text.starts_with(heading))
        .count()
}

fn first_note(store: &FsStore, t: &Ticket, heading: &str) -> String {
    store
        .read_ticket(&t.id)
        .unwrap()
        .notes
        .iter()
        .find(|note| note.text.starts_with(heading))
        .map(|note| note.text.clone())
        .unwrap()
}

fn record(op: &str, gates: &[(&str, GateResult)]) -> SpeculativeRecord {
    SpeculativeRecord {
        assignment: op.into(),
        ticket: TicketRef {
            connection_id: "source".into(),
            native_id: format!("native-{op}"),
        },
        slug: format!("HS-{op}"),
        worker: "worker".into(),
        base: BASE.into(),
        depth: 1,
        integration: IntegrationRule::PushPerTicket,
        gates: gates
            .iter()
            .map(|(commit, result)| PendingGate {
                blocker: "blocker".into(),
                slug: "HS-BLOCK".into(),
                commit: (*commit).into(),
                result: *result,
                run: None,
            })
            .collect(),
        state: SpeculationState::Provisional,
        risks: Vec::new(),
        reopen_reason: None,
        events: Vec::new(),
    }
}

fn read(result: GateResult, run: &str) -> GateObservation {
    GateObservation::with_run(result, run)
}

#[test]
fn hs2_2sjpch_ledger_matrix_of_recorded_gate_and_incoming_result() {
    use GateResult as G;
    use SpeculationState as S;
    let state_for = |gate| match gate {
        G::Green => S::Confirmed,
        G::Red => S::AtRisk,
        _ => S::Provisional,
    };
    for from in [
        G::Pending,
        G::Unknown,
        G::Superseded,
        G::Cancelled,
        G::Green,
        G::Red,
    ] {
        for incoming in G::ALL {
            let mut ledger = SpeculationLedger::default();
            ledger.record(record("a", &[(GATE_A, from)]), NOW).unwrap();
            let before = ledger.records["a"].state;
            assert_eq!(before, state_for(from));
            let changes = ledger
                .apply(GATE_A, &read(incoming, "5.1"), NOW, &|_| false)
                .unwrap();
            // A verdict always lands; nothing short of a verdict clears a verdict.
            let kept = matches!(from, G::Green | G::Red) && !matches!(incoming, G::Green | G::Red);
            let expected = if kept { from } else { incoming };
            assert_eq!(
                ledger.records["a"].gates[0].result, expected,
                "{from:?} <- {incoming:?}"
            );
            let state = ledger.records["a"].state;
            assert_eq!(state, state_for(expected), "{from:?} <- {incoming:?}");
            assert_eq!(
                changes.iter().map(|c| (c.from, c.to)).collect::<Vec<_>>(),
                if kept { vec![] } else { vec![(before, state)] },
                "{from:?} <- {incoming:?}"
            );
            // Replaying the same reading changes nothing and owes no second note.
            let owed = ledger.owed_notes().len();
            assert!(
                ledger
                    .apply(GATE_A, &read(incoming, "5.1"), NOW, &|_| false)
                    .unwrap()
                    .is_empty()
            );
            assert_eq!(ledger.owed_notes().len(), owed);
            assert_eq!(owed, usize::from(incoming == G::Red));
        }
    }
}

#[test]
fn hs2_2sjpch_two_gates_confirm_only_when_both_are_green_in_any_order() {
    for order in [[GATE_A, GATE_B], [GATE_B, GATE_A]] {
        let mut ledger = SpeculationLedger::default();
        ledger
            .record(
                record(
                    "a",
                    &[(GATE_A, GateResult::Pending), (GATE_B, GateResult::Unknown)],
                ),
                NOW,
            )
            .unwrap();
        ledger
            .apply(order[0], &read(GateResult::Green, "1.1"), NOW, &|_| false)
            .unwrap();
        assert_eq!(ledger.records["a"].state, SpeculationState::Provisional);
        assert_eq!(ledger.watched_commits(), vec![order[1].to_owned()]);
        ledger
            .apply(order[1], &read(GateResult::Green, "2.1"), NOW, &|_| false)
            .unwrap();
        assert_eq!(ledger.records["a"].state, SpeculationState::Confirmed);
        assert!(ledger.watched_commits().is_empty());
        // Interleaved: a late red on the first gate puts confirmed work at risk.
        let changes = ledger
            .apply(order[0], &read(GateResult::Red, "3.1"), NOW, &|_| true)
            .unwrap();
        assert_eq!(changes[0].from, SpeculationState::Confirmed);
        assert_eq!(changes[0].to, SpeculationState::AtRisk);
        assert_eq!(changes[0].recovery, Some(Recovery::ReverifyByCoveringRun));
    }
}

#[test]
fn hs2_2sjpch_ledger_refuses_invalid_records_runs_and_foreign_replays() {
    let mut ledger = SpeculationLedger::default();
    assert!(ledger.record(record("a", &[]), NOW).is_err());
    assert!(
        ledger
            .record(record("a", &[("short", GateResult::Pending)]), NOW)
            .is_err()
    );
    ledger
        .record(record("a", &[(GATE_A, GateResult::Pending)]), NOW)
        .unwrap();
    let mut foreign = record("a", &[(GATE_B, GateResult::Pending)]);
    foreign.worker = "other".into();
    assert!(ledger.record(foreign, NOW).is_err());
    // A replay keeps the first record, with every reading applied since.
    ledger
        .apply(GATE_A, &read(GateResult::Red, "9.1"), NOW, &|_| false)
        .unwrap();
    ledger
        .record(record("a", &[(GATE_A, GateResult::Pending)]), NOW)
        .unwrap();
    assert_eq!(ledger.records["a"].state, SpeculationState::AtRisk);
    assert!(
        ledger
            .apply(GATE_A, &read(GateResult::Red, "not a run!"), NOW, &|_| {
                false
            })
            .is_err()
    );
    assert!(
        ledger
            .apply("short", &read(GateResult::Red, "1.1"), NOW, &|_| false)
            .is_err()
    );
    // A red result that names no run is recorded once under a fixed name.
    let mut unnamed = SpeculationLedger::default();
    unnamed
        .record(record("b", &[(GATE_B, GateResult::Pending)]), NOW)
        .unwrap();
    for _ in 0..2 {
        unnamed
            .apply(GATE_B, &GateObservation::of(GateResult::Red), NOW, &|_| {
                false
            })
            .unwrap();
    }
    assert_eq!(unnamed.records["b"].risks.len(), 1);
    assert_eq!(unnamed.records["b"].risks[0].run, UNNAMED_RUN);
    let text = serde_json::to_string(&unnamed).unwrap();
    assert_eq!(
        serde_json::from_str::<SpeculationLedger>(&text).unwrap(),
        unnamed
    );
    assert!(serde_json::from_str::<SpeculationLedger>(r#"{"version":1,"extra":1}"#).is_err());
}

#[test]
fn hs2_2sjpch_default_policy_leaves_pool_snapshot_and_files_untouched() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, None);
    register(&c, Role::Integrator, "i", GEN, NOW);
    let before = serde_json::to_string(&c.snapshot(NOW).unwrap()).unwrap();
    // The default policy refuses whatever CI says, and reads no gate.
    let refused = c
        .speculation_reservation(
            &SpeculationPolicy::default(),
            &facts,
            &store.read_ticket(&dependent.id).unwrap(),
            &store.list_tickets().unwrap(),
            BASE,
        )
        .unwrap_err()
        .to_string();
    assert!(refused.contains("speculation_disabled"), "{refused}");
    assert_eq!(facts.reads.get(), 0);
    // Every read and every result leaves a pool without provisional work as it was.
    assert!(c.speculation_ledger().unwrap().is_none());
    assert!(
        c.speculation_result(GATE_A, read(GateResult::Red, "1.1"), Some(&store), NOW)
            .unwrap()
            .changes
            .is_empty()
    );
    assert!(
        c.speculation_sync(&facts, Some(&store), NOW)
            .unwrap()
            .changes
            .is_empty()
    );
    c.speculation_integration_rule("anything").unwrap();
    assert!(!c.directory.join(LEDGER_FILE).exists());
    let after = serde_json::to_string(&c.snapshot(NOW).unwrap()).unwrap();
    assert_eq!(before, after);
    assert!(!after.contains("speculation"));
    let view = worker_view::WorkerView::from_snapshot(
        &c.snapshot(NOW).unwrap(),
        NOW,
        &BTreeMap::new(),
        false,
    );
    assert!(
        !serde_json::to_string(&view)
            .unwrap()
            .contains("speculation")
    );
    assert_eq!(notes_with(&store, &dependent, "## Provisional"), 0);
}

#[test]
fn hs2_2sjpch_reservation_records_base_gate_and_depth_and_shows_them() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, Some("41.1"));
    let cursor = c.snapshot(NOW).unwrap().cursor;
    // A base that does not contain the prerequisite's commit is refused.
    let wrong = c
        .speculation_reservation(
            &policy,
            &facts,
            &dependent,
            &store.list_tickets().unwrap(),
            BASE_2,
        )
        .unwrap_err()
        .to_string();
    assert!(
        wrong.contains("does not contain the pushed commit"),
        "{wrong}"
    );
    let recorded = reserve(&c, &store, &policy, &facts, "op", &dependent, BASE)
        .unwrap()
        .unwrap();
    assert_eq!(recorded.base, BASE);
    assert_eq!(recorded.depth, 1);
    assert_eq!(recorded.state, SpeculationState::Provisional);
    assert_eq!(recorded.gates[0].commit, GATE_A);
    assert_eq!(recorded.gates[0].slug, blocker.slug);
    let explain = format!("provisional on {} CI 41.1", blocker.slug);
    assert_eq!(recorded.explain(), explain);
    // Status, the explanation and the worker view all show the same row.
    let snapshot = c.snapshot(NOW).unwrap();
    assert_ne!(snapshot.cursor, cursor, "a reservation wakes waiters");
    let row = &snapshot.speculation.as_ref().unwrap().records[0];
    assert_eq!(
        (row.base.as_str(), row.explain.as_str()),
        (BASE, explain.as_str())
    );
    let view = worker_view::WorkerView::from_snapshot(&snapshot, NOW, &BTreeMap::new(), false);
    assert_eq!(view.speculation, snapshot.speculation);
    let explained = c
        .speculation_explain(&policy, &facts, &dependent, &store.list_tickets().unwrap())
        .unwrap();
    assert_eq!(explained.explain, explain);
    assert_eq!(explained.record.unwrap().assignment, "op");
    assert!(!explained.opted_out);
    // A confirmed reservation records nothing.
    let free = ticket(&store, "free", Vec::new());
    assert!(
        reserve(&c, &store, &policy, &facts, "free", &free, BASE)
            .unwrap()
            .is_none()
    );
    assert_eq!(c.speculation_ledger().unwrap().unwrap().records.len(), 1);
    // An opted-out ticket is refused and says so.
    let mut never = store
        .read_ticket(&ticket(&store, "never", vec![blocker.id]).id)
        .unwrap();
    never.tags.push("speculate:never".into());
    store.write_ticket(&never).unwrap();
    let refused = reserve(&c, &store, &policy, &facts, "never", &never, BASE)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("speculation_opted_out"), "{refused}");
    assert!(
        c.speculation_explain(&policy, &facts, &never, &store.list_tickets().unwrap())
            .unwrap()
            .opted_out
    );
}

#[test]
fn hs2_2sjpch_green_confirms_and_changes_nothing_else() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, None);
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    let pool = c.state().unwrap();
    facts.set(GATE_A, GateResult::Green, Some("7.1"));
    let report = c.speculation_sync(&facts, Some(&store), NOW).unwrap();
    assert_eq!(report.changes.len(), 1);
    assert_eq!(report.changes[0].to, SpeculationState::Confirmed);
    assert!(report.notes_written.is_empty() && report.notes_owed.is_empty());
    assert_eq!(state_of(&c, "op"), SpeculationState::Confirmed);
    assert_eq!(c.state().unwrap().assignments, pool.assignments);
    assert_eq!(store.read_ticket(&dependent.id).unwrap().notes.len(), 0);
    // Nothing is left to watch, so a later pass reads no gate at all.
    let reads = facts.reads.get();
    assert!(
        c.speculation_sync(&facts, Some(&store), NOW)
            .unwrap()
            .changes
            .is_empty()
    );
    assert_eq!(facts.reads.get(), reads);
}

#[test]
fn hs2_2sjpch_red_then_fix_with_parked_and_integrated_dependents_across_a_restart() {
    let (root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let parked = ticket(&store, "parked", vec![blocker.id]);
    let landed = ticket(&store, "landed", vec![blocker.id]);
    let late = ticket(&store, "late", vec![blocker.id]);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, None);
    reserve(&c, &store, &policy, &facts, "parked", &parked, BASE).unwrap();
    reserve(&c, &store, &policy, &facts, "landed", &landed, BASE).unwrap();
    // push-per-ticket: the dependent lands after its own local gates, before the base
    // gate is decided.
    to_handoff(&c, "landed");
    c.mark("i", GEN, NOW, "landed", Phase::Integrating).unwrap();
    c.mark("i", GEN, NOW, "landed", Phase::Ci).unwrap();

    // Red arrives. The process stops after the ledger write and before any note.
    let changes = c
        .speculation_apply(&[(GATE_A.into(), read(GateResult::Red, "10.1"))], NOW)
        .unwrap();
    let recovery: BTreeMap<_, _> = changes
        .iter()
        .map(|change| (change.assignment.as_str(), change.recovery))
        .collect();
    assert_eq!(recovery["parked"], Some(Recovery::RebaseOntoFix));
    assert_eq!(recovery["landed"], Some(Recovery::ReverifyByCoveringRun));
    drop(c);

    // Restart: a new process finds the notes it still owes and writes each once.
    let reopen =
        || Coordinator::open(&root.path().join("project"), &root.path().join("store")).unwrap();
    let c = reopen();
    facts.set(GATE_A, GateResult::Red, Some("10.1"));
    let report = c.speculation_sync(&facts, Some(&store), NOW).unwrap();
    assert!(report.changes.is_empty(), "the result was already applied");
    assert_eq!(report.notes_written.len(), 3, "{:?}", report.notes_written);
    assert!(report.notes_owed.is_empty());
    assert_eq!(report.status.repair_first, vec![blocker.slug.clone()]);
    for dependent in [&parked, &landed] {
        assert_eq!(
            notes_with(&store, dependent, "## Provisional work at risk"),
            1
        );
        let text = first_note(&store, dependent, "## Provisional work at risk");
        assert!(text.contains("`10.1`") && text.contains(GATE_A) && text.contains(BASE));
    }
    assert!(
        first_note(&store, &parked, "## Provisional work at risk")
            .contains("rebase it onto the fix")
    );
    assert!(
        first_note(&store, &landed, "## Provisional work at risk").contains("next covering CI run")
    );
    assert_eq!(
        notes_with(&store, &blocker, "## CI red with provisional dependents"),
        1
    );
    // A replay of the same red, by sync or by hand, writes nothing more.
    for _ in 0..2 {
        let again = c
            .speculation_result(GATE_A, read(GateResult::Red, "10.1"), Some(&store), NOW)
            .unwrap();
        assert!(again.changes.is_empty() && again.notes_written.is_empty());
    }

    // The repair comes first: no new provisional work, and at-risk work does not integrate.
    let refused = reserve(&c, &store, &policy, &facts, "late", &late, BASE)
        .unwrap_err()
        .to_string();
    assert!(refused.contains("gate_red"), "{refused}");
    // Not even on another prerequisite whose own gate is still pending.
    let elsewhere = awaiting_ci(&store, "elsewhere", OTHER);
    let unrelated = ticket(&store, "unrelated", vec![elsewhere.id]);
    facts.push(OTHER, BASE, GateResult::Pending, None);
    let refused = reserve(&c, &store, &policy, &facts, "unrelated", &unrelated, BASE)
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains(&format!("repair {} before", blocker.slug)),
        "{refused}"
    );
    to_handoff(&c, "parked");
    let held = c
        .mark("i", GEN, NOW, "parked", Phase::Integrating)
        .unwrap_err()
        .to_string();
    assert!(held.contains("is at risk"), "{held}");

    // Repeated red: the re-run fails again under a new run, and each dependent is told.
    let repeated = c
        .speculation_result(GATE_A, read(GateResult::Red, "10.2"), Some(&store), NOW)
        .unwrap();
    assert_eq!(repeated.changes.len(), 2);
    assert!(
        repeated
            .changes
            .iter()
            .all(|change| change.from == change.to)
    );
    assert_eq!(
        notes_with(&store, &parked, "## Provisional work at risk"),
        2
    );
    assert_eq!(
        notes_with(&store, &blocker, "## CI red with provisional dependents"),
        2
    );
    // A pending re-run does not clear the risk.
    assert!(
        c.speculation_result(
            GATE_A,
            GateObservation::of(GateResult::Pending),
            Some(&store),
            NOW
        )
        .unwrap()
        .changes
        .is_empty()
    );
    assert_eq!(state_of(&c, "parked"), SpeculationState::AtRisk);

    // The fix is pushed as FIX; BASE_2 contains it.
    facts.push(FIX, BASE_2, GateResult::Pending, Some("11.1"));
    let rebase = |base: Option<&str>| Rebase {
        replaces: GATE_A.into(),
        gate_commit: FIX.into(),
        gate: facts.observed(FIX),
        base: base.map(str::to_owned),
    };
    // Parked work must name the base it was rebased onto, and that base must hold the fix.
    assert!(
        c.speculation_rebase_checked(&facts, "parked", &rebase(None), NOW, Some("i"))
            .is_err()
    );
    assert!(
        c.speculation_rebase_checked(&facts, "parked", &rebase(Some(BASE)), NOW, Some("i"))
            .is_err()
    );
    let rebased = c
        .speculation_rebase_checked(&facts, "parked", &rebase(Some(BASE_2)), NOW, Some("i"))
        .unwrap();
    assert_eq!(rebased.state, SpeculationState::Provisional);
    assert_eq!(
        (rebased.base.as_str(), rebased.gates[0].commit.as_str()),
        (BASE_2, FIX)
    );
    assert_eq!(rebased.risks.len(), 2, "the red history stays");
    // The replay is idempotent.
    assert_eq!(
        c.speculation_rebase_checked(&facts, "parked", &rebase(Some(BASE_2)), NOW, Some("i"))
            .unwrap(),
        rebased
    );
    // Integrated work keeps its place; only its gate is replaced.
    let reverify = c
        .speculation_rebase_checked(&facts, "landed", &rebase(None), NOW, Some("i"))
        .unwrap();
    assert_eq!(reverify.base, BASE);
    assert_eq!(reverify.state, SpeculationState::Provisional);
    // Rebased work may integrate again under push-per-ticket.
    c.mark("i", GEN, NOW, "parked", Phase::Integrating).unwrap();

    // Another restart, then the covering run of the fix goes green.
    drop(c);
    let c = reopen();
    facts.set(FIX, GateResult::Green, Some("11.1"));
    let green = c.speculation_sync(&facts, Some(&store), NOW).unwrap();
    assert_eq!(green.changes.len(), 2);
    assert!(
        green
            .changes
            .iter()
            .all(|change| change.to == SpeculationState::Confirmed)
    );
    assert!(green.status.repair_first.is_empty());
    assert!(green.notes_written.is_empty());
    // With the repair done, new provisional work is reserved again (empty, then refill).
    let next = awaiting_ci(&store, "next blocker", GATE_B);
    let refill = ticket(&store, "refill", vec![next.id]);
    facts.push(GATE_B, BASE_2, GateResult::Unknown, None);
    let record = reserve(&c, &store, &policy, &facts, "refill", &refill, BASE_2)
        .unwrap()
        .unwrap();
    assert_eq!(
        record.explain(),
        format!("provisional on {} CI unknown", next.slug)
    );
    assert_eq!(
        c.speculation_ledger().unwrap().unwrap().watched_commits(),
        vec![GATE_B.to_owned()]
    );
    // An unwatched gate carries no information: nothing changes and it is read once.
    let reads = facts.reads.get();
    assert!(
        c.speculation_sync(&facts, Some(&store), NOW)
            .unwrap()
            .changes
            .is_empty()
    );
    assert_eq!(facts.reads.get(), reads + 1);
}

#[test]
fn hs2_2sjpch_result_before_the_claim_marks_the_offer_at_risk() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, None);
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    assert_eq!(c.state().unwrap().assignments["op"].phase, Phase::Offered);
    let report = c
        .speculation_result(GATE_A, read(GateResult::Red, "3.1"), Some(&store), NOW)
        .unwrap();
    assert_eq!(report.changes[0].recovery, Some(Recovery::RebaseOntoFix));
    assert_eq!(
        notes_with(&store, &dependent, "## Provisional work at risk"),
        1
    );
    // The worker that accepts afterwards finds the record and the note; nothing was lost.
    c.worker_transition("worker-op", GEN, "op", Phase::Accepted, None)
        .unwrap();
    assert_eq!(state_of(&c, "op"), SpeculationState::AtRisk);
    // A green re-run of the same commit (a flake) confirms it.
    let green = c
        .speculation_result(GATE_A, read(GateResult::Green, "3.2"), Some(&store), NOW)
        .unwrap();
    assert_eq!(green.changes[0].to, SpeculationState::Confirmed);
}

#[test]
fn hs2_2sjpch_superseded_and_cancelled_wait_for_the_newest_covering_run() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, Some("1.1"));
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    for stale in [GateResult::Cancelled, GateResult::Superseded] {
        facts.set(GATE_A, stale, None);
        let report = c.speculation_sync(&facts, Some(&store), NOW).unwrap();
        assert_eq!(report.changes[0].to, SpeculationState::Provisional);
        assert_eq!(report.status.reevaluate.len(), 1);
        assert_eq!(report.status.reevaluate[0].result, stale);
        assert!(report.notes_written.is_empty());
        // The newest covering run is watched again and is pending.
        facts.set(GATE_A, GateResult::Pending, Some("2.1"));
        let report = c.speculation_sync(&facts, Some(&store), NOW).unwrap();
        assert!(report.status.reevaluate.is_empty());
        assert_eq!(
            report.status.records[0].gates[0].run.as_deref(),
            Some("2.1")
        );
    }
    // A stale gate may also be replaced by the commit that superseded it.
    facts.set(GATE_A, GateResult::Superseded, None);
    c.speculation_sync(&facts, Some(&store), NOW).unwrap();
    facts.push(FIX, BASE, GateResult::Green, Some("4.1"));
    let replaced = c
        .speculation_rebase_checked(
            &facts,
            "op",
            &Rebase {
                replaces: GATE_A.into(),
                gate_commit: FIX.into(),
                gate: read(GateResult::Green, "4.1"),
                base: Some(BASE.into()),
            },
            NOW,
            None,
        )
        .unwrap();
    assert_eq!(replaced.state, SpeculationState::Confirmed);
    // A green gate has nothing to rebase, and an unpushed fix is refused.
    assert!(
        c.speculation_rebase_checked(
            &facts,
            "op",
            &Rebase {
                replaces: FIX.into(),
                gate_commit: GATE_B.into(),
                gate: GateObservation::default(),
                base: Some(BASE.into()),
            },
            NOW,
            None,
        )
        .is_err()
    );
}

#[test]
fn hs2_2sjpch_parked_rule_holds_work_until_the_base_gate_is_green() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let policy = enabled(IntegrationRule::Parked);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, Some("1.1"));
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    to_handoff(&c, "op");
    for _ in 0..2 {
        let parked = c
            .mark("i", GEN, NOW, "op", Phase::Integrating)
            .unwrap_err()
            .to_string();
        assert!(parked.contains("stays parked"), "{parked}");
        assert_eq!(c.state().unwrap().assignments["op"].phase, Phase::Handoff);
    }
    c.speculation_result(GATE_A, read(GateResult::Green, "1.1"), Some(&store), NOW)
        .unwrap();
    c.mark("i", GEN, NOW, "op", Phase::Integrating).unwrap();
}

#[test]
fn hs2_2sjpch_reopen_keeps_the_record_notes_the_ticket_and_reopens_a_closed_one() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    facts.push(GATE_A, BASE, GateResult::Pending, None);
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    assert!(
        c.speculation_reopen_noted(&store, "op", "  ", NOW, None)
            .is_err()
    );
    assert!(
        c.speculation_reopen_noted(&store, "missing", "invalidated", NOW, None)
            .is_err()
    );
    c.speculation_result(GATE_A, read(GateResult::Red, "8.1"), Some(&store), NOW)
        .unwrap();
    // The work had already been completed when the fix invalidated it.
    let mut closed = store.read_ticket(&dependent.id).unwrap();
    closed.status = Status::Completed;
    store.write_ticket(&closed).unwrap();
    for _ in 0..2 {
        let reopened = c
            .speculation_reopen_noted(
                &store,
                "op",
                "the fix removed the API this used",
                NOW,
                Some("i"),
            )
            .unwrap();
        assert_eq!(reopened.state, SpeculationState::Reopened);
        assert_eq!(reopened.gates.len(), 1, "the record stays");
    }
    assert_eq!(
        notes_with(&store, &dependent, "## Provisional work reopened"),
        1
    );
    assert_eq!(
        store.read_ticket(&dependent.id).unwrap().status,
        Status::Started
    );
    // A reopened record ignores later results, cannot be rebased or integrated, and no
    // longer holds back new provisional work.
    assert!(
        c.speculation_result(GATE_A, read(GateResult::Green, "8.2"), Some(&store), NOW)
            .unwrap()
            .changes
            .is_empty()
    );
    assert!(c.speculation_integration_rule("op").is_err());
    let status = c.speculation_ledger().unwrap().unwrap().status();
    assert!(status.repair_first.is_empty());
    assert_eq!(status.records[0].state, SpeculationState::Reopened);
    // Confirmed work cannot be reopened.
    let other = ticket(&store, "other", vec![blocker.id]);
    facts.set(GATE_A, GateResult::Pending, None);
    reserve(&c, &store, &policy, &facts, "other", &other, BASE).unwrap();
    c.speculation_result(GATE_A, read(GateResult::Green, "8.2"), Some(&store), NOW)
        .unwrap();
    assert!(
        c.speculation_reopen_noted(&store, "other", "no", NOW, None)
            .is_err()
    );
}

#[test]
fn hs2_2sjpch_idle_wait_reads_once_per_wake_and_never_on_a_timer() {
    // Nothing changes and the wait times out: one read, one wait.
    let waits = Cell::new(0);
    let (changes, reads) = follow(
        Duration::from_secs(30),
        || Ok((Vec::<u8>::new(), 7)),
        |revision, _| {
            waits.set(waits.get() + 1);
            Ok(revision)
        },
    )
    .unwrap();
    assert!(changes.is_empty());
    assert_eq!((reads, waits.get()), (1, 1));
    // An unrelated event wakes it once: exactly one more read.
    let waits = Cell::new(0u64);
    let (_, reads) = follow(
        Duration::from_secs(30),
        || Ok((Vec::<u8>::new(), 7 + waits.get().min(1))),
        |revision, _| {
            waits.set(waits.get() + 1);
            Ok(if waits.get() == 1 {
                revision + 1
            } else {
                revision
            })
        },
    )
    .unwrap();
    assert_eq!((reads, waits.get()), (2, 2));
    // A change returns at once without waiting, and a zero wait never waits.
    let (changes, reads) = follow(
        Duration::from_secs(30),
        || Ok((vec![1u8], 1)),
        |_, _| panic!("a change must not wait"),
    )
    .unwrap();
    assert_eq!((changes, reads), (vec![1], 1));
    let (_, reads) = follow(
        Duration::ZERO,
        || Ok((Vec::<u8>::new(), 1)),
        |_, _| panic!("a zero wait must not wait"),
    )
    .unwrap();
    assert_eq!(reads, 1);
}

fn git(project: &Path, args: &[&str]) -> String {
    let output = hotsheet_ticketing::git::launch("git")
        .arg("-C")
        .arg(project)
        .args([
            "-c",
            "user.name=Synthetic",
            "-c",
            "user.email=synthetic@example.invalid",
            "-c",
            "commit.gpgsign=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "git {args:?}");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn hs2_2sjpch_live_probe_reads_git_and_the_ci_watch_store_without_requests() {
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let pushed = git(&project, &["rev-parse", "HEAD"]);
    git(&project, &["checkout", "-q", "-b", "side"]);
    git(&project, &["commit", "-q", "--allow-empty", "-m", "two"]);
    let unpushed = git(&project, &["rev-parse", "HEAD"]);
    let store = root.path().join("store");
    FsStore::init_git(&store, &hotsheet_ticketing::StoreMetadata::new("HS")).unwrap();
    let home = root.path().join("home");
    fs::create_dir(&home).unwrap();
    let c = Coordinator::open_with_registry_home(&project, &store, home.clone()).unwrap();
    let policy = SpeculationPolicy {
        base_branch: Some("main".into()),
        ..enabled(IntegrationRule::PushPerTicket)
    };
    let probe = c.speculation_probe(&policy);
    assert_eq!(probe.reachable(&pushed), Some(true));
    assert_eq!(probe.reachable(&unpushed), Some(false));
    assert_eq!(
        probe.reachable(GATE_A),
        None,
        "an unknown commit is not known pushed"
    );
    assert_eq!(probe.contains(&pushed, &unpushed), Some(true));
    assert_eq!(probe.contains(&unpushed, &pushed), Some(false));
    // Neither the named branch nor an upstream: reachability is unknown, never assumed.
    let missing = c.speculation_probe(&SpeculationPolicy {
        base_branch: Some("release".into()),
        ..policy.clone()
    });
    assert_eq!(missing.reachable(&pushed), None);
    assert_eq!(
        c.speculation_probe(&enabled(IntegrationRule::PushPerTicket))
            .reachable(&pushed),
        None
    );
    // No watch: unknown. A store that does not exist is not waited on.
    assert_eq!(probe.gate(&pushed).result, GateResult::Unknown);
    let started = std::time::Instant::now();
    assert_eq!(
        probe.wait(probe.revision(), Duration::from_secs(30)),
        probe.revision()
    );
    assert!(started.elapsed() < Duration::from_secs(10));
    assert!(!probe.watchable());
    // A queued CI watch Job of that commit reads as pending, whatever its workflow.
    let jobs = hotsheet_jobs::JobStore::at(&home);
    let target = hotsheet_jobs::CiTarget::new("synthetic/repo", &pushed, "CI", "main").unwrap();
    hotsheet_jobs::ensure_ci_watch(
        &jobs,
        &c.project_id,
        &target,
        &hotsheet_jobs::CiWatchOptions::default(),
    )
    .unwrap();
    assert_eq!(probe.gate(&pushed).result, GateResult::Pending);
    assert_eq!(probe.gate(&unpushed).result, GateResult::Unknown);
    assert!(probe.watchable());
    // Idle: while the gate stays pending, one bounded wait performs one read and the
    // wait itself returns the unchanged revision when its time is up.
    let reads = Cell::new(0);
    let (_, steps) = follow(
        Duration::from_millis(200),
        || {
            let revision = probe.revision();
            reads.set(reads.get() + 1);
            assert_eq!(probe.gate(&pushed).result, GateResult::Pending);
            Ok((Vec::<u8>::new(), revision))
        },
        |revision, remaining| Ok(probe.wait(revision, remaining)),
    )
    .unwrap();
    assert_eq!((steps, reads.get()), (1, 1));

    // The worktree branch records the base and the pending gate, and clears the gate
    // once confirmed.
    let tickets = FsStore::open_without_maintenance(&store).unwrap();
    let blocker = awaiting_ci(&tickets, "blocker", &pushed);
    let dependent = ticket(&tickets, "dependent", vec![blocker.id]);
    let fresh = tickets.list_tickets().unwrap();
    let reservation = c
        .speculation_reservation(&policy, &probe, &dependent, &fresh, &unpushed)
        .unwrap()
        .unwrap();
    assert_eq!(reservation.gates[0].result, GateResult::Pending);
    register(&c, Role::Integrator, "i", GEN, NOW);
    register(&c, Role::Worker, "w", GEN, NOW);
    let mut a = assignment("op", "w", &dependent.id.to_string());
    a.ticket.connection_id = git_connection_id(&tickets);
    a.slug = dependent.slug.clone();
    a.base = unpushed.clone();
    c.assign("i", GEN, NOW, a).unwrap();
    c.speculation_record("op", 1, policy.integration, reservation.gates, NOW)
        .unwrap();
    assert!(c.speculation_branch(&project, "missing").is_err());
    let written = c.speculation_branch(&project, "op").unwrap();
    assert_eq!(written.branch, "side");
    let config = |key: &str| {
        hotsheet_ticketing::git::launch("git")
            .arg("-C")
            .arg(&project)
            .args(["config", "--get-all", &format!("branch.side.{key}")])
            .output()
            .map(|output| String::from_utf8(output.stdout).unwrap().trim().to_owned())
            .unwrap()
    };
    assert_eq!(config(BRANCH_BASE_KEY), unpushed);
    assert_eq!(config(BRANCH_STATE_KEY), "provisional");
    assert_eq!(
        config(BRANCH_GATE_KEY),
        format!("{} {pushed} pending -", blocker.slug)
    );
    c.speculation_result(&pushed, read(GateResult::Green, "5.1"), Some(&tickets), NOW)
        .unwrap();
    for _ in 0..2 {
        let confirmed = c.speculation_branch(&project, "op").unwrap();
        assert!(confirmed.pending_gates.is_empty());
    }
    assert_eq!(config(BRANCH_STATE_KEY), "confirmed");
    assert_eq!(config(BRANCH_GATE_KEY), "");
    assert_eq!(config(BRANCH_BASE_KEY), unpushed);
}

/// HS2-J3SMMT: a ledger past its bound forgets the oldest confirmed and reopened
/// records first and never provisional or at-risk work.
#[test]
fn hs2_j3smmt_retiring_forgets_settled_records_oldest_first() {
    use super::speculation::{SpeculationEvent, SpeculationEventKind};
    let with = |op: &str, state: SpeculationState, at: i64| {
        let mut entry = record(op, &[("g", GateResult::Pending)]);
        entry.state = state;
        entry.events = vec![SpeculationEvent {
            at,
            kind: SpeculationEventKind::Recorded,
            gate_commit: None,
            result: None,
            run: None,
            actor: None,
        }];
        (op.to_owned(), entry)
    };
    let mut ledger = SpeculationLedger::default();
    // Empty, or within the bound: nothing goes.
    assert!(ledger.retire_settled(2).is_empty());
    ledger.records = [
        with("a-confirmed-old", SpeculationState::Confirmed, 10),
        with("b-reopened-mid", SpeculationState::Reopened, 20),
        with("c-confirmed-new", SpeculationState::Confirmed, 30),
        with("d-provisional-oldest", SpeculationState::Provisional, 1),
        with("e-at-risk-oldest", SpeculationState::AtRisk, 2),
    ]
    .into_iter()
    .collect();
    assert!(ledger.retire_settled(5).is_empty());
    // Two over the bound: the two oldest settled records go, not the older open work.
    assert_eq!(
        ledger.retire_settled(3),
        ["a-confirmed-old", "b-reopened-mid"]
    );
    assert_eq!(ledger.records.len(), 3);
    // However tight the bound, open work stays.
    assert_eq!(ledger.retire_settled(0), ["c-confirmed-new"]);
    let left: Vec<_> = ledger.records.keys().cloned().collect();
    assert_eq!(left, ["d-provisional-oldest", "e-at-risk-oldest"]);
    // Repeating changes nothing; a refill retires again.
    assert!(ledger.retire_settled(0).is_empty());
    let (key, entry) = with("f-confirmed", SpeculationState::Confirmed, 5);
    ledger.records.insert(key, entry);
    assert_eq!(ledger.retire_settled(2), ["f-confirmed"]);
}

fn planned_for(store: &FsStore, t: &Ticket, worker: &str, operation: &str) -> NextPlanEntry {
    NextPlanEntry {
        operation: operation.into(),
        worker: worker.into(),
        generation: GEN.into(),
        ticket: TicketRef {
            connection_id: git_connection_id(store),
            native_id: t.id.to_string(),
        },
        slug: t.slug.clone(),
        priority_at_plan: None,
        updated_at_plan: None,
        scope: Vec::new(),
    }
}

fn write_policy(c: &Coordinator, policy: &SpeculationPolicy) {
    policy
        .write(&hotsheet_ticketing::Settings::for_project(&c.project))
        .unwrap();
}

/// HS2-J77XJA: plan rows and plan-dispatch use the same provisional rule as
/// `assign`. Off, nothing changes; on, a prerequisite awaiting CI no longer holds a row
/// back, the row says it is provisional, the reservation is recorded, a base without
/// the gate commit is refused, and a red base gate stops new provisional dispatch.
#[test]
fn hs2_j77xja_plan_dispatch_uses_provisional_readiness_only_when_enabled() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let facts = Facts::default();
    let prerequisite = awaiting_ci(&store, "Prerequisite", GATE_A);
    let dependent = ticket(&store, "Dependent", vec![prerequisite.id]);
    let second = ticket(&store, "Second dependent", vec![prerequisite.id]);
    facts.push(GATE_A, BASE, GateResult::Pending, Some("run-1"));
    facts
        .contained
        .borrow_mut()
        .push((GATE_A.into(), BASE_2.into()));
    register(&c, Role::Integrator, "i", GEN, NOW);
    register(&c, Role::Worker, "w1", GEN, NOW);
    register(&c, Role::Worker, "w2", GEN, NOW);
    c.replace_next_plan(
        "i",
        GEN,
        &|| NOW,
        0,
        vec![
            planned_for(&store, &dependent, "w1", "plan-1"),
            planned_for(&store, &second, "w2", "plan-2"),
        ],
        true,
    )
    .unwrap();
    let rows = |c: &Coordinator| {
        c.snapshot_probed(NOW, Some(&facts))
            .unwrap()
            .planned
            .into_iter()
            .map(|row| (row.reasons, row.provisional))
            .collect::<Vec<_>>()
    };
    let held = (vec!["dependency_open_or_unknown".to_owned()], false);
    // Off: exactly the confirmed-only answers, and nothing is serialized for it.
    assert_eq!(rows(&c), [held.clone(), held.clone()]);
    let off = serde_json::to_value(c.snapshot_probed(NOW, Some(&facts)).unwrap()).unwrap();
    assert!(off["planned"][0].get("provisional").is_none());
    let refused = c
        .dispatch_next_plan_probed("i", GEN, &|| NOW, 1, "plan-1", BASE, Some(&facts))
        .unwrap_err()
        .to_string();
    assert!(
        refused.contains("open or unavailable dependency"),
        "{refused}"
    );
    assert!(!c.directory.join(LEDGER_FILE).exists());
    // On: both rows are dispatchable, provisionally.
    write_policy(&c, &enabled(IntegrationRule::PushPerTicket));
    let provisional = (Vec::<String>::new(), true);
    assert_eq!(rows(&c), [provisional.clone(), provisional.clone()]);
    // A base that does not contain the pushed gate commit is refused; nothing changes.
    let refused = c
        .dispatch_next_plan_probed("i", GEN, &|| NOW, 1, "plan-1", OTHER, Some(&facts))
        .unwrap_err()
        .to_string();
    assert!(refused.contains("does not contain"), "{refused}");
    assert_eq!(c.state().unwrap().plan_revision, 1);
    let assigned = c
        .dispatch_next_plan_probed("i", GEN, &|| NOW, 1, "plan-1", BASE, Some(&facts))
        .unwrap();
    assert_eq!(assigned.id, "plan-1");
    let ledger = c.speculation_ledger().unwrap().unwrap();
    assert_eq!(
        ledger.records["plan-1"].state,
        SpeculationState::Provisional
    );
    assert_eq!(ledger.records["plan-1"].gates[0].commit, GATE_A);
    // A replayed dispatch keeps the one record.
    c.dispatch_next_plan_probed("i", GEN, &|| NOW, 1, "plan-1", BASE, Some(&facts))
        .unwrap();
    assert_eq!(c.speculation_ledger().unwrap().unwrap().records.len(), 1);
    // A red base gate: the repair comes first, so the other row is held again.
    facts.set(GATE_A, GateResult::Red, Some("run-1"));
    c.speculation_result(
        GATE_A,
        GateObservation {
            result: GateResult::Red,
            run: Some("run-1".into()),
        },
        None,
        NOW,
    )
    .unwrap();
    assert_eq!(rows(&c), std::slice::from_ref(&held));
    assert!(
        c.dispatch_next_plan_probed("i", GEN, &|| NOW, 2, "plan-2", BASE_2, Some(&facts))
            .is_err()
    );
}

/// HS2-J77XJA: under dispatcher authority, a worker match admits a dependent whose
/// prerequisite awaits CI only with the opt-in, marked provisional, and never a
/// dependent of an open prerequisite.
#[test]
fn hs2_j77xja_worker_matches_mark_provisional_candidates() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let facts = Facts::default();
    let prerequisite = awaiting_ci(&store, "Prerequisite", GATE_A);
    let dependent = ticket(&store, "Dependent", vec![prerequisite.id]);
    let open = ticket(&store, "Open prerequisite", Vec::new());
    let blocked = ticket(&store, "Blocked dependent", vec![open.id]);
    facts.push(GATE_A, BASE, GateResult::Pending, Some("run-1"));
    register(&c, Role::Integrator, "i", GEN, NOW);
    c.set_authority("i", GEN, NOW, Authority::Dispatcher, true)
        .unwrap();
    register(&c, Role::Dispatcher, "d", GEN, NOW);
    c.publish("i", GEN, &|| NOW, BASE, Vec::new(), &[], &[], false)
        .unwrap();
    c.register_full(
        Role::Worker,
        "w",
        GEN,
        NOW,
        120,
        ReturnRoute::HumanRelay,
        StrategyRequest::default(),
        PreferenceRequest::replace(
            WorkerPreferences::stated(
                vec!["category:task".parse().unwrap()],
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap(),
            true,
        ),
    )
    .unwrap();
    let candidates = |c: &Coordinator| {
        c.snapshot_probed(NOW, Some(&facts)).unwrap().worker_matches["w"]
            .candidates
            .iter()
            .map(|candidate| (candidate.slug.clone(), candidate.provisional))
            .collect::<std::collections::BTreeMap<_, _>>()
    };
    let off = candidates(&c);
    assert!(!off.contains_key(&dependent.slug) && !off.contains_key(&blocked.slug));
    write_policy(&c, &enabled(IntegrationRule::Parked));
    let on = candidates(&c);
    assert_eq!(on.get(&dependent.slug), Some(&true));
    assert!(!on.contains_key(&blocked.slug));
    assert_eq!(on.get(&open.slug), Some(&false));
}

/// Records every watch request; `refuse` makes each one fail with that reason.
#[derive(Default)]
struct Watches {
    started: RefCell<Vec<(String, String)>>,
    refuse: Option<String>,
}

impl super::speculation_runtime::GateWatch for Watches {
    fn watch(&self, commit: &str, ticket: &TicketRef) -> std::result::Result<(), String> {
        if let Some(reason) = &self.refuse {
            return Err(reason.clone());
        }
        self.started
            .borrow_mut()
            .push((commit.to_owned(), ticket.native_id.clone()));
        Ok(())
    }
}

/// HS2-X6FB9V: a sync starts the watch of every unwatched gate the ledger waits on and
/// of every pushed prerequisite awaiting CI that open work depends on, once; a watched
/// gate, an unpushed commit and a closed dependent start nothing.
#[test]
fn hs2_x6fb9v_sync_starts_the_watches_its_gates_need() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    let reserved = awaiting_ci(&store, "reserved prerequisite", GATE_A);
    let dependent = ticket(&store, "dependent", vec![reserved.id]);
    facts.push(GATE_A, BASE, GateResult::Unknown, None);
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    // Not reserved yet, but open work waits on it.
    let waited_on = awaiting_ci(&store, "awaited prerequisite", GATE_B);
    ticket(&store, "waiting", vec![waited_on.id]);
    facts.push(GATE_B, BASE, GateResult::Unknown, None);
    // Not pushed: never watched.
    let unpushed = awaiting_ci(&store, "unpushed prerequisite", OTHER);
    ticket(&store, "waiting on unpushed", vec![unpushed.id]);
    let watches = Watches::default();
    let report = c
        .speculation_sync_watching(&facts, Some(&watches), Some(&store), NOW)
        .unwrap();
    assert_eq!(report.watched, [GATE_A, GATE_B]);
    assert_eq!(
        *watches.started.borrow(),
        [
            (GATE_A.to_owned(), reserved.id.to_string()),
            (GATE_B.to_owned(), waited_on.id.to_string()),
        ]
    );
    // Once watched (pending), nothing starts again.
    facts.set(GATE_A, GateResult::Pending, None);
    facts.set(GATE_B, GateResult::Pending, None);
    let again = c
        .speculation_sync_watching(&facts, Some(&watches), Some(&store), NOW)
        .unwrap();
    assert!(again.watched.is_empty());
    assert_eq!(watches.started.borrow().len(), 2);
    // A watch that cannot start says why, and the plain sync starts none.
    facts.set(GATE_A, GateResult::Unknown, None);
    let refused = Watches {
        refuse: Some("origin_repository_unknown".into()),
        ..Watches::default()
    };
    let report = c
        .speculation_sync_watching(&facts, Some(&refused), Some(&store), NOW)
        .unwrap();
    assert!(report.watched.is_empty());
    assert_eq!(
        report.watch_unavailable.as_deref(),
        Some("origin_repository_unknown")
    );
    let plain = c.speculation_sync(&facts, Some(&store), NOW).unwrap();
    assert!(plain.watched.is_empty() && plain.watch_unavailable.is_none());
}

/// HS2-X6FB9V: after a red gate the prerequisite's newer `CI gate commit` is reported
/// and watched; landed work moves onto it once it is green, work still on its branch
/// waits for `speculation-rebase --base`.
#[test]
fn hs2_x6fb9v_a_repaired_gate_is_reported_and_replaced_for_landed_work() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let landed = ticket(&store, "landed", vec![blocker.id]);
    let branch = ticket(&store, "on its branch", vec![blocker.id]);
    facts.push(GATE_A, BASE, GateResult::Pending, Some("10.1"));
    reserve(&c, &store, &policy, &facts, "landed", &landed, BASE).unwrap();
    reserve(&c, &store, &policy, &facts, "branch", &branch, BASE).unwrap();
    to_handoff(&c, "landed");
    c.mark("i", GEN, NOW, "landed", Phase::Integrating).unwrap();
    c.mark("i", GEN, NOW, "landed", Phase::Ci).unwrap();
    facts.set(GATE_A, GateResult::Red, Some("10.1"));
    let watches = Watches::default();
    let red = c
        .speculation_sync_watching(&facts, Some(&watches), Some(&store), NOW)
        .unwrap();
    assert!(red.repairs.is_empty(), "no newer gate commit yet");
    assert_eq!(state_of(&c, "landed"), SpeculationState::AtRisk);
    // The prerequisite records its fix.
    ops::add_note(
        &store,
        &blocker.id,
        Ulid::new(),
        at(),
        NoteKind::Regular,
        format!("Fixed and pushed.\n\nCI gate commit: {FIX}\n"),
    )
    .unwrap();
    facts.push(FIX, BASE_2, GateResult::Unknown, None);
    let reported = c
        .speculation_sync_watching(&facts, Some(&watches), Some(&store), NOW)
        .unwrap();
    let repairs: Vec<_> = reported
        .repairs
        .iter()
        .map(|r| {
            (
                r.assignment.as_str(),
                r.fix.as_str(),
                r.fix_result,
                r.replaced,
            )
        })
        .collect();
    assert_eq!(
        repairs,
        [
            ("branch", FIX, GateResult::Unknown, false),
            ("landed", FIX, GateResult::Unknown, false),
        ]
    );
    assert_eq!(reported.watched, [FIX]);
    // Green fix: landed work moves onto it and is confirmed; branch work waits.
    facts.set(FIX, GateResult::Green, Some("11.1"));
    let replaced = c
        .speculation_sync_watching(&facts, Some(&watches), Some(&store), NOW)
        .unwrap();
    let replaced: BTreeMap<_, _> = replaced
        .repairs
        .iter()
        .map(|r| (r.assignment.clone(), r.replaced))
        .collect();
    assert!(replaced["landed"]);
    assert!(!replaced["branch"]);
    assert_eq!(state_of(&c, "landed"), SpeculationState::Confirmed);
    assert_eq!(state_of(&c, "branch"), SpeculationState::AtRisk);
    let ledger = c.speculation_ledger().unwrap().unwrap();
    assert_eq!(ledger.records["landed"].gates[0].commit, FIX);
    // Repeating changes nothing more.
    let again = c
        .speculation_sync_watching(&facts, Some(&watches), Some(&store), NOW)
        .unwrap();
    assert!(again.repairs.iter().all(|r| r.assignment == "branch"));
}

/// HS2-X6FB9V: while its gate is red, the failing prerequisite is offered first.
#[test]
fn hs2_x6fb9v_a_red_gate_puts_its_repair_first_in_worker_matches() {
    let (_root, c) = fixture();
    let store = store_of(&c);
    let policy = enabled(IntegrationRule::PushPerTicket);
    let facts = Facts::default();
    let blocker = awaiting_ci(&store, "blocker", GATE_A);
    let dependent = ticket(&store, "dependent", vec![blocker.id]);
    let urgent = ops::create(
        &store,
        Ulid::new(),
        "HS",
        at(),
        NewTicket {
            title: "highest priority".into(),
            up_next: true,
            priority: hotsheet_model::Priority::Highest,
            ..NewTicket::default()
        },
    )
    .unwrap();
    facts.push(GATE_A, BASE, GateResult::Pending, Some("10.1"));
    reserve(&c, &store, &policy, &facts, "op", &dependent, BASE).unwrap();
    c.register_full(
        Role::Worker,
        "w",
        GEN,
        NOW,
        120,
        ReturnRoute::HumanRelay,
        StrategyRequest::default(),
        PreferenceRequest::replace(
            WorkerPreferences::stated(
                vec!["category:task".parse().unwrap()],
                Vec::new(),
                Vec::new(),
                None,
            )
            .unwrap(),
            true,
        ),
    )
    .unwrap();
    let first = |c: &Coordinator| {
        c.snapshot_probed(NOW, Some(&facts)).unwrap().worker_matches["w"].candidates[0]
            .slug
            .clone()
    };
    assert_eq!(first(&c), urgent.slug);
    c.speculation_apply(&[(GATE_A.into(), read(GateResult::Red, "10.1"))], NOW)
        .unwrap();
    assert_eq!(first(&c), blocker.slug);
}

/// HS2-X6FB9V: the live watch names the checkout's `origin` repository, the policy's
/// branch and workflow, and refuses without an origin.
#[test]
fn hs2_x6fb9v_the_live_watch_creates_one_job_from_origin_and_the_policy() {
    use super::speculation_runtime::GateWatch;
    let root = tempfile::tempdir().unwrap();
    let project = root.path().join("project");
    fs::create_dir(&project).unwrap();
    git(&project, &["init", "-q", "-b", "main"]);
    git(&project, &["commit", "-q", "--allow-empty", "-m", "one"]);
    let pushed = git(&project, &["rev-parse", "HEAD"]);
    let store = root.path().join("store");
    FsStore::init_git(&store, &hotsheet_ticketing::StoreMetadata::new("HS")).unwrap();
    let home = root.path().join("home");
    fs::create_dir(&home).unwrap();
    let c = Coordinator::open_with_registry_home(&project, &store, home.clone()).unwrap();
    let policy = SpeculationPolicy {
        base_branch: Some("main".into()),
        ci_workflow: Some("Build".into()),
        ..enabled(IntegrationRule::PushPerTicket)
    };
    let ticket = TicketRef {
        connection_id: "local".into(),
        native_id: Ulid::new().to_string(),
    };
    assert_eq!(
        c.speculation_watcher(&policy, None).watch(&pushed, &ticket),
        Err("origin_repository_unknown".to_owned())
    );
    git(
        &project,
        &[
            "remote",
            "add",
            "origin",
            "git@github.com:synthetic/repo.git",
        ],
    );
    let watch = c.speculation_watcher(&policy, None);
    watch.watch(&pushed, &ticket).unwrap();
    watch.watch(&pushed, &ticket).unwrap();
    let jobs = hotsheet_jobs::JobStore::at(&home)
        .list(&c.project_id)
        .unwrap()
        .jobs;
    assert_eq!(jobs.len(), 1, "a repeated watch keeps one Job");
    let (target, _) = hotsheet_jobs::CiTarget::parse(jobs[0].external.as_ref().unwrap()).unwrap();
    assert_eq!(
        (
            target.repo.as_str(),
            target.workflow.as_str(),
            target.branch.as_str()
        ),
        ("synthetic/repo", "Build", "main")
    );
    assert_eq!(
        c.speculation_probe(&policy).gate(&pushed).result,
        GateResult::Pending
    );
}
