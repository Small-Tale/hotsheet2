//! HS2-N1QDZW: a waiter starts the platform file watcher once, whatever the number of
//! directories it watches or steps it waits, and still wakes on every change.

mod common;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Barrier};
use std::time::{Duration, Instant};

use common::{CHECKOUT, Fixture, config, fixture_with, new_job};
use hotsheet_jobs::{ChangeWaiter, JobStore, KeptWaiter, ProducerFamily, StoreConfig};

/// File watchers can replay events from just before they started; wait those out.
fn settle(waiter: &ChangeWaiter) {
    while waiter.wait(Duration::from_millis(300)) {}
}

/// A store with room for every write these tests make.
fn roomy() -> Fixture {
    fixture_with(StoreConfig {
        max_jobs: 64,
        ..config()
    })
}

/// One store change: the checkout's revision grows.
fn change(store: &JobStore, checkout: &str) {
    let mut job = new_job(ProducerFamily::Watch);
    job.checkout_id = checkout.into();
    store.create(job).unwrap();
}

/// Change the store from another thread once the caller is about to wait.
fn change_soon(store: &JobStore, checkout: &'static str) -> std::thread::JoinHandle<()> {
    let go = Arc::new(Barrier::new(2));
    let writer = {
        let (store, go) = (store.clone(), Arc::clone(&go));
        std::thread::spawn(move || {
            go.wait();
            change(&store, checkout);
        })
    };
    go.wait();
    writer
}

fn wakes(waiter: &ChangeWaiter) -> bool {
    (0..20).any(|_| waiter.wait(Duration::from_millis(500)))
}

#[test]
fn hs2_n1qdzw_a_waiter_over_several_directories_starts_once_and_wakes_on_each() {
    let root = tempfile::tempdir().unwrap();
    let directories: Vec<PathBuf> = ["a", "b/deep", "c", "d"]
        .iter()
        .map(|name| root.path().join(name))
        .collect();
    let paths: Vec<&Path> = directories.iter().map(PathBuf::as_path).collect();
    let waiter = ChangeWaiter::watching(&paths).unwrap();
    assert_eq!(waiter.platform_starts(), 1, "four directories, one start");
    assert_eq!(
        ChangeWaiter::new(&directories[0])
            .unwrap()
            .platform_starts(),
        1
    );
    settle(&waiter);
    assert!(!waiter.wait(Duration::from_millis(300)), "nothing changed");

    // Repeated and interleaved changes: each one is a wake of its own.
    for (index, name) in [0, 1, 0, 3, 2, 2, 1].into_iter().zip("abcdefg".chars()) {
        std::fs::write(directories[index].join(name.to_string()), b"x").unwrap();
        assert!(wakes(&waiter), "a write under directory {index} must wake");
        settle(&waiter);
    }

    // A directory created after the wait started reports the files written into it.
    let later = directories[2].join("later/deeper");
    std::fs::create_dir_all(&later).unwrap();
    assert!(wakes(&waiter), "a new directory is a change");
    settle(&waiter);
    let mut woke = false;
    for attempt in 0..20 {
        std::fs::write(later.join("file"), attempt.to_string()).unwrap();
        woke = waiter.wait(Duration::from_millis(500));
        if woke {
            break;
        }
    }
    assert!(woke, "a write under a directory created later must wake");
}

#[test]
fn hs2_n1qdzw_a_dropped_waiter_can_be_started_again_on_the_same_directory() {
    let root = tempfile::tempdir().unwrap();
    for round in 0..4 {
        let waiter = ChangeWaiter::new(root.path()).unwrap();
        assert_eq!(waiter.platform_starts(), 1);
        settle(&waiter);
        std::fs::write(root.path().join(format!("round-{round}")), b"x").unwrap();
        assert!(wakes(&waiter), "round {round}");
    }
}

