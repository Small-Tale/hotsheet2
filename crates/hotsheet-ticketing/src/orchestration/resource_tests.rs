//! HS2-2FG91X: scheduling around claimable resources. Requirement names, the state
//! each reading of a resource puts work in, sequences of release, expiry and
//! renewal, machines, and the offline policies. Synthetic names only.

use super::scheduler_tests::{
    MIN, T0, World, blocked, fate, id, session, snapshot, starts, state, target, work,
};
use super::*;

fn need(text: &str) -> ResourceNeed {
    ResourceNeed::parse(text).unwrap()
}
fn needing(n: u128, on: u128, resources: &[&str]) -> WorkItem {
    let mut item = work(n, on);
    item.resources = resources.iter().map(|text| need(text)).collect();
    item
}
/// A claim on the machine of target `on` (each target is on its own machine).
fn held_on(on: u128, text: &str, expires_at_ms: u64) -> HeldResource {
    let ResourceNeed {
        namespace,
        resource,
    } = need(text);
    HeldResource {
        namespace,
        resource,
        expires_at_ms,
        server_id: Some(id(9000 + on)),
    }
}
fn unread(on: u128, namespace: &str, offline: OfflinePolicy) -> UnreadableClaims {
    UnreadableClaims {
        server_id: id(9000 + on),
        namespace: namespace.into(),
        offline,
        reason: UnreadableReason::BackendUnreachable,
    }
}
/// `(resource, state, expiry, taken by, waiting)` per contention entry.
type Seen = (
    String,
    &'static str,
    Option<u64>,
    Option<String>,
    Vec<String>,
);
fn contention(schedule: &Schedule) -> Vec<Seen> {
    schedule
        .contention
        .iter()
        .map(|entry| {
            (
                format!("{}/{}", entry.namespace, entry.resource),
                entry.state.code(),
                entry.expires_at_ms,
                entry.taken_by.clone(),
                entry.waiting.clone(),
            )
        })
        .collect()
}

#[test]
fn hs2_2fg91x_resource_names_follow_the_claim_service_rules() {
    for valid in [
        "vm/macos-vm",
        "a/B",
        "0.a_b-c/Sim_1.2-x",
        &format!("{}/{}", "n".repeat(64), "R".repeat(128)),
    ] {
        let parsed = ResourceNeed::parse(valid).unwrap_or_else(|_| panic!("{valid}"));
        assert_eq!(parsed.name(), valid);
    }
    for invalid in [
        "",
        "vm",
        "vm/",
        "/macos-vm",
        "VM/macos-vm",
        "vm/macos vm",
        "vm/a/b",
        "-vm/a",
        "vm/.a",
        "v..m/a",
        "vm/a\n",
        "v m/a",
        &format!("{}/a", "n".repeat(65)),
        &format!("a/{}", "r".repeat(129)),
    ] {
        assert!(ResourceNeed::parse(invalid).is_err(), "{invalid:?}");
    }
    // A set is bounded and names each resource once.
    let four: Vec<_> = (0..4).map(|n| need(&format!("vm/r{n}"))).collect();
    assert!(validate_needs(&four, MAX_STEP_RESOURCES).is_ok());
    assert!(validate_needs(&[], MAX_STEP_RESOURCES).is_ok());
    let mut five = four.clone();
    five.push(need("vm/r4"));
    assert!(validate_needs(&five, MAX_STEP_RESOURCES).is_err());
    assert!(validate_needs(&[need("vm/a"), need("vm/a")], MAX_STEP_RESOURCES).is_err());
    let bad = ResourceNeed {
        namespace: "vm".into(),
        resource: "has space".into(),
    };
    assert!(validate_needs(&[bad], MAX_STEP_RESOURCES).is_err());
}

