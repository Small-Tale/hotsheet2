//! HS2-ECWG50 scheduler core: scoring, packing, the oscillation guard, refusal
//! backoff, the decision journal and the placement memory. Synthetic targets, tools
//! and homes only; every time is an argument.

use super::*;

pub(super) const MIN: u64 = 60 * 1000;
const HOUR: u64 = 60 * MIN;
const DAY: u64 = 24 * HOUR;
pub(super) const T0: u64 = 20_000 * DAY + 12 * HOUR;
pub(super) const OWNER: &str = "owner-a1b2c3";

/// One change to an otherwise valid value, for a table of cases.
type Edit<T> = Box<dyn Fn(&mut T)>;

pub(super) const FULL: SignalSupport = SignalSupport {
    limits: true,
    reset_times: true,
    usage: true,
    cost: true,
};

pub(super) fn id(n: u128) -> String {
    ulid::Ulid::from(n).to_string()
}
pub(super) fn target(n: u128) -> String {
    id(1000 + n)
}

fn tool(name: &str, max_sessions: u32, weight: u32) -> ToolPolicy {
    ToolPolicy {
        tool: name.into(),
        max_sessions,
        weight,
        accounts: Vec::new(),
    }
}

fn target_policy(n: u128, max_sessions: u32, tools: &[&str]) -> TargetPolicy {
    TargetPolicy {
        target_id: target(n),
        enabled: true,
        actions: None,
        tools: tools
            .iter()
            .map(|tool| ToolScope {
                tool: (*tool).into(),
                models: Vec::new(),
            })
            .collect(),
        max_sessions,
        machine: None,
        speculation: None,
    }
}

/// Two tools, `alpha` preferred, and the given targets, each allowed both tools.
pub(super) fn policy(targets: &[(u128, u32)]) -> PolicyBody {
    PolicyBody {
        machine: MachineLimits {
            max_sessions: 4,
            ..MachineLimits::default()
        },
        tools: vec![tool("alpha", 4, 60), tool("beta", 4, 50)],
        targets: targets
            .iter()
            .map(|(n, cap)| target_policy(*n, *cap, &["alpha", "beta"]))
            .collect(),
        ..PolicyBody::default()
    }
}

pub(super) fn work(n: u128, on: u128) -> WorkItem {
    WorkItem {
        step_id: id(n),
        target_id: target(on),
        connection_id: "git".into(),
        native_id: format!("T-{n}"),
        level: WorkLevel::Confirmed,
        priority: None,
        up_next: false,
        due_at_ms: None,
        unblocks: 0,
        red_gate_stack: 0,
        ready_since_ms: Some(T0),
        created_at_ms: None,
        tool: None,
        model: None,
        resources: Vec::new(),
        speculation: None,
    }
}
fn prio(mut item: WorkItem, priority: &str) -> WorkItem {
    item.priority = Some(priority.into());
    item
}

/// A machine with both tools installed, a quiet CPU and plenty of disk.
pub(super) fn snapshot(server: u128, free_ram_mb: u64, running: u32) -> CapacitySnapshot {
    CapacitySnapshot {
        schema_version: 1,
        server_id: id(9000 + server),
        checkout_id: "checkout".into(),
        sampled_at_ms: Some(T0),
        memory: MemoryCapacity {
            free_mb: Some(free_ram_mb),
            total_mb: Some(32_768),
        },
        cpu: CpuCapacity {
            load_percent: Some(10),
            logical_cpus: Some(8),
        },
        disk: DiskCapacity {
            free_mb: Some(100_000),
            total_mb: Some(500_000),
        },
        tools: Some(
            ["alpha", "beta"]
                .iter()
                .map(|tool| ToolAvailability {
                    tool: (*tool).into(),
                    installed: true,
                    configured: true,
                })
                .collect(),
        ),
        sessions: Some(
            (0..running)
                .map(|n| SessionFootprint {
                    session_id: format!("m{n}"),
                    tool: Some("alpha".into()),
                    busy: true,
                    on_checkout: Some(true),
                    resident_mb: None,
                })
                .collect(),
        ),
        claims: None,
        claim_namespaces: Vec::new(),
        claims_offline: None,
        speculation: None,
        version: None,
        pool: None,
    }
}
pub(super) fn state(n: u128, capacity: CapacitySnapshot) -> TargetState {
    TargetState {
        target_id: target(n),
        capacity: Some(capacity),
        unavailable: None,
    }
}
fn roomy(n: u128) -> TargetState {
    state(n, snapshot(n, 16_384, 0))
}

fn signal(tool: &str, kind: SignalKind, at_ms: u64, resets_at_ms: Option<u64>) -> LimitSignal {
    LimitSignal {
        tool: tool.into(),
        account: None,
        kind,
        at_ms,
        resets_at_ms,
        cost_cents: None,
        session: None,
    }
}
fn entry(signals: &[LimitSignal]) -> CapacityEntry {
    let mut entry = None;
    for signal in signals {
        entry = Some(apply_signal(entry, signal).0);
    }
    entry.unwrap()
}
/// Both tools observed working.
fn healthy() -> Vec<CapacityEntry> {
    ["alpha", "beta"]
        .iter()
        .map(|tool| entry(&[signal(tool, SignalKind::Usage, T0 - HOUR, None)]))
        .collect()
}
fn limited(tool: &str, until_ms: u64) -> CapacityEntry {
    entry(&[
        signal(tool, SignalKind::Usage, T0 - HOUR, None),
        signal(tool, SignalKind::UsageLimit, T0 - MIN, Some(until_ms)),
    ])
}

pub(super) fn session(name: &str, on: u128, activity: SessionActivity) -> ScheduledSession {
    ScheduledSession {
        session_id: name.into(),
        target_id: target(on),
        tool: "alpha".into(),
        account: None,
        model: None,
        // A busy session works a run; one without a run is free capacity.
        run_id: (activity == SessionActivity::Busy).then(|| id(70)),
        owner_id: Some(OWNER.into()),
        activity,
        started_at_ms: T0 - 2 * HOUR,
        idle_since_ms: (activity == SessionActivity::Idle).then_some(T0 - MIN),
        safe_point: activity == SessionActivity::Idle,
        human_takeover: false,
        winding_down: false,
        role: None,
    }
}

#[derive(Default)]
pub(super) struct World {
    pub(super) policy: PolicyBody,
    pub(super) work: Vec<WorkItem>,
    pub(super) targets: Vec<TargetState>,
    pub(super) sessions: Vec<ScheduledSession>,
    pub(super) tools: Vec<CapacityEntry>,
    pub(super) held: Vec<HeldResource>,
    pub(super) memory: Vec<PlacementMemory>,
    /// HS2-2FG91X
    pub(super) unreadable: Vec<UnreadableClaims>,
    /// HS2-AWB7BX
    pub(super) capacity_streams: Vec<String>,
}

impl World {
    pub(super) fn new(targets: &[(u128, u32)]) -> Self {
        Self {
            policy: policy(targets),
            targets: targets.iter().map(|(n, _)| roomy(*n)).collect(),
            tools: healthy(),
            ..Self::default()
        }
    }
    pub(super) fn at(&self, now_ms: u64) -> Schedule {
        plan(
            &ScheduleInput {
                now_ms,
                owner_id: OWNER,
                policy: &self.policy,
                work: &self.work,
                targets: &self.targets,
                sessions: &self.sessions,
                tool_entries: &self.tools,
                held: &self.held,
                unreadable: &self.unreadable,
                memory: &self.memory,
                capacity_streams: &self.capacity_streams,
            },
            &|_| FULL,
        )
    }
    pub(super) fn now(&self) -> Schedule {
        self.at(T0)
    }
}

pub(super) fn starts(schedule: &Schedule) -> Vec<(String, String)> {
    schedule
        .placements
        .iter()
        .filter(|placement| placement.action == PlacementAction::Start)
        .map(|placement| (placement.steps[0].clone(), placement.tool.clone()))
        .collect()
}
pub(super) fn blocked(schedule: &Schedule, n: u128) -> (&'static str, Option<String>) {
    let item = schedule
        .unplaced
        .iter()
        .find(|item| item.step_id == id(n))
        .unwrap_or_else(|| panic!("step {n} was placed: {schedule:#?}"));
    (item.reason.code(), item.detail.clone())
}
pub(super) fn fate(schedule: &Schedule, name: &str) -> (&'static str, &'static str) {
    let placement = schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no decision for session {name}"));
    (placement.action.code(), placement.reason.code())
}

// ---- Scoring ----