#[test]
fn hs2_n1qdzw_a_follower_keeps_one_waiter_across_its_steps() {
    let fx = roomy();
    change(&fx.store, CHECKOUT);
    let kept = KeptWaiter::default();
    assert_eq!(kept.waiters_started(), 0);

    // Nothing changes: the step returns the same revision after its timeout.
    let mut since = fx.store.revision(CHECKOUT).unwrap();
    let started = Instant::now();
    let unchanged = fx
        .store
        .wait_for_change_keeping(&kept, CHECKOUT, since, Duration::from_millis(150))
        .unwrap();
    assert_eq!(unchanged, since);
    assert!(started.elapsed() >= Duration::from_millis(150));

    // Step after step, a write from another thread ends the wait.
    for step in 0..4 {
        let writer = change_soon(&fx.store, CHECKOUT);
        let started = Instant::now();
        let changed = fx
            .store
            .wait_for_change_keeping(&kept, CHECKOUT, since, Duration::from_secs(60))
            .unwrap();
        writer.join().unwrap();
        assert!(changed > since, "step {step} saw no change");
        assert!(started.elapsed() < Duration::from_secs(30));
        since = fx.store.revision(CHECKOUT).unwrap();
    }

    // A change between two steps is not missed, and a caller already behind does not
    // wait at all.
    change(&fx.store, CHECKOUT);
    let started = Instant::now();
    let behind = fx
        .store
        .wait_for_change_keeping(&kept, CHECKOUT, since, Duration::from_secs(60))
        .unwrap();
    assert!(behind > since);
    assert!(started.elapsed() < Duration::from_secs(30));
    // The file events of that write are not a change of their own.
    let started = Instant::now();
    let same = fx
        .store
        .wait_for_change_keeping(&kept, CHECKOUT, behind, Duration::from_millis(150))
        .unwrap();
    assert_eq!(same, behind);
    assert!(started.elapsed() >= Duration::from_millis(150));

    assert_eq!(kept.waiters_started(), 1, "one waiter for every step");
}

const OTHER: &str = "other-0123456789ab";

#[test]
fn hs2_n1qdzw_a_kept_waiter_is_replaced_when_its_directory_is() {
    let fx = roomy();
    change(&fx.store, CHECKOUT);
    let kept = KeptWaiter::default();
    let step = |checkout: &'static str| {
        let since = fx.store.revision(checkout).unwrap();
        let writer = change_soon(&fx.store, checkout);
        let changed = fx
            .store
            .wait_for_change_keeping(&kept, checkout, since, Duration::from_secs(60))
            .unwrap();
        writer.join().unwrap();
        assert!(changed > since, "no change seen in {checkout}");
    };
    step(CHECKOUT);
    assert_eq!(kept.waiters_started(), 1);

    // Another checkout is another directory: the kept waiter does not watch it.
    step(OTHER);
    assert_eq!(kept.waiters_started(), 2);
    step(OTHER);
    // Where a directory has no stable identity nothing is kept, as before this change.
    let reused = usize::from(cfg!(not(unix)));
    assert_eq!(kept.waiters_started(), 2 + reused);

    // The directory is removed and created again: a watch of the old one may be dead,
    // so the next step starts a new waiter, and it wakes.
    let directory = fx.store.checkout_dir(OTHER).unwrap();
    std::fs::remove_dir_all(&directory).unwrap();
    change(&fx.store, OTHER);
    step(OTHER);
    assert_eq!(kept.waiters_started(), 3 + 2 * reused);
    step(OTHER);
    assert_eq!(kept.waiters_started(), 3 + 3 * reused);
}