#[test]
fn hs2_2fg91x_each_reading_of_a_resource_decides_the_work_that_needs_it() {
    const VM: &str = "vm/macos-vm";
    let lease = T0 + 30 * MIN;
    type Arrange = Box<dyn Fn(&mut World)>;
    // (reading, what was read, what the one step ends as, its contention entry)
    let cases: Vec<(&str, Arrange, Option<&str>, Option<&str>)> = vec![
        ("nobody holds it", Box::new(|_| {}), None, None),
        (
            "a live claim holds it",
            Box::new(move |world| world.held = vec![held_on(1, VM, lease)]),
            Some("resource_held"),
            Some("held"),
        ),
        (
            "its claim lapsed",
            Box::new(|world| world.held = vec![held_on(1, VM, T0)]),
            None,
            None,
        ),
        (
            "the claim is on another machine's backend",
            Box::new(move |world| world.held = vec![held_on(2, VM, lease)]),
            None,
            None,
        ),
        (
            "another resource of the namespace is held",
            Box::new(move |world| world.held = vec![held_on(1, "vm/linux-vm", lease)]),
            None,
            None,
        ),
        (
            "a claim that names no machine counts everywhere",
            Box::new(move |world| {
                let mut claim = held_on(1, VM, lease);
                claim.server_id = None;
                world.held = vec![claim];
            }),
            Some("resource_held"),
            Some("held"),
        ),
        (
            "unreadable, and the backend fails closed",
            Box::new(|world| world.unreadable = vec![unread(1, "vm", OfflinePolicy::FailClosed)]),
            Some("resource_unknown"),
            Some("unreadable"),
        ),
        (
            "unreadable, and the backend allows provisional claims",
            Box::new(|world| world.unreadable = vec![unread(1, "vm", OfflinePolicy::Provisional)]),
            None,
            Some("unverified"),
        ),
        (
            "another namespace is unreadable",
            Box::new(|world| {
                world.unreadable = vec![unread(1, "ports", OfflinePolicy::FailClosed)]
            }),
            None,
            None,
        ),
        (
            "another machine's namespace is unreadable",
            Box::new(|world| world.unreadable = vec![unread(2, "vm", OfflinePolicy::FailClosed)]),
            None,
            None,
        ),
        (
            "one reading fails closed and one does not",
            Box::new(|world| {
                world.unreadable = vec![
                    unread(1, "vm", OfflinePolicy::Provisional),
                    unread(1, "vm", OfflinePolicy::FailClosed),
                ];
            }),
            Some("resource_unknown"),
            Some("unreadable"),
        ),
        (
            "held, and also unreadable",
            Box::new(move |world| {
                world.held = vec![held_on(1, VM, lease)];
                world.unreadable = vec![unread(1, "vm", OfflinePolicy::Provisional)];
            }),
            Some("resource_held"),
            Some("held"),
        ),
        (
            "the target's machine was not read",
            Box::new(|world| {
                world.targets[0].capacity = None;
                world.targets[0].unavailable = Some("target_unreachable".into());
            }),
            Some("target_unavailable"),
            Some("not_read"),
        ),
    ];
    for (name, arrange, reason, entry) in cases {
        let mut world = World::new(&[(1, 2)]);
        world.work = vec![needing(1, 1, &[VM])];
        arrange(&mut world);
        let schedule = world.now();
        match reason {
            None => assert_eq!(starts(&schedule).len(), 1, "{name}"),
            Some(reason) => {
                assert!(starts(&schedule).is_empty(), "{name}");
                assert_eq!(blocked(&schedule, 1).0, reason, "{name}");
                if reason.starts_with("resource_") {
                    assert_eq!(blocked(&schedule, 1).1.as_deref(), Some(VM), "{name}");
                }
            }
        }
        let seen: Vec<_> = contention(&schedule)
            .into_iter()
            .map(|entry| entry.1)
            .collect();
        assert_eq!(seen, entry.into_iter().collect::<Vec<_>>(), "{name}");
        // The same inputs in any order give the same schedule.
        world.held.reverse();
        world.unreadable.reverse();
        assert_eq!(world.now(), schedule, "{name}");
        // A schedule survives its own wire form.
        let wire = serde_json::to_value(&schedule).unwrap();
        assert_eq!(
            serde_json::from_value::<Schedule>(wire).unwrap(),
            schedule,
            "{name}"
        );
    }
}

