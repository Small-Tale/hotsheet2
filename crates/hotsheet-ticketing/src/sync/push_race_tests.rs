//! HS2-R2BC3N: a push that loses a race to another pusher is a normal conflict in a store
//! several machines share. The cycle fetches, integrates and pushes again at once, a
//! bounded number of times, and reports `PushRaced` (never `Offline`) only when every
//! attempt lost; a real outage during a retry is still `Offline`. Real Git against a
//! synthetic bare remote; synthetic file bodies only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::tests::{commit_file, ok, remote_with_two_clones};
use super::*;
use crate::sync_health::{SyncState, health};

/// Fast raced retries for tests: `retries` more attempts, a 1 ms pause unit.
/// Unix-only like the `RacingPeer` tests that use it (HS2-6EW369).
#[cfg(unix)]
fn quick(retries: u32) -> SyncLimits {
    SyncLimits {
        push_race_retries: retries,
        push_race_pause: Duration::from_millis(1),
        ..SyncLimits::default()
    }
}

/// A peer that publishes between the store's fetch and its push, at most `limit` times.
///
/// A `pre-push` hook in the store runs after the store fetched and after its push read
/// the remote's refs, but before the push updates them: it makes the peer commit
/// `race-<n>.md` and push it, so the remote refuses the store's update as a stale old
/// value, the way two machines pushing at once collide. After race `break_after`, it
/// also moves the remote away, so the store's next fetch fails like a real outage.
///
/// Unix-only (HS2-6EW369): the hook is a POSIX shell script with an executable bit;
/// the simultaneous-racers test below covers the retry on every platform.
#[cfg(unix)]
struct RacingPeer {
    counter: PathBuf,
}