#[test]
fn hs2_n1qdzw_two_threads_waiting_through_one_kept_waiter_both_wake() {
    let fx = roomy();
    change(&fx.store, CHECKOUT);
    let kept = Arc::new(KeptWaiter::default());
    let since = fx.store.revision(CHECKOUT).unwrap();
    let ready = Arc::new(Barrier::new(3));
    let waiters: Vec<_> = (0..2)
        .map(|_| {
            let (store, kept, ready) = (fx.store.clone(), Arc::clone(&kept), Arc::clone(&ready));
            std::thread::spawn(move || {
                ready.wait();
                store
                    .wait_for_change_keeping(&kept, CHECKOUT, since, Duration::from_secs(60))
                    .unwrap()
            })
        })
        .collect();
    ready.wait();
    change(&fx.store, CHECKOUT);
    let started = Instant::now();
    for waiter in waiters {
        assert!(waiter.join().unwrap() > since);
    }
    assert!(started.elapsed() < Duration::from_secs(30));
    // The thread that found the kept waiter busy used one of its own.
    assert!(kept.waiters_started() <= 1);
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            out.push(path);
        }
    }
}

/// Policy: the waiters of these crates start the platform watcher only through
/// `hotsheet_jobs::watch::PathWatcher`, which registers every path before one start.
/// A direct `watch` call per path restarts the macOS event stream each time.
/// HS2-654VBY: the AI-tool followers (Claude transcripts, Codex rollouts) and the server's
/// repository and orchestration-settings watchers joined; the server keeps its single-path
/// ticket-store watcher and its polling watchers, which have no event stream to restart.
#[test]
fn hs2_n1qdzw_waiters_start_the_platform_watcher_only_through_the_shared_one() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let sources = |packages: &[&str]| {
        let mut files = Vec::new();
        for package in packages {
            rust_sources(&crates.join(package).join("src"), &mut files);
        }
        files
    };
    let files = sources(&["hotsheet-jobs", "hotsheet-cli", "hotsheet-aitools"]);
    assert!(files.len() > 50, "scanned too few sources: {}", files.len());
    let sanctioned = crates.join("hotsheet-jobs/src/watch.rs");
    let needles = ["recommended_watcher", "RecommendedWatcher", "PollWatcher"];
    let offenders: Vec<String> = files
        .iter()
        .filter(|file| **file != sanctioned)
        .filter(|file| {
            let source = std::fs::read_to_string(file).unwrap();
            needles.iter().any(|needle| source.contains(needle))
        })
        .map(|file| file.strip_prefix(crates).unwrap().display().to_string())
        .collect();
    assert!(
        offenders.is_empty(),
        "start file watchers through hotsheet_jobs::watch::PathWatcher: {offenders:?}"
    );
    // These crates have no watcher library of their own to call.
    for package in ["hotsheet-cli", "hotsheet-aitools"] {
        let manifest = std::fs::read_to_string(crates.join(package).join("Cargo.toml")).unwrap();
        assert!(
            !manifest.lines().any(|line| line.starts_with("notify")),
            "{package} declares a watcher library"
        );
    }
    // The server starts one native watcher of its own: the ticket store's, over one path.
    let native: Vec<String> = sources(&["hotsheet-server"])
        .iter()
        .flat_map(|file| {
            let source = std::fs::read_to_string(file).unwrap();
            let name = file.strip_prefix(crates).unwrap().display().to_string();
            std::iter::repeat_n(name, source.matches("recommended_watcher(").count())
        })
        .collect();
    assert_eq!(native, ["hotsheet-server/src/watcher.rs"], "{native:?}");
    let repository =
        std::fs::read_to_string(crates.join("hotsheet-server/src/watcher.rs")).unwrap();
    let monitor = &repository[repository.find("fn run_native_repository_monitor").unwrap()..];
    let monitor = &monitor[..monitor.find("\n}\n").unwrap()];
    assert!(monitor.contains("PathWatcher::start("));
    assert!(!monitor.contains(".watch("));
    let settings = std::fs::read_to_string(
        crates.join("hotsheet-server/src/routes/orchestration_settings_watch.rs"),
    )
    .unwrap();
    assert!(settings.contains("PathWatcher::start(") && !settings.contains(".watch("));
    // The shared watcher batches on macOS and leaves the other platforms as they were.
    let shared = std::fs::read_to_string(sanctioned).unwrap();
    assert_eq!(shared.matches(".paths_mut()").count(), 2);
    assert_eq!(shared.matches("watcher.watch(").count(), 1);
}