#[test]
fn hs2_2fg91x_one_holder_at_a_time_through_release_expiry_and_renewal() {
    const VM: &str = "vm/macos-vm";
    let mut world = World::new(&[(1, 2)]);
    let (a, b) = (needing(1, 1, &[VM]), needing(2, 1, &[VM]));

    // Both ready and nobody holds it: the first starts, the second is deferred for
    // the first, with no time at which that changes by itself.
    world.work = vec![a.clone(), b.clone()];
    let first = world.now();
    assert_eq!(starts(&first), vec![(id(1), "alpha".into())]);
    assert_eq!(blocked(&first, 2), ("resource_held", Some(VM.into())));
    assert_eq!(first.retry_at_ms, None);
    assert_eq!(
        contention(&first),
        vec![(VM.into(), "taken", None, Some(id(1)), vec![id(2)])]
    );
    // The same pass again decides the same: nothing was claimed by deciding.
    assert_eq!(world.now(), first);

    // The first is in progress and its session claimed the resource: only the
    // second is ready, it is deferred, and the claim's expiry is when to look again.
    world.work = vec![b.clone()];
    world.sessions = vec![session("s1", 1, SessionActivity::Busy)];
    world.held = vec![held_on(1, VM, T0 + 20 * MIN)];
    let waiting = world.at(T0 + MIN);
    assert!(starts(&waiting).is_empty());
    assert_eq!(blocked(&waiting, 2), ("resource_held", Some(VM.into())));
    assert_eq!(waiting.unplaced[0].retry_at_ms, Some(T0 + 20 * MIN));
    assert_eq!(waiting.retry_at_ms, Some(T0 + 20 * MIN));
    assert_eq!(
        contention(&waiting),
        vec![(VM.into(), "held", Some(T0 + 20 * MIN), None, vec![id(2)])]
    );
    assert_eq!(fate(&waiting, "s1"), ("keep", "busy"));

    // The holder renews: the time to look again moves with it, and nothing starts.
    world.held = vec![held_on(1, VM, T0 + 50 * MIN)];
    let renewed = world.at(T0 + 19 * MIN);
    assert!(starts(&renewed).is_empty());
    assert_eq!(renewed.retry_at_ms, Some(T0 + 50 * MIN));
    // At the old expiry it is still held.
    assert!(starts(&world.at(T0 + 20 * MIN)).is_empty());

    // Released early: the deferred work starts at once.
    world.held.clear();
    let released = world.at(T0 + 25 * MIN);
    assert_eq!(starts(&released), vec![(id(2), "alpha".into())]);
    assert!(released.contention.is_empty());

    // Held again by someone else before the second started: deferred again.
    world.held = vec![held_on(1, VM, T0 + 40 * MIN)];
    assert!(starts(&world.at(T0 + 26 * MIN)).is_empty());
    // One millisecond before the expiry it is held; at the expiry it has lapsed.
    assert!(starts(&world.at(T0 + 40 * MIN - 1)).is_empty());
    let lapsed = world.at(T0 + 40 * MIN);
    assert_eq!(starts(&lapsed), vec![(id(2), "alpha".into())]);
    assert_eq!(lapsed.retry_at_ms, None);

    // Nothing ready, then the same work again: an empty pass keeps no contention.
    world.work.clear();
    assert!(world.at(T0 + 41 * MIN).contention.is_empty());
    world.work = vec![a, b];
    world.held.clear();
    world.sessions.clear();
    let refilled = world.at(T0 + 42 * MIN);
    assert_eq!(starts(&refilled).len(), 1);
    assert_eq!(blocked(&refilled, 2).0, "resource_held");
}

#[test]
fn hs2_2fg91x_two_claims_of_one_resource_wait_for_the_later_expiry() {
    // Two machines' answers, or a stale and a fresh reading: the later one counts.
    let mut world = World::new(&[(1, 1)]);
    world.work = vec![needing(1, 1, &["vm/a"])];
    world.held = vec![
        held_on(1, "vm/a", T0 + 5 * MIN),
        held_on(1, "vm/a", T0 + 9 * MIN),
        held_on(1, "vm/a", T0 - MIN),
    ];
    let schedule = world.now();
    assert_eq!(schedule.retry_at_ms, Some(T0 + 9 * MIN));
    assert_eq!(schedule.contention.len(), 1);
    assert_eq!(schedule.contention[0].waiting, [id(1)]);
}

