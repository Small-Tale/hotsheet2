//! HS2-5P112Z, HS2-B2N8EH: role sessions are decided per project and placed on its
//! home site, the project's own target. Workers on every other site join the home's
//! one pool; no site but the home ever gets a dispatcher or an integrator, and there
//! is no fallback home. Synthetic targets, tools and times only.

use super::scheduler_tests::{MIN, T0, World, id, session, target, work};
use super::*;

const D: RoleSessionKind = RoleSessionKind::Dispatcher;
const I: RoleSessionKind = RoleSessionKind::Integrator;
/// One room case: home cap, site cap, site enabled, the role starts, the step starts.
type RoomCase<'a> = (u32, u32, bool, Vec<&'a str>, Vec<&'a str>);

/// The policy lists `roles` for target `n`.
fn opted(world: &mut World, n: u128, roles: &[RoleSessionKind]) {
    world
        .policy
        .placement
        .role_sessions
        .push(TargetRoleSessions {
            target_id: target(n),
            roles: roles.to_vec(),
        });
}

/// Target `n`'s machine reports `pool`.
fn pool(world: &mut World, n: u128, pool: Option<PoolState>) {
    let state = world
        .targets
        .iter_mut()
        .find(|state| state.target_id == target(n))
        .unwrap();
    state.capacity.as_mut().unwrap().pool = pool;
}

fn handoffs(pending: u32) -> Option<PoolState> {
    Some(PoolState {
        handoffs_pending: pending,
        ..PoolState::default()
    })
}

/// Ready work `n` named on target `on`, which may also run on the other sites.
fn site_work(world: &World, n: u128, on: u128) -> WorkItem {
    WorkItem {
        sites: world.sites.other_sites(&target(on)),
        ..work(n, on)
    }
}

/// Every role start as `<role>@<target n>`, in placement order.
fn roles_started(schedule: &Schedule) -> Vec<String> {
    schedule
        .placements
        .iter()
        .filter(|placement| placement.action == PlacementAction::Start)
        .filter_map(|placement| {
            let role = placement.role?;
            let n = (1..10)
                .find(|n| target(*n) == placement.target_id)
                .unwrap_or(0);
            Some(format!("{}@{n}", role.code()))
        })
        .collect()
}

/// Where each step was started, as `<step n>@<target n>`.
fn steps_started(schedule: &Schedule) -> Vec<String> {
    schedule
        .placements
        .iter()
        .filter(|placement| placement.action == PlacementAction::Start)
        .filter(|placement| placement.role.is_none())
        .map(|placement| {
            let step = (1..10).find(|n| id(*n) == placement.steps[0]).unwrap_or(0);
            let on = (1..10)
                .find(|n| target(*n) == placement.target_id)
                .unwrap_or(0);
            format!("{step}@{on}")
        })
        .collect()
}

fn decided(schedule: &Schedule, name: &str) -> (&'static str, &'static str) {
    let placement = schedule
        .placements
        .iter()
        .find(|placement| placement.session_id.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("no decision for session {name}: {schedule:#?}"));
    (placement.action.code(), placement.reason.code())
}

fn role_session(name: &str, on: u128, role: SessionRole) -> ScheduledSession {
    ScheduledSession {
        role: Some(role),
        ..session(name, on, SessionActivity::Idle)
    }
}

/// Project 1 with its site 2, both with room; the home lists both roles and its pool
/// is known and empty.
fn project() -> World {
    let mut world = World::new(&[(1, 3), (2, 3)]);
    world.sites = ProjectSites::default().with_site(&target(2), &target(1));
    opted(&mut world, 1, &[D, I]);
    pool(&mut world, 1, Some(PoolState::default()));
    world
}

#[test]
fn hs2_5p112z_the_home_rule_is_the_projects_own_target() {
    let sites = ProjectSites::default().with_site(&target(2), &target(1));
    assert_eq!(sites.home_of(&target(1)), target(1));
    assert_eq!(sites.home_of(&target(2)), target(1));
    assert!(sites.has_sites(&target(1)) && sites.has_sites(&target(2)));
    // A target nobody groups is its own one-site home.
    assert_eq!(sites.home_of(&target(3)), target(3));
    assert!(!sites.has_sites(&target(3)));
    assert!(!ProjectSites::default().has_sites(&target(1)));
}

