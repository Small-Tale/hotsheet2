//! Short-lived memo of AI-tool discovery (HS2-QV8B7R).
//!
//! Discovering the installed AI tools launches `--version`/`models` subprocesses for every
//! detected drivable tool, about 0.4 s even with a warm model catalog. Project activation
//! calls `GET /ai-tools` and `GET /ai-settings` back to back, and `/ai-settings` (plus
//! terminal launch validation) repeats the same discovery. [`AiToolDiscoveryCache`] keeps
//! the last result for [`AI_TOOL_DISCOVERY_TTL`] next to the model-catalog cache, inside
//! the one `model_catalogs` mutex, so concurrent callers queue on the lock and all but the
//! first are answered from the memo instead of rescanning.
//!
//! - `refresh=true` always rescans (refreshing the model catalogs too) and repopulates the
//!   memo.
//! - The memo also records the invalidation generation it was scanned under. Anything that
//!   changes tool installation or plugin state bumps the generation (lock-free, so it never
//!   waits behind a running scan), and a memo from an older generation is never served.
//! - Tools installed or removed outside the server are picked up once the memo expires or
//!   on the next `refresh=true` request.
//! - A caller is timed from its *arrival*, not from when it acquires the lock, and the memo
//!   is stamped when its scan *completes*. A caller that queued behind a running scan is
//!   answered by that scan even when the scan outlasts the TTL; stamping the scan's start
//!   instead made every queued caller rescan in turn, about 10 s each with two tools at the
//!   probe deadline, so each project activation postponed the next (HS2-XZ3TTH).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use hotsheet_aitools::ModelCatalogCache;
use hotsheet_plugins::AiToolDescriptor;

/// How long a discovery result is reused before the next caller rescans.
const AI_TOOL_DISCOVERY_TTL: Duration = Duration::from_secs(10);

/// The scan a cache miss runs: plugin search dirs, project root, the model catalogs, and
/// whether to refresh them. Production uses
/// [`hotsheet_aitools::discover_ai_tool_descriptors`]; tests inject a hermetic scan.
pub type AiToolScanner = Arc<
    dyn Fn(&[PathBuf], &Path, &mut ModelCatalogCache, bool) -> Vec<AiToolDescriptor> + Send + Sync,
>;

/// How discovery runs: the memo lifetime and the scan behind it. Injectable per
/// `AppState` so wall-clock-sensitive tests never depend on how long the real AI tools
/// on `PATH` take to answer (HS2-BK350W).
#[derive(Clone)]
pub struct AiToolDiscoveryConfig {
    pub(crate) ttl: Duration,
    pub(crate) scanner: AiToolScanner,
}

impl Default for AiToolDiscoveryConfig {
    fn default() -> Self {
        Self {
            ttl: AI_TOOL_DISCOVERY_TTL,
            scanner: Arc::new(hotsheet_aitools::discover_ai_tool_descriptors),
        }
    }
}

/// One completed discovery and the conditions it was produced under.
struct DiscoveryMemo {
    /// When the scan completed: the TTL runs from here, and a caller that arrived before
    /// this instant is answered by this scan.
    completed_at: Instant,
    generation: u64,
    tools: Vec<AiToolDescriptor>,
}

/// The model-catalog cache plus a memo of the last full discovery.
#[derive(Default)]
pub struct AiToolDiscoveryCache {
    catalogs: ModelCatalogCache,
    memo: Option<DiscoveryMemo>,
}

