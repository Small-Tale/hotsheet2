//! HS2-2HZN2C: which ignored directories a checkout's repository watcher leaves out at the
//! source.
//!
//! The repository watcher watches the whole working tree. Build output that Git ignores
//! (`target/`, `node_modules/`, agent worktrees under `.claude/worktrees/`) is written far
//! more often than anything else, and its events never change the checkout's Git status. On
//! macOS a watch stream can name up to eight directories the file-event service drops itself
//! (`hotsheet_jobs::watch::MAX_EXCLUDED_PATHS`). This module picks them from the ignored
//! set the event filter already reads, and decides when a changed pick is worth replacing
//! the stream.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Directory names that usually hold large, constantly rewritten output. A candidate with
/// one of these names outranks an unknown one until churn has been observed.
const HEAVY_OUTPUT_NAMES: &[&str] = &[
    "target",
    "node_modules",
    "worktrees",
    ".worktrees",
    "dist",
    "build",
    "out",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".turbo",
    ".parcel-cache",
    ".cache",
    "coverage",
    "test-results",
    "playwright-report",
    ".venv",
    "venv",
    "__pycache__",
    ".gradle",
    "DerivedData",
    "bazel-out",
    "tmp",
];

/// How long after one stream replacement a pick that only swaps exclusions (rather than
/// adding to them or dropping one Git no longer ignores) waits before another, so churn
/// that alternates between more than eight ignored trees cannot restart the stream often.
pub(crate) const EXCLUSION_SWAP_INTERVAL: Duration = Duration::from_secs(30);

/// Pick at most `limit` directories to exclude from `candidates` (ignored entries in the
/// spelling the platform reports). A candidate is skipped when it is not a directory, when
/// it is or contains a `protected` path (a watched Git directory, the group registry), or
/// when it lies inside one already picked. Order: most events observed (`churn`), then a
/// known heavy output name, then the shallowest path, then the path itself.
pub(crate) fn select_exclusions(
    candidates: &[PathBuf],
    protected: &[PathBuf],
    churn: &HashMap<PathBuf, u64>,
    limit: usize,
    is_dir: impl Fn(&Path) -> bool,
) -> Vec<PathBuf> {
    let heavy = |path: &Path| {
        path.file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| HEAVY_OUTPUT_NAMES.contains(&name))
    };
    let mut ranked: Vec<&PathBuf> = candidates
        .iter()
        .filter(|candidate| !protected.iter().any(|kept| kept.starts_with(candidate)))
        .collect();
    ranked.sort_by(|a, b| {
        let key = |path: &PathBuf| {
            (
                std::cmp::Reverse(churn.get(path).copied().unwrap_or(0)),
                !heavy(path),
                path.components().count(),
            )
        };
        key(a).cmp(&key(b)).then_with(|| a.cmp(b))
    });
    ranked.dedup();
    let mut picked: Vec<PathBuf> = Vec::new();
    for candidate in ranked {
        if picked.len() == limit {
            break;
        }
        if picked.iter().any(|chosen| candidate.starts_with(chosen)) || !is_dir(candidate) {
            continue;
        }
        // A broader directory picked after one inside it replaces it.
        picked.retain(|chosen| !chosen.starts_with(candidate));
        picked.push(candidate.clone());
    }
    picked
}

