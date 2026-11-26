//! HS2-EX594V: a multi-site project's home out of reach is observed, remembered and
//! announced once per transition, debounced. Synthetic targets and times only.

use super::scheduler_tests::{MIN, T0, World, session, target, work};
use super::*;

const AFTER: u64 = HOME_OUTAGE_NOTIFY_AFTER_MS;

/// Project 1 with its site 2, work on the project, the home's pool known.
fn project() -> World {
    let mut world = World::new(&[(1, 3), (2, 3)]);
    world.sites = ProjectSites::default().with_site(&target(2), &target(1));
    home_pool(&mut world, Some(PoolState::default()));
    world.work.push(work(1, 1));
    world
}

fn home_pool(world: &mut World, pool: Option<PoolState>) {
    let home = world
        .targets
        .iter_mut()
        .find(|state| state.target_id == target(1))
        .unwrap();
    if let Some(capacity) = home.capacity.as_mut() {
        capacity.pool = pool;
    }
}

/// The home's capacity read failed with `code`.
fn home_unread(world: &mut World, code: &str) {
    let home = world
        .targets
        .iter_mut()
        .find(|state| state.target_id == target(1))
        .unwrap();
    home.capacity = None;
    home.unavailable = Some(code.into());
}

fn observe(world: &World) -> Vec<HomeObservation> {
    observe_bound(world, true)
}

fn observe_bound(world: &World, bound: bool) -> Vec<HomeObservation> {
    observe_following(world, &[], bound)
}

/// HS2-1CC1TQ: observed with `followed` as this machine's ledger before the pass.
fn observe_following(world: &World, followed: &[HomeOutage], bound: bool) -> Vec<HomeObservation> {
    observe_homes(
        &ScheduleInput {
            now_ms: T0,
            owner_id: "owner",
            policy: &world.policy,
            work: &world.work,
            targets: &world.targets,
            sessions: &world.sessions,
            tool_entries: &[],
            held: &[],
            unreadable: &[],
            memory: &[],
            capacity_streams: &[],
            sites: &world.sites,
        },
        followed,
        &|_| bound,
    )
}

fn outage(world: &World) -> Option<(HomeOutageReason, Option<String>)> {
    let observed = observe(world);
    assert_eq!(observed.len(), 1, "{observed:?}");
    assert_eq!(observed[0].home_target_id, target(1));
    observed[0].outage.clone()
}

/// One pass at `now_ms`: the ledger after it and the notices it sent.
fn pass(
    world: &World,
    ledger: &[HomeOutage],
    now_ms: u64,
) -> (Vec<HomeOutage>, Vec<HomeNoticeKind>) {
    let observed = observe_following(world, ledger, true);
    let mut after = advance_outages(ledger, &observed, &world.sites, now_ms);
    let notices = due_notices(&mut after, now_ms, AFTER);
    (
        after,
        notices.into_iter().map(|notice| notice.kind).collect(),
    )
}

#[test]
fn hs2_ex594v_each_reason_the_home_cannot_coordinate_is_observed() {
    let mut world = project();
    assert_eq!(outage(&world), None);
    // The machine answered without its pool.
    home_pool(&mut world, None);
    assert_eq!(
        outage(&world),
        Some((HomeOutageReason::PoolUnreadable, None))
    );
    // Any failed read of a bound home is unreachable, with its code: an offline
    // machine answers `target_unavailable` too.
    for code in ["unreachable", "trust_failed", "target_unavailable"] {
        let mut world = project();
        home_unread(&mut world, code);
        assert_eq!(
            outage(&world),
            Some((HomeOutageReason::Unreachable, Some(code.into()))),
            "{code}"
        );
        // The same failure of a home this machine holds no binding for is unbound.
        let observed = observe_bound(&world, false);
        assert_eq!(
            observed[0].outage,
            Some((HomeOutageReason::Unbound, Some(code.into()))),
            "{code}"
        );
    }
    // Disabled in the policy: the pass does not read it at all.
    let mut world = project();
    world.targets.retain(|state| state.target_id != target(1));
    world.policy.targets[0].enabled = false;
    assert_eq!(outage(&world), Some((HomeOutageReason::Disabled, None)));
}

