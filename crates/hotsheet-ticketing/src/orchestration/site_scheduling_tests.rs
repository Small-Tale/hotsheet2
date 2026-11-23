//! HS2-9FKTB3: the scheduler chooses which site of a project runs a step. Sites with
//! mixed operating systems and caps, a site whose machine is offline or did not say
//! what it is, steps of one project competing for room, a site removed between passes,
//! and ties broken the same way whatever the input order. Synthetic ids only.

use super::scheduler_tests::{
    MIN, OWNER, T0, World, blocked, fate, id, session, snapshot, starts, state, target, work,
};
use super::*;

/// Target `n` is on machine `n` with this operating system.
fn os(world: &mut World, n: u128, family: OsFamily) {
    let capacity = world
        .targets
        .iter_mut()
        .find(|state| state.target_id == target(n))
        .and_then(|state| state.capacity.as_mut())
        .unwrap();
    capacity.capabilities = Some(HostCapabilities::bare(family, "x86_64"));
}

/// Step `n` named on site `on`, which may run on any of `sites`.
fn project(n: u128, on: u128, sites: &[u128]) -> WorkItem {
    let mut item = work(n, on);
    item.sites = sites.iter().map(|site| target(*site)).collect();
    item
}

fn needs(mut item: WorkItem, spec: &str) -> WorkItem {
    item.requirements.add(spec).unwrap();
    item.requirements = item.requirements.clone().normalized().unwrap();
    item
}

/// The site a step's session was started or kept on.
fn placed_on(schedule: &Schedule, n: u128) -> String {
    schedule
        .placements
        .iter()
        .find(|placement| placement.steps.contains(&id(n)))
        .unwrap_or_else(|| panic!("step {n} was not placed: {schedule:#?}"))
        .target_id
        .clone()
}

type Pass = (String, &'static str, Option<&'static str>);
fn pass(site: u128, reason: &'static str, detail: Option<&'static str>) -> Pass {
    (target(site), reason, detail)
}

/// The other sites of step `n` and why each was passed over, from its standing.
fn passes(schedule: &Schedule, n: u128) -> Vec<(String, String, Option<String>)> {
    schedule
        .standing(&id(n))
        .unwrap()
        .sites
        .into_iter()
        .map(|pass| (pass.target_id, pass.reason, pass.detail))
        .collect()
}
fn expect(passes: &[Pass]) -> Vec<(String, String, Option<String>)> {
    passes
        .iter()
        .map(|(site, reason, detail)| {
            (
                site.clone(),
                (*reason).to_owned(),
                detail.map(str::to_owned),
            )
        })
        .collect()
}

/// A step that needs Windows runs on the project's Windows site; every other site is
/// named with the requirement it misses. Steps without one spread to the quietest
/// machine.
#[test]
fn hs2_9fktb3_a_step_runs_on_the_site_that_meets_its_requirements() {
    let mut world = World::new(&[(1, 2), (2, 2), (3, 2)]);
    os(&mut world, 1, OsFamily::Macos);
    os(&mut world, 2, OsFamily::Windows);
    os(&mut world, 3, OsFamily::Linux);
    world.work = vec![
        needs(project(1, 1, &[1, 2, 3]), "os=windows"),
        needs(project(2, 1, &[1, 2, 3]), "os=linux"),
        project(3, 1, &[1, 2, 3]),
    ];
    let schedule = world.now();
    assert_eq!(starts(&schedule).len(), 3, "{schedule:#?}");
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[
            pass(1, "requirement_unmet", Some("os:windows")),
            pass(3, "requirement_unmet", Some("os:windows")),
        ])
    );
    assert_eq!(placed_on(&schedule, 2), target(3));
    // Each machine runs one session: the named site wins the tie.
    assert_eq!(placed_on(&schedule, 3), target(1));
    assert_eq!(
        passes(&schedule, 3),
        expect(&[
            pass(2, "not_chosen", Some("fewer_sessions")),
            pass(3, "not_chosen", Some("fewer_sessions")),
        ])
    );
    // A one-site project is unchanged: no sites, the named target or nothing.
    world.work = vec![needs(work(4, 1), "os=windows")];
    let schedule = world.now();
    assert_eq!(
        blocked(&schedule, 4),
        ("requirement_unmet", Some("os:windows".into()))
    );
    assert!(schedule.unplaced[0].sites.is_empty());
}