/// Whether the stream should be replaced to exclude `desired` instead of `active`.
/// - An active exclusion Git no longer ignores must go at once: its tracked changes are
///   otherwise never seen.
/// - A pick that only adds exclusions (a build tree created after the start) is taken at once.
/// - Any other difference waits [`EXCLUSION_SWAP_INTERVAL`] after the last replacement.
pub(crate) fn replacement_due(
    active: &[PathBuf],
    desired: &[PathBuf],
    still_ignored: impl Fn(&Path) -> bool,
    last_replacement: Option<Instant>,
    now: Instant,
) -> bool {
    let same = active.len() == desired.len() && active.iter().all(|path| desired.contains(path));
    if same {
        return false;
    }
    if active.iter().any(|path| !still_ignored(path)) {
        return true;
    }
    if active.iter().all(|path| desired.contains(path)) {
        return true;
    }
    last_replacement.is_none_or(|at| now.duration_since(at) >= EXCLUSION_SWAP_INTERVAL)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn paths(items: &[&str]) -> Vec<PathBuf> {
        items.iter().map(PathBuf::from).collect()
    }

    #[test]
    fn hs2_2hzn2c_heavy_shallow_directories_win_until_churn_is_seen() {
        let candidates = paths(&[
            "/r/notes",
            "/r/a/b/c/deep-cache",
            "/r/clients/web/node_modules",
            "/r/target",
            "/r/.claude/worktrees",
            "/r/.env",
        ]);
        let none = HashMap::new();
        let is_dir = |path: &Path| path != Path::new("/r/.env");
        let picked = select_exclusions(&candidates, &[], &none, 8, is_dir);
        assert_eq!(
            picked,
            paths(&[
                "/r/target",
                "/r/.claude/worktrees",
                "/r/clients/web/node_modules",
                "/r/notes",
                "/r/a/b/c/deep-cache",
            ]),
            "files are skipped; heavy names first, then shallow"
        );
        // Observed churn outranks the name heuristic.
        let churn = HashMap::from([(PathBuf::from("/r/a/b/c/deep-cache"), 5_000)]);
        let picked = select_exclusions(&candidates, &[], &churn, 2, is_dir);
        assert_eq!(picked, paths(&["/r/a/b/c/deep-cache", "/r/target"]));
    }

    #[test]
    fn hs2_2hzn2c_more_than_eight_candidates_keep_the_eight_best() {
        let mut candidates: Vec<PathBuf> = (0..12)
            .map(|i| PathBuf::from(format!("/r/gen{i:02}")))
            .collect();
        candidates.push(PathBuf::from("/r/deep/x/target"));
        let churn: HashMap<PathBuf, u64> = (0..12)
            .map(|i| (PathBuf::from(format!("/r/gen{i:02}")), i))
            .collect();
        let picked = select_exclusions(&candidates, &[], &churn, 8, |_| true);
        assert_eq!(picked.len(), 8);
        let expected: Vec<PathBuf> = (4..12)
            .rev()
            .map(|i| PathBuf::from(format!("/r/gen{i:02}")))
            .collect();
        assert_eq!(picked, expected, "the eight with the most churn");
        // Empty and then refilled.
        assert!(select_exclusions(&[], &[], &churn, 8, |_| true).is_empty());
        assert_eq!(
            select_exclusions(&candidates, &[], &churn, 8, |_| true),
            expected
        );
    }

    #[test]
    fn hs2_2hzn2c_protected_and_nested_paths_are_never_excluded_twice_or_wrongly() {
        let candidates = paths(&["/r/.hotsheet2", "/r/out", "/r/out/sub", "/r/vendor/target"]);
        let protected = paths(&["/r/.hotsheet2/groups", "/r/vendor/target/.git"]);
        let picked = select_exclusions(&candidates, &protected, &HashMap::new(), 8, |_| true);
        assert_eq!(
            picked,
            paths(&["/r/out"]),
            "protected ancestors and nested picks are skipped"
        );
        // A deeper candidate ranked first by churn gives way to its ancestor.
        let churn = HashMap::from([(PathBuf::from("/r/out/sub"), 10)]);
        let picked = select_exclusions(&candidates, &protected, &churn, 8, |_| true);
        assert_eq!(picked, paths(&["/r/out"]));
    }

    #[test]
    fn hs2_2hzn2c_replacement_transitions() {
        let now = Instant::now();
        let ignored = |path: &Path| path != Path::new("/r/now-tracked");
        let recently = Some(now);
        let long_ago = now.checked_sub(EXCLUSION_SWAP_INTERVAL);
        // Same set (any order): nothing to do.
        assert!(!replacement_due(
            &paths(&["/r/a", "/r/b"]),
            &paths(&["/r/b", "/r/a"]),
            ignored,
            recently,
            now
        ));
        // A new ignored tree (empty, then refilled; or one more) is taken at once.
        assert!(replacement_due(
            &[],
            &paths(&["/r/target"]),
            ignored,
            recently,
            now
        ));
        assert!(replacement_due(
            &paths(&["/r/target"]),
            &paths(&["/r/target", "/r/dist"]),
            ignored,
            recently,
            now
        ));
        // An exclusion Git no longer ignores is dropped at once, even right after a swap.
        assert!(replacement_due(
            &paths(&["/r/now-tracked"]),
            &[],
            ignored,
            recently,
            now
        ));
        // An ignored tree that disappeared is dropped only after the swap interval.
        assert!(!replacement_due(
            &paths(&["/r/gone"]),
            &[],
            ignored,
            recently,
            now
        ));
        assert!(replacement_due(
            &paths(&["/r/gone"]),
            &[],
            ignored,
            long_ago,
            now
        ));
        // A swap between churning trees waits for the interval, then goes ahead.
        let (active, desired) = (paths(&["/r/a"]), paths(&["/r/b"]));
        assert!(!replacement_due(&active, &desired, ignored, recently, now));
        assert!(replacement_due(&active, &desired, ignored, long_ago, now));
        assert!(replacement_due(&active, &desired, ignored, None, now));
    }
}