#[test]
fn hs2_ex594v_a_home_the_pass_learned_nothing_of_is_not_observed() {
    // A pass without an actor reads no target: that says nothing of the home.
    let mut world = project();
    home_unread(&mut world, "actor_required");
    assert!(observe(&world).is_empty());
    // A home the pass did not read is not observed.
    let mut world = project();
    world.targets.retain(|state| state.target_id != target(1));
    assert!(observe(&world).is_empty());
    // A disabled home of a project with no work and no session does not matter now.
    let mut world = project();
    world.work.clear();
    world.targets.retain(|state| state.target_id != target(1));
    world.policy.targets[0].enabled = false;
    assert!(observe(&world).is_empty());
    // A session on a site keeps the project busy.
    world
        .sessions
        .push(session("site-worker", 2, SessionActivity::Busy));
    assert_eq!(outage(&world), Some((HomeOutageReason::Disabled, None)));
    // Work named on the site counts for the project too.
    world.sessions.clear();
    world.work.push(work(2, 2));
    assert_eq!(outage(&world), Some((HomeOutageReason::Disabled, None)));
}

#[test]
fn hs2_ex594v_a_one_site_project_has_no_home_outage() {
    let mut world = World::new(&[(1, 3)]);
    world.work.push(work(1, 1));
    home_unread(&mut world, "unreachable");
    assert!(observe(&world).is_empty());
    assert!(ProjectSites::default().homes_with_sites().is_empty());
    let sites = ProjectSites::default()
        .with_site(&target(2), &target(1))
        .with_site(&target(3), &target(1))
        .with_site(&target(5), &target(4));
    assert_eq!(sites.homes_with_sites(), vec![target(1), target(4)]);
}

