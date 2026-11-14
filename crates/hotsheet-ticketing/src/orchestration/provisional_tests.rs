//! HS2-JSF985: the cross-target provisional decision, its place in the scheduler's
//! ranking and packing, and the provisional document store. Synthetic targets,
//! tickets, commits and homes only.

use std::collections::BTreeMap;

use super::scheduler_tests::{
    T0, World, blocked, fate, id as work_id, session, starts, target as work_target, work,
};
use super::*;
use crate::provisional::GateResult;

fn id(value: u128) -> String {
    ulid::Ulid::from(value).to_string()
}
fn sha(digit: char) -> String {
    std::iter::repeat_n(digit, 40).collect()
}
/// A ticket of target project `project`.
fn ticket(project: u128, native: &str) -> RunTarget {
    RunTarget {
        target_id: id(project),
        connection_id: "tickets".into(),
        native_id: native.into(),
    }
}
fn requires(target: &RunTarget, gates: GateRequirement, commit: Option<String>) -> Prerequisite {
    Prerequisite::Ticket {
        target_id: target.target_id.clone(),
        connection_id: target.connection_id.clone(),
        native_id: target.native_id.clone(),
        gates,
        commit,
    }
}
fn ci(target: &RunTarget) -> Prerequisite {
    requires(target, GateRequirement::Ci, None)
}
fn step(number: u128, target: &RunTarget, prerequisites: Vec<Prerequisite>) -> PlanStep {
    PlanStep {
        step_id: id(number),
        owner_checkout_id: "owner-0123456789ab".into(),
        ticket: RunTicket {
            connection_id: "plan".into(),
            native_id: id(900),
        },
        target: target.clone(),
        prerequisites,
        resources: Vec::new(),
        tool: None,
        revision: 1,
        created_at_ms: 1,
        updated_at_ms: 1,
        updated_by: None,
    }
}
fn seen(status: &str, opted_out: Option<bool>) -> TicketFacts {
    TicketFacts {
        observed: Some(TicketSeen {
            at_ms: 10,
            exists: true,
            status: status.into(),
            priority: None,
            up_next: None,
            created_at_ms: None,
            opted_out,
            slug: None,
        }),
        unreachable: None,
    }
}
fn gate(state: GateState, commits: &[char]) -> GateFacts {
    GateFacts {
        ci: state,
        ci_reason: None,
        deployment: None,
        commits: commits.iter().map(|digit| sha(*digit)).collect(),
        gate_ids: commits
            .iter()
            .map(|digit| id(5000 + u128::from(u32::from(*digit))))
            .collect(),
    }
}

/// Project 1 holds prerequisite ticket `A`; project 2 holds the dependent `B`.
struct Fixture {
    a: RunTarget,
    b: RunTarget,
    facts: PlanFacts,
    policy: PolicyBody,
    own: BTreeMap<String, TargetSpeculation>,
    steps: Vec<PlanStep>,
}

impl Fixture {
    /// `B` requires `A` with CI; `A` is integrated at commit `a` and its gate pends.
    /// Both policies and both targets opt in with depth 1.
    fn new() -> Self {
        let (a, b) = (ticket(1, "A"), ticket(2, "B"));
        let mut facts = PlanFacts::default();
        facts
            .tickets
            .insert(ticket_key(&a), seen("started", Some(false)));
        facts
            .tickets
            .insert(ticket_key(&b), seen("not_started", Some(false)));
        facts
            .gates
            .insert(ticket_key(&a), gate(GateState::Pending, &['a']));
        let policy = PolicyBody {
            speculation: Speculation {
                allowed: true,
                ..Speculation::default()
            },
            ..PolicyBody::default()
        };
        let on = TargetSpeculation {
            enabled: true,
            max_depth: 1,
            integration: Default::default(),
        };
        Self {
            steps: vec![step(2, &b, vec![ci(&a)])],
            own: [(id(1), on), (id(2), on)].into(),
            a,
            b,
            facts,
            policy,
        }
    }
    fn standings(&self) -> BTreeMap<String, ProvisionalStanding> {
        provisional_standings(
            &evaluate_plan(&self.steps, &self.facts),
            &self.facts,
            &SpeculationInputs {
                policy: &self.policy,
                targets: &self.own,
            },
        )
    }
    fn standing(&self, number: u128) -> ProvisionalStanding {
        self.standings()
            .remove(&id(number))
            .unwrap_or_else(|| panic!("step {number} is not provisional"))
    }
    /// `(allowed, refusal, depth)` of step 2.
    fn answer(&self) -> (bool, Option<String>, u32) {
        let standing = self.standing(2);
        (standing.allowed, standing.refusal, standing.depth)
    }
}