/// An offline site and one that did not say what it is are not trusted with the
/// work; the named site's reason leads. Once a site says it qualifies the step starts
/// there, and stops again when that machine changes.
#[test]
fn hs2_9fktb3_unknown_and_unmet_sites_are_reported_until_one_qualifies() {
    let mut world = World::new(&[(1, 2), (2, 2), (3, 2)]);
    os(&mut world, 1, OsFamily::Macos);
    world.targets[1] = TargetState {
        target_id: target(2),
        capacity: None,
        unavailable: Some("target_unreachable".into()),
    };
    world.work = vec![needs(project(1, 1, &[2, 3]), "os=windows")];
    let schedule = world.now();
    assert_eq!(
        blocked(&schedule, 1),
        ("requirement_unmet", Some("os:windows".into()))
    );
    assert_eq!(
        passes(&schedule, 1),
        expect(&[
            pass(2, "requirement_unknown", Some("capacity_not_read")),
            pass(3, "requirement_unknown", Some("os:windows")),
        ])
    );
    assert_eq!(schedule.unplaced[0].target_id, target(1));

    os(&mut world, 3, OsFamily::Windows);
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(3));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[
            pass(1, "requirement_unmet", Some("os:windows")),
            pass(2, "requirement_unknown", Some("capacity_not_read")),
        ])
    );

    os(&mut world, 3, OsFamily::Linux);
    assert_eq!(
        blocked(&world.now(), 1),
        ("requirement_unmet", Some("os:windows".into()))
    );
}

/// Steps of one project fill the quietest machine first, never past any machine's
/// cap; once every machine is full the work waits with each site's reason.
#[test]
fn hs2_9fktb3_work_spreads_across_sites_under_machine_caps() {
    let mut world = World::new(&[(1, 4), (2, 4)]);
    world.policy.machine.max_sessions = 3;
    // Machine 1 already runs two sessions of other work.
    world.targets[0] = state(1, snapshot(1, 16_384, 2));
    world.work = (1..=5).map(|n| project(n, 1, &[1, 2])).collect();
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(1, "not_chosen", Some("fewer_sessions"))])
    );
    assert_eq!(placed_on(&schedule, 2), target(2));
    // Three sessions on each machine: the site running fewer of the project's wins.
    assert_eq!(placed_on(&schedule, 3), target(1));
    assert_eq!(
        passes(&schedule, 3),
        expect(&[pass(2, "not_chosen", Some("fewer_sessions"))])
    );
    assert_eq!(placed_on(&schedule, 4), target(2));
    assert_eq!(
        passes(&schedule, 4),
        expect(&[pass(1, "machine_refused", Some("session_cap_reached"))])
    );
    assert_eq!(
        blocked(&schedule, 5),
        ("machine_refused", Some("session_cap_reached".into()))
    );
    assert_eq!(
        passes(&schedule, 5),
        expect(&[pass(2, "machine_refused", Some("session_cap_reached"))])
    );
    // No machine is asked for more than its cap.
    for (server, running) in [(1u128, 2usize), (2, 0)] {
        let planned = schedule
            .placements
            .iter()
            .filter(|placement| placement.action == PlacementAction::Start)
            .filter(|placement| placement.target_id == target(server))
            .count();
        assert!(running + planned <= 3, "machine {server}: {schedule:#?}");
    }
}

/// A free session on another site of the project takes the work before anything
/// starts; the named site is passed over for it.
#[test]
fn hs2_9fktb3_a_free_session_on_another_site_takes_the_work() {
    let mut world = World::new(&[(1, 2), (2, 2)]);
    world.sessions = vec![session("free", 2, SessionActivity::Idle)];
    world.work = vec![project(1, 1, &[1, 2])];
    let schedule = world.now();
    assert!(starts(&schedule).is_empty());
    assert_eq!(fate(&schedule, "free"), ("keep", "refill"));
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(1, "not_chosen", Some("free_session"))])
    );
    let standing = schedule.standing(&id(1)).unwrap();
    assert_eq!(standing.standing, "keep");
    assert_eq!(standing.placement.unwrap().sites, standing.sites);
}