#[test]
fn hs2_ex594v_unavailable_is_announced_once_after_the_delay_and_returned_once() {
    let healthy = project();
    let mut down = project();
    home_unread(&mut down, "unreachable");

    // Seen down: remembered from now, nothing sent before the delay.
    let (ledger, sent) = pass(&down, &[], T0);
    assert!(sent.is_empty());
    assert_eq!(ledger.len(), 1);
    assert_eq!(ledger[0].since_ms, Some(T0));
    assert_eq!(ledger[0].sites, vec![target(2)]);
    assert_eq!(ledger[0].reason, Some(HomeOutageReason::Unreachable));
    assert_eq!(next_notice_at(&ledger, AFTER), Some(T0 + AFTER));
    let (ledger, sent) = pass(&down, &ledger, T0 + AFTER - 1);
    assert!(sent.is_empty());
    assert_eq!(ledger[0].since_ms, Some(T0), "the start is kept");
    // At the delay: one "unavailable".
    let (ledger, sent) = pass(&down, &ledger, T0 + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    assert!(ledger[0].announced);
    assert_eq!(next_notice_at(&ledger, AFTER), None);
    // Repeated passes while it stays down send nothing more.
    let (ledger, sent) = pass(&down, &ledger, T0 + 10 * MIN);
    assert!(sent.is_empty());
    let (ledger, sent) = pass(&down, &ledger, T0 + 20 * MIN);
    assert!(sent.is_empty());
    // Back: one "returned", and the ledger is empty.
    let (ledger, sent) = pass(&healthy, &ledger, T0 + 21 * MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Returned]);
    assert!(ledger.is_empty());
    // Staying up sends nothing.
    let (ledger, sent) = pass(&healthy, &ledger, T0 + 22 * MIN);
    assert!(sent.is_empty() && ledger.is_empty());
    // Down again: a new outage with its own start, announced after its own delay.
    let (ledger, sent) = pass(&down, &ledger, T0 + 30 * MIN);
    assert!(sent.is_empty());
    assert_eq!(ledger[0].since_ms, Some(T0 + 30 * MIN));
    let (_, sent) = pass(&down, &ledger, T0 + 30 * MIN + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
}

#[test]
fn hs2_ex594v_a_flapping_home_is_never_announced_within_the_delay() {
    let healthy = project();
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let mut ledger = Vec::new();
    let mut sent = Vec::new();
    // Down and up every half delay: each outage is shorter than the delay.
    for round in 0..10 {
        let at = T0 + round * AFTER;
        let (next, notices) = pass(&down, &ledger, at);
        sent.extend(notices);
        let (next, notices) = pass(&healthy, &next, at + AFTER / 2);
        sent.extend(notices);
        ledger = next;
        assert!(
            ledger.is_empty(),
            "an unannounced outage leaves nothing behind"
        );
    }
    assert!(sent.is_empty(), "{sent:?}");
}

#[test]
fn hs2_ex594v_down_again_before_returned_went_out_continues_the_told_outage() {
    let healthy = project();
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let (ledger, _) = pass(&down, &[], T0);
    let (ledger, sent) = pass(&down, &ledger, T0 + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    // Seen back by a pass that could not notify (a headless tick): "returned" waits.
    let ledger = advance_outages(&ledger, &observe(&healthy), &healthy.sites, T0 + 5 * MIN);
    assert_eq!(ledger.len(), 1);
    assert!(!ledger[0].current());
    assert_eq!(ledger[0].returned_at_ms, Some(T0 + 5 * MIN));
    // A later pass that can notify sends it, dated when the home came back.
    let mut later = ledger.clone();
    let notices = due_notices(&mut later, T0 + 6 * MIN, AFTER);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, HomeNoticeKind::Returned);
    assert_eq!(notices[0].at_ms, T0 + 5 * MIN);
    assert!(later.is_empty());
    // Down again before it went out: still the outage the person was told of.
    let (ledger, sent) = pass(&down, &ledger, T0 + 6 * MIN);
    assert!(sent.is_empty(), "{sent:?}");
    assert!(ledger[0].announced && ledger[0].current());
    assert_eq!(ledger[0].returned_at_ms, None);
    // So its end is announced once.
    let (ledger, sent) = pass(&healthy, &ledger, T0 + 7 * MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Returned]);
    assert!(ledger.is_empty());
}

#[test]
fn hs2_ex594v_a_refusal_backoff_keeps_the_reason_seen_before_it() {
    let mut down = project();
    home_unread(&mut down, "trust_failed");
    let (ledger, _) = pass(&down, &[], T0);
    let mut waiting = project();
    home_unread(&mut waiting, "refusal_backoff");
    let (ledger, _) = pass(&waiting, &ledger, T0 + MIN);
    assert_eq!(ledger[0].reason, Some(HomeOutageReason::Unreachable));
    assert_eq!(ledger[0].detail.as_deref(), Some("trust_failed"));
    assert_eq!(ledger[0].since_ms, Some(T0));
    // A backoff with nothing seen before stands as unreachable.
    let (fresh, _) = pass(&waiting, &[], T0);
    assert_eq!(fresh[0].reason, Some(HomeOutageReason::Unreachable));
    assert_eq!(fresh[0].detail.as_deref(), Some("refusal_backoff"));
    // A new reason replaces the old one; the start stays.
    let mut disabled = project();
    disabled.policy.targets[0].enabled = false;
    let (ledger, _) = pass(&disabled, &ledger, T0 + 2 * MIN);
    assert_eq!(ledger[0].reason, Some(HomeOutageReason::Disabled));
    assert_eq!(ledger[0].detail, None);
    assert_eq!(ledger[0].since_ms, Some(T0));
}