fn target_policy(n: u128, speculation: Option<bool>) -> TargetPolicy {
    TargetPolicy {
        target_id: id(n),
        enabled: true,
        actions: None,
        tools: Vec::new(),
        max_sessions: 1,
        machine: None,
        speculation,
    }
}

#[test]
fn hs2_jsf985_a_pending_gate_is_built_on_only_when_every_policy_allows_it() {
    let allowed = Fixture::new();
    let standing = allowed.standing(2);
    assert_eq!(
        (standing.allowed, standing.depth, standing.max_depth),
        (true, 1, 1)
    );
    assert_eq!(
        standing.bases,
        vec![ProvisionalBasis {
            target_id: id(1),
            connection_id: "tickets".into(),
            native_id: "A".into(),
            commit: sha('a'),
            gate_ids: vec![id(5000 + u128::from(u32::from('a')))],
            gate: GateResult::Pending,
            depth: 1,
            pinned: false,
        }]
    );
    assert!(standing.refusal.is_none() && standing.refused_prerequisite.is_none());

    // One change to the allowed fixture each: who says no is named.
    type Edit = Box<dyn Fn(&mut Fixture)>;
    let off = TargetSpeculation {
        enabled: false,
        max_depth: 1,
        integration: Default::default(),
    };
    let cases: Vec<(&str, Edit, &str, bool)> = vec![
        (
            "the orchestration policy is off",
            Box::new(|f| f.policy.speculation.allowed = false),
            "policy_speculation_not_allowed",
            false,
        ),
        (
            "the policy overrides the working target off",
            Box::new(|f| f.policy.targets = vec![target_policy(2, Some(false))]),
            "policy_speculation_not_allowed",
            false,
        ),
        (
            "the policy overrides the prerequisite's target off",
            Box::new(|f| f.policy.targets = vec![target_policy(1, Some(false))]),
            "prerequisite_speculation_not_allowed",
            true,
        ),
        (
            "the working target's own policy is off",
            Box::new(move |f| {
                f.own.insert(id(2), off);
            }),
            "target_speculation_disabled",
            false,
        ),
        (
            "the prerequisite target's own policy is off",
            Box::new(move |f| {
                f.own.insert(id(1), off);
            }),
            "target_speculation_disabled",
            true,
        ),
        (
            "the working target's own policy is unknown",
            Box::new(|f| {
                f.own.remove(&id(2));
            }),
            "target_speculation_unknown",
            false,
        ),
        (
            "the prerequisite target's own policy is unknown",
            Box::new(|f| {
                f.own.remove(&id(1));
            }),
            "target_speculation_unknown",
            true,
        ),
        (
            "the dependent carries the opt-out tag",
            Box::new(|f| {
                f.facts
                    .tickets
                    .insert(ticket_key(&f.b), seen("not_started", Some(true)));
            }),
            "speculation_opted_out",
            true,
        ),
        (
            "the prerequisite carries the opt-out tag",
            Box::new(|f| {
                f.facts
                    .tickets
                    .insert(ticket_key(&f.a), seen("started", Some(true)));
            }),
            "speculation_opted_out",
            true,
        ),
        (
            "the dependent's tags were never read",
            Box::new(|f| {
                f.facts
                    .tickets
                    .insert(ticket_key(&f.b), seen("not_started", None));
            }),
            "opt_out_unknown",
            false,
        ),
        (
            "the prerequisite's tags were never read",
            Box::new(|f| {
                f.facts
                    .tickets
                    .insert(ticket_key(&f.a), seen("started", None));
            }),
            "opt_out_unknown",
            true,
        ),
        (
            "the prerequisite also needs a deployment",
            Box::new(|f| {
                f.steps = vec![step(
                    2,
                    &f.b,
                    vec![requires(&f.a, GateRequirement::Deployment, None)],
                )];
            }),
            "deployment_gate_pending",
            true,
        ),
        (
            "the standing gates are about two commits",
            Box::new(|f| {
                f.facts
                    .gates
                    .insert(ticket_key(&f.a), gate(GateState::Pending, &['a', 'c']));
            }),
            "gate_commit_ambiguous",
            true,
        ),
    ];
    for (name, edit, code, about_prerequisite) in cases {
        let mut fixture = Fixture::new();
        edit(&mut fixture);
        let standing = fixture.standing(2);
        assert_eq!(
            (standing.allowed, standing.refusal.as_deref()),
            (false, Some(code)),
            "{name}"
        );
        assert_eq!(
            standing.refused_prerequisite.is_some(),
            about_prerequisite,
            "{name}"
        );
        assert!(standing.bases.is_empty() && standing.depth == 0, "{name}");
    }

    // A policy that is off everywhere except the working target opts that target in.
    let mut only = Fixture::new();
    only.policy.speculation.allowed = false;
    only.policy.targets = vec![target_policy(2, Some(true)), target_policy(1, Some(true))];
    assert_eq!(only.answer(), (true, None, 1));
    assert!(only.policy.speculates());
    assert!(!PolicyBody::default().speculates());
}

