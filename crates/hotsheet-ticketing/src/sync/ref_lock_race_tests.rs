//! HS2-8BSDBA: a fetch or push whose output names a local lock file that another Git
//! process in the clone releases moments later (a headless write's background `git push`
//! updating the remote-tracking ref, HS2-D9ZGAE) lost a race; the cycle retries it within
//! the HS2-R2BC3N loop instead of reporting `Blocked`. A lock that stays is still
//! `Blocked` (HS2-6ACT5Y), and a real outage after the race is still `Offline`. Real Git
//! against a synthetic bare remote; synthetic file bodies only.

use std::path::{Path, PathBuf};
use std::time::Duration;

use super::tests::remote_with_two_clones;
use super::*;
// Only the Unix hook-driven tests commit files, run Git and read sync health
// (HS2-8BSDBA, HS2-QNZ60H): a Windows test build denies unused imports.
#[cfg(unix)]
use super::tests::{commit_file, ok};
#[cfg(unix)]
use crate::sync_health::{SyncState, health};

/// Fast raced retries and a settle window far above the hook's release delay, so the
/// transient-lock tests do not depend on scheduling under load.
#[cfg(unix)]
fn patient(retries: u32) -> SyncLimits {
    SyncLimits {
        push_race_retries: retries,
        push_race_pause: Duration::from_millis(1),
        lock_race_settle: Duration::from_secs(10),
        ..SyncLimits::default()
    }
}

/// How long the simulated Git process holds the lock: longer than Git's own lock retry
/// (`core.filesRefLockTimeout`, 100 ms by default), so the fetch really fails on it.
#[cfg(unix)]
const HOLD_MS: u32 = 300;

/// A short settle window for locks that are expected to stay.
fn brief() -> SyncLimits {
    SyncLimits {
        push_race_retries: 2,
        push_race_pause: Duration::from_millis(1),
        lock_race_settle: Duration::from_millis(150),
        ..SyncLimits::default()
    }
}

/// Another Git process in the store's clone that holds the remote-tracking ref's lock
/// at the moment the store's fetch updates that ref, the way the store's own detached
/// background push holds `refs/remotes/<remote>/<branch>.lock` when it publishes.
///
/// A `reference-transaction` hook in its `preparing` state runs before Git takes the
/// ref's lock, so creating the lock there makes the fetch fail with Git's own
/// `Unable to create '<...>.lock': File exists` once Git's own short lock retry ends. A
/// detached process then removes it after `release_ms` (`None`: never, a lock that stays
/// held), at most `limit` times.
/// After race `break_after` it also moves the remote away, like a real outage.
///
/// Unix-only like the other hook tests (HS2-6EW369): a POSIX shell script with an
/// executable bit.
#[cfg(unix)]
struct LockHoldingGit {
    counter: PathBuf,
}

#[cfg(unix)]
impl LockHoldingGit {
    fn install(
        store: &Path,
        limit: u32,
        release_ms: Option<u32>,
        break_after: Option<u32>,
    ) -> Self {
        let counter = store.parent().unwrap().join("lock-races");
        let git_dir = store.join(".git");
        let remote = store.parent().unwrap().join("remote.git");
        let moved = store.parent().unwrap().join("remote.moved");
        let release = release_ms.map_or_else(String::new, |ms| {
            format!(
                "(sleep {}; rm -f \"$lock\") </dev/null >/dev/null 2>&1 &\n",
                f64::from(ms) / 1000.0
            )
        });
        let outage = break_after.map_or_else(String::new, |at| {
            format!(
                "[ \"$n\" -eq {at} ] && mv '{}' '{}'\n",
                remote.display(),
                moved.display()
            )
        });
        let script = format!(
            "#!/bin/sh\n\
             [ \"$1\" = preparing ] || {{ cat >/dev/null; exit 0; }}\n\
             line=$(grep ' refs/remotes/origin/' | grep -v ' refs/remotes/origin/HEAD$') || exit 0\n\
             n=$(cat '{counter}' 2>/dev/null || echo 0)\n\
             [ \"$n\" -lt {limit} ] || exit 0\n\
             n=$((n + 1))\n\
             echo $n > '{counter}'\n\
             ref=${{line##* }}\n\
             lock='{git_dir}'/\"$ref\".lock\n\
             : > \"$lock\"\n\
             {release}\
             {outage}\
             exit 0\n",
            counter = counter.display(),
            git_dir = git_dir.display(),
        );
        let hook = store.join(".git/hooks/reference-transaction");
        std::fs::write(&hook, script).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self { counter }
    }

    fn races(&self) -> u32 {
        std::fs::read_to_string(&self.counter)
            .ok()
            .and_then(|count| count.trim().parse().ok())
            .unwrap_or(0)
    }

    fn remove(store: &Path) {
        std::fs::remove_file(store.join(".git/hooks/reference-transaction")).unwrap();
    }
}