#[test]
fn hs2_ex594v_an_unobserved_home_keeps_its_entry_and_a_regrouped_one_is_dropped() {
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let (ledger, _) = pass(&down, &[], T0);
    // A pass that did not read the home leaves the outage as it was.
    let mut idle = project();
    idle.work.clear();
    idle.targets.retain(|state| state.target_id != target(1));
    let kept = advance_outages(&ledger, &observe(&idle), &idle.sites, T0 + MIN);
    assert_eq!(kept, ledger);
    // The delay counts from the start, so a pass after it announces even unread.
    let (_, sent) = pass(&idle, &ledger, T0 + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    // Once the site is separated into its own project the entry is dropped silently.
    let mut split = project();
    split.sites = ProjectSites::default();
    let (after, sent) = pass(&split, &ledger, T0 + AFTER);
    assert!(after.is_empty() && sent.is_empty());
}

#[test]
fn hs2_ex594v_the_store_writes_only_changes_and_reads_its_ledger_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = HomeOutageStore::new(dir.path());
    assert!(store.outages("owner").unwrap().is_empty());
    let file = store.directory().join("owner.json");
    assert!(!file.exists(), "a read creates nothing");
    let world = project();
    let mut down = project();
    home_unread(&mut down, "unreachable");
    // Healthy: nothing to remember, nothing written.
    let (ledger, sent) = store
        .advance("owner", &observe(&world), &world.sites, T0, Some(AFTER))
        .unwrap();
    assert!(ledger.is_empty() && sent.is_empty());
    assert!(!file.exists());
    // Down, without a notifier: remembered, nothing sent, nothing marked sent.
    let (ledger, sent) = store
        .advance("owner", &observe(&down), &down.sites, T0, None)
        .unwrap();
    assert_eq!(ledger.len(), 1);
    assert!(sent.is_empty());
    assert_eq!(store.outages("owner").unwrap(), ledger);
    let (_, sent) = store
        .advance("owner", &observe(&down), &down.sites, T0 + AFTER, None)
        .unwrap();
    assert!(sent.is_empty());
    assert!(!store.outages("owner").unwrap()[0].announced);
    // With one: sent once, then never again.
    let (_, sent) = store
        .advance(
            "owner",
            &observe(&down),
            &down.sites,
            T0 + AFTER,
            Some(AFTER),
        )
        .unwrap();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].kind, HomeNoticeKind::Unavailable);
    assert_eq!(sent[0].at_ms, T0);
    let (_, sent) = store
        .advance(
            "owner",
            &observe(&down),
            &down.sites,
            T0 + 2 * AFTER,
            Some(AFTER),
        )
        .unwrap();
    assert!(sent.is_empty());
    // Other owners are apart.
    assert!(store.outages("other").unwrap().is_empty());
    let (ledger, sent) = store
        .advance(
            "owner",
            &observe(&world),
            &world.sites,
            T0 + 3 * AFTER,
            Some(AFTER),
        )
        .unwrap();
    assert!(ledger.is_empty());
    assert_eq!(sent[0].kind, HomeNoticeKind::Returned);
    assert!(store.outages("owner").unwrap().is_empty());
    assert!(store.outages("../escape").is_err());
}

#[test]
fn hs2_ex594v_a_damaged_ledger_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let store = HomeOutageStore::new(dir.path());
    std::fs::create_dir_all(store.directory()).unwrap();
    let path = store.directory().join("owner.json");
    std::fs::write(
        &path,
        b"{\"schema_version\":1,\"outages\":[{\"home_target_id\":\"../x\",\"sites\":[]}]}",
    )
    .unwrap();
    assert!(matches!(
        store.outages("owner"),
        Err(OrchestrationError::Corrupt)
    ));
    std::fs::write(&path, b"not json").unwrap();
    assert!(store.outages("owner").is_err());
}

/// HS2-1CC1TQ: the project with no work and no session left; its home read as `read`
/// (`None`: read fine), as a pass reads a followed home.
fn idle(read: Option<&str>) -> World {
    let mut world = project();
    world.work.clear();
    if let Some(code) = read {
        home_unread(&mut world, code);
    }
    world
}