/// The native repository monitor over a real checkout (HS2-2HZN2C). FSEvents does not
/// deliver inside the command sandbox; run these unsandboxed on macOS.
#[cfg(test)]
mod monitor_tests {
    use crate::events::{ChangeEvent, EventLog};
    use crate::watcher::{RepositoryMonitorProbe, run_native_repository_monitor_with};
    use std::path::{Path, PathBuf};
    use std::sync::{Arc, Mutex};
    use std::time::{Duration, Instant};
    use tokio::sync::broadcast;

    fn git(root: &Path, args: &[&str]) {
        let status = hotsheet_ticketing::git::command()
            .arg("-C")
            .arg(root)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success());
    }

    /// How long a native stream may take to start or be replaced on a loaded Mac.
    const PLATFORM: Duration = Duration::from_secs(120);

    fn wait_for(what: &str, within: Duration, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + within;
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    struct Monitor {
        probe: Arc<RepositoryMonitorProbe>,
        changes: broadcast::Receiver<ChangeEvent>,
        _stop: std::sync::mpsc::Sender<()>,
    }

    fn monitor(root: &Path, exclude: bool) -> Monitor {
        let (events, changes) = broadcast::channel(1024);
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let probe = Arc::new(RepositoryMonitorProbe::default());
        let (root, thread_probe) = (root.to_path_buf(), probe.clone());
        std::thread::spawn(move || {
            run_native_repository_monitor_with(
                root,
                "checkout-2hzn2c".into(),
                events,
                Arc::new(Mutex::new(EventLog::default())),
                stop_rx,
                exclude,
                &thread_probe,
            );
        });
        Monitor {
            probe,
            changes,
            _stop: stop_tx,
        }
    }

    impl Monitor {
        /// Wait for the next `repository_changed` announcement.
        fn announced(&mut self, what: &str) {
            wait_for(
                what,
                Duration::from_secs(30),
                || matches!(self.changes.try_recv(), Ok(event) if event.kind == "repository_changed"),
            );
        }
    }

    fn repo(ignore: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = hotsheet_sync::paths::canonicalize(dir.path()).unwrap();
        git(&root, &["init", "--quiet"]);
        std::fs::write(root.join(".gitignore"), ignore).unwrap();
        (dir, root)
    }

    fn churn(dir: &Path, files: usize) {
        std::fs::create_dir_all(dir).unwrap();
        for index in 0..files {
            std::fs::write(dir.join(format!("out-{index}.o")), index.to_string()).unwrap();
        }
    }

    /// Walks the transitions: ignored at the start (excluded), an ignored tree created after
    /// the start (added), churn in both (not delivered, on macOS), a tracked change (still
    /// announced), and an excluded tree Git stops ignoring (dropped from the exclusions and
    /// its changes announced again).
    #[test]
    fn hs2_2hzn2c_the_monitor_excludes_ignored_trees_and_follows_the_ignored_set() {
        let (_dir, root) = repo("target/\n.claude/worktrees/\n");
        churn(&root.join("target/debug"), 3);
        let mut monitor = monitor(&root, true);
        wait_for("the first stream", PLATFORM, || {
            monitor.probe.streams() >= 1
        });
        assert_eq!(monitor.probe.excluded(), vec![root.join("target")]);

        // An ignored tree created after the start: its first events are relevant until the
        // ignored set is read again, then the stream is replaced to exclude it too.
        churn(&root.join(".claude/worktrees/agent/target"), 20);
        wait_for("the worktrees exclusion", PLATFORM, || {
            monitor
                .probe
                .excluded()
                .contains(&root.join(".claude/worktrees"))
        });
        assert_eq!(monitor.probe.streams(), 2);
        assert!(monitor.probe.excluded().contains(&root.join("target")));

        // Churn in both excluded trees, then a tracked change: announced, and on macOS the
        // churn never reached the monitor.
        while monitor.changes.try_recv().is_ok() {}
        let before = monitor.probe.delivered();
        churn(&root.join("target/debug/deps"), 400);
        churn(&root.join(".claude/worktrees/agent/target/debug"), 400);
        std::fs::write(root.join("tracked.txt"), "visible").unwrap();
        monitor.announced("a tracked change beside the churn");
        let delivered = monitor.probe.delivered() - before;
        if cfg!(target_os = "macos") {
            assert!(
                delivered < 40,
                "{delivered} deliveries for 800 excluded writes"
            );
        }

        // Git lists `.claude/` as ignored only because everything in it is; no pattern
        // ignores it, so it is never excluded and a new file in it is still announced.
        assert!(!monitor.probe.excluded().contains(&root.join(".claude")));
        std::fs::write(root.join(".claude/settings.json"), "{}").unwrap();
        monitor.announced("a file Git does not ignore beside the excluded worktrees");

        // Git stops ignoring `target/`: it leaves the exclusions, and its changes count again.
        std::fs::write(root.join(".gitignore"), ".claude/worktrees/\n").unwrap();
        monitor.announced("the .gitignore edit");
        wait_for("target to be watched again", PLATFORM, || {
            !monitor.probe.excluded().contains(&root.join("target"))
        });
        assert!(
            monitor
                .probe
                .excluded()
                .contains(&root.join(".claude/worktrees"))
        );
        let streams = monitor.probe.streams();
        while monitor.changes.try_recv().is_ok() {}
        git(&root, &["add", "target/debug/out-0.o"]);
        monitor.announced("staging a file in the formerly excluded tree");
        std::fs::write(root.join("target/debug/out-0.o"), "edited").unwrap();
        monitor.announced("an edit inside the formerly excluded tree");
        assert_eq!(monitor.probe.streams(), streams, "no further replacement");
    }

    /// HS2-2HZN2C before/after evidence: deliveries and the monitor's own CPU while a churn
    /// loop writes thousands of files under `target/` and `.claude/worktrees/` of a
    /// repo-shaped checkout. Run with `--run-ignored only --nocapture`, unsandboxed.
    #[cfg(unix)]
    #[test]
    #[ignore = "measurement for HS2-2HZN2C; prints numbers rather than asserting them"]
    fn hs2_2hzn2c_measure_churn_with_and_without_exclusions() {
        let files: usize = std::env::var("HS2_2HZN2C_FILES")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(4_000);
        // `HS2_2HZN2C_MODE=off` or `on` runs one variant, so an outside sampler (fseventsd CPU)
        // can be read around it.
        let modes: &[bool] = match std::env::var("HS2_2HZN2C_MODE").as_deref() {
            Ok("off") => &[false],
            Ok("on") => &[true],
            _ => &[false, true],
        };
        for &exclude in modes {
            let (_dir, root) = repo("target/\n.claude/worktrees/\nnode_modules/\n");
            std::fs::create_dir_all(root.join("src")).unwrap();
            churn(&root.join("target"), 1);
            churn(&root.join(".claude/worktrees/agent/target"), 1);
            churn(&root.join("node_modules"), 1);
            let mut monitor = monitor(&root, exclude);
            wait_for("the first stream", PLATFORM, || {
                monitor.probe.streams() >= 1
            });
            std::thread::sleep(Duration::from_secs(2));
            while monitor.changes.try_recv().is_ok() {}
            let cpu_before = cpu_seconds();
            let delivered_before = monitor.probe.delivered();
            let started = Instant::now();
            // The churn runs in a child process, so its own CPU is not counted.
            let script = format!(
                "for round in 1 2 3; do for d in target/debug/deps .claude/worktrees/agent/target/debug; do \
                 mkdir -p $d; i=0; while [ $i -lt {half} ]; do echo $round > $d/f$i.o; i=$((i+1)); done; done; done; \
                 echo done > src/marker.rs",
                half = files / 2
            );
            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(&script)
                .current_dir(&root)
                .status()
                .unwrap();
            assert!(status.success());
            monitor.announced("the marker");
            let elapsed = started.elapsed();
            let delivered = monitor.probe.delivered() - delivered_before;
            let cpu = cpu_seconds() - cpu_before;
            println!(
                "HS2-2HZN2C exclude={exclude} writes={} delivered={delivered} monitor_process_cpu={cpu:.3}s wall={:.2}s excluded={:?}",
                files * 3,
                elapsed.as_secs_f64(),
                monitor.probe.excluded()
            );
        }
    }

    /// This process's user and system CPU seconds (children excluded).
    #[cfg(unix)]
    fn cpu_seconds() -> f64 {
        // SAFETY: getrusage fills the zeroed struct it is given.
        let usage = unsafe {
            let mut usage: libc::rusage = std::mem::zeroed();
            libc::getrusage(libc::RUSAGE_SELF, &mut usage);
            usage
        };
        let seconds = |time: libc::timeval| time.tv_sec as f64 + time.tv_usec as f64 / 1e6;
        seconds(usage.ru_utime) + seconds(usage.ru_stime)
    }
}