#[test]
fn hs2_2fg91x_a_resource_belongs_to_the_machine_that_hosts_the_target() {
    const VM: &str = "vm/macos-vm";
    // Two targets on two machines: the same name is two resources.
    let mut world = World::new(&[(1, 1), (2, 1)]);
    world.work = vec![needing(1, 1, &[VM]), needing(2, 2, &[VM])];
    let apart = world.now();
    assert_eq!(starts(&apart).len(), 2);
    assert!(apart.contention.is_empty());
    // A claim on the first machine defers only the first target's work.
    world.held = vec![held_on(1, VM, T0 + 10 * MIN)];
    let one_held = world.now();
    assert_eq!(starts(&one_held), vec![(id(2), "alpha".into())]);
    assert_eq!(blocked(&one_held, 1), ("resource_held", Some(VM.into())));
    assert_eq!(
        one_held.contention[0].server_id.as_deref(),
        Some(id(9001).as_str())
    );
    // The other machine cannot be read and fails closed: only its work waits.
    world.held.clear();
    world.unreadable = vec![unread(2, "vm", OfflinePolicy::FailClosed)];
    let one_unread = world.now();
    assert_eq!(starts(&one_unread), vec![(id(1), "alpha".into())]);
    assert_eq!(blocked(&one_unread, 2).0, "resource_unknown");

    // Two targets on one machine share its resources.
    let mut shared = World::new(&[(1, 1), (2, 1)]);
    shared.targets[1] = state(2, snapshot(1, 16_384, 0));
    shared.work = vec![needing(1, 1, &[VM]), needing(2, 2, &[VM])];
    let together = shared.now();
    assert_eq!(starts(&together), vec![(id(1), "alpha".into())]);
    assert_eq!(blocked(&together, 2), ("resource_held", Some(VM.into())));
    assert_eq!(
        contention(&together),
        vec![(VM.into(), "taken", None, Some(id(1)), vec![id(2)])]
    );
    shared.held = vec![held_on(1, VM, T0 + MIN)];
    assert!(starts(&shared.now()).is_empty());
}

#[test]
fn hs2_2fg91x_unreadable_claims_follow_the_backend_offline_policy() {
    const VM: &str = "vm/macos-vm";
    let mut world = World::new(&[(1, 3)]);
    world.work = vec![needing(1, 1, &[VM]), needing(2, 1, &[VM]), work(3, 1)];

    // Fail closed: nothing that needs the resource starts or is handed to a free
    // session; work that needs nothing is untouched.
    world.unreadable = vec![unread(1, "vm", OfflinePolicy::FailClosed)];
    world.sessions = vec![session("idle", 1, SessionActivity::Idle)];
    let closed = world.now();
    assert_eq!(fate(&closed, "idle"), ("keep", "refill"));
    assert_eq!(closed.placements.last().unwrap().steps, [id(3)]);
    assert!(starts(&closed).is_empty());
    for n in [1, 2] {
        assert_eq!(blocked(&closed, n), ("resource_unknown", Some(VM.into())));
    }
    // Nothing says when a backend returns: the core names no time for it.
    assert_eq!(closed.retry_at_ms, None);
    assert_eq!(
        contention(&closed),
        vec![(VM.into(), "unreadable", None, None, vec![id(1), id(2)])]
    );
    assert_eq!(
        closed.contention[0].unreadable,
        Some(UnreadableReason::BackendUnreachable)
    );
    // A namespace one read had no room for fails closed too, and says so.
    world.unreadable[0].reason = UnreadableReason::NamespaceLimit;
    assert_eq!(
        world.now().contention[0].unreadable,
        Some(UnreadableReason::NamespaceLimit)
    );
    world.unreadable[0].reason = UnreadableReason::BackendUnreachable;

    // Provisional: the best-ranked work starts unverified, and still only one
    // piece of work is given the resource in a pass.
    world.sessions.clear();
    world.unreadable = vec![unread(1, "vm", OfflinePolicy::Provisional)];
    let provisional = world.now();
    assert_eq!(
        starts(&provisional),
        vec![(id(1), "alpha".into()), (id(3), "alpha".into())]
    );
    assert_eq!(blocked(&provisional, 2), ("resource_held", Some(VM.into())));
    assert_eq!(
        contention(&provisional),
        vec![(VM.into(), "unverified", None, Some(id(1)), vec![id(2)])]
    );
    // The backend returns and reports the claim the first session took: the
    // second keeps waiting, now on a known expiry.
    world.unreadable.clear();
    world.work.remove(0);
    world.held = vec![held_on(1, VM, T0 + 15 * MIN)];
    let reconciled = world.at(T0 + MIN);
    assert_eq!(blocked(&reconciled, 2).0, "resource_held");
    assert_eq!(reconciled.retry_at_ms, Some(T0 + 15 * MIN));
    // And unreachable again under fail closed, then readable and free.
    world.held.clear();
    world.unreadable = vec![unread(1, "vm", OfflinePolicy::FailClosed)];
    assert_eq!(blocked(&world.at(T0 + 2 * MIN), 2).0, "resource_unknown");
    world.unreadable.clear();
    assert!(
        starts(&world.at(T0 + 3 * MIN))
            .iter()
            .any(|(step, _)| step == &id(2))
    );
}