#[cfg(unix)]
fn remote_files(remote: &Path) -> Vec<String> {
    git_stdout(remote, &["ls-tree", "--name-only", "HEAD"])
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

#[cfg(unix)]
fn remote_of(store: &Path) -> PathBuf {
    store.parent().unwrap().join("remote.git")
}

/// The store's branch, whatever the test Git's default branch name is.
fn tracking_lock(store: &Path) -> PathBuf {
    let branch = git_stdout(store, &["symbolic-ref", "--quiet", "--short", "HEAD"]).unwrap();
    store.join(format!(".git/refs/remotes/origin/{}.lock", branch.trim()))
}

#[cfg(unix)]
fn assert_published(store: &Path, files: &[&str]) {
    let remote = remote_files(&remote_of(store));
    for file in files {
        assert!(remote.iter().any(|name| name == file), "{file}: {remote:?}");
    }
}

/// The reproduction: a peer published, and the store's fetch found the tracking ref
/// locked by a local Git process that released it a moment later. The cycle is not
/// `Blocked`: it retries the fetch at once and publishes, with no failure counted.
/// Repeated over several rounds on one store, each round racing once.
#[cfg(unix)]
#[test]
fn hs2_8bsdba_a_fetch_that_meets_a_briefly_held_tracking_lock_is_retried_not_blocked() {
    let (_dir, store, peer) = remote_with_two_clones();
    for round in 1..=4_u32 {
        ok(&peer, &["pull", "--quiet", "--rebase"]);
        commit_file(&peer, &format!("peer-{round}.md"), "peer\n");
        ok(&peer, &["push", "--quiet"]);
        commit_file(&store, &format!("local-{round}.md"), "local\n");
        let _ = std::fs::remove_file(store.parent().unwrap().join("lock-races"));
        let holder = LockHoldingGit::install(&store, 1, Some(HOLD_MS), None);

        let report = sync_once_with(&store, patient(PUSH_RACE_RETRIES));

        assert_eq!(
            holder.races(),
            1,
            "round {round}: the fetch met the lock once"
        );
        assert_eq!(
            report,
            SyncReport::Synced {
                pulled: true,
                pushed: true
            },
            "round {round}: {:?}",
            health(&store).reason
        );
        assert!(!tracking_lock(&store).exists(), "round {round}");
        let synced = health(&store);
        assert_eq!(synced.state, SyncState::Synced);
        assert_eq!(synced.consecutive_failures, 0);
        assert_eq!(synced.unpublished_commits, 0);
        LockHoldingGit::remove(&store);
        assert_published(
            &store,
            &[&format!("peer-{round}.md"), &format!("local-{round}.md")],
        );
    }
}

/// Every fetch of the cycle meets a briefly held lock: the cycle stops after its bounded
/// retries and reports a race with the lock reason, never `Blocked`, and counts no
/// failure. A second such cycle stays a race; once the holder stops, the next publishes.
#[cfg(unix)]
#[test]
fn hs2_8bsdba_repeated_transient_locks_report_raced_then_recover() {
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&peer, "peer.md", "peer\n");
    ok(&peer, &["push", "--quiet"]);
    commit_file(&store, "local.md", "local\n");
    let holder = LockHoldingGit::install(&store, 1_000, Some(HOLD_MS), None);

    assert_eq!(sync_once_with(&store, patient(2)), SyncReport::PushRaced);
    assert_eq!(holder.races(), 3, "the first fetch and two retries");
    let raced = health(&store);
    assert_eq!(raced.state, SyncState::PushRaced);
    assert_eq!(raced.reason.as_deref(), Some(LOCK_RACE_REASON));
    assert_eq!(raced.consecutive_failures, 0);
    assert_eq!(raced.next_action, None, "nothing for a person to do");

    assert_eq!(sync_once_with(&store, patient(1)), SyncReport::PushRaced);
    assert_eq!(holder.races(), 5);
    assert_eq!(health(&store).consecutive_failures, 0);

    LockHoldingGit::remove(&store);
    assert_eq!(
        sync_once_with(&store, patient(1)),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert_published(&store, &["local.md", "peer.md"]);
    assert_eq!(health(&store).consecutive_failures, 0);
}

/// A transient lock followed by a real outage within one cycle: the retry's fetch cannot
/// reach the remote, so the cycle is `Offline` with the transport reason, not a race and
/// not `Blocked`. Once the remote is back, the next cycle publishes.
#[cfg(unix)]
#[test]
fn hs2_8bsdba_a_transient_lock_then_a_real_outage_is_offline_then_recovers() {
    let (dir, store, peer) = remote_with_two_clones();
    commit_file(&peer, "peer.md", "peer\n");
    ok(&peer, &["push", "--quiet"]);
    commit_file(&store, "local.md", "local\n");
    let holder = LockHoldingGit::install(&store, 1, Some(HOLD_MS), Some(1));

    assert_eq!(sync_once_with(&store, patient(3)), SyncReport::Offline);
    assert_eq!(holder.races(), 1);
    let offline = health(&store);
    assert_eq!(offline.state, SyncState::Offline);
    assert_eq!(offline.consecutive_failures, 1);
    let reason = offline.reason.unwrap_or_default();
    assert!(reason.contains("could not be reached"), "{reason}");
    assert_ne!(reason, LOCK_RACE_REASON);

    LockHoldingGit::remove(&store);
    std::fs::rename(dir.path().join("remote.moved"), remote_of(&store)).unwrap();
    assert_eq!(
        sync_once_with(&store, patient(3)),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert_published(&store, &["local.md", "peer.md"]);
    assert_eq!(health(&store).consecutive_failures, 0);
}

/// A lock that another process takes as the fetch runs and never releases is not a race:
/// the cycle reports `Blocked` naming the fetch and the lock, repeated passes count as
/// consecutive failures, and removing the lock recovers. HS2-6ACT5Y's contract holds.
#[cfg(unix)]
#[test]
fn hs2_8bsdba_a_lock_that_stays_held_after_the_fetch_is_still_blocked() {
    let (_dir, store, peer) = remote_with_two_clones();
    commit_file(&peer, "peer.md", "peer\n");
    ok(&peer, &["push", "--quiet"]);
    commit_file(&store, "local.md", "local\n");
    let holder = LockHoldingGit::install(&store, 1, None, None);

    assert_eq!(sync_once_with(&store, brief()), SyncReport::Blocked);
    assert_eq!(holder.races(), 1, "no retry for a lock that stays");
    let blocked = health(&store);
    assert_eq!(blocked.state, SyncState::Blocked);
    assert_eq!(blocked.consecutive_failures, 1);
    let reason = blocked.reason.unwrap_or_default();
    assert!(reason.contains("during fetch"), "{reason}");
    assert!(reason.contains("refs/remotes/origin/"), "{reason}");
    assert!(tracking_lock(&store).exists());

    // Repeated: the next pass's pre-check finds the same young lock held and blocks again.
    assert_eq!(sync_once_with(&store, brief()), SyncReport::Blocked);
    assert_eq!(health(&store).consecutive_failures, 2);

    LockHoldingGit::remove(&store);
    std::fs::remove_file(tracking_lock(&store)).unwrap();
    assert_eq!(
        sync_once_with(&store, brief()),
        SyncReport::Synced {
            pulled: true,
            pushed: true
        }
    );
    assert_published(&store, &["local.md", "peer.md"]);
}

/// A failed push that names a local lock: present for the whole window it is `Blocked`;
/// gone, it is a race carrying the lock reason. A remote's lock is never `Blocked` nor a
/// local lock race.
#[test]
fn hs2_8bsdba_a_push_lock_is_a_race_only_once_it_is_gone() {
    let (_dir, store, _peer) = remote_with_two_clones();
    let lock = tracking_lock(&store);
    let output = format!(
        "error: update_ref failed for ref 'refs/remotes/origin/main': cannot lock ref \
         'refs/remotes/origin/main': Unable to create '{}': File exists.\n",
        lock.display()
    );
    std::fs::write(&lock, "").unwrap();
    let (report, detail) = classify_push_failure(&store, &output, brief());
    assert_eq!(report, SyncReport::Blocked);
    assert!(detail.reason.unwrap_or_default().contains("during push"));

    std::fs::remove_file(&lock).unwrap();
    let (report, detail) = classify_push_failure(&store, &output, brief());
    assert_eq!(report, SyncReport::PushRaced);
    assert_eq!(detail.reason.as_deref(), Some(LOCK_RACE_REASON));

    let remote = "remote: error: cannot lock ref 'refs/heads/main': Unable to create \
                  '/srv/r.git/refs/heads/main.lock': File exists.\n \
                  ! [remote rejected] main -> main (reference already exists)\n";
    let (report, detail) = classify_push_failure(&store, remote, brief());
    assert_ne!(report, SyncReport::Blocked);
    assert_ne!(detail.reason.as_deref(), Some(LOCK_RACE_REASON));
}

/// The settle window: absent locks pass at once, a lock released inside the window
/// passes, and one that stays fails once the window ends, not before.
#[test]
fn hs2_8bsdba_named_locks_are_rechecked_within_a_bounded_window() {
    let dir = tempfile::tempdir().unwrap();
    let first = dir.path().join("a.lock");
    let second = dir.path().join("b.lock");
    let both = [first.clone(), second.clone()];
    assert!(locks_released_within(&both, Duration::ZERO));

    std::fs::write(&first, "").unwrap();
    let started = std::time::Instant::now();
    assert!(!locks_released_within(&both, Duration::from_millis(120)));
    assert!(started.elapsed() >= Duration::from_millis(120));

    std::fs::write(&second, "").unwrap();
    let release = {
        let both = both.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(30));
            for lock in &both {
                std::fs::remove_file(lock).unwrap();
            }
        })
    };
    assert!(locks_released_within(&both, Duration::from_secs(10)));
    release.join().unwrap();
}