#[test]
fn hs2_5p112z_role_sessions_follow_a_project_from_work_on_a_site_to_wound_down() {
    let mut world = project();
    // Nothing anywhere: nothing starts, also when repeated.
    for _ in 0..2 {
        assert!(roles_started(&world.now()).is_empty());
    }

    // Work named only on the site: the home's dispatcher is placed, never one on the
    // site, before the worker.
    world.work = vec![work(1, 2)];
    let schedule = world.now();
    assert_eq!(roles_started(&schedule), ["dispatcher@1"]);
    assert_eq!(steps_started(&schedule), ["1@2"]);

    // The worker runs on the site: the home dispatcher stays for it.
    world.work.clear();
    world.sessions = vec![
        role_session("dispatcher", 1, SessionRole::Dispatcher),
        session("worker", 2, SessionActivity::Busy),
    ];
    let later = T0 + 600 * MIN;
    assert_eq!(
        decided(&world.at(later), "dispatcher"),
        ("keep", "role_needed")
    );

    // Handed off into the home's pool: the integrator is placed on the home. A
    // handoff the site's own pool shows starts nothing there.
    world.sessions[1] = session("worker", 2, SessionActivity::Idle);
    pool(&mut world, 1, handoffs(1));
    pool(&mut world, 2, handoffs(4));
    let schedule = world.now();
    assert_eq!(roles_started(&schedule), ["integrator@1"]);
    assert_eq!(decided(&schedule, "dispatcher"), ("keep", "idle_grace"));

    // Integrated: both wind down at their safe points once nothing needs them.
    world
        .sessions
        .push(role_session("integrator", 1, SessionRole::Integrator));
    assert_eq!(
        decided(&world.at(later), "integrator"),
        ("keep", "role_needed")
    );
    pool(&mut world, 1, Some(PoolState::default()));
    let schedule = world.at(later);
    assert_eq!(decided(&schedule, "integrator").0, "wind_down");
    assert_eq!(decided(&schedule, "dispatcher").0, "wind_down");
}

#[test]
fn hs2_5p112z_an_entry_naming_a_site_starts_nothing_until_it_is_its_own_project() {
    let mut world = World::new(&[(1, 3), (2, 3)]);
    world.sites = ProjectSites::default().with_site(&target(2), &target(1));
    opted(&mut world, 2, &[D, I]);
    // HS2-R2N33M: the home opts out, so it is not Auto either.
    opted(&mut world, 1, &[]);
    pool(&mut world, 1, Some(PoolState::default()));
    pool(&mut world, 2, handoffs(2));
    world.work = vec![work(1, 2)];
    // The site is not a home: no eager dispatcher or integrator on it.
    let schedule = world.now();
    assert!(roles_started(&schedule).is_empty(), "{schedule:#?}");
    assert_eq!(steps_started(&schedule), ["1@2"]);

    // The site is removed from the project (`--clear-site-of`): it is its own home
    // now, and its entry places its roles there as before sites existed.
    world.sites = ProjectSites::default();
    assert_eq!(
        roles_started(&world.now()),
        ["dispatcher@2", "integrator@2"]
    );

    // Added again: back to nothing on the site.
    world.sites = ProjectSites::default().with_site(&target(2), &target(1));
    assert!(roles_started(&world.now()).is_empty());
}

#[test]
fn hs2_5p112z_an_offline_home_starts_no_role_anywhere_and_there_is_no_fallback() {
    let mut world = project();
    pool(&mut world, 2, Some(PoolState::default()));
    // The site lists roles too: it is still not a home.
    opted(&mut world, 2, &[D, I]);
    world.work = vec![site_work(&world, 1, 1)];
    let online = world.now();
    assert_eq!(roles_started(&online), ["dispatcher@1"]);

    // The home's machine cannot be read: nothing starts as its replacement. The work
    // itself still goes to the site, which waits for a dispatcher as any run does.
    world.targets[0] = TargetState {
        target_id: target(1),
        capacity: None,
        unavailable: Some("target_unreachable".into()),
    };
    let offline = world.now();
    assert!(roles_started(&offline).is_empty(), "{offline:#?}");
    assert_eq!(steps_started(&offline), ["1@2"]);

    // A home whose pool is unknown is the same.
    let mut unknown = project();
    pool(&mut unknown, 1, None);
    unknown.work = vec![site_work(&unknown, 1, 1)];
    assert!(roles_started(&unknown.now()).is_empty());

    // A home disabled in the policy is the same.
    let mut disabled = project();
    disabled.policy.targets[0].enabled = false;
    disabled.work = vec![work(1, 2)];
    assert!(roles_started(&disabled.now()).is_empty());
}