/// An outage announced at `T0 + AFTER`, seen while the project had work.
fn announced() -> Vec<HomeOutage> {
    let mut down = project();
    home_unread(&mut down, "target_unavailable");
    let (ledger, _) = pass(&down, &[], T0);
    let (ledger, sent) = pass(&down, &ledger, T0 + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    ledger
}

#[test]
fn hs2_1cc1tq_an_idle_project_still_follows_its_home_until_it_is_back() {
    // Without a ledger entry an idle project's home is not read, so not observed.
    let idle_up = idle(None);
    let mut unread = idle(None);
    unread.targets.retain(|state| state.target_id != target(1));
    assert!(observe(&unread).is_empty());
    // Out, announced, then the project goes idle with the home still out: a pass
    // reads it (the scheduler reads every followed home) and keeps the outage as it
    // was, its start and its announcement, and sends nothing more.
    let ledger = announced();
    let idle_down = idle(Some("target_unavailable"));
    let observed = observe_following(&idle_down, &ledger, true);
    assert_eq!(
        observed[0].outage,
        Some((
            HomeOutageReason::Unreachable,
            Some("target_unavailable".into())
        ))
    );
    let (still, sent) = pass(&idle_down, &ledger, T0 + 10 * MIN);
    assert!(sent.is_empty(), "{sent:?}");
    assert_eq!(still, ledger, "nothing changed while it stays out");
    let (still, sent) = pass(&idle_down, &still, T0 + 40 * MIN);
    assert!(sent.is_empty());
    assert_eq!(still[0].since_ms, Some(T0));
    // Back while idle: "back" once, and the ledger is empty.
    let (after, sent) = pass(&idle_up, &still, T0 + 41 * MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Returned]);
    assert!(after.is_empty());
    // With nothing left on record the idle home is not read or observed again.
    let (after, sent) = pass(&idle_up, &after, T0 + 42 * MIN);
    assert!(after.is_empty() && sent.is_empty());
    assert!(observe_following(&unread, &after, true).is_empty());
}

#[test]
fn hs2_1cc1tq_an_idle_projects_unannounced_outage_ends_silently() {
    // Out (not announced yet), idle, back before the delay: nothing sent either way.
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let (ledger, _) = pass(&down, &[], T0);
    let (ledger, sent) = pass(&idle(Some("unreachable")), &ledger, T0 + AFTER / 2);
    assert!(sent.is_empty());
    assert_eq!(ledger[0].since_ms, Some(T0));
    let (ledger, sent) = pass(&idle(None), &ledger, T0 + AFTER - 1);
    assert!(ledger.is_empty() && sent.is_empty());
    // Out and idle long enough: the delay counts from the start, idle or not.
    let (ledger, _) = pass(&down, &[], T0);
    let (ledger, sent) = pass(&idle(Some("unreachable")), &ledger, T0 + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    let (ledger, sent) = pass(&idle(None), &ledger, T0 + AFTER + MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Returned]);
    assert!(ledger.is_empty());
}