#[test]
fn hs2_ecwg50_priority_and_urgency_order_work() {
    let policy = policy(&[(1, 1)]);
    let base = || work(1, 1);
    let with = |edit: &dyn Fn(&mut WorkItem)| {
        let mut item = base();
        edit(&mut item);
        item
    };
    // (name, the work that must rank first, the work it outranks)
    let cases: Vec<(&str, WorkItem, WorkItem)> = vec![
        (
            "a higher band wins with equal urgency",
            prio(base(), "high"),
            prio(base(), "default"),
        ),
        ("an absent priority is default", base(), prio(base(), "low")),
        (
            "an unknown priority is default",
            prio(base(), "sideways"),
            prio(base(), "lowest"),
        ),
        (
            "Up Next lifts default work over plain high work",
            with(&|item| item.up_next = true),
            prio(base(), "high"),
        ),
        (
            "Up Next does not lift default work over highest",
            prio(base(), "highest"),
            with(&|item| item.up_next = true),
        ),
        (
            "an overdue date outranks Up Next",
            with(&|item| item.due_at_ms = Some(T0 - 1)),
            with(&|item| item.up_next = true),
        ),
        (
            "due within a day outranks due within a week",
            with(&|item| item.due_at_ms = Some(T0 + HOUR)),
            with(&|item| item.due_at_ms = Some(T0 + 3 * DAY)),
        ),
        (
            "due within a week outranks a far date",
            with(&|item| item.due_at_ms = Some(T0 + 3 * DAY)),
            with(&|item| item.due_at_ms = Some(T0 + 30 * DAY)),
        ),
        (
            "unblocking more outranks unblocking less",
            with(&|item| item.unblocks = 3),
            with(&|item| item.unblocks = 1),
        ),
        (
            "a red gate's repair outranks new work of equal priority",
            with(&|item| item.red_gate_stack = 1),
            with(&|item| item.up_next = true),
        ),
        (
            "more stacked work makes the repair more urgent",
            with(&|item| item.red_gate_stack = 4),
            with(&|item| item.red_gate_stack = 1),
        ),
        (
            "waiting longer outranks equal fresh work",
            with(&|item| item.ready_since_ms = Some(T0 - 30 * MIN)),
            base(),
        ),
        (
            "confirmed outranks equal provisional work at the default risk weight",
            base(),
            with(&|item| item.level = WorkLevel::Provisional),
        ),
    ];
    for (name, first, second) in cases {
        let (a, b) = (score(&first, &policy, T0), score(&second, &policy, T0));
        assert!(a.total > b.total, "{name}: {a:?} vs {b:?}");
    }
    // The capped terms stop growing.
    let unblocks = |n| score(&with(&|item| item.unblocks = n), &policy, T0).unblocks;
    assert_eq!(unblocks(5), unblocks(500));
    let stack = |n| score(&with(&|item| item.red_gate_stack = n), &policy, T0).red_gate;
    assert_eq!(stack(8), stack(800));
    // Every term is reported, and they add up.
    let all = with(&|item| {
        item.priority = Some("high".into());
        item.up_next = true;
        item.due_at_ms = Some(T0);
        item.unblocks = 2;
        item.red_gate_stack = 2;
        item.ready_since_ms = Some(T0 - 90 * MIN);
    });
    assert_eq!(
        score(&all, &policy, T0),
        ScoreParts {
            priority: 3000,
            up_next: 1500,
            due: 2000,
            unblocks: 400,
            red_gate: 2500,
            aging: 1500,
            risk_percent: 0,
            total: 10_900,
        }
    );
    // A time before the work became ready, or no time at all, ages nothing.
    assert_eq!(score(&base(), &policy, T0 - HOUR).aging, 0);
    let unknown = with(&|item| item.ready_since_ms = None);
    assert_eq!(score(&unknown, &policy, T0 + DAY).aging, 0);
    let created = with(&|item| {
        item.ready_since_ms = None;
        item.created_at_ms = Some(T0 - HOUR);
    });
    assert_eq!(score(&created, &policy, T0).aging, 1000);
    // No overflow at the far end of time.
    assert!(
        score(
            &with(&|item| item.ready_since_ms = Some(0)),
            &policy,
            u64::MAX
        )
        .total
            > 0
    );
}

#[test]
fn hs2_ecwg50_provisional_work_is_ranked_by_risk_weight_and_never_placed() {
    let mut world = World::new(&[(1, 2)]);
    let mut provisional = prio(work(2, 1), "highest");
    provisional.level = WorkLevel::Provisional;
    world.work = vec![work(1, 1), provisional.clone()];
    for (risk, first) in [(0, 2), (50, 1), (100, 1)] {
        world.policy.speculation.risk_weight = risk;
        let schedule = world.now();
        assert_eq!(schedule.ranking[0].step_id, id(first), "risk {risk}");
        assert_eq!(starts(&schedule), vec![(id(1), "alpha".into())]);
        assert_eq!(blocked(&schedule, 2).0, "provisional_not_dispatched");
        let parts = score(&provisional, &world.policy, T0);
        assert_eq!(parts.risk_percent, risk);
        assert_eq!(parts.total, 4000 * u64::from(100 - risk) / 100);
    }
}

#[test]
fn hs2_ecwg50_aging_keeps_low_priority_work_from_starving() {
    // One slot, and a fresh high-priority ticket arrives on every pass.
    let mut world = World::new(&[(1, 1)]);
    let low = prio(work(1, 1), "low");
    let mut placed_at = None;
    for pass in 0..12u64 {
        let now = T0 + pass * 30 * MIN;
        let mut fresh = prio(work(100 + u128::from(pass), 1), "high");
        fresh.ready_since_ms = Some(now);
        world.work = vec![fresh, low.clone()];
        let schedule = world.at(now);
        assert_eq!(starts(&schedule).len(), 1, "one slot, one start");
        if starts(&schedule)[0].0 == id(1) {
            placed_at = Some(pass);
            break;
        }
        assert_eq!(blocked(&schedule, 1).0, "target_cap");
    }
    // Two bands behind, one band per hour: level after two hours, and a tie goes to
    // the work that has waited longer.
    assert_eq!(placed_at, Some(4));
    // A longer aging time postpones it, and never removes it.
    world.policy.placement.aging_minutes = 240;
    world.work = vec![prio(work(2, 1), "high"), low];
    world.work[0].ready_since_ms = Some(T0 + 9 * HOUR);
    assert_eq!(starts(&world.at(T0 + 4 * HOUR))[0].0, id(2));
    assert_eq!(starts(&world.at(T0 + 9 * HOUR))[0].0, id(1));
}

// ---- Tools ----

#[test]
fn hs2_ecwg50_a_limited_tool_moves_work_to_another_allowed_tool() {
    let mut world = World::new(&[(1, 2)]);
    world.work = vec![work(1, 1)];
    assert_eq!(starts(&world.now()), vec![(id(1), "alpha".into())]);

    // `alpha` hits its limit: the same work goes to `beta`.
    world.tools = vec![limited("alpha", T0 + 2 * HOUR), healthy().remove(1)];
    let moved = world.now();
    assert_eq!(starts(&moved), vec![(id(1), "beta".into())]);
    assert!(!moved.placements[0].probe);
    assert_eq!(
        moved.placements[0].inputs.tool_state.as_deref(),
        Some("available")
    );

    // Both limited: nothing starts, and the pass says when to look again.
    world.tools = vec![
        limited("alpha", T0 + 2 * HOUR),
        limited("beta", T0 + 3 * HOUR),
    ];
    let waiting = world.now();
    assert!(starts(&waiting).is_empty());
    assert_eq!(
        blocked(&waiting, 1),
        ("no_tool_available", Some("limited".into()))
    );
    assert_eq!(waiting.unplaced[0].retry_at_ms, Some(T0 + 2 * HOUR));
    assert_eq!(waiting.retry_at_ms, Some(T0 + 2 * HOUR));

    // The first reset passes: `alpha` is available again on the provider's word.
    assert_eq!(
        starts(&world.at(T0 + 2 * HOUR)),
        vec![(id(1), "alpha".into())]
    );

    // A target that allows only the limited tool has nowhere to move.
    world.policy.targets[0].tools.truncate(1);
    assert_eq!(blocked(&world.now(), 1).0, "no_tool_available");

    // Work that asks for a tool the target does not allow takes one it does.
    world.tools = healthy();
    world.work[0].tool = Some("beta".into());
    assert_eq!(starts(&world.now()), vec![(id(1), "alpha".into())]);
}

#[test]
fn hs2_ecwg50_tool_and_account_caps_bound_starts_across_targets() {
    let mut world = World::new(&[(1, 4), (2, 4)]);
    world.policy.machine.max_sessions = 8;
    world.policy.tools = vec![tool("alpha", 2, 60), tool("beta", 1, 50)];
    world.work = (1..=6).map(|n| work(n, 1 + n % 2)).collect();
    let schedule = world.now();
    let tools: Vec<String> = starts(&schedule)
        .into_iter()
        .map(|(_, tool)| tool)
        .collect();
    assert_eq!(tools, ["alpha", "alpha", "beta"]);
    assert_eq!(schedule.unplaced.len(), 3);
    for item in &schedule.unplaced {
        assert_eq!(item.reason, Blocker::NoTool);
        assert_eq!(item.detail.as_deref(), Some("tool_cap"));
    }
    // Sessions already running count against the same caps.
    world.sessions = vec![
        session("s1", 1, SessionActivity::Busy),
        session("s2", 2, SessionActivity::Busy),
    ];
    let tools: Vec<String> = starts(&world.now())
        .into_iter()
        .map(|(_, tool)| tool)
        .collect();
    assert_eq!(tools, ["beta"]);

    // Accounts: each account's cap holds on its own.
    world.sessions.clear();
    world.policy.tools = vec![ToolPolicy {
        accounts: vec![
            AccountPolicy {
                account: "one".into(),
                max_sessions: 1,
                budget_cents_per_day: None,
            },
            AccountPolicy {
                account: "two".into(),
                max_sessions: 2,
                budget_cents_per_day: None,
            },
        ],
        ..tool("alpha", 4, 60)
    }];
    for target in &mut world.policy.targets {
        target.tools.truncate(1);
    }
    world.tools.clear();
    let schedule = world.now();
    let mut accounts: Vec<Option<String>> = schedule
        .placements
        .iter()
        .map(|placement| placement.account.clone())
        .collect();
    accounts.sort();
    assert_eq!(
        accounts,
        [Some("one".into()), Some("two".into()), Some("two".into())]
    );
}