#[test]
fn hs2_5p112z_two_projects_each_get_their_roles_on_their_own_home() {
    let mut world = World::new(&[(1, 3), (2, 3), (3, 3), (4, 3)]);
    world.policy.machine.max_sessions = 8;
    world.sites = ProjectSites::default()
        .with_site(&target(2), &target(1))
        .with_site(&target(4), &target(3));
    for n in [1, 3] {
        opted(&mut world, n, &[D, I]);
        pool(&mut world, n, Some(PoolState::default()));
    }
    // Work only on the second project's site: only its home gets a dispatcher.
    world.work = vec![work(1, 4)];
    assert_eq!(roles_started(&world.now()), ["dispatcher@3"]);
    // Work on both sites: each home its own.
    world.work = vec![work(1, 4), work(2, 2)];
    assert_eq!(
        roles_started(&world.now()),
        ["dispatcher@1", "dispatcher@3"]
    );
    // A handoff pending in one home's pool starts that home's integrator only.
    world.work.clear();
    pool(&mut world, 3, handoffs(1));
    assert_eq!(roles_started(&world.now()), ["integrator@3"]);
    // A worker of one project keeps only its own home's dispatcher.
    pool(&mut world, 3, Some(PoolState::default()));
    world.sessions = vec![
        role_session("d1", 1, SessionRole::Dispatcher),
        role_session("d3", 3, SessionRole::Dispatcher),
        session("worker", 2, SessionActivity::Busy),
    ];
    let later = T0 + 600 * MIN;
    let schedule = world.at(later);
    assert_eq!(decided(&schedule, "d1"), ("keep", "role_needed"));
    assert_eq!(decided(&schedule, "d3").0, "wind_down");
}

#[test]
fn hs2_5p112z_the_dispatcher_takes_the_homes_last_slot_when_another_site_has_room() {
    // (home cap, site cap, site enabled, expected roles, expected steps)
    let cases: Vec<RoomCase<'_>> = vec![
        // The site can take the worker: the dispatcher takes the home's only slot.
        (1, 2, true, vec!["dispatcher@1"], vec!["1@2"]),
        // The site is disabled: as a one-site project, the worker goes first.
        (1, 2, false, vec![], vec!["1@1"]),
        // Room for both on the home; the worker still goes where fewer sessions run.
        (2, 2, true, vec!["dispatcher@1"], vec!["1@2"]),
        // A one-slot home and a full site: the worker takes the home's slot.
        (1, 0, true, vec![], vec!["1@1"]),
    ];
    for (home_cap, site_cap, enabled, roles, steps) in cases {
        let mut world = World::new(&[(1, home_cap), (2, site_cap)]);
        world.sites = ProjectSites::default().with_site(&target(2), &target(1));
        world.policy.targets[1].enabled = enabled;
        opted(&mut world, 1, &[D]);
        pool(&mut world, 1, Some(PoolState::default()));
        world.work = vec![site_work(&world, 1, 1)];
        let schedule = world.now();
        let case = format!("home {home_cap}, site {site_cap}, enabled {enabled}");
        assert_eq!(roles_started(&schedule), roles, "{case}");
        assert_eq!(steps_started(&schedule), steps, "{case}");
    }
}

#[test]
fn hs2_5p112z_a_role_session_on_another_site_keeps_its_own_targets_rule() {
    // A person's dispatcher on the site serves the site's own pool: it is kept while
    // a worker there has a run, and the home's work alone does not keep it.
    let mut world = project();
    world.sessions = vec![
        role_session("site-dispatcher", 2, SessionRole::Dispatcher),
        session("worker", 2, SessionActivity::Busy),
    ];
    let later = T0 + 600 * MIN;
    assert_eq!(
        decided(&world.at(later), "site-dispatcher"),
        ("keep", "role_needed")
    );
    world.sessions[1] = session("worker", 1, SessionActivity::Busy);
    assert_eq!(decided(&world.at(later), "site-dispatcher").0, "wind_down");
}

/// HS2-SYZB33: an entry on a site that is not its project's home is a warning that
/// names the home, never a problem; the home's own entry, a one-site project and a
/// site separated again warn nothing. The schedule ignores the site's entry either way.
#[test]
fn hs2_syzb33_role_sessions_on_a_site_warn_and_name_the_home() {
    let mut world = project();
    assert!(world.policy.warnings(&world.sites).is_empty());
    opted(&mut world, 2, &[D]);
    let warnings = world.policy.warnings(&world.sites);
    assert_eq!(
        warnings,
        vec![PolicyWarning {
            code: ROLE_SESSIONS_NOT_HOME,
            message: "role sessions start only on the project's home site; this entry has no effect",
            subject: target(2),
            home_target_id: Some(target(1)),
        }]
    );
    assert!(
        world.policy.problems(None).is_empty(),
        "a warning is not a problem"
    );
    let json = serde_json::to_value(&warnings).unwrap();
    assert_eq!(json[0]["code"], "role_sessions_not_home");
    assert_eq!(json[0]["home_target_id"], target(1));
    assert!(
        !roles_started(&world.now())
            .iter()
            .any(|name| name.ends_with("@2"))
    );
    // Separated again: target 2 is its own home, and its entry takes effect.
    assert!(world.policy.warnings(&ProjectSites::default()).is_empty());
}