#[test]
fn hs2_1cc1tq_a_home_flapping_while_its_project_is_idle_is_told_once_each_way() {
    let ledger = announced();
    let mut sent = Vec::new();
    let mut ledger = ledger;
    // Out and back repeatedly with no work: each end is told once, and an idle
    // project starts no new outage of its own (nothing reads its home any more).
    for round in 0..4 {
        let at = T0 + 10 * MIN + round * 10 * MIN;
        let (next, notices) = pass(&idle(Some("unreachable")), &ledger, at);
        sent.extend(notices);
        let (next, notices) = pass(&idle(None), &next, at + MIN);
        sent.extend(notices);
        ledger = next;
    }
    assert_eq!(sent, vec![HomeNoticeKind::Returned], "{sent:?}");
    assert!(ledger.is_empty());
    // Work comes back with the home out: a new outage, announced after its own delay,
    // then the project goes idle and the home comes back: "back" once.
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let start = T0 + 60 * MIN;
    let (ledger, sent) = pass(&down, &ledger, start);
    assert!(sent.is_empty());
    let (ledger, sent) = pass(&idle(Some("unreachable")), &ledger, start + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    // Down again before "back" went out (seen back by a headless pass) continues it.
    let back = advance_outages(
        &ledger,
        &observe_following(&idle(None), &ledger, true),
        &idle(None).sites,
        start + AFTER + MIN,
    );
    assert!(!back[0].current() && back[0].announced);
    let (ledger, sent) = pass(&idle(Some("unreachable")), &back, start + AFTER + 2 * MIN);
    assert!(sent.is_empty(), "{sent:?}");
    assert!(ledger[0].current());
    let (ledger, sent) = pass(&idle(None), &ledger, start + AFTER + 3 * MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Returned]);
    assert!(ledger.is_empty());
}

#[test]
fn hs2_1cc1tq_a_disabled_home_of_an_idle_project_is_followed_until_enabled() {
    let off = || {
        let mut world = project();
        world.targets.retain(|state| state.target_id != target(1));
        world.policy.targets[0].enabled = false;
        world
    };
    let disabled = off();
    let (ledger, _) = pass(&disabled, &[], T0);
    let (ledger, sent) = pass(&disabled, &ledger, T0 + AFTER);
    assert_eq!(sent, vec![HomeNoticeKind::Unavailable]);
    // Idle and still disabled: still observed, so still current.
    let mut idle_disabled = off();
    idle_disabled.work.clear();
    let (ledger, sent) = pass(&idle_disabled, &ledger, T0 + 10 * MIN);
    assert!(sent.is_empty());
    assert_eq!(ledger[0].reason, Some(HomeOutageReason::Disabled));
    // A disabled home is not re-read on a timer: a policy change wakes the loop.
    assert_eq!(next_recheck_at(&ledger, T0 + 10 * MIN, MIN, 30 * MIN), None);
    // Enabled again and read fine while idle: back.
    let (ledger, sent) = pass(&idle(None), &ledger, T0 + 11 * MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Returned]);
    assert!(ledger.is_empty());
}

#[test]
fn hs2_1cc1tq_a_home_out_is_read_again_on_a_bounded_backoff() {
    let (base, cap) = (HOME_RECHECK_BASE_MS, HOME_RECHECK_CAP_MS);
    assert_eq!((base, cap), (MIN, 30 * MIN));
    assert_eq!(next_recheck_at(&[], T0, base, cap), None);
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let (ledger, _) = pass(&down, &[], T0);
    // Read again after the base, then twice as far apart each time, up to the cap.
    let mut at = T0;
    let mut waits = Vec::new();
    for _ in 0..8 {
        let next = next_recheck_at(&ledger, at, base, cap).unwrap();
        waits.push((next - at) / MIN);
        at = next;
    }
    assert_eq!(waits, vec![1, 1, 2, 4, 8, 16, 30, 30]);
    // A pass between two wakes (another event) does not bring the next one closer
    // than the base.
    assert_eq!(
        next_recheck_at(&ledger, T0 + 1, base, cap),
        Some(T0 + 1 + base)
    );
    // A clock behind the start waits the base.
    assert_eq!(next_recheck_at(&ledger, T0 - MIN, base, cap), Some(T0));
    // The earliest of several homes wins; an entry waiting only for "back" is not read
    // again on a timer (the pass that saw it back sends it).
    let older = HomeOutage {
        home_target_id: target(4),
        since_ms: Some(T0 - 60 * MIN),
        ..ledger[0].clone()
    };
    let returning = HomeOutage {
        home_target_id: target(5),
        since_ms: None,
        announced: true,
        returned_at_ms: Some(T0),
        ..ledger[0].clone()
    };
    let several = vec![older, ledger[0].clone(), returning.clone()];
    assert_eq!(next_recheck_at(&several, T0, base, cap), Some(T0 + base));
    assert_eq!(next_recheck_at(&[returning], T0, base, cap), None);
    // A shortened base (tests) never waits less than it, even with a smaller cap.
    assert_eq!(next_recheck_at(&ledger, T0, 500, 100), Some(T0 + 500));
}