#[test]
fn hs2_jsf985_only_the_provisional_level_gets_an_answer() {
    // Gate state by ticket state: what §26.15 calls the step, and what the rule adds.
    let cases = [
        (GateState::Pending, "started", Some((true, None, 1))),
        // Green and not yet confirmed closed: nothing is pending beneath it.
        (GateState::Passed, "started", Some((true, None, 0))),
        (GateState::Pending, "completed", Some((true, None, 1))),
        (GateState::Passed, "completed", None),
        (GateState::Failed, "started", None),
        (GateState::Unknown, "completed", None),
        (GateState::Unknown, "started", None),
    ];
    for (state, status, expected) in cases {
        let mut fixture = Fixture::new();
        fixture
            .facts
            .tickets
            .insert(ticket_key(&fixture.a), seen(status, Some(false)));
        fixture
            .facts
            .gates
            .insert(ticket_key(&fixture.a), gate(state, &['a']));
        let found = fixture
            .standings()
            .remove(&id(2))
            .map(|standing| (standing.allowed, standing.refusal, standing.depth));
        assert_eq!(found, expected, "{state:?} {status}");
    }
    // A pinned commit is judged by that commit's gates and marked as pinned.
    let mut pinned = Fixture::new();
    pinned.steps = vec![step(
        2,
        &pinned.b,
        vec![requires(&pinned.a, GateRequirement::Ci, Some(sha('c')))],
    )];
    pinned
        .facts
        .commit_gates
        .insert((id(1), sha('c')), gate(GateState::Pending, &['c']));
    let standing = pinned.standing(2);
    assert_eq!(
        (
            standing.allowed,
            standing.bases[0].commit.clone(),
            standing.bases[0].pinned
        ),
        (true, sha('c'), true)
    );
}

#[test]
fn hs2_jsf985_depth_counts_stacked_gates_and_takes_the_smallest_limit() {
    // Z (project 3) <- A (project 1) <- B (project 2). A was itself started ahead of
    // Z's gate, is integrated, and its own gate pends: two unconfirmed gates under B.
    let mut fixture = Fixture::new();
    let z = ticket(3, "Z");
    fixture
        .facts
        .tickets
        .insert(ticket_key(&z), seen("started", Some(false)));
    fixture
        .facts
        .gates
        .insert(ticket_key(&z), gate(GateState::Pending, &['d']));
    fixture.steps = vec![
        step(1, &fixture.a, vec![ci(&z)]),
        step(2, &fixture.b, vec![ci(&fixture.a)]),
    ];
    fixture.facts.active.insert(ticket_key(&fixture.a));
    let deep = TargetSpeculation {
        enabled: true,
        max_depth: 2,
        integration: Default::default(),
    };
    fixture.own.insert(id(3), deep);
    let refused = (false, Some("speculation_depth_exceeded".to_owned()), 0);

    // Every limit at 1: one gate too many.
    assert_eq!(fixture.answer(), refused);
    assert_eq!(fixture.standing(2).max_depth, 1);
    // The policy alone raised: each target still says 1.
    fixture.policy.speculation.max_depth = 2;
    assert_eq!(fixture.answer(), refused);
    // The working target raised too, the prerequisite's target not: still 1.
    fixture.own.insert(id(2), deep);
    assert_eq!(fixture.answer(), refused);
    assert_eq!(fixture.standing(2).max_depth, 1);
    // All three at 2: allowed at depth 2, exactly at the limit.
    fixture.own.insert(id(1), deep);
    assert_eq!(fixture.answer(), (true, None, 2));
    assert_eq!(fixture.standing(2).max_depth, 2);
    // The policy lowered again mid-way: refused again.
    fixture.policy.speculation.max_depth = 1;
    assert_eq!(fixture.answer(), refused);

    // Z's gate turns green and Z closes: one gate is left, and depth 1 is enough.
    fixture
        .facts
        .tickets
        .insert(ticket_key(&z), seen("completed", Some(false)));
    fixture
        .facts
        .gates
        .insert(ticket_key(&z), gate(GateState::Passed, &['d']));
    assert_eq!(fixture.answer(), (true, None, 1));
    // Z's gate green while Z is not yet confirmed closed counts for nothing either.
    fixture
        .facts
        .tickets
        .insert(ticket_key(&z), seen("started", Some(false)));
    assert_eq!(fixture.answer(), (true, None, 1));
    // And red again (a later attempt of the same gate): A is not provisional work any
    // more beneath B, it is blocked work, which still stacks.
    fixture
        .facts
        .gates
        .insert(ticket_key(&z), gate(GateState::Failed, &['d']));
    assert_eq!(fixture.answer(), refused);
}