/// HS2-70Z2Z6: one two-site project whose home (target 1) runs Linux and whose site
/// (target 2) runs `site_os`, with these caps, the home listing the dispatcher, and
/// ready step 1 that needs Linux and may run on either site.
fn linux_step_world(home_cap: u32, site_cap: u32, site_os: OsFamily) -> World {
    let mut world = World::new(&[(1, home_cap), (2, site_cap)]);
    world.sites = ProjectSites::default().with_site(&target(2), &target(1));
    for (n, family) in [(1, OsFamily::Linux), (2, site_os)] {
        world
            .targets
            .iter_mut()
            .find(|state| state.target_id == target(n))
            .and_then(|state| state.capacity.as_mut())
            .unwrap()
            .capabilities = Some(HostCapabilities::bare(family, "x86_64"));
    }
    opted(&mut world, 1, &[D]);
    pool(&mut world, 1, Some(PoolState::default()));
    let mut item = site_work(&world, 1, 1);
    item.requirements.add("os=linux").unwrap();
    item.requirements = item.requirements.clone().normalized().unwrap();
    world.work = vec![item];
    world
}

/// The repro: a one-slot home and a one-slot site that cannot run the only ready
/// step. The dispatcher would hold the home's slot for work nobody could take, so the
/// worker starts there instead, on every pass.
#[test]
fn hs2_70z2z6_the_dispatcher_leaves_the_last_slot_to_work_that_fits_nowhere_else() {
    let world = linux_step_world(1, 1, OsFamily::Macos);
    for now in [T0, T0 + MIN, T0 + 600 * MIN] {
        let schedule = world.at(now);
        assert_eq!(
            roles_started(&schedule),
            Vec::<String>::new(),
            "{schedule:#?}"
        );
        assert_eq!(steps_started(&schedule), vec!["1@1"], "{schedule:#?}");
    }
    // A worker already running on the site that cannot run the step does not let
    // the dispatcher take the home's slot either.
    let mut world = linux_step_world(1, 2, OsFamily::Macos);
    world.sessions = vec![session("site-worker", 2, SessionActivity::Idle)];
    let schedule = world.now();
    assert_eq!(
        roles_started(&schedule),
        Vec::<String>::new(),
        "{schedule:#?}"
    );
    assert_eq!(steps_started(&schedule), vec!["1@1"], "{schedule:#?}");
}

/// The site can run the step and has room: the dispatcher takes the home's last slot
/// and the worker starts on the site.
#[test]
fn hs2_70z2z6_the_dispatcher_takes_the_last_slot_when_the_work_runs_elsewhere() {
    let schedule = linux_step_world(1, 1, OsFamily::Linux).now();
    assert_eq!(
        roles_started(&schedule),
        vec!["dispatcher@1"],
        "{schedule:#?}"
    );
    assert_eq!(steps_started(&schedule), vec!["1@2"], "{schedule:#?}");
}

/// Room for both on the home: the dispatcher starts even though only the home can
/// run the step, and the worker starts beside it.
#[test]
fn hs2_70z2z6_the_dispatcher_starts_beside_the_work_when_the_home_has_spare_room() {
    let schedule = linux_step_world(2, 1, OsFamily::Macos).now();
    assert_eq!(
        roles_started(&schedule),
        vec!["dispatcher@1"],
        "{schedule:#?}"
    );
    assert_eq!(steps_started(&schedule), vec!["1@1"], "{schedule:#?}");
}

/// No site meets the step's requirements: nothing starts, not even the dispatcher
/// with room to spare, and the step stays unplaced naming what each site misses.
#[test]
fn hs2_70z2z6_work_no_site_can_run_starts_no_dispatcher() {
    let mut world = linux_step_world(3, 3, OsFamily::Macos);
    world
        .targets
        .iter_mut()
        .find(|state| state.target_id == target(1))
        .and_then(|state| state.capacity.as_mut())
        .unwrap()
        .capabilities = Some(HostCapabilities::bare(OsFamily::Windows, "x86_64"));
    let schedule = world.now();
    assert_eq!(
        roles_started(&schedule),
        Vec::<String>::new(),
        "{schedule:#?}"
    );
    assert_eq!(
        steps_started(&schedule),
        Vec::<String>::new(),
        "{schedule:#?}"
    );
    let standing = schedule.standing(&id(1)).unwrap();
    assert_eq!(
        (
            standing.standing,
            standing.reason,
            standing.detail.as_deref()
        ),
        ("not_placed", "requirement_unmet", Some("os:linux")),
        "{standing:#?}"
    );
    let sites: Vec<_> = standing
        .sites
        .iter()
        .map(|pass| (pass.target_id.clone(), pass.reason.as_str()))
        .collect();
    assert_eq!(sites, vec![(target(2), "requirement_unmet")]);
}