#[test]
fn hs2_ecwg50_an_unverified_tool_is_a_marked_probe_and_a_degraded_one_takes_one_session() {
    let mut world = World::new(&[(1, 3)]);
    world.policy.targets[0].tools.truncate(1);
    world.work = vec![work(1, 1), work(2, 1)];

    // Nothing observed: usable, and marked as unverified.
    world.tools.clear();
    let unknown = world.now();
    assert_eq!(starts(&unknown).len(), 2);
    assert!(unknown.placements.iter().all(|placement| placement.probe));

    // A guessed backoff ended: degraded until a turn confirms it. One probe only.
    world.tools = vec![entry(&[
        signal("alpha", SignalKind::Usage, T0 - 3 * HOUR, None),
        signal("alpha", SignalKind::UsageLimit, T0 - HOUR, None),
    ])];
    let degraded = world.now();
    assert_eq!(starts(&degraded), vec![(id(1), "alpha".into())]);
    assert!(degraded.placements[0].probe);
    assert_eq!(
        blocked(&degraded, 2),
        ("no_tool_available", Some("probe_in_flight".into()))
    );
    // While that probe runs, no second session joins it.
    world.sessions = vec![session("probe", 1, SessionActivity::Busy)];
    world.work = vec![work(2, 1)];
    assert_eq!(
        blocked(&world.now(), 2).1.as_deref(),
        Some("probe_in_flight")
    );
}

// ---- Machines ----

#[test]
fn hs2_ecwg50_a_machine_below_its_ram_floor_takes_no_session() {
    let mut world = World::new(&[(1, 4), (2, 4)]);
    world.work = vec![work(1, 1), work(2, 2)];
    // Target 1's machine: 2500 MB free, floor 2048, footprint 1024: no room.
    world.targets[0] = state(1, snapshot(1, 2500, 0));
    let schedule = world.now();
    assert_eq!(
        blocked(&schedule, 1),
        ("machine_refused", Some("below_ram_floor".into()))
    );
    // The other machine is unaffected.
    assert_eq!(starts(&schedule), vec![(id(2), "alpha".into())]);

    // Room for exactly one: the second start on that machine is refused, not packed.
    world.targets[0] = state(1, snapshot(1, 2048 + 1024 + 100, 0));
    world.work = vec![work(1, 1), work(3, 1)];
    let schedule = world.now();
    assert_eq!(starts(&schedule), vec![(id(1), "alpha".into())]);
    assert_eq!(schedule.placements[0].inputs.free_ram_mb, Some(3172));
    assert_eq!(schedule.placements[0].inputs.footprint_mb, Some(1024));
    assert_eq!(
        blocked(&schedule, 3).1.as_deref(),
        Some("below_ram_floor"),
        "the first start's memory is spoken for"
    );

    // A target's own machine override is the floor that counts.
    world.policy.targets[0].machine = Some(MachineLimits {
        min_free_ram_mb: 1024,
        max_sessions: 4,
        ..MachineLimits::default()
    });
    assert_eq!(starts(&world.now()).len(), 2);

    // Each other limit names itself.
    let cases: Vec<(&str, Edit<CapacitySnapshot>)> = vec![
        (
            "above_cpu_ceiling",
            Box::new(|snapshot| snapshot.cpu.load_percent = Some(95)),
        ),
        (
            "below_disk_floor",
            Box::new(|snapshot| snapshot.disk.free_mb = Some(100)),
        ),
        (
            "capacity_unknown",
            Box::new(|snapshot| snapshot.memory.free_mb = None),
        ),
        (
            "capacity_unknown",
            Box::new(|snapshot| snapshot.sessions = None),
        ),
        (
            "session_cap_reached",
            Box::new(|snapshot| *snapshot = self::snapshot(1, 16_384, 4)),
        ),
    ];
    for (reason, edit) in cases {
        let mut capacity = snapshot(1, 16_384, 0);
        edit(&mut capacity);
        world.targets[0] = state(1, capacity);
        world.work = vec![work(1, 1)];
        assert_eq!(
            blocked(&world.now(), 1),
            ("machine_refused", Some(reason.into()))
        );
    }

    // No snapshot at all: nothing is assumed.
    world.targets[0] = TargetState {
        target_id: target(1),
        capacity: None,
        unavailable: Some("target_unreachable".into()),
    };
    assert_eq!(
        blocked(&world.now(), 1),
        ("target_unavailable", Some("target_unreachable".into()))
    );
    world.targets.remove(0);
    assert_eq!(
        blocked(&world.now(), 1),
        ("target_unavailable", Some("capacity_not_read".into()))
    );
}

#[test]
fn hs2_ecwg50_targets_on_one_machine_share_its_session_cap() {
    // Two targets on the same machine, which runs one session already; cap 4.
    let mut world = World::new(&[(1, 4), (2, 4)]);
    world.targets = vec![
        state(1, snapshot(7, 16_384, 1)),
        state(2, snapshot(7, 16_384, 1)),
    ];
    world.work = (1..=6).map(|n| work(n, 1 + n % 2)).collect();
    let schedule = world.now();
    assert_eq!(starts(&schedule).len(), 3, "one runs, three more fit");
    assert_eq!(schedule.unplaced.len(), 3);
    for item in &schedule.unplaced {
        assert_eq!(item.detail.as_deref(), Some("session_cap_reached"));
    }
    assert_eq!(
        schedule
            .placements
            .iter()
            .map(|placement| placement.inputs.machine_sessions.unwrap())
            .collect::<Vec<_>>(),
        [2, 3, 4]
    );
    // A tool the machine does not have: the next allowed tool is tried.
    let mut capacity = snapshot(7, 16_384, 0);
    capacity.tools.as_mut().unwrap()[0].installed = false;
    world.targets = vec![state(1, capacity.clone())];
    world.work = vec![work(1, 1)];
    assert_eq!(starts(&world.now()), vec![(id(1), "beta".into())]);
    capacity.tools.as_mut().unwrap()[1].configured = false;
    world.targets = vec![state(1, capacity)];
    assert_eq!(
        blocked(&world.now(), 1),
        ("no_tool_available", Some("tool_not_configured".into()))
    );
}

#[test]
fn hs2_ecwg50_no_schedule_exceeds_a_cap() {
    // A grid of loads: whatever the mix, no cap is ever passed.
    for target_cap in 1..=3u32 {
        for machine_cap in target_cap..=4u32 {
            for tool_cap in 1..=3u32 {
                for running in 0..=2u32 {
                    let mut world = World::new(&[(1, target_cap), (2, target_cap)]);
                    world.policy.machine.max_sessions = machine_cap;
                    world.policy.tools = vec![tool("alpha", tool_cap, 60), tool("beta", 1, 50)];
                    world.targets = vec![
                        state(1, snapshot(1, 16_384, running)),
                        state(2, snapshot(1, 16_384, running)),
                    ];
                    world.sessions = (0..running)
                        .map(|n| session(&format!("m{n}"), 1, SessionActivity::Busy))
                        .collect();
                    world.work = (1..=8).map(|n| work(n, 1 + n % 2)).collect();
                    let schedule = world.now();
                    let started: Vec<&Placement> = schedule
                        .placements
                        .iter()
                        .filter(|placement| placement.action == PlacementAction::Start)
                        .collect();
                    let on = |n: u128| {
                        started
                            .iter()
                            .filter(|placement| placement.target_id == target(n))
                            .count() as u32
                    };
                    let of = |tool: &str| {
                        started
                            .iter()
                            .filter(|placement| placement.tool == tool)
                            .count() as u32
                    };
                    let case = (target_cap, machine_cap, tool_cap, running);
                    let within = |used: u32, cap: u32| used == 0 || used + running <= cap;
                    assert!(within(on(1), target_cap), "{case:?}");
                    assert!(on(2) <= target_cap, "{case:?}");
                    assert!(within(started.len() as u32, machine_cap), "{case:?}");
                    assert!(within(of("alpha"), tool_cap), "{case:?}");
                    assert!(of("beta") <= 1, "{case:?}");
                    assert_eq!(
                        started.len() + schedule.unplaced.len(),
                        8,
                        "every item is accounted for: {case:?}"
                    );
                }
            }
        }
    }
}

// ---- Refusals ----

#[test]
fn hs2_ecwg50_refusal_backoff_doubles_to_its_cap() {
    let minutes: Vec<u64> = (0..=8).map(|n| refusal_backoff_ms(n) / MIN).collect();
    assert_eq!(minutes, [0, 1, 2, 4, 8, 16, 30, 30, 30]);
    for refusals in [63, 64, 65, u32::MAX] {
        assert_eq!(refusal_backoff_ms(refusals), REFUSAL_BACKOFF_CAP_MS);
    }
}