#[test]
fn hs2_jsf985_a_cycle_beneath_a_prerequisite_never_confirms_and_never_hangs() {
    // A and C (both project 1) require each other; B requires A, whose gate pends.
    let mut fixture = Fixture::new();
    let c = ticket(1, "C");
    fixture
        .facts
        .tickets
        .insert(ticket_key(&c), seen("started", Some(false)));
    fixture
        .facts
        .gates
        .insert(ticket_key(&c), gate(GateState::Pending, &['c']));
    fixture.steps = vec![
        step(1, &fixture.a, vec![ci(&c)]),
        step(3, &c, vec![ci(&fixture.a)]),
        step(2, &fixture.b, vec![ci(&fixture.a)]),
    ];
    fixture.policy.speculation.max_depth = 8;
    for target in fixture.own.values_mut() {
        target.max_depth = 8;
    }
    assert_eq!(
        fixture.answer(),
        (false, Some("speculation_depth_exceeded".to_owned()), 0)
    );
    // The same plan in any order gives the same answer.
    let first = fixture.standings();
    fixture.steps.reverse();
    assert_eq!(fixture.standings(), first);
}

fn provisional(n: u128, on: u128, rule: Option<(bool, Option<&str>)>) -> WorkItem {
    let mut item = work(n, on);
    item.level = WorkLevel::Provisional;
    item.speculation = rule.map(|(allowed, refusal)| WorkSpeculation {
        allowed,
        refusal: refusal.map(Into::into),
        depth: 1,
    });
    item
}

#[test]
fn hs2_jsf985_without_the_opt_in_the_schedule_is_what_it_always_was() {
    // The rule's answer is present and says yes; the policy never opted in.
    let mut world = World::new(&[(1, 2)]);
    world.work = vec![work(1, 1), provisional(2, 1, Some((true, None)))];
    world.sessions = vec![session("idle", 1, SessionActivity::Idle)];
    let with_answer = world.now();
    world.work[1].speculation = None;
    let without = world.now();
    assert_eq!(with_answer, without);
    assert_eq!(
        blocked(&without, 2),
        ("provisional_not_dispatched", None),
        "the reason and its absent detail are unchanged"
    );
    assert!(
        without
            .placements
            .iter()
            .all(|placement| !placement.provisional)
    );
    // Nothing new appears on the wire while nothing is provisional.
    let wire = serde_json::to_value(&without).unwrap();
    assert!(
        wire["placements"]
            .as_array()
            .unwrap()
            .iter()
            .all(|placement| placement.get("provisional").is_none())
    );
    let item = serde_json::to_value(&world.work[0]).unwrap();
    assert!(item.get("speculation").is_none());
    // A tie between confirmed and provisional work keeps its old order: the work
    // that has been ready longer first.
    world.sessions.clear();
    world.policy.speculation.risk_weight = 0;
    world.work[1].ready_since_ms = Some(T0 - 1);
    assert_eq!(world.now().ranking[0].step_id, work_id(2));
    // The opt-in for another target changes nothing for this one's placement.
    world.policy.targets[0].speculation = Some(false);
    world.policy.speculation.allowed = true;
    world.work[1].speculation = Some(WorkSpeculation {
        allowed: true,
        refusal: None,
        depth: 1,
    });
    assert_eq!(
        blocked(&world.now(), 2),
        ("provisional_not_dispatched", None)
    );
}