/// Equal sites go to the named one, then by id, whatever order the sites or the work
/// were listed in.
#[test]
fn hs2_9fktb3_ties_are_broken_the_same_way_in_any_order() {
    let mut world = World::new(&[(1, 2), (2, 2), (3, 2)]);
    world.work = vec![project(1, 1, &[3, 2, 1])];
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(1));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[
            pass(2, "not_chosen", Some("site_order")),
            pass(3, "not_chosen", Some("site_order")),
        ])
    );
    // The named site cannot: the lowest id of the others.
    os(&mut world, 1, OsFamily::Macos);
    os(&mut world, 2, OsFamily::Linux);
    os(&mut world, 3, OsFamily::Linux);
    world.work = vec![
        needs(project(1, 1, &[3, 2]), "os=linux"),
        needs(project(2, 1, &[2, 3]), "os=linux"),
        needs(project(3, 1, &[3, 1, 2, 2]), "os=linux"),
    ];
    let forward = world.now();
    assert_eq!(placed_on(&forward, 1), target(2));
    assert_eq!(placed_on(&forward, 2), target(3));
    assert_eq!(placed_on(&forward, 3), target(2));
    world.work.reverse();
    for item in &mut world.work {
        item.sites.reverse();
    }
    world.targets.reverse();
    assert_eq!(world.now(), forward);
}

/// A site removed between passes is not tried again; added back, it is.
#[test]
fn hs2_9fktb3_a_site_removed_between_passes_is_no_longer_tried() {
    let mut world = World::new(&[(1, 1), (2, 1)]);
    world.sessions = vec![session("busy", 1, SessionActivity::Busy)];
    world.work = vec![project(1, 1, &[2])];
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(passes(&schedule, 1), expect(&[pass(1, "target_cap", None)]));

    // Removed from the manifest: the runtime lists it no more and stops reading it.
    let full = (
        world.work.clone(),
        world.targets.clone(),
        world.policy.clone(),
    );
    world.work[0].sites.clear();
    world.targets.truncate(1);
    world.policy.targets.truncate(1);
    let schedule = world.now();
    assert_eq!(blocked(&schedule, 1), ("target_cap", None));
    assert!(passes(&schedule, 1).is_empty());

    // HS2-N5BDE4: still listed but out of the policy: Auto, so tried; with its
    // capacity no longer read, it waits for the read rather than being refused.
    world.work[0].sites = vec![target(2)];
    let schedule = world.now();
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(2, "target_unavailable", Some("capacity_not_read"))])
    );

    // Added back.
    (world.work, world.targets, world.policy) = full;
    assert_eq!(placed_on(&world.now(), 1), target(2));
}

/// A resource held on one site's machine sends the work to another site; held on
/// every machine, the work waits on each and says so in the contention list.
#[test]
fn hs2_9fktb3_resources_are_checked_on_each_sites_machine() {
    let mut world = World::new(&[(1, 2), (2, 2)]);
    let need = ResourceNeed::parse("ports/8080").unwrap();
    let mut item = project(1, 1, &[2]);
    item.resources = vec![need.clone()];
    world.work = vec![item];
    let held_on = |machine: u128| HeldResource {
        namespace: need.namespace.clone(),
        resource: need.resource.clone(),
        expires_at_ms: T0 + 10 * MIN,
        server_id: Some(id(9000 + machine)),
    };
    world.held = vec![held_on(1)];
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(1, "resource_held", Some("ports/8080"))])
    );
    assert!(
        schedule
            .contention
            .iter()
            .all(|entry| !entry.waiting.contains(&id(1))),
        "placed work waits on nothing: {:#?}",
        schedule.contention
    );
    assert_eq!(schedule.retry_at_ms, None);

    world.held.push(held_on(2));
    let schedule = world.now();
    assert_eq!(
        blocked(&schedule, 1),
        ("resource_held", Some("ports/8080".into()))
    );
    let waiting: Vec<Option<String>> = schedule
        .contention
        .iter()
        .filter(|entry| entry.waiting.contains(&id(1)))
        .map(|entry| entry.server_id.clone())
        .collect();
    assert_eq!(waiting, vec![Some(id(9001)), Some(id(9002))]);
    assert_eq!(schedule.retry_at_ms, Some(T0 + 10 * MIN));
}

/// A site that refused a start recently is passed over while another site of the
/// project can take the work; the backoff then sets no retry time for the step.
#[test]
fn hs2_9fktb3_a_refusing_site_is_passed_over_for_another() {
    let mut world = World::new(&[(1, 2), (2, 2)]);
    world.memory = remember(
        &[],
        &[(
            target(1),
            PlacementOutcome::Refused("below_ram_floor".into()),
        )],
        T0 - 1000,
    );
    world.work = vec![project(1, 1, &[2])];
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(1, "refusal_backoff", Some("below_ram_floor"))])
    );
    assert_eq!(schedule.retry_at_ms, None);
    // Alone, the step waits out the backoff as before.
    world.work[0].sites.clear();
    let schedule = world.now();
    assert_eq!(
        blocked(&schedule, 1),
        ("refusal_backoff", Some("below_ram_floor".into()))
    );
    assert!(schedule.retry_at_ms.is_some());
}