#[test]
fn hs2_2fg91x_work_holding_several_resources_needs_every_one() {
    let mut world = World::new(&[(1, 3)]);
    world.work = vec![
        needing(1, 1, &["vm/a", "ports/8080"]),
        needing(2, 1, &["ports/8080", "sim/ios"]),
        needing(3, 1, &["sim/ios"]),
        needing(4, 1, &["sim/tv"]),
    ];
    // 1 takes both of its own; 2 loses the port; 3 is free to take what 2 could not
    // use; 4 shares nothing.
    let schedule = world.now();
    assert_eq!(
        starts(&schedule)
            .into_iter()
            .map(|(step, _)| step)
            .collect::<Vec<_>>(),
        [id(1), id(3), id(4)]
    );
    assert_eq!(blocked(&schedule, 2).1.as_deref(), Some("ports/8080"));
    // The first resource that blocks, in declared order, is the one named.
    world.held = vec![
        held_on(1, "sim/ios", T0 + 9 * MIN),
        held_on(1, "ports/8080", T0 + 4 * MIN),
    ];
    let schedule = world.now();
    assert_eq!(blocked(&schedule, 1).1.as_deref(), Some("ports/8080"));
    assert_eq!(blocked(&schedule, 2).1.as_deref(), Some("ports/8080"));
    assert_eq!(blocked(&schedule, 3).1.as_deref(), Some("sim/ios"));
    assert_eq!(starts(&schedule), vec![(id(4), "alpha".into())]);
    // The earliest expiry is when to look again.
    assert_eq!(schedule.retry_at_ms, Some(T0 + 4 * MIN));
    // Work deferred for one resource takes none of its others.
    assert!(
        schedule
            .contention
            .iter()
            .all(|entry| entry.state == ResourceState::Held)
    );
}

#[test]
fn hs2_2fg91x_a_free_session_that_takes_the_work_takes_its_resource() {
    const VM: &str = "vm/macos-vm";
    let mut world = World::new(&[(1, 2)]);
    world.sessions = vec![session("idle", 1, SessionActivity::Idle)];
    world.work = vec![needing(1, 1, &[VM]), needing(2, 1, &[VM])];
    let schedule = world.now();
    assert_eq!(fate(&schedule, "idle"), ("keep", "refill"));
    assert!(starts(&schedule).is_empty());
    assert_eq!(blocked(&schedule, 2), ("resource_held", Some(VM.into())));
    assert_eq!(schedule.contention[0].taken_by, Some(id(1)));

    // A target that was not read: its free session is handed only work that needs
    // nothing, because nothing is known about that machine's claims.
    world.targets[0].capacity = None;
    world.targets[0].unavailable = Some("refusal_backoff".into());
    world.work.push(work(3, 1));
    let unread = world.now();
    assert_eq!(fate(&unread, "idle"), ("keep", "refill"));
    assert_eq!(unread.placements.last().unwrap().steps, [id(3)]);
    assert_eq!(
        contention(&unread),
        vec![(VM.into(), "not_read", None, None, vec![id(1), id(2)])]
    );
    assert_eq!(unread.contention[0].server_id, None);
    // Provisional work is ranked and its resources are not looked at.
    let mut provisional = needing(4, 1, &[VM]);
    provisional.level = WorkLevel::Provisional;
    world.work = vec![provisional];
    let ranked = world.now();
    assert_eq!(blocked(&ranked, 4).0, "provisional_not_dispatched");
    assert!(ranked.contention.is_empty());
}