#[test]
fn hs2_jsf985_allowed_provisional_work_is_placed_below_confirmed_work() {
    let mut world = World::new(&[(1, 1)]);
    world.policy.speculation.allowed = true;
    world.policy.speculation.risk_weight = 0;
    // Equal score, the provisional item ready longer and with the lower step id: the
    // confirmed one still goes first and takes the target's one session.
    let mut waiting = provisional(1, 1, Some((true, None)));
    waiting.ready_since_ms = Some(T0 - 1);
    world.work = vec![waiting, work(2, 1)];
    let schedule = world.now();
    assert_eq!(schedule.ranking[0].step_id, work_id(2));
    assert_eq!(starts(&schedule), vec![(work_id(2), "alpha".into())]);
    assert_eq!(blocked(&schedule, 1), ("target_cap", None));

    // With room for both, the provisional start is marked and counted like any other.
    world.policy.targets[0].max_sessions = 2;
    let schedule = world.now();
    let placed: Vec<_> = schedule
        .placements
        .iter()
        .map(|placement| {
            (
                placement.steps[0].clone(),
                placement.reason.code(),
                placement.provisional,
                placement.inputs.target_sessions,
            )
        })
        .collect();
    assert_eq!(
        placed,
        vec![
            (work_id(2), "ready_work", false, 1),
            (work_id(1), "provisional_work", true, 2),
        ]
    );
    let wire = serde_json::to_value(&schedule.placements[1]).unwrap();
    assert_eq!(wire["provisional"], true);
    assert_eq!(wire["reason"], "provisional_work");

    // The risk weight scales the whole score: at 50 a `highest` provisional item
    // (4000) ranks with `default` confirmed work (2000) and so below it; a confirmed
    // `low` item (1000) ranks below both.
    world.policy.speculation.risk_weight = 50;
    world.work[0].priority = Some("highest".into());
    let mut low = work(3, 1);
    low.priority = Some("low".into());
    world.work.push(low);
    let ranking: Vec<_> = world
        .now()
        .ranking
        .into_iter()
        .map(|ranked| ranked.step_id)
        .collect();
    assert_eq!(ranking, vec![work_id(2), work_id(1), work_id(3)]);
    // At 100 it ranks last and is still placed when there is room.
    world.policy.speculation.risk_weight = 100;
    world.policy.targets[0].max_sessions = 3;
    let schedule = world.now();
    assert_eq!(schedule.ranking.last().unwrap().step_id, work_id(1));
    assert_eq!(starts(&schedule).len(), 3);

    // A free session takes provisional work instead of a new one starting.
    world.sessions = vec![session("idle", 1, SessionActivity::Idle)];
    world.work = vec![provisional(1, 1, Some((true, None)))];
    let schedule = world.now();
    assert_eq!(fate(&schedule, "idle"), ("keep", "refill"));
    assert!(schedule.placements[0].provisional);
    assert!(starts(&schedule).is_empty());
}

#[test]
fn hs2_jsf985_refused_or_undecided_provisional_work_is_never_placed() {
    let mut world = World::new(&[(1, 2), (2, 2)]);
    world.policy.speculation.allowed = true;
    world.work = vec![
        provisional(1, 1, Some((false, Some("speculation_depth_exceeded")))),
        provisional(2, 1, Some((false, Some("target_speculation_disabled")))),
        // The caller listed it without asking the rule: never placed.
        provisional(3, 1, None),
        provisional(4, 2, Some((true, None))),
    ];
    // The policy's own switch for target 2 is off, whatever the rule said.
    world.policy.targets[1].speculation = Some(false);
    world.sessions = vec![session("idle", 1, SessionActivity::Idle)];
    let schedule = world.now();
    assert!(starts(&schedule).is_empty());
    assert_eq!(
        blocked(&schedule, 1),
        (
            "speculation_refused",
            Some("speculation_depth_exceeded".into())
        )
    );
    assert_eq!(
        blocked(&schedule, 2),
        (
            "speculation_refused",
            Some("target_speculation_disabled".into())
        )
    );
    assert_eq!(blocked(&schedule, 3), ("provisional_not_dispatched", None));
    assert_eq!(blocked(&schedule, 4), ("provisional_not_dispatched", None));
    // The idle session was offered none of it.
    assert_ne!(fate(&schedule, "idle").1, "refill");
    assert_eq!(Blocker::SpeculationRefused.code(), "speculation_refused");
    assert_eq!(
        serde_json::to_value(Blocker::SpeculationRefused).unwrap(),
        "speculation_refused"
    );
    assert_eq!(work_target(1), world.policy.targets[0].target_id);
}