/// The decision log keeps each passed-over site as an id, a code and a bounded detail.
#[test]
fn hs2_9fktb3_a_decision_record_names_passed_over_sites_by_code() {
    let mut record = DecisionRecord {
        at_ms: T0,
        tick_id: id(1),
        mode: DecisionMode::DryRun,
        trigger: "manual".into(),
        kind: DecisionKind::Start,
        actor_id: Some(OWNER.into()),
        target_id: Some(target(2)),
        tool: Some("alpha".into()),
        account: None,
        model: None,
        session_id: None,
        step_id: Some(id(2)),
        reason: Some("ready_work".into()),
        detail: None,
        outcome: None,
        outcome_detail: None,
        operation_id: None,
        run_id: None,
        probe: false,
        retry_at_ms: None,
        score: None,
        inputs: None,
        tick: None,
        sites: vec![SitePass {
            target_id: target(1),
            reason: "requirement_unmet".into(),
            detail: Some("os:windows".into()),
        }],
    };
    record.validate().unwrap();
    let json = serde_json::to_value(&record).unwrap();
    assert_eq!(json["sites"][0]["detail"], "os:windows");
    record.sites[0].detail = Some("two words".into());
    assert!(record.validate().is_err());
    record.sites[0].detail = None;
    record.sites[0].target_id = "../escape".into();
    assert!(record.validate().is_err());
}

/// HS2-6NQKP9: each site is judged by its own `placement.resources` entry, never by
/// the entry of the site the step names. Two sites require different resources;
/// whichever site is chosen, only its own requirement is checked and taken there.
#[test]
fn hs2_6nqkp9_each_site_requires_its_own_policy_resources() {
    let mut world = World::new(&[(1, 2), (2, 2), (3, 2)]);
    let mac = ResourceNeed::parse("vm/macos-vm").unwrap();
    let win = ResourceNeed::parse("vm/windows-vm").unwrap();
    world.policy.placement.resources = vec![
        TargetResources {
            target_id: target(1),
            resources: vec![mac.clone()],
        },
        TargetResources {
            target_id: target(2),
            resources: vec![win.clone()],
        },
    ];
    let held = |need: &ResourceNeed, machine: u128| HeldResource {
        namespace: need.namespace.clone(),
        resource: need.resource.clone(),
        expires_at_ms: T0 + 10 * MIN,
        server_id: Some(id(9000 + machine)),
    };
    world.work = vec![project(1, 1, &[2])];

    // The named site's resource is held there: the other site needs only its own,
    // which is free, so the step runs there.
    world.held = vec![held(&mac, 1)];
    let schedule = world.now();
    assert_eq!(placed_on(&schedule, 1), target(2));
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(1, "resource_held", Some("vm/macos-vm"))])
    );

    // The named site's resource held on the other machine means nothing there.
    world.held = vec![held(&mac, 2)];
    assert_eq!(placed_on(&world.now(), 1), target(1));

    // The other site's own resource is held there: it is passed over by that name,
    // and the named site is passed over for its own.
    world.held = vec![held(&mac, 1), held(&win, 2)];
    let schedule = world.now();
    assert_eq!(
        blocked(&schedule, 1),
        ("resource_held", Some("vm/macos-vm".into()))
    );
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(2, "resource_held", Some("vm/windows-vm"))])
    );

    // A site without an entry requires nothing of the policy; the step's own
    // resource still goes wherever the step runs.
    let port = ResourceNeed::parse("ports/8080").unwrap();
    let mut item = project(1, 1, &[3]);
    item.resources = vec![port.clone()];
    world.work = vec![item];
    world.held = vec![held(&mac, 1)];
    assert_eq!(placed_on(&world.now(), 1), target(3));
    world.held = vec![held(&mac, 1), held(&port, 3)];
    let schedule = world.now();
    assert_eq!(
        passes(&schedule, 1),
        expect(&[pass(3, "resource_held", Some("ports/8080"))])
    );

    // Two steps that may run on either site: each site's own resource is given to
    // one step only, so the second goes to the other site.
    world.work = vec![project(1, 1, &[2]), project(2, 1, &[2])];
    world.held = Vec::new();
    let schedule = world.now();
    let mut sites = vec![placed_on(&schedule, 1), placed_on(&schedule, 2)];
    sites.sort();
    assert_eq!(sites, vec![target(1), target(2)], "{schedule:#?}");
    assert_eq!(
        world.work[0].needs_on(&world.policy, &target(2)),
        vec![&win]
    );
}