#[test]
fn hs2_1cc1tq_an_announced_outage_of_a_regrouped_home_ends_with_one_notice() {
    let ledger = announced();
    let mut split = project();
    split.sites = ProjectSites::default();
    // The site is separated into its own project: one "regrouped" notice, then nothing.
    let (after, sent) = pass(&split, &ledger, T0 + 10 * MIN);
    assert_eq!(sent, vec![HomeNoticeKind::Regrouped]);
    assert!(after.is_empty());
    let (after, sent) = pass(&split, &after, T0 + 11 * MIN);
    assert!(after.is_empty() && sent.is_empty());
    // Seen by a pass that cannot notify: kept, not current, until one can.
    let kept = advance_outages(&ledger, &[], &split.sites, T0 + 10 * MIN);
    assert_eq!(kept.len(), 1);
    assert!(kept[0].regrouped && kept[0].announced && !kept[0].current());
    assert_eq!(kept[0].sites, vec![target(2)], "the last known sites stay");
    assert_eq!(next_recheck_at(&kept, T0 + 10 * MIN, MIN, 30 * MIN), None);
    let again = advance_outages(&kept, &[], &split.sites, T0 + 12 * MIN);
    assert_eq!(again, kept, "repeated passes change nothing");
    let mut later = again.clone();
    let notices = due_notices(&mut later, T0 + 13 * MIN, AFTER);
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].kind, HomeNoticeKind::Regrouped);
    assert_eq!(notices[0].at_ms, T0 + 10 * MIN);
    assert!(later.is_empty());
    // Grouped again before the notice went out, and still out: the told outage
    // goes on and nothing is sent.
    let mut down = project();
    home_unread(&mut down, "unreachable");
    let (regrouped_back, sent) = pass(&down, &kept, T0 + 14 * MIN);
    assert!(sent.is_empty(), "{sent:?}");
    assert!(regrouped_back[0].current() && !regrouped_back[0].regrouped);
    // A regrouped home whose outage was never announced is dropped silently.
    let mut fresh = project();
    home_unread(&mut fresh, "unreachable");
    let (unannounced, _) = pass(&fresh, &[], T0);
    let (after, sent) = pass(&split, &unannounced, T0 + MIN);
    assert!(after.is_empty() && sent.is_empty());
}

#[test]
fn hs2_1cc1tq_a_regrouped_entry_is_stored_and_read_back() {
    let dir = tempfile::tempdir().unwrap();
    let store = HomeOutageStore::new(dir.path());
    let mut down = project();
    home_unread(&mut down, "unreachable");
    store
        .advance("owner", &observe(&down), &down.sites, T0, None)
        .unwrap();
    store
        .advance(
            "owner",
            &observe(&down),
            &down.sites,
            T0 + AFTER,
            Some(AFTER),
        )
        .unwrap();
    let split = ProjectSites::default();
    let (ledger, sent) = store
        .advance("owner", &[], &split, T0 + 5 * MIN, None)
        .unwrap();
    assert!(sent.is_empty());
    assert!(ledger[0].regrouped);
    assert_eq!(store.outages("owner").unwrap(), ledger);
    let (ledger, sent) = store
        .advance("owner", &[], &split, T0 + 6 * MIN, Some(AFTER))
        .unwrap();
    assert!(ledger.is_empty());
    assert_eq!(sent[0].kind, HomeNoticeKind::Regrouped);
    assert!(store.outages("owner").unwrap().is_empty());
}