#[derive(Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
struct Document {
    #[serde(default)]
    entries: Vec<String>,
}

/// HS2-J3SMMT: this test document keeps its newest three entries.
impl Retiring for Document {
    fn retire(&mut self) -> bool {
        let excess = self.entries.len().saturating_sub(3);
        self.entries.drain(..excess);
        excess > 0
    }
}

#[test]
fn hs2_jsf985_the_provisional_store_is_created_only_by_a_change() {
    let home = tempfile::tempdir().unwrap();
    let store = ProvisionalStore::new(home.path());
    let path = home.path().join("orchestration-provisional.json");
    let (first, second) = ("owner-0123456789ab", "owner-ba9876543210");

    // Reads, and a change that changes nothing, leave no file behind.
    assert_eq!(store.read::<Document>(first).unwrap(), None);
    store
        .update::<Document, _>(first, |_| Ok(((), false)))
        .unwrap();
    assert!(!path.exists());

    let push = |owner: &str, entry: &str| {
        store
            .update::<Document, _>(owner, |document| {
                document.entries.push(entry.into());
                Ok((document.entries.len(), true))
            })
            .unwrap()
    };
    assert_eq!(push(first, "one"), 1);
    assert_eq!(push(first, "two"), 2);
    assert_eq!(push(second, "other"), 1);
    assert_eq!(
        store.read::<Document>(first).unwrap().unwrap().entries,
        ["one", "two"]
    );
    assert_eq!(
        store.read::<Document>(second).unwrap().unwrap().entries,
        ["other"]
    );
    // An unchanged answer is not rewritten, and a refused change writes nothing.
    let before = std::fs::read(&path).unwrap();
    store
        .update::<Document, _>(first, |document| {
            document.entries.clear();
            Ok(((), false))
        })
        .unwrap();
    assert!(matches!(
        store.update::<Document, ()>(first, |document| {
            document.entries.clear();
            Err(OrchestrationError::Conflict)
        }),
        Err(OrchestrationError::Conflict)
    ));
    assert_eq!(std::fs::read(&path).unwrap(), before);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600);
    }

    // Damaged or newer data is an error and is never overwritten.
    for (bytes, newer) in [
        (&b"{\"schema_version\":2,\"owners\":{}}"[..], true),
        (&b"not json"[..], false),
    ] {
        std::fs::write(&path, bytes).unwrap();
        let read = store.read::<Document>(first);
        let written = store.update::<Document, _>(first, |_| Ok(((), true)));
        for result in [read.map(|_| ()), written] {
            match result {
                Err(OrchestrationError::UnsupportedVersion { found: 2 }) if newer => {}
                Err(OrchestrationError::Corrupt) if !newer => {}
                other => panic!("unexpected {other:?}"),
            }
        }
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_jsf985_new_fields_are_absent_until_they_say_something() {
    // A capacity snapshot from a peer that predates the field reads as "not said".
    let snapshot = super::scheduler_tests::snapshot(1, 8192, 0);
    let mut wire = serde_json::to_value(&snapshot).unwrap();
    assert!(wire.get("speculation").is_none());
    assert_eq!(
        serde_json::from_value::<CapacitySnapshot>(wire.clone())
            .unwrap()
            .speculation,
        None
    );
    let said = snapshot.with_speculation(Some(TargetSpeculation {
        enabled: true,
        max_depth: 2,
        integration: Default::default(),
    }));
    wire = serde_json::to_value(&said).unwrap();
    assert_eq!(
        wire["speculation"],
        serde_json::json!({"enabled": true, "max_depth": 2})
    );
    assert_eq!(
        serde_json::from_value::<CapacitySnapshot>(wire).unwrap(),
        said
    );

    // A ticket read stored before the opt-out field loads, and a newer read that
    // does not say keeps what an older one said.
    let old: TicketSeen =
        serde_json::from_str(r#"{"at_ms":5,"exists":true,"status":"started"}"#).unwrap();
    assert_eq!(old.opted_out, None);
    assert!(
        serde_json::to_value(&old)
            .unwrap()
            .get("opted_out")
            .is_none()
    );
    let merged = seen("started", Some(true)).merge(TicketFacts {
        observed: Some(TicketSeen { at_ms: 20, ..old }),
        unreachable: None,
    });
    assert_eq!(merged.observed.unwrap().opted_out, Some(true));

    assert_eq!(
        ReadinessError::ProvisionalRefused.code(),
        "readiness_provisional_refused"
    );
    assert_eq!(
        ReadinessError::NotProvisional.code(),
        "readiness_not_provisional"
    );
    // The level's own refusal is untouched: the default answer is the old one.
    assert_eq!(
        ReadinessLevel::Provisional.refusal(),
        Some(ReadinessError::Provisional)
    );
    assert_eq!(
        serde_json::to_value(RunEventKind::Provisional).unwrap(),
        "provisional"
    );
}

/// HS2-J3SMMT: the store asks a changed document to retire what is beyond its bound
/// before it is written, so many changes never reach the file's limit; an unchanged
/// document is neither retired nor written.
#[test]
fn hs2_j3smmt_a_changed_document_retires_before_it_is_written() {
    let home = tempfile::tempdir().unwrap();
    let store = ProvisionalStore::new(home.path());
    let owner = "owner-0123456789ab";
    for n in 0..1_000 {
        store
            .update::<Document, _>(owner, |document| {
                document.entries.push(format!("entry-{n}"));
                Ok(((), true))
            })
            .unwrap();
    }
    assert_eq!(
        store.read::<Document>(owner).unwrap().unwrap().entries,
        ["entry-997", "entry-998", "entry-999"]
    );
    // A change that says nothing changed is not retired or written, even beyond the
    // bound in memory.
    let path = home.path().join("orchestration-provisional.json");
    let before = std::fs::read(&path).unwrap();
    store
        .update::<Document, _>(owner, |document| {
            document.entries.push("not kept".into());
            Ok(((), false))
        })
        .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
}

/// HS2-08F1M8: standing CI gates about several commits are built on with every commit
/// as a base; each commit is judged by its own gates, a green one is no base, a red
/// one refuses, and a commit without its own gate facts stays ambiguous.
#[test]
fn hs2_08f1m8_gates_about_several_commits_make_every_commit_a_base() {
    let several = |c: GateState| {
        let mut f = Fixture::new();
        f.facts
            .gates
            .insert(ticket_key(&f.a), gate(GateState::Pending, &['a', 'c']));
        f.facts
            .commit_gates
            .insert((id(1), sha('a')), gate(GateState::Pending, &['a']));
        f.facts
            .commit_gates
            .insert((id(1), sha('c')), gate(c, &['c']));
        f
    };
    let bases = |standing: &ProvisionalStanding| -> Vec<(String, GateResult, bool)> {
        standing
            .bases
            .iter()
            .map(|basis| (basis.commit.clone(), basis.gate, basis.pinned))
            .collect()
    };
    // Both pending: both commits are bases, side by side, so the depth is one.
    let both = several(GateState::Pending).standing(2);
    assert!(both.allowed, "{both:?}");
    assert_eq!(both.depth, 1);
    assert_eq!(
        bases(&both),
        [
            (sha('a'), GateResult::Pending, false),
            (sha('c'), GateResult::Pending, false),
        ]
    );
    assert!(both.bases.iter().all(|basis| basis.native_id == "A"));
    // One commit already green: only the pending one is a base.
    let one = several(GateState::Passed).standing(2);
    assert!(one.allowed);
    assert_eq!(bases(&one), [(sha('a'), GateResult::Pending, false)]);
    // One commit red: refused, as a single red gate is.
    let red = several(GateState::Failed).standing(2);
    assert_eq!(
        (red.allowed, red.refusal.as_deref()),
        (false, Some("gate_red"))
    );
    // A commit whose own gates are unknown: still ambiguous.
    let mut unknown = several(GateState::Pending);
    unknown.facts.commit_gates.remove(&(id(1), sha('c')));
    assert_eq!(
        unknown.standing(2).refusal.as_deref(),
        Some("gate_commit_ambiguous")
    );
}