#[test]
fn hs2_ecwg50_a_target_that_refuses_a_start_is_left_alone_then_asked_again() {
    let mut world = World::new(&[(1, 2), (2, 2)]);
    world.work = vec![work(1, 1), work(2, 2)];
    assert_eq!(starts(&world.now()).len(), 2);

    // Target 1 refuses. Target 2 took its session.
    let refused = PlacementOutcome::Refused("below_ram_floor".into());
    world.memory = remember(
        &world.memory,
        &[
            (target(1), refused.clone()),
            (target(2), PlacementOutcome::Started),
        ],
        T0,
    );
    world.work = vec![work(1, 1), work(3, 2)];
    let schedule = world.at(T0 + 30 * 1000);
    assert_eq!(
        blocked(&schedule, 1),
        ("refusal_backoff", Some("below_ram_floor".into()))
    );
    assert_eq!(schedule.unplaced[0].retry_at_ms, Some(T0 + MIN));
    assert_eq!(schedule.retry_at_ms, Some(T0 + MIN));
    assert_eq!(starts(&schedule), vec![(id(3), "alpha".into())]);

    // The backoff ends: it is asked again. A second refusal waits twice as long.
    assert_eq!(starts(&world.at(T0 + MIN)).len(), 2);
    world.memory = remember(&world.memory, &[(target(1), refused.clone())], T0 + MIN);
    assert_eq!(world.memory[0].refusals, 2);
    assert_eq!(blocked(&world.at(T0 + 2 * MIN), 1).0, "refusal_backoff");
    assert_eq!(starts(&world.at(T0 + 3 * MIN)).len(), 2);

    // Several refusals in one pass count once; a start clears them.
    world.memory = remember(
        &world.memory,
        &[(target(1), refused.clone()), (target(1), refused.clone())],
        T0 + 3 * MIN,
    );
    assert_eq!(world.memory[0].refusals, 3);
    world.memory = remember(
        &world.memory,
        &[(target(1), PlacementOutcome::Started)],
        T0 + 9 * MIN,
    );
    assert_eq!(world.memory[0].refusals, 0);
    assert_eq!(world.memory[0].refused_until_ms(), None);
    assert_eq!(starts(&world.at(T0 + 9 * MIN)).len(), 2);

    // A start and a refusal of another request in one pass: the start stands.
    for order in [[0, 1], [1, 0]] {
        let outcomes = [
            (target(1), PlacementOutcome::Started),
            (target(1), refused.clone()),
        ];
        let memory = remember(
            &[],
            &[outcomes[order[0]].clone(), outcomes[order[1]].clone()],
            T0,
        );
        assert_eq!(memory[0].refusals, 0, "{order:?}");
        assert_eq!(memory[0].last_started_at_ms, Some(T0));
    }
    // Nothing applied remembers nothing, and an untouched target is not listed.
    assert!(remember(&[], &[], T0).is_empty());
    assert_eq!(remember(&world.memory, &[], T0), world.memory);
}

#[test]
fn hs2_ecwg50_a_standing_reason_is_named_before_a_temporary_one() {
    // The target refused a start and wound a session down a moment ago.
    let mut world = World::new(&[(1, 1)]);
    world.work = vec![work(1, 1)];
    world.memory = remember(
        &[],
        &[
            (target(1), PlacementOutcome::Refused("refused".into())),
            (target(1), PlacementOutcome::WoundDown),
        ],
        T0,
    );
    assert_eq!(blocked(&world.now(), 1).0, "refusal_backoff");
    // The backoff ends first; the cooldown still holds.
    assert_eq!(blocked(&world.at(T0 + 2 * MIN), 1).0, "cooldown");
    // A full target is why nothing starts there, whatever else is also true.
    let mut foreign = session("other", 1, SessionActivity::Busy);
    foreign.owner_id = None;
    world.sessions = vec![foreign];
    assert_eq!(blocked(&world.now(), 1).0, "target_cap");
    // And a policy that does not permit the target is named before all of them.
    world.policy.targets[0].enabled = false;
    assert_eq!(
        blocked(&world.now(), 1),
        ("policy_denied", Some("policy_target_disabled".into()))
    );
}

// ---- Policy changes mid-run ----

#[test]
fn hs2_ecwg50_a_policy_change_mid_run_stops_starts_and_winds_sessions_down_safely() {
    let mut world = World::new(&[(1, 3)]);
    let mut working = session("busy", 1, SessionActivity::Busy);
    working.run_id = Some(id(50));
    world.sessions = vec![
        working,
        session("idle", 1, SessionActivity::Idle),
        session("gate", 1, SessionActivity::WaitingOnGate),
    ];
    world.work = vec![work(1, 1), work(2, 1), work(3, 1)];
    // Before the change: the two free sessions are refilled; the cap stops a start.
    let before = world.now();
    assert_eq!(fate(&before, "idle"), ("keep", "refill"));
    assert_eq!(fate(&before, "gate"), ("keep", "refill"));
    assert_eq!(fate(&before, "busy"), ("keep", "busy"));
    assert!(starts(&before).is_empty());
    assert_eq!(blocked(&before, 3).0, "target_cap");

    // The target is disabled while its sessions run.
    world.policy.targets[0].enabled = false;
    let disabled = world.now();
    assert!(starts(&disabled).is_empty());
    assert_eq!(
        fate(&disabled, "idle"),
        ("wind_down", "policy_not_permitted")
    );
    // Neither is at a safe point: they are kept until they are.
    assert_eq!(fate(&disabled, "busy"), ("keep", "awaiting_safe_point"));
    assert_eq!(fate(&disabled, "gate"), ("keep", "awaiting_safe_point"));
    for n in 1..=3 {
        assert_eq!(
            blocked(&disabled, n),
            ("policy_denied", Some("policy_target_disabled".into()))
        );
    }
    let detail = |schedule: &Schedule, name: &str| {
        schedule
            .placements
            .iter()
            .find(|placement| placement.session_id.as_deref() == Some(name))
            .and_then(|placement| placement.detail.clone())
    };
    assert_eq!(
        detail(&disabled, "idle").as_deref(),
        Some("policy_target_disabled")
    );

    // Each other way a session can fall outside the policy.
    let cases: Vec<(&str, Edit<PolicyBody>)> = vec![
        (
            "policy_target_not_permitted",
            Box::new(|policy| policy.targets.clear()),
        ),
        (
            "policy_tool_not_permitted",
            Box::new(|policy| {
                policy.targets[0].tools.remove(0);
            }),
        ),
        (
            "policy_model_not_permitted",
            Box::new(|policy| policy.targets[0].tools[0].models = vec!["m-large".into()]),
        ),
    ];
    for (code, edit) in cases {
        world.policy = policy(&[(1, 3)]);
        edit(&mut world.policy);
        let schedule = world.now();
        assert_eq!(
            fate(&schedule, "idle"),
            ("wind_down", "policy_not_permitted"),
            "{code}"
        );
        assert_eq!(detail(&schedule, "idle").as_deref(), Some(code));
        assert_eq!(fate(&schedule, "busy"), ("keep", "awaiting_safe_point"));
    }
    // A model the policy now requires is chosen for new starts.
    world.policy = policy(&[(1, 3)]);
    world.policy.targets[0].tools[0].models = vec!["m-large".into(), "m-small".into()];
    world.sessions.clear();
    world.work = vec![work(1, 1)];
    assert_eq!(world.now().placements[0].model.as_deref(), Some("m-large"));
    world.work[0].model = Some("m-small".into());
    assert_eq!(world.now().placements[0].model.as_deref(), Some("m-small"));
    world.work[0].model = Some("m-other".into());
    assert_eq!(starts(&world.now()), vec![(id(1), "beta".into())]);
}

#[test]
fn hs2_ecwg50_a_lowered_cap_winds_down_the_freest_sessions_first() {
    let mut world = World::new(&[(1, 4)]);
    let mut long_idle = session("idle-long", 1, SessionActivity::Idle);
    long_idle.idle_since_ms = Some(T0 - 10 * MIN);
    let mut gate = session("gate", 1, SessionActivity::WaitingOnGate);
    gate.safe_point = true;
    world.sessions = vec![
        session("busy", 1, SessionActivity::Busy),
        session("idle-new", 1, SessionActivity::Idle),
        long_idle,
        gate,
    ];
    let leaving = |world: &World| -> Vec<String> {
        world
            .now()
            .placements
            .iter()
            .filter(|placement| placement.action == PlacementAction::WindDown)
            .map(|placement| {
                assert_eq!(placement.reason, PlacementReason::OverCap);
                placement.session_id.clone().unwrap()
            })
            .collect()
    };
    assert!(leaving(&world).is_empty());
    world.policy.targets[0].max_sessions = 3;
    assert_eq!(leaving(&world), ["idle-long"]);
    world.policy.targets[0].max_sessions = 2;
    assert_eq!(leaving(&world), ["idle-long", "idle-new"]);
    // Below what is busy: the waiting session goes, the busy one is never touched.
    world.policy.targets[0].max_sessions = 1;
    assert_eq!(leaving(&world), ["gate", "idle-long", "idle-new"]);
    assert_eq!(fate(&world.now(), "busy"), ("keep", "busy"));
    // A session already leaving is not counted twice, and nothing new starts.
    world.sessions[2].winding_down = true;
    world.sessions[1].winding_down = true;
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    assert_eq!(leaving(&world), ["gate"]);
    assert_eq!(fate(&schedule, "idle-long"), ("keep", "winding_down"));
    assert_eq!(blocked(&schedule, 1).0, "target_cap");
}

// ---- Sessions ----