#[test]
fn hs2_2fg91x_resource_standings_say_how_each_requirement_stands_for_one_step() {
    let needs = [need("vm/a"), need("vm/b")];
    let entry =
        |resource: &str, state, taken_by: Option<u128>, waiting: &[u128]| ResourceContention {
            server_id: (state != ResourceState::NotRead).then(|| id(9001)),
            namespace: "vm".into(),
            resource: resource.into(),
            state,
            expires_at_ms: (state == ResourceState::Held).then_some(T0 + MIN),
            unreadable: matches!(state, ResourceState::Unreadable | ResourceState::Unverified)
                .then_some(UnreadableReason::BackendUnreachable),
            taken_by: taken_by.map(id),
            waiting: waiting.iter().map(|n| id(*n)).collect(),
        };
    let states = |contention: &[ResourceContention], evaluated| {
        resource_standings(contention, &id(1), &needs, evaluated)
            .into_iter()
            .map(|standing| (standing.state.code(), standing.taken_by))
            .collect::<Vec<_>>()
    };
    use ResourceState as S;
    // Nothing recorded: free when the pass looked, unknown when it did not.
    assert_eq!(states(&[], true), [("free", None), ("free", None)]);
    assert_eq!(states(&[], false), [("not_read", None), ("not_read", None)]);
    let cases: Vec<(ResourceContention, (&str, Option<String>))> = vec![
        (entry("a", S::Held, None, &[1]), ("held", None)),
        (entry("a", S::Taken, Some(2), &[1]), ("taken", Some(id(2)))),
        (entry("a", S::Unreadable, None, &[1]), ("unreadable", None)),
        (entry("a", S::NotRead, None, &[1]), ("not_read", None)),
        // Given to this step: free to claim, or unverified when nothing was read.
        (entry("a", S::Taken, Some(1), &[2]), ("free", None)),
        (
            entry("a", S::Unverified, Some(1), &[]),
            ("unverified", None),
        ),
        // Another step started on it unverified: for this one it is taken.
        (
            entry("a", S::Unverified, Some(2), &[1]),
            ("taken", Some(id(2))),
        ),
        // An entry about other steps says nothing of this one.
        (entry("a", S::Held, None, &[3]), ("free", None)),
    ];
    for (contention, expected) in cases {
        let all = [contention, entry("b", S::Held, None, &[7])];
        let standings = resource_standings(&all, &id(1), &needs, true);
        assert_eq!(
            (standings[0].state.code(), standings[0].taken_by.clone()),
            expected,
            "{all:?}"
        );
        assert_eq!(standings[1].state, S::Free);
        if standings[0].state == S::Held {
            assert_eq!(standings[0].expires_at_ms, Some(T0 + MIN));
            assert_eq!(standings[0].server_id, Some(id(9001)));
        }
    }
    // Every state has one wire spelling, the one `code()` gives.
    for state in [
        S::Free,
        S::Held,
        S::Taken,
        S::Unreadable,
        S::Unverified,
        S::NotRead,
    ] {
        assert_eq!(serde_json::to_value(state).unwrap(), state.code());
    }
    assert_eq!(
        serde_json::to_value(Blocker::ResourceUnknown).unwrap(),
        Blocker::ResourceUnknown.code()
    );
}

#[test]
fn hs2_2fg91x_policy_requires_resources_per_target_and_validates_them() {
    let base = World::new(&[(1, 1), (2, 1)]).policy;
    let entry = |on: u128, resources: &[&str]| TargetResources {
        target_id: target(on),
        resources: resources.iter().map(|text| need(text)).collect(),
    };
    let codes = |resources: Vec<TargetResources>| {
        let mut policy = base.clone();
        policy.placement.resources = resources;
        policy
            .problems(None)
            .into_iter()
            .map(|problem| problem.code)
            .collect::<Vec<_>>()
    };
    assert_eq!(codes(Vec::new()), [""; 0]);
    assert_eq!(codes(vec![entry(1, &["vm/macos-vm"])]), [""; 0]);
    assert_eq!(
        codes(vec![
            entry(1, &["vm/a", "vm/b", "vm/c", "vm/d"]),
            entry(2, &["vm/a"])
        ]),
        [""; 0]
    );
    assert_eq!(codes(vec![entry(1, &[])]), ["invalid_resource_count"]);
    assert_eq!(
        codes(vec![entry(1, &["vm/a", "vm/b", "vm/c", "vm/d", "vm/e"])]),
        ["invalid_resource_count"]
    );
    assert_eq!(
        codes(vec![entry(1, &["vm/a", "vm/a"])]),
        ["duplicate_resource"]
    );
    assert_eq!(
        codes(vec![entry(1, &["vm/a"]), entry(1, &["vm/b"])]),
        ["duplicate_resource_target"]
    );
    assert_eq!(
        codes(vec![entry(3, &["vm/a"])]),
        ["resources_without_target"]
    );
    let mut bad = entry(1, &["vm/a"]);
    bad.resources[0].resource = "has space".into();
    assert_eq!(codes(vec![bad]), ["invalid_resource"]);
    let mut bad = entry(1, &["vm/a"]);
    bad.target_id = "not-a-ulid".into();
    assert_eq!(
        codes(vec![bad]),
        ["invalid_target", "resources_without_target"]
    );

    // A policy without requirements is stored as it always was.
    assert!(
        serde_json::to_value(&base)
            .unwrap()
            .get("placement")
            .is_none()
    );
    let mut policy = base.clone();
    policy.placement.resources = vec![entry(1, &["vm/macos-vm"])];
    let wire = serde_json::to_value(&policy).unwrap();
    assert_eq!(
        wire["placement"]["resources"],
        serde_json::json!([{"target_id": target(1),
            "resources": [{"namespace": "vm", "resource": "macos-vm"}]}])
    );
    assert_eq!(serde_json::from_value::<PolicyBody>(wire).unwrap(), policy);
    assert_eq!(policy.target_resources(&target(1)), [need("vm/macos-vm")]);
    assert!(policy.target_resources(&target(2)).is_empty());
    // An unknown member of an entry is refused, not dropped.
    assert!(
        serde_json::from_value::<TargetResources>(serde_json::json!({
            "target_id": target(1), "resources": [], "holder": "x"}))
        .is_err()
    );
}