impl AiToolDiscoveryCache {
    /// Return the memoized discovery when a caller that `arrived` at that instant is no more
    /// than `ttl` behind its completion (a caller that arrived while it was scanning is
    /// always answered by it), it was scanned under the current invalidation `generation`,
    /// and `refresh` is not requested. Otherwise run `scan` against the model catalogs and
    /// remember its result as of `completed()`, read once the scan returns.
    pub(crate) fn discover(
        &mut self,
        arrived: Instant,
        generation: u64,
        ttl: Duration,
        refresh: bool,
        scan: impl FnOnce(&mut ModelCatalogCache, bool) -> Vec<AiToolDescriptor>,
        completed: impl FnOnce() -> Instant,
    ) -> Vec<AiToolDescriptor> {
        if !refresh
            && let Some(memo) = &self.memo
            && memo.generation == generation
            && arrived.saturating_duration_since(memo.completed_at) < ttl
        {
            return memo.tools.clone();
        }
        let tools = scan(&mut self.catalogs, refresh);
        self.memo = Some(DiscoveryMemo {
            completed_at: completed(),
            generation,
            tools: tools.clone(),
        });
        tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hotsheet_sync::LockExt;

    fn tool(id: &str) -> AiToolDescriptor {
        AiToolDescriptor {
            id: id.to_string(),
            display_name: id.to_string(),
            models: Vec::new(),
            default_model: None,
            default_effort: None,
            actions: Vec::new(),
            ..AiToolDescriptor::default()
        }
    }

    /// Drive one discovery, recording whether (and with which refresh flag) it scanned.
    fn discover(
        cache: &mut AiToolDiscoveryCache,
        now: Instant,
        generation: u64,
        refresh: bool,
        scans: &mut Vec<bool>,
        id: &str,
    ) -> Vec<String> {
        cache
            .discover(
                now,
                generation,
                AI_TOOL_DISCOVERY_TTL,
                refresh,
                |_, refresh| {
                    scans.push(refresh);
                    vec![tool(id)]
                },
                || now,
            )
            .into_iter()
            .map(|tool| tool.id)
            .collect()
    }

    #[test]
    fn a_fresh_memo_is_reused_until_it_expires() {
        let mut cache = AiToolDiscoveryCache::default();
        let start = Instant::now();
        let mut scans = Vec::new();
        assert_eq!(
            discover(&mut cache, start, 0, false, &mut scans, "a"),
            ["a"]
        );
        // Hit: the second scan result ("b") is never produced.
        let almost = start + AI_TOOL_DISCOVERY_TTL - Duration::from_millis(1);
        assert_eq!(
            discover(&mut cache, almost, 0, false, &mut scans, "b"),
            ["a"]
        );
        assert_eq!(scans, [false]);
        // Expiry: exactly at the TTL the memo is stale and the next caller rescans.
        let expired = start + AI_TOOL_DISCOVERY_TTL;
        assert_eq!(
            discover(&mut cache, expired, 0, false, &mut scans, "c"),
            ["c"]
        );
        assert_eq!(scans, [false, false]);
        // The rescan repopulated the memo from its own time.
        let later = expired + Duration::from_secs(1);
        assert_eq!(
            discover(&mut cache, later, 0, false, &mut scans, "d"),
            ["c"]
        );
        assert_eq!(scans, [false, false]);
    }

    #[test]
    fn refresh_bypasses_a_fresh_memo_and_repopulates_it() {
        let mut cache = AiToolDiscoveryCache::default();
        let now = Instant::now();
        let mut scans = Vec::new();
        assert_eq!(discover(&mut cache, now, 0, false, &mut scans, "a"), ["a"]);
        assert_eq!(discover(&mut cache, now, 0, true, &mut scans, "b"), ["b"]);
        assert_eq!(
            scans,
            [false, true],
            "refresh reaches the catalogs as refresh"
        );
        // A later ordinary call reuses what the refresh found.
        assert_eq!(discover(&mut cache, now, 0, false, &mut scans, "c"), ["b"]);
        // Repeated refreshes each rescan.
        assert_eq!(discover(&mut cache, now, 0, true, &mut scans, "d"), ["d"]);
        assert_eq!(scans, [false, true, true]);
    }

    #[test]
    fn a_new_generation_invalidates_the_memo() {
        let mut cache = AiToolDiscoveryCache::default();
        let now = Instant::now();
        let mut scans = Vec::new();
        assert_eq!(discover(&mut cache, now, 0, false, &mut scans, "a"), ["a"]);
        assert_eq!(discover(&mut cache, now, 1, false, &mut scans, "b"), ["b"]);
        assert_eq!(discover(&mut cache, now, 1, false, &mut scans, "c"), ["b"]);
        // Invalidated twice before the next call: still one rescan.
        assert_eq!(discover(&mut cache, now, 3, false, &mut scans, "d"), ["d"]);
        assert_eq!(discover(&mut cache, now, 3, false, &mut scans, "e"), ["d"]);
        assert_eq!(scans, [false, false, false]);
    }

    #[test]
    fn an_empty_discovery_is_memoized_and_refilled_on_expiry() {
        let mut cache = AiToolDiscoveryCache::default();
        let start = Instant::now();
        let mut runs = 0;
        let empty = cache.discover(
            start,
            0,
            AI_TOOL_DISCOVERY_TTL,
            false,
            |_, _| {
                runs += 1;
                Vec::new()
            },
            || start,
        );
        assert!(empty.is_empty());
        let still_empty = cache.discover(
            start,
            0,
            AI_TOOL_DISCOVERY_TTL,
            false,
            |_, _| {
                runs += 1;
                vec![tool("late")]
            },
            || start,
        );
        assert!(still_empty.is_empty());
        let refilled = cache.discover(
            start + AI_TOOL_DISCOVERY_TTL,
            0,
            AI_TOOL_DISCOVERY_TTL,
            false,
            |_, _| {
                runs += 1;
                vec![tool("late")]
            },
            || start + AI_TOOL_DISCOVERY_TTL,
        );
        assert_eq!(refilled.len(), 1);
        assert_eq!(runs, 2);
    }

    /// Callers that queue on the shared mutex while a scan runs are answered from its memo:
    /// eight concurrent discoveries run the (slow) scan once.
    #[test]
    fn concurrent_callers_on_the_shared_lock_coalesce_into_one_scan() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::{Arc, Barrier, Mutex};

        let cache = Arc::new(Mutex::new(AiToolDiscoveryCache::default()));
        let scans = Arc::new(AtomicUsize::new(0));
        let barrier = Arc::new(Barrier::new(8));
        // One fixed instant for every caller: the coalescing must not depend on how long
        // the threads take to queue under load (HS2-BK350W).
        let now = Instant::now();
        let handles = (0..8)
            .map(|_| {
                let cache = cache.clone();
                let scans = scans.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut cache = cache.lock_or_recover();
                    cache.discover(
                        now,
                        0,
                        AI_TOOL_DISCOVERY_TTL,
                        false,
                        |_, _| {
                            scans.fetch_add(1, Ordering::SeqCst);
                            std::thread::sleep(Duration::from_millis(50));
                            vec![tool("shared")]
                        },
                        || now,
                    )
                })
            })
            .collect::<Vec<_>>();
        for handle in handles {
            let tools = handle.join().unwrap();
            assert_eq!(tools.len(), 1);
            assert_eq!(tools[0].id, "shared");
        }
        assert_eq!(scans.load(Ordering::SeqCst), 1);
    }

    /// HS2-XZ3TTH: a scan that outlasts the TTL (each probe ending at its deadline) answers
    /// every caller that arrived while it ran, and the TTL counts from its completion. A
    /// caller under a newer generation, or asking for a refresh, still rescans.
    #[test]
    fn hs2_xz3tth_a_scan_longer_than_the_ttl_answers_callers_that_arrived_during_it() {
        /// One discovery that arrived at `arrived` and, if it scans, completes at `completed`.
        fn run(
            cache: &mut AiToolDiscoveryCache,
            runs: &mut Vec<String>,
            (arrived, completed): (Instant, Instant),
            generation: u64,
            refresh: bool,
            id: &str,
        ) -> Vec<String> {
            cache
                .discover(
                    arrived,
                    generation,
                    AI_TOOL_DISCOVERY_TTL,
                    refresh,
                    |_, _| {
                        runs.push(id.to_string());
                        vec![tool(id)]
                    },
                    || completed,
                )
                .into_iter()
                .map(|tool| tool.id)
                .collect()
        }
        let mut cache = AiToolDiscoveryCache::default();
        let mut runs = Vec::new();
        let start = Instant::now();
        let finished = start + 2 * AI_TOOL_DISCOVERY_TTL;
        let at = |instant: Instant| (instant, instant);
        let slow = run(&mut cache, &mut runs, (start, finished), 0, false, "slow");
        assert_eq!(slow, ["slow"]);
        // Queued behind the scan, 15 s after the first arrival: served by it, no rescan.
        let queued = start + AI_TOOL_DISCOVERY_TTL + Duration::from_secs(5);
        assert_eq!(
            run(&mut cache, &mut runs, at(queued), 0, false, "x"),
            ["slow"]
        );
        // Arrived exactly as the scan completed, and just inside the TTL after it.
        assert_eq!(
            run(&mut cache, &mut runs, at(finished), 0, false, "x"),
            ["slow"]
        );
        let inside = finished + AI_TOOL_DISCOVERY_TTL - Duration::from_millis(1);
        assert_eq!(
            run(&mut cache, &mut runs, at(inside), 0, false, "x"),
            ["slow"]
        );
        assert_eq!(runs, ["slow"]);
        // A plugin change during the scan bumped the generation: that caller rescans.
        let next = run(&mut cache, &mut runs, (queued, finished), 1, false, "next");
        assert_eq!(next, ["next"]);
        // An explicit refresh always rescans, and a full TTL after completion is stale.
        let fresh = run(&mut cache, &mut runs, (queued, finished), 1, true, "fresh");
        assert_eq!(fresh, ["fresh"]);
        let stale = finished + AI_TOOL_DISCOVERY_TTL;
        assert_eq!(
            run(&mut cache, &mut runs, at(stale), 1, false, "late"),
            ["late"]
        );
        assert_eq!(runs, ["slow", "next", "fresh", "late"]);
    }

    #[test]
    fn a_clock_that_moves_backwards_keeps_the_memo() {
        let mut cache = AiToolDiscoveryCache::default();
        let now = Instant::now() + Duration::from_secs(60);
        let mut scans = Vec::new();
        assert_eq!(discover(&mut cache, now, 0, false, &mut scans, "a"), ["a"]);
        let earlier = now - Duration::from_secs(5);
        assert_eq!(
            discover(&mut cache, earlier, 0, false, &mut scans, "b"),
            ["a"]
        );
        assert_eq!(scans, [false]);
    }
}