#[test]
fn hs2_ecwg50_a_session_waiting_on_ci_is_refilled_with_independent_work() {
    let mut world = World::new(&[(1, 1), (2, 1)]);
    let mut waiting = session("gate", 1, SessionActivity::WaitingOnGate);
    waiting.run_id = Some(id(60));
    world.sessions = vec![waiting, session("busy", 2, SessionActivity::Busy)];
    world.work = vec![work(1, 1), work(2, 2)];
    let schedule = world.now();
    // The target is at its cap of one, and still takes the work: no new session.
    assert!(starts(&schedule).is_empty());
    let refill = schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some("gate"))
        .unwrap();
    assert_eq!(
        (refill.action, refill.reason, refill.steps.clone()),
        (PlacementAction::Keep, PlacementReason::Refill, vec![id(1)])
    );
    assert!(refill.score.is_some());
    // A busy session is not free capacity.
    assert_eq!(blocked(&schedule, 2).0, "target_cap");
    assert_eq!(schedule.standing(&id(1)).unwrap().standing, "keep");

    // With nothing ready it simply waits; it is not wound down for being quiet.
    world.work.clear();
    assert_eq!(
        fate(&world.at(T0 + DAY), "gate"),
        ("keep", "waiting_on_gate")
    );

    // An idle session is taken before one that may be called back by its gate.
    world
        .sessions
        .push(session("idle", 1, SessionActivity::Idle));
    world.policy.targets[0].max_sessions = 2;
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    assert_eq!(fate(&schedule, "idle"), ("keep", "refill"));
    assert_eq!(fate(&schedule, "gate"), ("keep", "waiting_on_gate"));

    // Work that asks for another tool does not fit the free session.
    world.work[0].tool = Some("beta".into());
    world.policy.targets[0].max_sessions = 3;
    let schedule = world.now();
    assert_eq!(starts(&schedule), vec![(id(1), "beta".into())]);
    assert_eq!(fate(&schedule, "idle"), ("keep", "idle_grace"));
}

#[test]
fn hs2_ecwg50_sessions_the_scheduler_does_not_own_are_load_and_nothing_else() {
    let mut world = World::new(&[(1, 3)]);
    let mut foreign = session("foreign", 1, SessionActivity::Idle);
    foreign.owner_id = Some("owner-other".into());
    let mut manual = session("manual", 1, SessionActivity::Idle);
    manual.owner_id = None;
    let mut taken = session("taken", 1, SessionActivity::Idle);
    taken.human_takeover = true;
    for session in [&mut foreign, &mut manual, &mut taken] {
        session.idle_since_ms = Some(T0 - DAY);
    }
    world.sessions = vec![foreign, manual, taken];
    world.work = vec![work(1, 1)];
    let schedule = world.now();
    assert_eq!(fate(&schedule, "foreign"), ("keep", "not_owned"));
    assert_eq!(fate(&schedule, "manual"), ("keep", "not_owned"));
    assert_eq!(fate(&schedule, "taken"), ("keep", "human_takeover"));
    // They fill the target's cap, are not refilled and are never wound down.
    assert_eq!(blocked(&schedule, 1).0, "target_cap");
    // Even when the policy forbids the target.
    world.policy.targets.clear();
    let schedule = world.now();
    assert!(
        schedule
            .placements
            .iter()
            .all(|placement| placement.action == PlacementAction::Keep)
    );
}

#[test]
fn hs2_ecwg50_idle_sessions_wind_down_after_the_idle_time_beyond_keep_warm() {
    let mut world = World::new(&[(1, 4)]);
    let idle = |name: &str, minutes: u64| {
        let mut session = session(name, 1, SessionActivity::Idle);
        session.idle_since_ms = Some(T0 - minutes * MIN);
        session
    };
    world.sessions = vec![idle("a", 14), idle("b", 15), idle("c", 60)];
    let schedule = world.now();
    assert_eq!(fate(&schedule, "a"), ("keep", "idle_grace"));
    assert_eq!(fate(&schedule, "b"), ("wind_down", "idle"));
    assert_eq!(fate(&schedule, "c"), ("wind_down", "idle"));
    assert_eq!(schedule.retry_at_ms, Some(T0 + MIN), "when `a` turns idle");

    // The most recently idle sessions are the warm ones.
    world.policy.wind_down.keep_warm = 2;
    let schedule = world.now();
    assert_eq!(fate(&schedule, "a"), ("keep", "keep_warm"));
    assert_eq!(fate(&schedule, "b"), ("keep", "keep_warm"));
    assert_eq!(fate(&schedule, "c"), ("wind_down", "idle"));
    assert_eq!(schedule.retry_at_ms, None);

    // A session that is not at a safe point is asked later, not now.
    world.sessions[2].safe_point = false;
    assert_eq!(fate(&world.now(), "c"), ("keep", "awaiting_safe_point"));
    // An idle time that was never reported counts from now.
    world.sessions[2].idle_since_ms = None;
    world.policy.wind_down.keep_warm = 0;
    assert_eq!(fate(&world.now(), "c"), ("keep", "idle_grace"));
}

// ---- Sequences ----

/// Applies each schedule the way a faithful executor would.
struct Run {
    world: World,
    now: u64,
    next_session: u32,
    started: u32,
    stopped: u32,
}

impl Run {
    fn new(targets: &[(u128, u32)]) -> Self {
        Self {
            world: World::new(targets),
            now: T0,
            next_session: 0,
            started: 0,
            stopped: 0,
        }
    }
    /// One pass after `minutes`, with exactly this work ready.
    fn pass(&mut self, minutes: u64, ready: Vec<WorkItem>) -> Schedule {
        self.now += minutes * MIN;
        self.world.work = ready;
        let schedule = self.world.at(self.now);
        let mut applied = Vec::new();
        for placement in &schedule.placements {
            match placement.action {
                PlacementAction::Start => {
                    self.next_session += 1;
                    self.started += 1;
                    let mut session =
                        session(&format!("s{}", self.next_session), 1, SessionActivity::Busy);
                    session.target_id = placement.target_id.clone();
                    session.tool = placement.tool.clone();
                    session.started_at_ms = self.now;
                    self.world.sessions.push(session);
                    applied.push((placement.target_id.clone(), PlacementOutcome::Started));
                }
                PlacementAction::WindDown => {
                    self.stopped += 1;
                    self.world.sessions.retain(|session| {
                        Some(&session.session_id) != placement.session_id.as_ref()
                    });
                    applied.push((placement.target_id.clone(), PlacementOutcome::WoundDown));
                }
                PlacementAction::Keep if placement.reason == PlacementReason::Refill => {
                    self.set(
                        placement.session_id.as_deref().unwrap(),
                        SessionActivity::Busy,
                    );
                }
                PlacementAction::Keep => {}
            }
        }
        self.world.memory = remember(&self.world.memory, &applied, self.now);
        schedule
    }
    fn set(&mut self, name: &str, activity: SessionActivity) {
        let now = self.now;
        for session in &mut self.world.sessions {
            if session.session_id == name {
                session.activity = activity;
                session.run_id = (activity == SessionActivity::Busy).then(|| id(70));
                session.safe_point = activity == SessionActivity::Idle;
                session.idle_since_ms = (activity == SessionActivity::Idle).then_some(now);
            }
        }
    }
    /// Every session finishes its ticket.
    fn finish_all(&mut self) {
        let names: Vec<String> = self
            .world
            .sessions
            .iter()
            .map(|session| session.session_id.clone())
            .collect();
        for name in names {
            self.set(&name, SessionActivity::Idle);
        }
    }
}

#[test]
fn hs2_ecwg50_an_empty_queue_refills_without_a_second_session() {
    let mut run = Run::new(&[(1, 2)]);
    // Empty: nothing happens, however often it is asked.
    for _ in 0..3 {
        let schedule = run.pass(1, Vec::new());
        assert_eq!(schedule, Schedule::default());
    }
    // Work arrives: one session.
    assert_eq!(starts(&run.pass(1, vec![work(1, 1)])).len(), 1);
    // The same pass again changes nothing: the work is now in progress, not ready.
    let again = run.pass(1, Vec::new());
    assert_eq!(fate(&again, "s1"), ("keep", "busy"));
    // The queue empties; the session idles inside its grace time.
    run.finish_all();
    assert_eq!(fate(&run.pass(1, Vec::new()), "s1"), ("keep", "idle_grace"));
    // It refills: the idle session takes it. No second session.
    let refill = run.pass(1, vec![work(2, 1)]);
    assert_eq!(fate(&refill, "s1"), ("keep", "refill"));
    assert!(starts(&refill).is_empty());
    // Two more arrive while it is busy: one more session, up to the cap, no further.
    let more = run.pass(1, vec![work(3, 1), work(4, 1)]);
    assert_eq!(starts(&more).len(), 1);
    assert_eq!(blocked(&more, 4).0, "target_cap");
    // Empty again and quiet past the idle time: both wind down, and it ends empty.
    run.finish_all();
    let quiet = run.pass(20, Vec::new());
    assert_eq!(fate(&quiet, "s1"), ("wind_down", "idle"));
    assert_eq!(fate(&quiet, "s2"), ("wind_down", "idle"));
    assert_eq!(run.pass(1, Vec::new()), Schedule::default());
    assert_eq!((run.started, run.stopped), (2, 2));
}