#[test]
fn hs2_2fg91x_a_snapshot_names_an_offline_policy_only_for_unknown_claims() {
    let mut unknown = snapshot(1, 1024, 0);
    unknown.claim_namespaces = vec!["vm".into()];
    let unknown = unknown.with_claims_offline(Some(OfflinePolicy::Provisional));
    assert_eq!(unknown.claims_offline, Some(OfflinePolicy::Provisional));
    let wire = serde_json::to_value(&unknown).unwrap();
    assert_eq!(wire["claims_offline"], "provisional");
    assert_eq!(
        serde_json::from_value::<CapacitySnapshot>(wire).unwrap(),
        unknown
    );
    // Known claims carry no policy, and the member is then left out, so a snapshot
    // from before this member reads the same.
    let mut known = snapshot(1, 1024, 0);
    known.claims = Some(Vec::new());
    let known = known.with_claims_offline(Some(OfflinePolicy::Provisional));
    assert_eq!(known.claims_offline, None);
    let mut wire = serde_json::to_value(&known).unwrap();
    assert!(wire.get("claims_offline").is_none());
    wire["claims"] = serde_json::Value::Null;
    assert_eq!(
        serde_json::from_value::<CapacitySnapshot>(wire)
            .unwrap()
            .claims_offline,
        None
    );
    assert_eq!(OfflinePolicy::default(), OfflinePolicy::FailClosed);
    assert!(OfflinePolicy::FailClosed < OfflinePolicy::Provisional);
}

/// HS2-NPAZQW: `*` matches any run of characters; anything else matches itself.
#[test]
fn hs2_npazqw_wildcards_match_any_run_of_characters() {
    for (pattern, text, matches) in [
        ("vm/*", "vm/macos-vm", true),
        ("*", "", true),
        ("a*", "a", true),
        ("*a", "ba", true),
        ("a*a", "a", false),
        ("mac*-vm", "macos-vm", true),
        ("mac*-vm", "macos-vm2", false),
        ("*-*-*", "a-b-c", true),
        ("*-*-*", "a-b", false),
        ("exact", "exact", true),
        ("exact", "exactly", false),
    ] {
        assert_eq!(wildcard_match(pattern, text), matches, "{pattern} {text}");
    }
    // The policy may require a pattern of resource names; a plan step and a session
    // name exact resources only, and the namespace is always exact.
    let pattern = ResourceNeed {
        namespace: "vm".into(),
        resource: "*".into(),
    };
    assert!(pattern.is_pattern() && pattern.validate_requirement().is_ok());
    assert!(pattern.validate().is_err() && ResourceNeed::parse("vm/*").is_err());
    for bad in [("v*", "a"), ("vm", "a b*"), ("vm", "")] {
        let need = ResourceNeed {
            namespace: bad.0.into(),
            resource: bad.1.into(),
        };
        assert!(need.validate_requirement().is_err(), "{bad:?}");
    }
    assert!(pattern.satisfied_by(&need("vm/macos-vm")));
    assert!(!pattern.satisfied_by(&need("ports/8080")));
    assert!(!pattern.satisfied_by(&pattern));
    let base = World::new(&[(1, 1)]).policy;
    let mut policy = base.clone();
    policy.placement.resources = vec![TargetResources {
        target_id: target(1),
        resources: vec![pattern.clone(), need("ports/8080")],
    }];
    assert!(
        policy.problems(None).is_empty(),
        "{:?}",
        policy.problems(None)
    );
}

