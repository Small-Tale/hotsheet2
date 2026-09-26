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

use std::time::{Duration, Instant};

use hotsheet_aitools::ModelCatalogCache;
use hotsheet_plugins::AiToolDescriptor;

/// How long a discovery result is reused before the next caller rescans.
pub(crate) const AI_TOOL_DISCOVERY_TTL: Duration = Duration::from_secs(10);

/// One completed discovery and the conditions it was produced under.
struct DiscoveryMemo {
    scanned_at: Instant,
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
    /// Return the memoized discovery when it is younger than `ttl`, was scanned under the
    /// current invalidation `generation`, and `refresh` is not requested; otherwise run
    /// `scan` against the model catalogs and remember its result as of `now`.
    pub(crate) fn discover(
        &mut self,
        now: Instant,
        generation: u64,
        ttl: Duration,
        refresh: bool,
        scan: impl FnOnce(&mut ModelCatalogCache, bool) -> Vec<AiToolDescriptor>,
    ) -> Vec<AiToolDescriptor> {
        if !refresh
            && let Some(memo) = &self.memo
            && memo.generation == generation
            && now.saturating_duration_since(memo.scanned_at) < ttl
        {
            return memo.tools.clone();
        }
        let tools = scan(&mut self.catalogs, refresh);
        self.memo = Some(DiscoveryMemo {
            scanned_at: now,
            generation,
            tools: tools.clone(),
        });
        tools
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(id: &str) -> AiToolDescriptor {
        AiToolDescriptor {
            id: id.to_string(),
            display_name: id.to_string(),
            models: Vec::new(),
            default_model: None,
            default_effort: None,
            actions: Vec::new(),
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
        let empty = cache.discover(start, 0, AI_TOOL_DISCOVERY_TTL, false, |_, _| {
            runs += 1;
            Vec::new()
        });
        assert!(empty.is_empty());
        let still_empty = cache.discover(start, 0, AI_TOOL_DISCOVERY_TTL, false, |_, _| {
            runs += 1;
            vec![tool("late")]
        });
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
        let handles = (0..8)
            .map(|_| {
                let cache = cache.clone();
                let scans = scans.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    let mut cache = cache.lock().unwrap();
                    cache.discover(Instant::now(), 0, AI_TOOL_DISCOVERY_TTL, false, |_, _| {
                        scans.fetch_add(1, Ordering::SeqCst);
                        std::thread::sleep(Duration::from_millis(50));
                        vec![tool("shared")]
                    })
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