#[test]
fn hs2_ecwg50_work_that_comes_and_goes_does_not_flap_sessions() {
    // Work appears on every other pass, a minute apart, for an hour.
    let mut run = Run::new(&[(1, 1)]);
    for minute in 0..60u128 {
        let ready = if minute % 2 == 0 {
            vec![work(100 + minute, 1)]
        } else {
            Vec::new()
        };
        run.pass(1, ready);
        run.finish_all();
    }
    // One session did all of it: it was refilled, never restarted.
    assert_eq!((run.started, run.stopped), (1, 0));

    // Quiet past the idle time: it winds down once.
    let quiet = run.pass(16, Vec::new());
    assert_eq!(fate(&quiet, "s1"), ("wind_down", "idle"));
    // Work one minute later: the target just wound down, so it waits out the cooldown.
    let soon = run.pass(1, vec![work(1, 1)]);
    assert!(starts(&soon).is_empty());
    assert_eq!(blocked(&soon, 1).0, "cooldown");
    assert_eq!(soon.retry_at_ms, Some(run.now + 4 * MIN));
    // The cooldown ends: it starts, exactly at the time the pass named.
    assert!(starts(&run.pass(3, vec![work(1, 1)])).is_empty());
    assert_eq!(starts(&run.pass(1, vec![work(1, 1)])).len(), 1);
    assert_eq!((run.started, run.stopped), (2, 1));

    // A session that just started is not wound down for idling, whatever the idle
    // time says: the guard holds in the other direction too.
    run.world.policy.wind_down.idle_minutes = 1;
    run.finish_all();
    let early = run.pass(2, Vec::new());
    assert_eq!(fate(&early, "s2"), ("keep", "cooldown"));
    assert_eq!(early.retry_at_ms, Some(run.now + 3 * MIN));
    assert_eq!(fate(&run.pass(3, Vec::new()), "s2"), ("wind_down", "idle"));

    // With no cooldown the idle time alone is the guard.
    run.world.policy.placement.cooldown_minutes = 0;
    assert_eq!(starts(&run.pass(0, vec![work(2, 1)])).len(), 1);
    assert_eq!((run.started, run.stopped), (3, 2));
}

#[test]
fn hs2_ecwg50_the_same_inputs_give_the_same_schedule_in_any_order() {
    let mut world = World::new(&[(1, 2), (2, 2), (3, 1)]);
    world.work = (1..=9)
        .map(|n| {
            let mut item = work(n, 1 + n % 3);
            item.up_next = n % 2 == 0;
            item.unblocks = (n % 4) as u32;
            item
        })
        .collect();
    world.sessions = vec![
        session("a", 1, SessionActivity::Idle),
        session("b", 2, SessionActivity::Busy),
        session("c", 3, SessionActivity::WaitingOnGate),
    ];
    world.memory = remember(
        &[],
        &[(target(3), PlacementOutcome::Refused("refused".into()))],
        T0,
    );
    let expected = world.now();
    assert_eq!(world.now(), expected, "repeated");
    world.work.reverse();
    world.sessions.reverse();
    world.targets.reverse();
    world.tools.reverse();
    world.policy.targets.reverse();
    world.memory.reverse();
    assert_eq!(world.now(), expected, "reordered");
    // Duplicated inputs are one piece of work and one session.
    world.work.extend(world.work.clone());
    world.sessions.extend(world.sessions.clone());
    assert_eq!(world.now(), expected, "duplicated");
    // Ties rank by how long the work has been ready, then by step id.
    let mut tied = World::new(&[(1, 1)]);
    let mut older = work(2, 1);
    older.ready_since_ms = Some(T0 - 1);
    tied.work = vec![work(1, 1), older, work(3, 1)];
    tied.policy.placement.aging_minutes = 10_080;
    let order: Vec<String> = tied
        .now()
        .ranking
        .into_iter()
        .map(|ranked| ranked.step_id)
        .collect();
    assert_eq!(order, [id(2), id(1), id(3)]);
}

#[test]
fn hs2_ecwg50_a_session_started_for_work_takes_it_before_any_run_exists() {
    // A pass started a session for step 1. Until the target's dispatcher assigns the
    // ticket there is no run, and the step is still ready work.
    let mut world = World::new(&[(1, 3)]);
    world.work = vec![work(1, 1)];
    for activity in [SessionActivity::Busy, SessionActivity::Idle] {
        let mut fresh = session("fresh", 1, activity);
        fresh.run_id = None;
        fresh.idle_since_ms = None;
        fresh.safe_point = false;
        world.sessions = vec![fresh];
        // However often it is asked, no second session starts for the same step.
        for _ in 0..3 {
            let schedule = world.now();
            assert!(starts(&schedule).is_empty(), "{activity:?}");
            assert_eq!(fate(&schedule, "fresh"), ("keep", "refill"));
            assert_eq!(schedule.placements[0].steps, [id(1)]);
        }
    }
    // More work than that one session: exactly one more start per extra step.
    world.work = vec![work(1, 1), work(2, 1)];
    assert_eq!(starts(&world.now()).len(), 1);
    // With no work left, a session that says it is busy is not wound down.
    world.work.clear();
    world.sessions[0].activity = SessionActivity::Busy;
    assert_eq!(fate(&world.at(T0 + DAY), "fresh"), ("keep", "busy"));
}

// ---- Resources ----

#[test]
fn hs2_ecwg50_a_held_resource_is_unavailable_capacity() {
    let mut world = World::new(&[(1, 3)]);
    let need = ResourceNeed {
        namespace: "ports".into(),
        resource: "8080".into(),
    };
    let needing = |n| {
        let mut item = work(n, 1);
        item.resources = vec![need.clone()];
        item
    };
    world.work = vec![needing(1), needing(2), work(3, 1)];
    // Free: the first takes it, and the second cannot have it in the same pass.
    let schedule = world.now();
    assert_eq!(starts(&schedule).len(), 2);
    assert_eq!(
        blocked(&schedule, 2),
        ("resource_held", Some("ports/8080".into()))
    );
    assert_eq!(schedule.unplaced[0].retry_at_ms, None);

    // Held by someone: neither starts, the unrelated work does, and the expiry is
    // when to look again.
    world.held = vec![HeldResource {
        namespace: "ports".into(),
        resource: "8080".into(),
        expires_at_ms: T0 + 10 * MIN,
        server_id: None,
    }];
    let schedule = world.now();
    assert_eq!(starts(&schedule), vec![(id(3), "alpha".into())]);
    assert_eq!(schedule.retry_at_ms, Some(T0 + 10 * MIN));
    // A free session does not get the held work either; it takes the other.
    world.sessions = vec![session("idle", 1, SessionActivity::Idle)];
    let schedule = world.now();
    assert_eq!(fate(&schedule, "idle"), ("keep", "refill"));
    assert_eq!(schedule.placements.last().unwrap().steps, [id(3)]);
    // An expired claim, or one on another resource, holds nothing.
    world.sessions.clear();
    assert_eq!(starts(&world.at(T0 + 10 * MIN)).len(), 2);
    world.held[0].resource = "9090".into();
    assert_eq!(starts(&world.now()).len(), 2);
}

// ---- Explain ----

#[test]
fn hs2_ecwg50_standing_says_where_a_step_is_and_why() {
    let mut world = World::new(&[(1, 1)]);
    world.work = vec![prio(work(1, 1), "high"), work(2, 1)];
    let schedule = world.now();
    let first = schedule.standing(&id(1)).unwrap();
    assert_eq!(
        (first.standing, first.reason, first.rank),
        ("start", "ready_work", 1)
    );
    assert_eq!(first.placement.unwrap().tool, "alpha");
    let second = schedule.standing(&id(2)).unwrap();
    assert_eq!(
        (second.standing, second.reason, second.rank),
        ("not_placed", "target_cap", 2)
    );
    assert_eq!(second.score.priority, 2000);
    assert_eq!(schedule.standing(&id(3)), None);
    assert_eq!(Schedule::default().standing(&id(1)), None);
    // Every reason is spelled on the wire as its code, so a log, an explanation and
    // a plan name the same thing the same way.
    for blocker in [
        Blocker::Provisional,
        Blocker::PolicyDenied,
        Blocker::TargetUnavailable,
        Blocker::TargetCap,
        Blocker::NoTool,
        Blocker::MachineRefused,
        Blocker::ResourceHeld,
        Blocker::Cooldown,
        Blocker::RefusalBackoff,
    ] {
        assert_eq!(serde_json::to_value(blocker).unwrap(), blocker.code());
    }
    for reason in [
        PlacementReason::ReadyWork,
        PlacementReason::Refill,
        PlacementReason::Busy,
        PlacementReason::WaitingOnGate,
        PlacementReason::KeepWarm,
        PlacementReason::IdleGrace,
        PlacementReason::Cooldown,
        PlacementReason::AwaitingSafePoint,
        PlacementReason::HumanTakeover,
        PlacementReason::NotOwned,
        PlacementReason::WindingDown,
        PlacementReason::Idle,
        PlacementReason::PolicyNotPermitted,
        PlacementReason::OverCap,
    ] {
        assert_eq!(serde_json::to_value(reason).unwrap(), reason.code());
    }
    for action in [
        PlacementAction::Start,
        PlacementAction::Keep,
        PlacementAction::WindDown,
    ] {
        assert_eq!(serde_json::to_value(action).unwrap(), action.code());
    }
}

// ---- Policy section ----