/// HS2-NPAZQW: a pattern the policy requires of a target means "hold any one declared
/// resource that matches". The pass gives each piece of work one free match, in name
/// order, never one held or given to better work, and never a name nobody declared.
#[test]
fn hs2_npazqw_a_pattern_requirement_takes_one_free_declared_resource() {
    let lease = T0 + 30 * MIN;
    let world = |declared: &[&str]| {
        let mut world = World::new(&[(1, 3), (2, 1)]);
        world.policy.placement.resources = vec![TargetResources {
            target_id: target(1),
            resources: vec![ResourceNeed {
                namespace: "vm".into(),
                resource: "mac-*".into(),
            }],
        }];
        if !declared.is_empty() {
            // Another target's requirement declares the exact names.
            world.policy.placement.resources.push(TargetResources {
                target_id: target(2),
                resources: declared.iter().map(|text| need(text)).collect(),
            });
        }
        world.work = vec![work(1, 1), work(2, 1)];
        world
    };
    let resources = |schedule: &Schedule, n: u128| {
        schedule
            .placements
            .iter()
            .find(|placement| placement.steps.first() == Some(&id(n)))
            .map(|placement| {
                placement
                    .resources
                    .iter()
                    .map(ResourceNeed::name)
                    .collect::<Vec<_>>()
            })
    };
    let declared = ["vm/mac-b", "vm/mac-a", "vm/win-a"];

    // Two free matches: each step gets its own, in name order; never vm/win-a.
    let free = world(&declared).now();
    assert_eq!(starts(&free).len(), 2);
    assert_eq!(resources(&free, 1), Some(vec!["vm/mac-a".to_owned()]));
    assert_eq!(resources(&free, 2), Some(vec!["vm/mac-b".to_owned()]));

    // One held: the first step takes the other; the second waits for the lease.
    let mut one_held = world(&declared);
    one_held.held = vec![held_on(1, "vm/mac-a", lease)];
    let schedule = one_held.now();
    assert_eq!(resources(&schedule, 1), Some(vec!["vm/mac-b".to_owned()]));
    assert_eq!(
        blocked(&schedule, 2),
        ("resource_held", Some("vm/mac-*".to_owned()))
    );
    assert_eq!(schedule.retry_at_ms, Some(lease));

    // Both held: nothing starts, and the earlier lease is when to look again.
    let mut both = world(&declared);
    both.held = vec![
        held_on(1, "vm/mac-a", lease),
        held_on(1, "vm/mac-b", lease - MIN),
    ];
    let schedule = both.now();
    assert!(starts(&schedule).is_empty());
    assert_eq!(blocked(&schedule, 1).0, "resource_held");
    assert_eq!(schedule.retry_at_ms, Some(lease - MIN));

    // Released again: the same world places both once more.
    both.held.clear();
    assert_eq!(starts(&both.now()).len(), 2);

    // Nothing declared: a pattern never invents a name.
    let schedule = world(&[]).now();
    assert!(starts(&schedule).is_empty());
    assert_eq!(
        blocked(&schedule, 1),
        ("resource_unknown", Some("vm/mac-*".to_owned()))
    );

    // A live claim on the target's machine declares the name it holds; a claim on
    // another machine does not.
    let mut claimed = world(&[]);
    claimed.held = vec![held_on(2, "vm/mac-z", lease)];
    assert_eq!(blocked(&claimed.now(), 1).0, "resource_unknown");
    claimed.held = vec![held_on(1, "vm/mac-z", lease)];
    assert_eq!(blocked(&claimed.now(), 1).0, "resource_held");

    // A plan step's exact resource declares it too, and is not given twice.
    let mut stepped = world(&[]);
    stepped.work = vec![work(1, 1), needing(2, 1, &["vm/mac-c"])];
    let schedule = stepped.now();
    let given: Vec<_> = [1, 2]
        .iter()
        .filter_map(|n| resources(&schedule, *n))
        .collect();
    assert_eq!(given.concat(), ["vm/mac-c".to_owned()], "{schedule:#?}");
}