#[cfg(unix)]
impl RacingPeer {
    fn install(store: &Path, peer: &Path, limit: u32, break_after: Option<u32>) -> Self {
        let counter = store.parent().unwrap().join("races");
        let remote = store.parent().unwrap().join("remote.git");
        let moved = store.parent().unwrap().join("remote.moved");
        let quote = |path: &Path| format!("'{}'", path.display());
        let outage = break_after.map_or_else(String::new, |at| {
            format!(
                "[ \"$n\" -eq {at} ] && mv {} {}\n",
                quote(&remote),
                quote(&moved)
            )
        });
        let script = format!(
            "#!/bin/sh\n\
             cat >/dev/null\n\
             n=$(cat {counter} 2>/dev/null || echo 0)\n\
             [ \"$n\" -lt {limit} ] || exit 0\n\
             n=$((n + 1))\n\
             echo \"$n\" > {counter}\n\
             unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE GIT_PREFIX GIT_COMMON_DIR\n\
             cd {peer} || exit 0\n\
             git pull -q --rebase >/dev/null 2>&1\n\
             echo \"race $n\" > \"race-$n.md\"\n\
             git add -A && git commit -q -m \"race $n\" && git push -q origin HEAD >/dev/null 2>&1\n\
             {outage}\
             exit 0\n",
            counter = quote(&counter),
            peer = quote(peer),
        );
        let hook = store.join(".git/hooks/pre-push");
        std::fs::write(&hook, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { counter }
    }

    /// How many times the peer has raced the store so far.
    fn races(&self) -> u32 {
        std::fs::read_to_string(&self.counter)
            .ok()
            .and_then(|count| count.trim().parse().ok())
            .unwrap_or(0)
    }

    fn remove(store: &Path) {
        std::fs::remove_file(store.join(".git/hooks/pre-push")).unwrap();
    }
}

/// The files the remote's branch holds.
fn remote_files(remote: &Path) -> Vec<String> {
    git_stdout(remote, &["ls-tree", "--name-only", "HEAD"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

fn remote_of(store: &Path) -> PathBuf {
    store.parent().unwrap().join("remote.git")
}

/// The jittered pause doubles its window per attempt, stays inside it for any random
/// value, and stops growing after the fourth attempt.
#[test]
fn hs2_r2bc3n_the_raced_retry_pause_is_jittered_inside_a_doubling_window() {
    let unit = Duration::from_millis(100);
    for (attempt, floor) in [(1, 100), (2, 200), (3, 400), (4, 800), (5, 800), (40, 800)] {
        let floor = Duration::from_millis(floor);
        assert_eq!(
            push_race_pause(unit, attempt, 0),
            floor,
            "attempt {attempt}"
        );
        for random in [1, 7, 99_999_999, u64::MAX / 3, u64::MAX] {
            let pause = push_race_pause(unit, attempt, random);
            assert!(pause >= floor && pause <= floor * 2, "{attempt}: {pause:?}");
        }
    }
    assert_eq!(push_race_pause(Duration::ZERO, 3, u64::MAX), Duration::ZERO);
    // Distinct random values spread racers apart.
    assert_ne!(
        push_race_pause(unit, 1, 1),
        push_race_pause(unit, 1, 50_000_000)
    );
}

/// A peer publishes between the fetch and the push twice; the cycle integrates each and
/// publishes on its third attempt, reporting a synced pass with no failure recorded.
#[cfg(unix)]
#[test]
fn hs2_r2bc3n_a_peer_push_between_fetch_and_push_is_retried_within_the_cycle() {
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&store, "local.md", "local\n");
    let racer = RacingPeer::install(&store, &peer, 2, None);

    let report = sync_once_with(&store, quick(PUSH_RACE_RETRIES));

    assert_eq!(
        report,
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert_eq!(racer.races(), 2);
    let files = remote_files(&remote_of(&store));
    for file in ["local.md", "race-1.md", "race-2.md"] {
        assert!(files.iter().any(|name| name == file), "{file}: {files:?}");
    }
    let health = health(&store);
    assert_eq!(health.state, SyncState::Synced);
    assert_eq!(health.consecutive_failures, 0);
    assert_eq!(health.unpublished_commits, 0);
    assert!(health.last_success_at.is_some());
}

/// Every attempt loses: the cycle stops after its bounded retries and reports a push
/// race, not an outage, with no failure counted and the commit kept. A second cycle that
/// loses again stays a race; once the peer stops, the next cycle publishes.
#[cfg(unix)]
#[test]
fn hs2_r2bc3n_repeated_lost_races_report_push_raced_then_recover() {
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&store, "local.md", "local\n");
    let racer = RacingPeer::install(&store, &peer, 1_000, None);

    assert_eq!(sync_once_with(&store, quick(2)), SyncReport::PushRaced);
    assert_eq!(racer.races(), 3, "the first attempt and two retries");
    let raced = health(&store);
    assert_eq!(raced.state, SyncState::PushRaced);
    assert_eq!(raced.reason.as_deref(), Some(PUSH_RACE_REASON));
    assert_eq!(raced.consecutive_failures, 0);
    assert!(raced.last_success_at.is_none());
    assert!(raced.unpublished_commits > 0, "the local commit is kept");
    assert_eq!(raced.next_action, None, "nothing for a person to do");

    // Repeated: the same store loses every attempt of another cycle.
    assert_eq!(sync_once_with(&store, quick(1)), SyncReport::PushRaced);
    assert_eq!(racer.races(), 5);
    assert_eq!(health(&store).consecutive_failures, 0);

    RacingPeer::remove(&store);
    assert_eq!(
        sync_once_with(&store, quick(1)),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    let files = remote_files(&remote_of(&store));
    assert!(files.iter().any(|name| name == "local.md"), "{files:?}");
    assert!(files.iter().any(|name| name == "race-5.md"), "{files:?}");
    let recovered = health(&store);
    assert_eq!(recovered.state, SyncState::Synced);
    assert_eq!(recovered.unpublished_commits, 0);
}

/// A race followed by a real outage within one cycle: the retry's fetch fails, so the
/// cycle is `Offline` (and backs off) with the transport reason, not a race. Once the
/// remote is back, the next cycle integrates the race and publishes.
#[cfg(unix)]
#[test]
fn hs2_r2bc3n_a_race_then_a_real_outage_is_offline_then_recovers() {
    let (dir, store, peer) = remote_with_two_clones();
    commit_file(&store, "local.md", "local\n");
    let racer = RacingPeer::install(&store, &peer, 1, Some(1));

    assert_eq!(sync_once_with(&store, quick(3)), SyncReport::Offline);
    assert_eq!(racer.races(), 1);
    let offline = health(&store);
    assert_eq!(offline.state, SyncState::Offline);
    assert_eq!(offline.consecutive_failures, 1);
    let reason = offline.reason.unwrap_or_default();
    assert!(reason.contains("could not be reached"), "{reason}");
    assert_ne!(reason, PUSH_RACE_REASON);

    std::fs::rename(dir.path().join("remote.moved"), remote_of(&store)).unwrap();
    assert_eq!(
        sync_once_with(&store, quick(3)),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    let files = remote_files(&remote_of(&store));
    for file in ["local.md", "race-1.md"] {
        assert!(files.iter().any(|name| name == file), "{file}: {files:?}");
    }
    assert_eq!(health(&store).consecutive_failures, 0);
}

/// Several machines push to one remote at the same moment, round after round. Every
/// loser retries within its cycle, so each round ends with every racer synced and the
/// remote holding every racer's commit; nothing reports `Offline`.
#[test]
fn hs2_r2bc3n_several_simultaneous_racers_all_publish_within_one_cycle() {
    const RACERS: usize = 4;
    const ROUNDS: usize = 3;
    let (dir, first, _peer) = remote_with_two_clones();
    let remote = remote_of(&first);
    let mut clones = vec![first];
    for index in 1..RACERS {
        let clone = dir.path().join(format!("racer-{index}"));
        ok(
            dir.path(),
            &[
                "clone",
                "--quiet",
                remote.to_str().unwrap(),
                clone.to_str().unwrap(),
            ],
        );
        super::tests::identity(&clone);
        clones.push(clone);
    }
    // Generous retries: a push can also lose to a concurrent push that itself loses, so
    // `RACERS - 1` attempts are not a strict bound under real concurrency.
    let limits = SyncLimits {
        push_race_retries: 12,
        push_race_pause: Duration::from_millis(10),
        ..SyncLimits::default()
    };
    for round in 0..ROUNDS {
        for (index, clone) in clones.iter().enumerate() {
            commit_file(clone, &format!("racer-{index}-round-{round}.md"), "work\n");
        }
        let barrier = std::sync::Barrier::new(RACERS);
        let reports: Vec<SyncReport> = std::thread::scope(|scope| {
            let handles: Vec<_> = clones
                .iter()
                .map(|clone| {
                    let barrier = &barrier;
                    scope.spawn(move || {
                        barrier.wait();
                        sync_once_with(clone, limits)
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|handle| handle.join().unwrap())
                .collect()
        });
        for (index, report) in reports.iter().enumerate() {
            assert!(
                matches!(report, SyncReport::Synced { pushed: true, .. }),
                "round {round} racer {index}: {report:?} ({:?})",
                health(&clones[index]).reason
            );
        }
        let files = remote_files(&remote);
        for index in 0..RACERS {
            let file = format!("racer-{index}-round-{round}.md");
            assert!(files.contains(&file), "{file}: {files:?}");
        }
    }
}