#[test]
fn hs2_ecwg50_placement_policy_defaults_validates_and_stays_out_of_old_files() {
    let body = policy(&[(1, 1)]);
    assert_eq!(body.placement, PlacementPolicy::default());
    assert!(!body.placement.automatic, "nothing is automatic by default");
    assert_eq!(
        (
            body.placement.aging_minutes,
            body.placement.cooldown_minutes
        ),
        (60, 5)
    );
    // A policy written before this section reads the same and is written the same.
    let json = serde_json::to_value(&body).unwrap();
    assert!(json.get("placement").is_none());
    assert_eq!(serde_json::from_value::<PolicyBody>(json).unwrap(), body);
    let partial: PolicyBody =
        serde_json::from_value(serde_json::json!({"placement": {"automatic": true}})).unwrap();
    assert_eq!(partial.placement.aging_minutes, 60);
    assert!(
        serde_json::from_value::<PolicyBody>(serde_json::json!({"placement": {"speed": 1}}))
            .is_err()
    );

    let codes = |edit: &dyn Fn(&mut PlacementPolicy)| {
        let mut body = policy(&[(1, 1)]);
        edit(&mut body.placement);
        body.problems(None)
            .into_iter()
            .map(|problem| problem.code)
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(&|_| {}), [""; 0]);
    // HS2-XYAHB4: automatic placement needs no named actor.
    assert_eq!(codes(&|placement| placement.automatic = true), [""; 0]);
    assert_eq!(
        codes(&|placement| {
            placement.automatic = true;
            placement.actor = Some("scheduler".into());
        }),
        [""; 0]
    );
    assert_eq!(
        codes(&|placement| placement.actor = Some("not an id".into())),
        ["invalid_placement_actor"]
    );
    for minutes in [0, 10_081] {
        assert_eq!(
            codes(&|placement| placement.aging_minutes = minutes),
            ["invalid_aging_minutes"]
        );
    }
    assert_eq!(
        codes(&|placement| placement.aging_minutes = 10_080),
        [""; 0]
    );
    assert_eq!(codes(&|placement| placement.cooldown_minutes = 0), [""; 0]);
    assert_eq!(
        codes(&|placement| placement.cooldown_minutes = 1441),
        ["invalid_cooldown_minutes"]
    );
}

// ---- Storage ----

fn record(kind: DecisionKind) -> DecisionRecord {
    DecisionRecord {
        at_ms: T0,
        tick_id: id(1),
        mode: DecisionMode::DryRun,
        trigger: "manual".into(),
        kind,
        actor_id: Some("scheduler".into()),
        target_id: Some(target(1)),
        tool: Some("alpha".into()),
        account: None,
        model: Some("m-large".into()),
        session_id: None,
        step_id: Some(id(2)),
        reason: Some("ready_work".into()),
        detail: Some("ports/8080".into()),
        outcome: None,
        outcome_detail: None,
        operation_id: None,
        run_id: None,
        probe: false,
        retry_at_ms: None,
        score: Some(ScoreParts::default()),
        inputs: Some(PlacementInputs::default()),
        tick: None,
    }
}

#[test]
fn hs2_ecwg50_the_decision_journal_keeps_identifiers_and_codes_only() {
    let home = tempfile::tempdir().unwrap();
    let journal = DecisionJournal::new(home.path(), OWNER).unwrap();
    assert!(journal.read(0, 10).unwrap().entries.is_empty());
    let first = journal.append(record(DecisionKind::Start)).unwrap();
    let tick = DecisionRecord {
        tick: Some(TickInputs {
            executor: "unavailable".into(),
            work: 3,
            ..TickInputs::default()
        }),
        ..record(DecisionKind::Tick)
    };
    journal.append(tick.clone()).unwrap();
    let page = journal.read(0, 10).unwrap();
    assert_eq!(page.entries.len(), 2);
    assert_eq!(page.entries[0].seq, first.seq);
    assert_eq!(page.entries[1].record, tick);
    assert_eq!(journal.read(page.cursor, 10).unwrap().entries.len(), 0);
    // The largest record a pass writes stays inside one journal entry.
    let mut full = record(DecisionKind::Unplaced);
    let long = "x".repeat(160);
    for field in [
        &mut full.account,
        &mut full.session_id,
        &mut full.outcome,
        &mut full.outcome_detail,
        &mut full.operation_id,
    ] {
        *field = Some(long.clone());
    }
    full.detail = Some("y".repeat(200));
    full.retry_at_ms = Some(u64::MAX);
    full.score = Some(ScoreParts {
        priority: u64::MAX,
        up_next: u64::MAX,
        due: u64::MAX,
        unblocks: u64::MAX,
        red_gate: u64::MAX,
        aging: u64::MAX,
        risk_percent: 100,
        total: u64::MAX,
    });
    journal.append(full).unwrap();

    // Free text is refused wherever it could enter.
    let edits: Vec<Edit<DecisionRecord>> = vec![
        Box::new(|record| record.tick_id = "not-a-ulid".into()),
        Box::new(|record| record.target_id = Some("some target".into())),
        Box::new(|record| record.trigger = "because I said so".into()),
        Box::new(|record| record.reason = Some("it seemed best".into())),
        Box::new(|record| record.outcome = Some("went fine".into())),
        Box::new(|record| record.detail = Some("two words".into())),
        Box::new(|record| record.detail = Some("z".repeat(201))),
        Box::new(|record| record.model = Some("a model".into())),
        Box::new(|record| record.session_id = Some("a/b".into())),
    ];
    for (index, edit) in edits.iter().enumerate() {
        let mut bad = record(DecisionKind::Keep);
        edit(&mut bad);
        assert!(journal.append(bad).is_err(), "case {index}");
    }
    assert_eq!(journal.read(0, 10).unwrap().entries.len(), 3);
}

#[test]
fn hs2_ecwg50_placement_memory_is_per_owner_and_never_created_by_a_read() {
    let home = tempfile::tempdir().unwrap();
    let store = SchedulerStore::new(home.path());
    let path = home.path().join("orchestration-scheduler.json");
    assert!(store.memory(OWNER).unwrap().is_empty());
    assert!(store.record(OWNER, &[], T0).unwrap().is_empty());
    assert!(!path.exists(), "a read, or nothing applied, writes nothing");

    let started = [(target(1), PlacementOutcome::Started)];
    let memory = store.record(OWNER, &started, T0).unwrap();
    assert_eq!(memory[0].last_started_at_ms, Some(T0));
    assert_eq!(store.memory(OWNER).unwrap(), memory);
    assert!(store.memory("owner-other").unwrap().is_empty());
    store
        .record(
            "owner-other",
            &[(target(2), PlacementOutcome::WoundDown)],
            T0,
        )
        .unwrap();
    assert_eq!(store.memory(OWNER).unwrap(), memory, "owners do not mix");
    // The same outcome at the same time changes nothing on disk.
    let before = std::fs::read(&path).unwrap();
    store.record(OWNER, &started, T0).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(store.record("not an owner", &started, T0).is_err());

    // Damaged or newer data is an error and is left as it is.
    let cases: [(&[u8], &str); 4] = [
        (b"{not json", "corrupt_orchestration_data"),
        (br#"{"schema_version":2,"owners":{}}"#, "upgrade_required"),
        (
            br#"{"schema_version":1,"owners":{"bad owner":[]}}"#,
            "corrupt_orchestration_data",
        ),
        (
            br#"{"schema_version":1,"owners":{"o":[{"target_id":"x"}]}}"#,
            "corrupt_orchestration_data",
        ),
    ];
    for (bytes, code) in cases {
        std::fs::write(&path, bytes).unwrap();
        assert_eq!(store.memory(OWNER).unwrap_err().code(), code);
        assert_eq!(store.record(OWNER, &started, T0).unwrap_err().code(), code);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}

#[test]
fn hs2_ecwg50_a_session_view_is_identifiers_only() {
    let good = session("s1", 1, SessionActivity::Idle);
    assert!(good.validate().is_ok());
    let edits: Vec<Edit<ScheduledSession>> = vec![
        Box::new(|session| session.session_id = "a b".into()),
        Box::new(|session| session.target_id = "target".into()),
        Box::new(|session| session.tool = String::new()),
        Box::new(|session| session.account = Some("a/b".into())),
        Box::new(|session| session.run_id = Some("a run".into())),
        Box::new(|session| session.owner_id = Some("an owner".into())),
        Box::new(|session| session.model = Some(" ".into())),
    ];
    for (index, edit) in edits.iter().enumerate() {
        let mut bad = good.clone();
        edit(&mut bad);
        assert!(bad.validate().is_err(), "case {index}");
    }
    // The wire form refuses a member it does not know.
    let mut json = serde_json::to_value(&good).unwrap();
    assert_eq!(
        serde_json::from_value::<ScheduledSession>(json.clone()).unwrap(),
        good
    );
    json["command_line"] = "x".into();
    assert!(serde_json::from_value::<ScheduledSession>(json).is_err());
}

/// HS2-XYAHB4: who the loop acts as. A named actor wins; otherwise the orchestrator
/// acts as the orchestration project; an owner id that would not make a bounded
/// identifier falls back to a fixed one. Validation never asks for an actor.
#[test]
fn hs2_xyahb4_the_loop_acts_as_the_orchestration_project_without_a_named_actor() {
    let long = "x".repeat(200);
    let cases: [(Option<&str>, &str, String); 4] = [
        (Some("scheduler"), "launch-0123456789ab", "scheduler".into()),
        (
            None,
            "launch-0123456789ab",
            "orchestrator.launch-0123456789ab".into(),
        ),
        (None, long.as_str(), "orchestrator".into()),
        (None, "has space", "orchestrator".into()),
    ];
    for (actor, owner, expected) in cases {
        for automatic in [false, true] {
            let placement = PlacementPolicy {
                automatic,
                actor: actor.map(Into::into),
                ..PlacementPolicy::default()
            };
            assert_eq!(placement.acting_actor(owner), expected, "{actor:?} {owner}");
            let mut body = policy(&[(1, 1)]);
            body.placement = placement;
            assert!(
                body.problems(None).is_empty(),
                "{actor:?} automatic={automatic}"
            );
        }
    }
    assert!(
        PlacementPolicy::default()
            .acting_actor("owner")
            .starts_with(ORCHESTRATOR_ACTOR_PREFIX)
    );
}

/// HS2-AWB7BX: a capacity refusal on a target whose capacity changes arrive as events
/// waits for the change; elsewhere, and for any other refusal, the backoff retry stays.
#[test]
fn hs2_awb7bx_an_evented_capacity_refusal_sets_no_retry_time_and_a_change_lifts_it() {
    let mut world = World::new(&[(1, 2), (2, 2)]);
    world.work = vec![work(1, 1), work(2, 2)];
    let ram = PlacementOutcome::Refused("below_ram_floor".into());
    let other = PlacementOutcome::Refused("session_start_failed".into());
    world.memory = remember(&world.memory, &[(target(1), ram), (target(2), other)], T0);
    let at = T0 + 30 * 1000;
    // No stream: both keep their timed retry (the fallback).
    let plain = world.at(at);
    assert_eq!(plain.unplaced[0].retry_at_ms, Some(T0 + MIN));
    assert_eq!(plain.retry_at_ms, Some(T0 + MIN));
    // A stream on both targets: only the capacity refusal waits for the event.
    world.capacity_streams = vec![target(1), target(2)];
    let evented = world.at(at);
    assert_eq!(
        blocked(&evented, 1),
        ("refusal_backoff", Some("below_ram_floor".into()))
    );
    let retry = |schedule: &Schedule, n: u128| {
        schedule
            .unplaced
            .iter()
            .find(|item| item.step_id == id(n))
            .unwrap()
            .retry_at_ms
    };
    assert_eq!(retry(&evented, 1), None);
    assert_eq!(retry(&evented, 2), Some(T0 + MIN));
    assert_eq!(evented.retry_at_ms, Some(T0 + MIN));
    // The change lifts the capacity refusal only: target 1 is asked again at once,
    // target 2 still waits out its backoff.
    let lifted = lift_capacity_refusal(&world.memory, &target(1));
    assert_eq!(
        lift_capacity_refusal(&world.memory, &target(2)),
        world.memory
    );
    world.memory = lifted;
    let after = world.at(at);
    assert_eq!(starts(&after), vec![(id(1), "alpha".into())]);
    assert_eq!(blocked(&after, 2).0, "refusal_backoff");
    // Lifting again, or lifting an unknown target, changes nothing.
    assert_eq!(
        lift_capacity_refusal(&world.memory, &target(1)),
        world.memory
    );
    assert_eq!(
        lift_capacity_refusal(&world.memory, &target(9)),
        world.memory
    );
    assert!(lift_capacity_refusal(&[], &target(1)).is_empty());
}

#[test]
fn hs2_awb7bx_capacity_refusals_are_the_admission_reasons() {
    for code in [
        "session_admission_refused",
        "tool_not_installed",
        "tool_not_configured",
        "session_cap_reached",
        "above_cpu_ceiling",
        "below_disk_floor",
        "below_ram_floor",
        "capacity_unknown",
    ] {
        assert!(is_capacity_refusal(code), "{code}");
    }
    for code in [
        "session_start_failed",
        "resource_unknown",
        "permission_required",
        "",
    ] {
        assert!(!is_capacity_refusal(code), "{code}");
    }
}

#[test]
fn hs2_awb7bx_the_store_lifts_a_capacity_refusal_once() {
    let home = tempfile::tempdir().unwrap();
    let store = SchedulerStore::new(home.path());
    let owner = "owner-0123456789ab";
    assert!(!store.lift_capacity_refusal(owner, &target(1)).unwrap());
    store
        .record(
            owner,
            &[(
                target(1),
                PlacementOutcome::Refused("below_ram_floor".into()),
            )],
            T0,
        )
        .unwrap();
    assert!(store.lift_capacity_refusal(owner, &target(1)).unwrap());
    assert!(store.memory(owner).unwrap().is_empty());
    assert!(!store.lift_capacity_refusal(owner, &target(1)).unwrap());
}

#[test]
fn hs2_awb7bx_a_policy_change_lifts_every_capacity_refusal_of_the_owner() {
    let home = tempfile::tempdir().unwrap();
    let store = SchedulerStore::new(home.path());
    let owner = "owner-0123456789ab";
    assert!(!store.lift_capacity_refusals(owner).unwrap());
    store
        .record(
            owner,
            &[
                (
                    target(1),
                    PlacementOutcome::Refused("session_cap_reached".into()),
                ),
                (
                    target(2),
                    PlacementOutcome::Refused("session_start_failed".into()),
                ),
            ],
            T0,
        )
        .unwrap();
    assert!(store.lift_capacity_refusals(owner).unwrap());
    let left = store.memory(owner).unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].target_id, target(2));
    assert!(!store.lift_capacity_refusals(owner).unwrap());
}

/// HS2-TH53NN: a plan step's preferred tool and model are preferences. Selection tries
/// them first; one that leaves nothing to start on falls back to any allowed tool; the
/// preferred model goes with the preferred tool only; a free session that has them
/// takes the work first, and any other free session of the target otherwise.
#[test]
fn hs2_th53nn_a_preferred_tool_and_model_are_tried_first_and_never_required() {
    let models = |world: &mut World, tool: &str, list: &[&str]| {
        world.policy.targets[0]
            .tools
            .iter_mut()
            .find(|scope| scope.tool == tool)
            .unwrap()
            .models = list.iter().map(|model| (*model).into()).collect();
    };
    let started = |schedule: &Schedule| {
        schedule
            .placements
            .iter()
            .filter(|placement| placement.action == PlacementAction::Start)
            .map(|placement| (placement.tool.clone(), placement.model.clone()))
            .collect::<Vec<_>>()
    };
    let mut world = World::new(&[(1, 2)]);
    models(&mut world, "alpha", &["a-1", "a-2"]);
    models(&mut world, "beta", &["b-1", "b-2"]);
    world.work = vec![work(1, 1)];
    // No preference: the policy's preferred tool and its first model.
    assert_eq!(
        started(&world.now()),
        [("alpha".into(), Some("a-1".into()))]
    );
    // A preferred tool and model the target allows.
    world.work[0].tool = Some("beta".into());
    world.work[0].model = Some("b-2".into());
    assert_eq!(started(&world.now()), [("beta".into(), Some("b-2".into()))]);
    // The preferred tool is limited: another tool takes it, with its own first model.
    world.tools = vec![healthy().remove(0), limited("beta", T0 + HOUR)];
    assert_eq!(
        started(&world.now()),
        [("alpha".into(), Some("a-1".into()))]
    );
    world.tools = healthy();
    // A model no allowed tool offers falls back to any allowed tool account.
    world.work[0].tool = None;
    world.work[0].model = Some("unknown-model".into());
    assert_eq!(
        started(&world.now()),
        [("alpha".into(), Some("a-1".into()))]
    );
    // A model alone, offered by one tool, picks that model where it is listed.
    world.work[0].model = Some("b-1".into());
    let alone = started(&world.now());
    assert!(
        alone == [("beta".into(), Some("b-1".into()))]
            || alone == [("alpha".into(), Some("a-1".into()))],
        "{alone:?}"
    );
    // Repeated passes decide the same.
    world.work[0].tool = Some("beta".into());
    world.work[0].model = Some("b-2".into());
    for _ in 0..3 {
        assert_eq!(started(&world.now()), [("beta".into(), Some("b-2".into()))]);
    }

    // Refill: a free session with the preferred tool takes the work before another.
    let mut world = World::new(&[(1, 2)]);
    world.work = vec![work(1, 1)];
    world.work[0].tool = Some("beta".into());
    let mut other = session("s-alpha", 1, SessionActivity::Idle);
    other.tool = "alpha".into();
    let mut wanted = session("s-beta", 1, SessionActivity::Idle);
    wanted.tool = "beta".into();
    world.sessions = vec![other.clone(), wanted];
    let refilled = world.now();
    assert_eq!(fate(&refilled, "s-beta"), ("keep", "refill"));
    assert!(starts(&refilled).is_empty());
    // Only a session of another tool is free: with room, the preferred tool starts.
    world.sessions = vec![other];
    let started_beta = world.now();
    assert_eq!(starts(&started_beta), vec![(id(1), "beta".into())]);
    // At the target's cap, the free session of another tool takes it instead.
    world.policy.targets[0].max_sessions = 1;
    let refilled = world.now();
    assert_eq!(fate(&refilled, "s-alpha"), ("keep", "refill"));
    assert!(starts(&refilled).is_empty());
    // The preferred tool limited, no other candidate left after the fallback: the
    // free session takes it rather than a second start.
    world.policy.targets[0].max_sessions = 2;
    world.tools = vec![healthy().remove(0), limited("beta", T0 + HOUR)];
    let limited_beta = world.now();
    assert_eq!(fate(&limited_beta, "s-alpha"), ("keep", "refill"));
    assert!(starts(&limited_beta).is_empty());
    // Empty, then refilled: nothing free, the preferred tool starts again.
    world.tools = healthy();
    world.sessions.clear();
    assert_eq!(starts(&world.now()), vec![(id(1), "beta".into())]);
}
