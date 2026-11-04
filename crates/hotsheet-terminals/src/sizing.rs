//! **Server-arbitrated multi-viewer PTY sizing** (`docs/06` §6.7, `docs/05` §5.4, HS2-BD7Q74).
//!
//! One PTY has one size, but many viewports — several views on one device, and several
//! devices (incl. remote iOS/macOS) — attach at once and want different sizes depending on
//! focus. The **server** arbitrates: each viewport sends a leased size **claim**
//! ([`ViewportClaim`]) + heartbeat over the terminal WS; the [`SizeArbiter`] picks the PTY
//! size by [`SizePolicy`] and reports who drove it; on disconnect the lease expires and the
//! size recomputes (self-heal). This generalizes HS1's ad-hoc largest/last-writer consensus
//! (which never handled remotes) to all viewports.
//!
//! The default policy is **focus-follows** (tmux window-size latest): the PTY follows the
//! visible, sizing-eligible viewport a human most recently interacted with, with a short
//! **focus-hold** so a rapid flip doesn't thrash the size. Only claims the client marks
//! `interacting` count as human interaction; a first claim, a reconnect, an eligibility change,
//! or a heartbeat never takes the size from the last-interacted viewport (HS2-1YX5VS).
//! Everything here is a **pure state machine with an injected clock** (`now_ms` passed in), so
//! it gets transition-matrix + adversarial-sequence tests directly.

use std::collections::HashMap;

/// Hold the current size for this long after a focus change before following the new
/// viewport — debounces a rapid focus flip (`docs/06` §6.7 `SIZE_FOCUS_HOLD_MS`).
pub const SIZE_FOCUS_HOLD_MS: u64 = 500;
/// How many disconnected viewports' interaction times the arbiter remembers, so a viewport that
/// reconnects under the same id regains its place instead of ranking as never interacted.
pub const SIZE_DEPARTED_MEMORY: usize = 64;
/// Ignore a resize smaller than this many rows/cols of delta (avoid churn on ±1).
pub const SIZE_MIN_DELTA: u16 = 2;
/// Don't resize the PTY more often than this (`SIZE_RESIZE_MIN_INTERVAL_MS`). This keeps
/// interactive drags responsive while bounding SIGWINCH churn to ten updates per second.
pub const SIZE_RESIZE_MIN_INTERVAL_MS: u64 = 100;

/// One viewport's leased size claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewportClaim {
    /// Per-viewport id (NOT per-device — one device may show several views).
    pub viewer_id: String,
    pub cols: u16,
    pub rows: u16,
    /// This viewport currently has focus (the user is looking at / typing in it).
    pub focus: bool,
    /// This viewport is on-screen at all (a background tab is `visible: false`).
    pub visible: bool,
    /// This claim was triggered by a genuine user interaction (a tap/click, a focus gain, or a
    /// keystroke) rather than the steady heartbeat/geometry claim every viewport sends. Only an
    /// interacting claim advances the recency tiebreak, so one device's heartbeats can't steal
    /// size control from the device the user last actually touched (HS2-3ZBQDG).
    pub interacting: bool,
    /// This viewport's emulator can answer the program's terminal queries (cursor position,
    /// device attributes, focus reports). A read-only preview whose emulator emits no data
    /// declares `false`, so it is never chosen as the answerer (HS2-0GZZHV).
    pub answers_queries: bool,
    /// When this viewport last saw a genuine interaction — the recency tiebreak. The arbiter
    /// takes it from an interacting claim only; any other claim keeps the viewport's prior value
    /// (0 for a viewport that never interacted), whatever the caller passed (see
    /// [`SizeArbiter::upsert`], HS2-1YX5VS).
    pub activity_at_ms: u64,
}

/// How the arbiter reconciles many claims into one size.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum SizePolicy {
    /// The PTY follows the most-recently-focused visible viewport (the default, tmux-style).
    #[default]
    FocusFollows,
    /// The smallest visible viewport (everyone can see everything).
    Smallest,
    /// The largest visible viewport (fill the biggest screen; others letterbox/scroll).
    LargestVisible,
    /// A fixed size regardless of viewports.
    Pinned { cols: u16, rows: u16 },
}

/// The arbiter's chosen PTY size + who drove it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Decision {
    pub cols: u16,
    pub rows: u16,
    /// The viewport whose size won, if any (`None` for pinned / hold-current).
    pub driven_by: Option<String>,
    /// The one viewport whose emulator answers the program's terminal queries: the driver when
    /// it can answer, else the earliest-attached live viewport that can (HS2-0GZZHV). `None`
    /// when no attached viewport can answer yet. See [`SizeArbiter::answerer`].
    pub answered_by: Option<String>,
}

#[derive(Debug, Clone, Copy)]
struct Seen {
    seq: u64,
    claimed_at_ms: u64,
}

/// Arbitrates a PTY's size across many viewport claims.
#[derive(Debug)]
pub struct SizeArbiter {
    policy: SizePolicy,
    claims: HashMap<String, ViewportClaim>,
    /// Per live viewport: its attach order (the final tiebreak) and when its last claim arrived
    /// (the lease clock for [`Self::expire`]).
    seen: HashMap<String, Seen>,
    next_seq: u64,
    /// The interaction times of recently disconnected viewports, bounded by
    /// [`SIZE_DEPARTED_MEMORY`], restored when the same viewport id claims again (HS2-1YX5VS).
    departed: HashMap<String, u64>,
    /// The last size actually applied to the PTY (guards min-delta + min-interval).
    applied: Option<(u16, u16)>,
    /// The viewport whose claim produced `applied` (`None` for a seeded spawn size or a
    /// pinned/hold decision), so a newly attached viewer learns the driver too (HS2-7Y1BQ2).
    applied_by: Option<String>,
    applied_at_ms: u64,
    /// The viewport currently driving under focus-follows.
    driver: Option<String>,
    /// When a non-driver viewport last became a takeover candidate — by gaining focus or by a
    /// genuine interaction. A different candidate must persist for `SIZE_FOCUS_HOLD_MS` past this
    /// before it takes over, debouncing rapid focus flips and stray double-taps.
    focus_changed_at_ms: u64,
    /// When the owner should decide again: the trailing edge of the min-interval rate limit or
    /// the end of a focus-hold takeover, until a later `decide` applies or drops it.
    deferred_until_ms: Option<u64>,
    /// The answerer named by the last size frame the owner broadcast, so an answerer change
    /// without a resize is still announced (HS2-0GZZHV).
    announced_answerer: Option<String>,
}

impl Default for SizeArbiter {
    fn default() -> Self {
        Self::new(SizePolicy::default())
    }
}

impl SizeArbiter {
    pub fn new(policy: SizePolicy) -> Self {
        Self {
            policy,
            claims: HashMap::new(),
            seen: HashMap::new(),
            next_seq: 0,
            departed: HashMap::new(),
            applied: None,
            applied_by: None,
            applied_at_ms: 0,
            driver: None,
            focus_changed_at_ms: 0,
            deferred_until_ms: None,
            announced_answerer: None,
        }
    }

    pub fn set_policy(&mut self, policy: SizePolicy) {
        self.policy = policy;
    }

    /// Seed the "already applied" size (e.g. the terminal's spawn size), so the min-delta
    /// guard has a baseline and "nothing focused" holds something sensible.
    pub fn set_applied(&mut self, cols: u16, rows: u16) {
        self.applied = Some((cols, rows));
        self.applied_by = None;
    }

    /// The size currently applied to the PTY and the viewport that drove it, or `None` before
    /// any size was seeded or decided. A newly attached viewer receives this immediately, so a
    /// terminal whose size is stable still reports its real grid (HS2-7Y1BQ2).
    pub fn applied(&self) -> Option<Decision> {
        self.applied.map(|(cols, rows)| Decision {
            cols,
            rows,
            driven_by: self.applied_by.clone(),
            answered_by: self.answerer(),
        })
    }

    /// The one viewport whose emulator answers the program's terminal queries (HS2-0GZZHV).
    /// Every attached xterm answers a query such as a cursor-position request, so with several
    /// viewers the program would read several replies. The answerer is the viewport that drove
    /// the applied size, when it can answer; otherwise the earliest-attached live viewport that
    /// can. Attach order is stable while that viewport stays attached, so an undriven terminal's
    /// answerer never flaps between idle viewers. `None` when no attached viewport can answer.
    pub fn answerer(&self) -> Option<String> {
        if let Some(driver) = self.applied_by.as_deref()
            && self.claims.get(driver).is_some_and(|c| c.answers_queries)
        {
            return Some(driver.to_string());
        }
        self.claims
            .values()
            .filter(|c| c.answers_queries)
            .min_by_key(|c| {
                self.seen
                    .get(&c.viewer_id)
                    .map_or(u64::MAX, |seen| seen.seq)
            })
            .map(|c| c.viewer_id.clone())
    }

    /// The applied size naming a new answerer, when the answerer changed since the last
    /// broadcast frame without a resize: a viewport attaching to an undriven terminal, or the
    /// answerer leaving. The owner broadcasts it so the new answerer starts and the old one
    /// stops at once (HS2-0GZZHV). `None` when the announced answerer is still current.
    pub fn answerer_notice(&mut self) -> Option<Decision> {
        let current = self.applied()?;
        if current.answered_by == self.announced_answerer {
            return None;
        }
        self.announced_answerer = current.answered_by.clone();
        Some(current)
    }

    pub fn viewer_count(&self) -> usize {
        self.claims.len()
    }

    /// Add or update a viewport's claim. When a viewport *newly* becomes eligible or is
    /// interacted with and isn't already the driver, that starts the focus-hold clock so a rapid
    /// flip doesn't switch the driver.
    ///
    /// The recency tiebreak (`activity_at_ms`) tracks genuine **interaction** only: an
    /// interacting claim sets it to `now_ms`; every other claim keeps the viewport's prior
    /// interaction time. A viewport's first claim, a reconnect, a sizing-eligibility gain, and a
    /// heartbeat are passive, so none of them can take the size from the viewport a human last
    /// touched (HS2-3ZBQDG, HS2-1YX5VS). A viewport that reconnects under the same id regains
    /// the interaction time it had when it left.
    pub fn upsert(&mut self, mut claim: ViewportClaim, now_ms: u64) {
        let prev = self.claims.get(&claim.viewer_id);
        let gained_focus = claim.focus && prev.is_none_or(|p| !p.focus);
        // A non-driver viewport that gains eligibility *or* is actively interacted with is a
        // takeover candidate; arm the focus-hold so a rapid flip (or a stray double-tap) must
        // persist for SIZE_FOCUS_HOLD_MS before it actually switches the driver.
        let starts_takeover = (gained_focus || claim.interacting)
            && self.driver.as_deref() != Some(claim.viewer_id.as_str());
        if starts_takeover {
            self.focus_changed_at_ms = now_ms;
        }
        if claim.interacting {
            claim.activity_at_ms = now_ms;
        } else {
            claim.activity_at_ms = match prev {
                Some(previous) => previous.activity_at_ms,
                None => self.departed.get(&claim.viewer_id).copied().unwrap_or(0),
            };
        }
        self.departed.remove(&claim.viewer_id);
        let seq = match self.seen.get(&claim.viewer_id) {
            Some(seen) => seen.seq,
            None => {
                self.next_seq += 1;
                self.next_seq
            }
        };
        self.seen.insert(
            claim.viewer_id.clone(),
            Seen {
                seq,
                claimed_at_ms: now_ms,
            },
        );
        self.claims.insert(claim.viewer_id.clone(), claim);
    }

    /// Drop a viewport (it disconnected). If it was the driver, the size recomputes
    /// immediately on the next `decide` (self-heal — no hold on a disconnect).
    pub fn remove(&mut self, viewer_id: &str) {
        self.seen.remove(viewer_id);
        if let Some(gone) = self.claims.remove(viewer_id)
            && gone.activity_at_ms > 0
        {
            self.remember_departed(gone.viewer_id, gone.activity_at_ms);
        }
        if self.driver.as_deref() == Some(viewer_id) {
            self.driver = None;
        }
        // A held size outlives the viewport that drove it; report it as undriven from now on.
        if self.applied_by.as_deref() == Some(viewer_id) {
            self.applied_by = None;
        }
    }

    fn remember_departed(&mut self, viewer_id: String, activity_at_ms: u64) {
        if self.departed.len() >= SIZE_DEPARTED_MEMORY
            && !self.departed.contains_key(&viewer_id)
            && let Some(oldest) = self
                .departed
                .iter()
                .min_by_key(|(_, at)| **at)
                .map(|(id, _)| id.clone())
        {
            self.departed.remove(&oldest);
        }
        self.departed.insert(viewer_id, activity_at_ms);
    }

    /// Drop viewports whose last claim (of any kind, heartbeats included) is older than
    /// `lease_ms`. Returns how many were expired.
    pub fn expire(&mut self, now_ms: u64, lease_ms: u64) -> usize {
        let stale: Vec<String> = self
            .seen
            .iter()
            .filter(|(_, seen)| now_ms.saturating_sub(seen.claimed_at_ms) > lease_ms)
            .map(|(id, _)| id.clone())
            .collect();
        for id in &stale {
            self.remove(id);
        }
        stale.len()
    }

    /// Compute the PTY size to apply **now**, or `None` if nothing should change (no viewers,
    /// within the min-interval, or a sub-min-delta change). A returned `Decision` is recorded
    /// as the applied size.
    pub fn decide(&mut self, now_ms: u64) -> Option<Decision> {
        self.deferred_until_ms = None;
        let target = self.target(now_ms)?;

        // Guard: ignore a sub-threshold change from what's already applied.
        if let Some((c, r)) = self.applied {
            let d_cols = c.abs_diff(target.cols);
            let d_rows = r.abs_diff(target.rows);
            if d_cols < SIZE_MIN_DELTA && d_rows < SIZE_MIN_DELTA {
                return None;
            }
        }
        // Guard: don't resize too often. The change is deferred, not dropped: the owner calls
        // `decide` again at `deferred_until` so the last size of a burst still lands.
        if self.applied.is_some()
            && now_ms.saturating_sub(self.applied_at_ms) < SIZE_RESIZE_MIN_INTERVAL_MS
        {
            self.defer_until(self.applied_at_ms + SIZE_RESIZE_MIN_INTERVAL_MS);
            return None;
        }
        self.applied = Some((target.cols, target.rows));
        self.applied_by = target.driven_by.clone();
        self.applied_at_ms = now_ms;
        let answered_by = self.answerer();
        self.announced_answerer = answered_by.clone();
        Some(Decision {
            answered_by,
            ..target
        })
    }

    /// When the owner should call `decide` again because the last one suppressed a change: the
    /// min-interval rate limit (HS2-GSRZX6) or a focus-hold takeover (HS2-G4C082). The earliest
    /// pending deadline wins; the next `decide` recomputes any remaining one.
    pub fn deferred_until(&self) -> Option<u64> {
        self.deferred_until_ms
    }

    fn defer_until(&mut self, at_ms: u64) {
        self.deferred_until_ms = Some(
            self.deferred_until_ms
                .map_or(at_ms, |current| current.min(at_ms)),
        );
    }

    /// The size the policy wants right now (ignoring the resize-rate guards). `None` = no
    /// applicable viewport (hold whatever's applied).
    fn target(&mut self, now_ms: u64) -> Option<Decision> {
        match &self.policy {
            SizePolicy::Pinned { cols, rows } => Some(Decision {
                cols: *cols,
                rows: *rows,
                driven_by: None,
                answered_by: None,
            }),
            SizePolicy::Smallest => self.by_extreme(false),
            SizePolicy::LargestVisible => self.by_extreme(true),
            SizePolicy::FocusFollows => self.focus_follows(now_ms),
        }
    }

    /// Smallest/largest visible viewport by area (cols*rows).
    fn by_extreme(&self, largest: bool) -> Option<Decision> {
        let mut best: Option<&ViewportClaim> = None;
        for c in self.claims.values().filter(|c| c.visible) {
            let area = c.cols as u32 * c.rows as u32;
            let take = match best {
                None => true,
                Some(b) => {
                    let ba = b.cols as u32 * b.rows as u32;
                    if largest { area > ba } else { area < ba }
                }
            };
            if take {
                best = Some(c);
            }
        }
        best.map(|c| Decision {
            cols: c.cols,
            rows: c.rows,
            driven_by: Some(c.viewer_id.clone()),
            answered_by: None,
        })
    }

    /// Focus-follows: the most-recently-interacted eligible viewport drives. A *different* live
    /// viewport only takes over once it has held focus for `SIZE_FOCUS_HOLD_MS` (debounce); a
    /// disconnected driver recomputes immediately (self-heal); nothing focused holds current.
    fn focus_follows(&mut self, now_ms: u64) -> Option<Decision> {
        let candidate = self.latest_focused().map(|c| c.viewer_id.clone());
        let Some(cand) = candidate else {
            // Nothing focused → hold whatever size is applied.
            return self.applied.map(|(cols, rows)| Decision {
                cols,
                rows,
                driven_by: None,
                answered_by: None,
            });
        };
        let driver_present = self
            .driver
            .as_ref()
            .is_some_and(|d| self.claims.contains_key(d));

        if self.driver.as_deref() == Some(cand.as_str()) && driver_present {
            // The focused viewport is already the driver.
        } else if !driver_present {
            // No live driver → follow the focused viewport immediately (self-heal).
            self.driver = Some(cand.clone());
        } else if now_ms.saturating_sub(self.focus_changed_at_ms) >= SIZE_FOCUS_HOLD_MS {
            // Focus moved to a different live viewport and has been held long enough.
            self.driver = Some(cand.clone());
        } else {
            // Within the hold window: keep the current driver (hold its size), and ask the owner to
            // decide again when the hold ends so the takeover lands without waiting for another
            // claim — otherwise it could wait for the next 5 s heartbeat (HS2-G4C082).
            self.defer_until(self.focus_changed_at_ms + SIZE_FOCUS_HOLD_MS);
        }

        let d = self.driver.clone().unwrap_or(cand);
        self.claims.get(&d).map(|c| Decision {
            cols: c.cols,
            rows: c.rows,
            driven_by: Some(c.viewer_id.clone()),
            answered_by: None,
        })
    }

    /// The eligible (focused and visible) viewport with the newest human interaction. Ties —
    /// for example several viewports that never interacted — prefer the current driver, then
    /// the earliest attached, so the choice never depends on map order (HS2-1YX5VS).
    fn latest_focused(&self) -> Option<&ViewportClaim> {
        let driver = self.driver.as_deref();
        self.claims
            .values()
            .filter(|c| c.focus && c.visible)
            .max_by_key(|c| {
                let seq = self
                    .seen
                    .get(&c.viewer_id)
                    .map_or(u64::MAX, |seen| seen.seq);
                (
                    c.activity_at_ms,
                    driver == Some(c.viewer_id.as_str()),
                    std::cmp::Reverse(seq),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: &str, cols: u16, rows: u16, focus: bool, at: u64) -> ViewportClaim {
        ViewportClaim {
            viewer_id: id.into(),
            cols,
            rows,
            focus,
            visible: true,
            interacting: false,
            answers_queries: true,
            activity_at_ms: at,
        }
    }

    /// `applied` reports the size on the PTY and who drove it across seed, decide, a
    /// suppressed change, a deferred change, a driver disconnect, and a re-seed (HS2-7Y1BQ2).
    #[test]
    fn applied_tracks_the_size_on_the_pty_and_its_driver() {
        let mut a = SizeArbiter::default();
        assert_eq!(a.applied(), None, "nothing seeded or decided yet");
        a.set_applied(80, 24);
        assert_eq!(
            a.applied(),
            Some(Decision {
                cols: 80,
                rows: 24,
                driven_by: None,
                answered_by: None
            })
        );
        // Past the seed's min-interval, so the first claim applies immediately.
        a.upsert(claim("v1", 132, 40, true, 0), 0);
        assert!(a.decide(SIZE_RESIZE_MIN_INTERVAL_MS).is_some());
        assert_eq!(
            a.applied(),
            Some(Decision {
                cols: 132,
                rows: 40,
                driven_by: Some("v1".into()),
                answered_by: Some("v1".into())
            })
        );
        // A rate-limited change is deferred, not applied, until the interval passes.
        a.upsert(claim("v1", 100, 30, true, 150), 150);
        assert!(a.decide(150).is_none());
        assert_eq!(a.applied().map(|d| d.cols), Some(132));
        assert!(a.decide(250).is_some());
        assert_eq!(a.applied().map(|d| (d.cols, d.rows)), Some((100, 30)));
        // A sub-min-delta change is suppressed, so the applied size and driver stay put.
        a.upsert(claim("v1", 101, 30, true, 1_000), 1_000);
        assert!(a.decide(1_000).is_none());
        assert_eq!(
            a.applied().map(|d| (d.cols, d.driven_by)),
            Some((100, Some("v1".into())))
        );
        // The driver leaves: nothing focused holds the applied size, now undriven.
        a.remove("v1");
        assert!(
            a.decide(2_000).is_none(),
            "holding the same size is not a change"
        );
        assert_eq!(
            a.applied(),
            Some(Decision {
                cols: 100,
                rows: 30,
                driven_by: None,
                answered_by: None
            })
        );
        // A re-seed (e.g. a respawn) clears the stale driver.
        a.set_applied(90, 30);
        assert_eq!(
            a.applied(),
            Some(Decision {
                cols: 90,
                rows: 30,
                driven_by: None,
                answered_by: None
            })
        );
    }

    /// A claim carrying a genuine user interaction (tap/keystroke), which advances the recency
    /// tiebreak even when the viewport was already focused.
    fn interacting_claim(id: &str, cols: u16, rows: u16, focus: bool, at: u64) -> ViewportClaim {
        ViewportClaim {
            interacting: true,
            ..claim(id, cols, rows, focus, at)
        }
    }

    #[test]
    fn single_viewport_drives_the_size() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        let d = a.decide(0).unwrap();
        assert_eq!((d.cols, d.rows), (100, 40));
        assert_eq!(d.driven_by.as_deref(), Some("v1"));
    }

    #[test]
    fn min_interval_and_min_delta_guards() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        assert!(a.decide(0).is_some());

        assert_eq!(a.deferred_until(), None);

        // A change within the min-interval is suppressed, and deferred to the interval's end.
        a.upsert(claim("v1", 120, 50, true, 50), 50);
        assert!(a.decide(50).is_none(), "too soon after the last resize");
        assert_eq!(a.deferred_until(), Some(SIZE_RESIZE_MIN_INTERVAL_MS));

        // After the interval, it applies.
        a.upsert(claim("v1", 120, 50, true, 100), 100);
        assert_eq!(a.decide(100).map(|d| (d.cols, d.rows)), Some((120, 50)));
        assert_eq!(a.deferred_until(), None, "applying clears the deferral");

        // A sub-min-delta change (±1) is ignored even after the interval, and never deferred.
        a.upsert(claim("v1", 121, 50, true, 150), 150);
        assert!(a.decide(150).is_none());
        assert_eq!(a.deferred_until(), None, "nothing worth applying later");
        a.upsert(claim("v1", 121, 50, true, 2000), 2000);
        assert!(a.decide(2000).is_none(), "±1 is below SIZE_MIN_DELTA");

        // A change that returns to the applied size before the deferral fires drops it.
        a.upsert(claim("v1", 140, 60, true, 2000), 2000);
        assert!(a.decide(2000).is_some());
        a.upsert(claim("v1", 160, 70, true, 2050), 2050);
        assert!(a.decide(2050).is_none());
        assert_eq!(a.deferred_until(), Some(2100));
        a.upsert(claim("v1", 140, 60, true, 2080), 2080);
        assert!(a.decide(2080).is_none());
        assert_eq!(
            a.deferred_until(),
            None,
            "the burst ended at the applied size"
        );
    }

    #[test]
    fn focus_follows_switches_only_after_the_hold_window() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        assert_eq!(a.decide(0).unwrap().driven_by.as_deref(), Some("v1")); // applied (100,40)

        // v2 (a different size) takes focus at t=1000; v1 blurs. Within the hold window, v1
        // still drives — so a decide holds the size (100,40), i.e. no resize to v2's (80,24).
        a.upsert(claim("v1", 100, 40, false, 1000), 1000);
        a.upsert(claim("v2", 80, 24, true, 1000), 1000);
        // Past the resize interval, but still inside the 500ms focus-hold (t=1000+300):
        assert!(
            a.decide(1300).is_none(),
            "within the focus-hold the size holds at v1's — no resize to v2"
        );

        // After the hold window (t >= 1500) and the resize interval, v2 wins.
        let d = a.decide(1600).expect("v2 takes over after the hold");
        assert_eq!(d.driven_by.as_deref(), Some("v2"));
        assert_eq!((d.cols, d.rows), (80, 24));
    }

    #[test]
    fn a_rapid_focus_flip_within_the_hold_does_not_switch() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        a.decide(0); // v1 drives, applied (100,40)

        // v2 blips focus at t=1000, then v1 re-takes focus at t=1200 (within v2's hold window).
        a.upsert(claim("v1", 100, 40, false, 1000), 1000);
        a.upsert(claim("v2", 80, 24, true, 1000), 1000);
        a.upsert(claim("v2", 80, 24, false, 1200), 1200);
        a.upsert(claim("v1", 100, 40, true, 1200), 1200);

        // Well past everything, v1 is still the driver (the blip never took over).
        let d = a.decide(3000);
        assert!(
            d.is_none() || d.unwrap().driven_by.as_deref() == Some("v1"),
            "a focus blip absorbed within the hold must not switch the driver"
        );
        assert_eq!(a.driver.as_deref(), Some("v1"));
    }

    #[test]
    fn a_disconnecting_driver_self_heals_immediately() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        a.upsert(claim("v2", 80, 24, true, 0), 0);
        // The user interacts with v2 at t=600, promoting it past the hold (a heartbeat would not).
        a.upsert(interacting_claim("v2", 80, 24, true, 600), 600);
        let d = a.decide(1000).unwrap();
        assert_eq!(d.driven_by.as_deref(), Some("v2"));

        // v2 disconnects — the size recomputes from the remaining focused viewport, no hold.
        a.remove("v2");
        let d = a.decide(2000).unwrap();
        assert_eq!(d.driven_by.as_deref(), Some("v1"));
        assert_eq!((d.cols, d.rows), (100, 40));
    }

    #[test]
    fn expire_drops_stale_viewports() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        a.upsert(claim("v2", 80, 24, false, 0), 0);
        assert_eq!(a.viewer_count(), 2);
        // At t=5000 with a 3000ms lease, both last-active-at-0 are stale.
        assert_eq!(a.expire(5000, 3000), 2);
        assert_eq!(a.viewer_count(), 0);
    }

    #[test]
    fn nothing_focused_holds_the_current_size() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v1", 100, 40, true, 0), 0);
        a.decide(0);
        // v1 blurs (no focus anywhere).
        a.upsert(claim("v1", 100, 40, false, 1000), 1000);
        let d = a.decide(1000);
        // Holds the applied size, driven_by none.
        assert_eq!(d.map(|d| (d.cols, d.rows)), None); // within min-delta of applied → no change
        // Force a would-be change: still holds because nothing is focused.
        a.upsert(claim("v1", 200, 60, false, 2000), 2000);
        assert!(
            a.decide(2000).is_none(),
            "an unfocused viewport can't drive a resize"
        );
    }

    #[test]
    fn smallest_and_largest_and_pinned_policies() {
        let mut a = SizeArbiter::new(SizePolicy::Smallest);
        a.upsert(claim("big", 200, 60, false, 0), 0);
        a.upsert(claim("small", 80, 24, false, 0), 0);
        assert_eq!(a.decide(0).map(|d| (d.cols, d.rows)), Some((80, 24)));

        a.set_policy(SizePolicy::LargestVisible);
        assert_eq!(a.decide(1000).map(|d| (d.cols, d.rows)), Some((200, 60)));

        a.set_policy(SizePolicy::Pinned {
            cols: 132,
            rows: 43,
        });
        let d = a.decide(2000).unwrap();
        assert_eq!((d.cols, d.rows), (132, 43));
        assert_eq!(d.driven_by, None);
    }

    /// Adversarial: interleaved focus flips + a disconnect + rapid claims never panic and
    /// always converge to a focused viewport's size (or hold).
    #[test]
    fn adversarial_interleaved_focus_and_disconnects() {
        let mut a = SizeArbiter::default();
        let mut t = 0u64;
        for i in 0..50 {
            t += 100;
            let focus = i % 3 == 0;
            a.upsert(claim("v1", 100, 40, focus, t), t);
            a.upsert(claim("v2", 80, 24, !focus, t), t);
            let _ = a.decide(t);
            if i % 7 == 0 {
                a.remove("v2");
            }
            if i % 11 == 0 {
                let _ = a.expire(t, 50);
            }
        }
        // Ends in a consistent state — a final decide either holds or names a live viewer.
        t += SIZE_RESIZE_MIN_INTERVAL_MS;
        a.upsert(claim("v1", 90, 30, true, t), t);
        let d = a.decide(t).unwrap();
        assert_eq!(d.driven_by.as_deref(), Some("v1"));
    }

    /// HS2-3ZBQDG regression: two focused devices of different sizes (e.g. a desktop and a
    /// phone) both stream steady heartbeats. Heartbeats must NOT move the recency tiebreak, so
    /// the driver stays the device the user last actually interacted with and the PTY size does
    /// not thrash. (Before the interaction-aware recency, interleaved heartbeats flipped
    /// "latest focused" every ~5s, resizing the PTY and leaving the phone rendering the
    /// desktop's size — "terminals showing empty on mobile".)
    #[test]
    fn steady_heartbeats_do_not_steal_the_driver() {
        let mut a = SizeArbiter::default();
        // The phone (v2) attaches focused first; then the user interacts with the desktop (v1).
        a.upsert(claim("v2", 80, 24, true, 0), 0);
        a.upsert(interacting_claim("v1", 200, 50, true, 100), 100);
        let first = a.decide(200).unwrap();
        assert_eq!(first.driven_by.as_deref(), Some("v1"));
        assert_eq!((first.cols, first.rows), (200, 50));

        // Now both devices only heartbeat (focus stays true, no new interaction) for a long time.
        let mut t = 300;
        while t <= 30_000 {
            a.upsert(claim("v2", 80, 24, true, t), t);
            a.upsert(claim("v1", 200, 50, true, t), t);
            if let Some(d) = a.decide(t) {
                assert_eq!(
                    d.driven_by.as_deref(),
                    Some("v1"),
                    "a heartbeat must never hand control to the phone at t={t}"
                );
                assert_eq!((d.cols, d.rows), (200, 50));
            }
            t += 700; // interleave the two devices' heartbeats at an odd cadence
        }
        assert_eq!(a.driver.as_deref(), Some("v1"));
    }

    /// The counterpart: when the user *actually interacts* with the other device (a tap/keystroke,
    /// `interacting: true`) it takes over after the focus-hold — even though its focus never
    /// toggled (dashboard tiles stay focused on every device).
    #[test]
    fn a_genuine_interaction_takes_over_after_the_hold() {
        let mut a = SizeArbiter::default();
        a.upsert(claim("v2", 80, 24, true, 0), 0);
        a.upsert(interacting_claim("v1", 200, 50, true, 100), 100);
        assert_eq!(a.decide(200).unwrap().driven_by.as_deref(), Some("v1"));

        // The user picks up the phone and taps it at t=5000 (focus was already true — heartbeats).
        a.upsert(interacting_claim("v2", 80, 24, true, 5000), 5000);
        // Within the focus-hold, the desktop still drives (no thrash on the tap).
        assert!(
            a.decide(5300).is_none(),
            "within the hold the phone's tap has not switched control yet"
        );
        // Past the hold, the phone (the last interacted device) takes control.
        let d = a.decide(5600).expect("the phone takes over after the hold");
        assert_eq!(d.driven_by.as_deref(), Some("v2"));
        assert_eq!((d.cols, d.rows), (80, 24));
    }

    /// A held takeover asks for a re-decide at the end of the hold, and that decide switches the
    /// driver with no further claim (HS2-G4C082).
    #[test]
    fn a_held_takeover_defers_a_re_decide_to_the_end_of_the_hold() {
        let mut a = SizeArbiter::default();
        a.upsert(interacting_claim("desk", 200, 50, true, 0), 0);
        assert!(a.decide(0).is_some());
        assert_eq!(a.deferred_until(), None);
        a.upsert(interacting_claim("phone", 80, 24, true, 5000), 5000);
        assert!(a.decide(5000).is_none(), "held");
        assert_eq!(a.deferred_until(), Some(5000 + SIZE_FOCUS_HOLD_MS));
        // A heartbeat inside the hold keeps the same deadline (no drift, no earlier switch).
        a.upsert(claim("desk", 200, 50, true, 5200), 5200);
        assert!(a.decide(5200).is_none());
        assert_eq!(a.deferred_until(), Some(5000 + SIZE_FOCUS_HOLD_MS));
        let d = a
            .decide(5000 + SIZE_FOCUS_HOLD_MS)
            .expect("the takeover lands at the deadline");
        assert_eq!(d.driven_by.as_deref(), Some("phone"));
        assert_eq!(a.deferred_until(), None, "nothing left pending");
    }

    /// When the hold and the min-interval both defer, the earliest deadline is reported; a
    /// re-decide then reports the remaining one.
    #[test]
    fn overlapping_hold_and_rate_limit_report_the_earliest_deadline() {
        let mut a = SizeArbiter::default();
        a.upsert(interacting_claim("desk", 200, 50, true, 0), 0);
        assert!(a.decide(0).is_some()); // applied at t=0
        a.upsert(interacting_claim("phone", 80, 24, true, 50), 50);
        // Held (until 550); the held desk size equals the applied size, so no rate-limit deferral.
        assert!(a.decide(50).is_none());
        assert_eq!(a.deferred_until(), Some(50 + SIZE_FOCUS_HOLD_MS));
        // The desk resizes inside both windows: rate limit (until 100) and hold (until 550).
        a.upsert(claim("desk", 180, 48, true, 60), 60);
        assert!(a.decide(60).is_none());
        assert_eq!(a.deferred_until(), Some(SIZE_RESIZE_MIN_INTERVAL_MS));
        let desk = a
            .decide(SIZE_RESIZE_MIN_INTERVAL_MS)
            .expect("the desk resize lands first");
        assert_eq!((desk.cols, desk.rows), (180, 48));
        assert_eq!(
            a.deferred_until(),
            Some(50 + SIZE_FOCUS_HOLD_MS),
            "the hold is still pending"
        );
        let phone = a
            .decide(50 + SIZE_FOCUS_HOLD_MS)
            .expect("then the takeover");
        assert_eq!(phone.driven_by.as_deref(), Some("phone"));
        assert_eq!(a.deferred_until(), None);
    }

    /// A rapid flip back to the current driver inside the hold cancels the pending takeover.
    #[test]
    fn a_flip_back_inside_the_hold_leaves_nothing_pending() {
        let mut a = SizeArbiter::default();
        a.upsert(interacting_claim("desk", 200, 50, true, 0), 0);
        assert!(a.decide(0).is_some());
        a.upsert(interacting_claim("phone", 80, 24, true, 1000), 1000);
        assert!(a.decide(1000).is_none());
        assert!(a.deferred_until().is_some());
        a.upsert(interacting_claim("desk", 200, 50, true, 1100), 1100);
        assert!(a.decide(1100).is_none());
        assert_eq!(
            a.deferred_until(),
            None,
            "the desk is the focused driver again"
        );
        assert!(
            a.decide(1600).is_none(),
            "no takeover after the old deadline"
        );
    }

    /// Every decision an arbiter makes while `step` feeds it claims, as `(at, driver, cols)`.
    fn decisions(
        a: &mut SizeArbiter,
        from: u64,
        to: u64,
        every: u64,
        mut step: impl FnMut(&mut SizeArbiter, u64),
    ) -> Vec<(u64, String, u16)> {
        let mut out = Vec::new();
        let mut t = from;
        while t <= to {
            step(a, t);
            if let Some(d) = a.decide(t) {
                out.push((t, d.driven_by.unwrap_or_default(), d.cols));
            }
            t += every;
        }
        out
    }

    /// A desk that the user typed in drives, then a phone attaches and stays passive.
    fn desk_driving_with_a_passive_phone() -> SizeArbiter {
        let mut a = SizeArbiter::default();
        a.upsert(interacting_claim("desk", 200, 50, true, 0), 1_000);
        assert_eq!(a.decide(1_000).unwrap().driven_by.as_deref(), Some("desk"));
        a.upsert(claim("phone", 80, 24, true, 0), 2_000);
        assert!(a.decide(2_000).is_none(), "a passive arrival never resizes");
        a
    }

    /// HS2-1YX5VS: a passive viewer's first claim, repeated geometry claims (ResizeObserver
    /// jitter), visibility flips, and heartbeats never take the size from the viewport a human
    /// last interacted with, and two idle viewers never resize the PTY.
    #[test]
    fn passive_claims_never_take_the_size_from_the_last_interacted_viewport() {
        let mut a = desk_driving_with_a_passive_phone();
        let mut jitter = 0u16;
        let churn = decisions(&mut a, 2_100, 60_000, 100, |a, t| {
            jitter = (jitter + 1) % 3;
            // The phone's observer reports a slightly different grid every 100 ms and its
            // visibility blinks; the desk heartbeats every 5 s.
            a.upsert(claim("phone", 80 + jitter, 24 + jitter, true, t), t);
            if t % 1_000 == 0 {
                a.upsert(
                    ViewportClaim {
                        visible: t % 2_000 == 0,
                        ..claim("phone", 80, 24, true, t)
                    },
                    t,
                );
            }
            if t % 5_000 == 0 {
                a.upsert(claim("desk", 200, 50, true, t), t);
            }
        });
        assert_eq!(churn, vec![], "two idle viewers never resize the PTY");
        assert_eq!(a.driver.as_deref(), Some("desk"));
    }

    /// HS2-1YX5VS: a passive viewer that reconnects (drop, then a fresh first claim under the
    /// same id) is still passive and does not take the size.
    #[test]
    fn a_passive_viewer_reconnecting_does_not_take_the_size() {
        let mut a = desk_driving_with_a_passive_phone();
        for round in 0..5u64 {
            let t = 3_000 + round * 1_000;
            a.remove("phone");
            assert!(
                a.decide(t).is_none(),
                "the passive phone leaving changes nothing"
            );
            a.upsert(claim("phone", 80, 24, true, 0), t);
            assert!(a.decide(t + SIZE_FOCUS_HOLD_MS).is_none());
        }
        assert_eq!(a.driver.as_deref(), Some("desk"));
    }

    /// HS2-1YX5VS: an interaction hands the size over after the hold, back and forth, and each
    /// handoff resizes exactly once while the other side keeps heartbeating.
    #[test]
    fn interaction_hands_the_size_back_and_forth() {
        let mut a = desk_driving_with_a_passive_phone();
        let mut handoffs = Vec::new();
        for (round, who) in ["phone", "desk", "phone", "desk"].into_iter().enumerate() {
            let t = 10_000 + round as u64 * 10_000;
            let (cols, rows) = if who == "phone" { (80, 24) } else { (200, 50) };
            a.upsert(interacting_claim(who, cols, rows, true, 0), t);
            assert!(a.decide(t).is_none(), "held at t={t}");
            let other = if who == "phone" {
                ("desk", 200, 50)
            } else {
                ("phone", 80, 24)
            };
            handoffs.extend(decisions(&mut a, t + 100, t + 9_000, 100, |a, now| {
                if now % 1_000 == 0 {
                    a.upsert(claim(other.0, other.1, other.2, true, 0), now);
                }
            }));
        }
        assert_eq!(
            handoffs,
            vec![
                (10_500, "phone".into(), 80),
                (20_500, "desk".into(), 200),
                (30_500, "phone".into(), 80),
                (40_500, "desk".into(), 200),
            ]
        );
    }

    /// HS2-1YX5VS: with three viewers, the most recently interacted one drives; the others'
    /// passive claims and reconnects never displace it.
    #[test]
    fn three_viewers_follow_the_most_recent_interaction() {
        let mut a = desk_driving_with_a_passive_phone();
        a.upsert(claim("tablet", 120, 40, true, 0), 2_500);
        assert!(a.decide(2_500).is_none());
        a.upsert(interacting_claim("tablet", 120, 40, true, 0), 5_000);
        assert_eq!(
            a.decide(5_000 + SIZE_FOCUS_HOLD_MS).map(|d| d.driven_by),
            Some(Some("tablet".into()))
        );
        // The desk and phone reconnect and heartbeat; the tablet keeps the size.
        a.remove("desk");
        a.remove("phone");
        a.upsert(claim("desk", 200, 50, true, 0), 7_000);
        a.upsert(claim("phone", 80, 24, true, 0), 7_000);
        assert!(a.decide(8_000).is_none());
        // The tablet leaves: the desk interacted more recently than the phone, so it drives.
        a.remove("tablet");
        assert_eq!(
            a.decide(9_000).map(|d| d.driven_by),
            Some(Some("desk".into()))
        );
    }

    /// HS2-1YX5VS: when the authority disconnects, a survivor drives at once (self-heal). If the
    /// authority comes back under the same id it regains the size, because its last interaction
    /// is still the newest; if someone interacted elsewhere meanwhile, it does not.
    #[test]
    fn the_authority_regains_the_size_after_a_reconnect_unless_someone_interacted_since() {
        let mut a = desk_driving_with_a_passive_phone();
        a.upsert(interacting_claim("phone", 80, 24, true, 0), 3_000);
        assert_eq!(
            a.decide(3_000 + SIZE_FOCUS_HOLD_MS).map(|d| d.driven_by),
            Some(Some("phone".into()))
        );
        // The phone's socket drops: the desk takes over immediately.
        a.remove("phone");
        assert_eq!(
            a.decide(5_000).map(|d| d.driven_by),
            Some(Some("desk".into()))
        );
        // The phone reconnects passively: it was the last device touched, so it drives again
        // once the focus-hold confirms it stayed.
        a.upsert(claim("phone", 80, 24, true, 0), 6_000);
        assert!(a.decide(6_000).is_none(), "held");
        assert_eq!(
            a.decide(6_000 + SIZE_FOCUS_HOLD_MS).map(|d| d.driven_by),
            Some(Some("phone".into()))
        );
        // It drops again, and this time the user types on the desk before it returns.
        a.remove("phone");
        assert!(a.decide(7_000).is_some());
        a.upsert(interacting_claim("desk", 200, 50, true, 0), 7_500);
        assert!(a.decide(7_500).is_none(), "the desk already drives");
        a.upsert(claim("phone", 80, 24, true, 0), 8_000);
        assert!(
            a.decide(9_000).is_none(),
            "the returning phone is older than the desk's interaction"
        );
        assert_eq!(a.driver.as_deref(), Some("desk"));
    }

    /// HS2-1YX5VS: a lone viewer drives even if it never interacted; a second viewer that never
    /// interacted ties with it and the current driver keeps the size, whatever the map order.
    #[test]
    fn a_lone_viewer_drives_and_ties_keep_the_current_driver() {
        for order in [["first", "second"], ["second", "first"]] {
            let mut a = SizeArbiter::default();
            a.upsert(claim(order[0], 100, 40, true, 0), 0);
            assert_eq!(
                a.decide(0).map(|d| d.driven_by),
                Some(Some(order[0].into()))
            );
            a.upsert(claim(order[1], 60, 20, true, 0), 1_000);
            assert!(a.decide(5_000).is_none(), "a tie keeps the driver");
            // The driver leaves: the remaining viewer drives at once.
            a.remove(order[0]);
            assert_eq!(
                a.decide(6_000).map(|d| d.driven_by),
                Some(Some(order[1].into()))
            );
        }
        // With no driver at all, the earliest attached of several passive viewers drives.
        let mut a = SizeArbiter::default();
        a.set_applied(80, 24);
        for id in ["v3", "v1", "v2"] {
            a.upsert(
                ViewportClaim {
                    visible: false,
                    ..claim(id, 100, 40, true, 0)
                },
                0,
            );
        }
        assert!(a.decide(0).is_none(), "nothing visible holds the size");
        for id in ["v2", "v1", "v3"] {
            a.upsert(claim(id, 100, 40, true, 0), 1_000);
        }
        assert_eq!(
            a.decide(1_000).map(|d| d.driven_by),
            Some(Some("v3".into()))
        );
    }

    /// HS2-1YX5VS: the departed-viewer memory is bounded, dropping the oldest interaction.
    #[test]
    fn the_departed_memory_is_bounded() {
        let mut a = SizeArbiter::default();
        let count = SIZE_DEPARTED_MEMORY as u64 + 10;
        for i in 0..count {
            let id = format!("v{i}");
            a.upsert(interacting_claim(&id, 80, 24, true, 0), 1 + i);
            a.remove(&id);
        }
        assert_eq!(a.departed.len(), SIZE_DEPARTED_MEMORY);
        assert!(!a.departed.contains_key("v0"), "the oldest was forgotten");
        assert!(a.departed.contains_key(&format!("v{}", count - 1)));
        // A viewer that never interacted leaves nothing behind.
        a.upsert(claim("passive", 80, 24, true, 0), 500);
        a.remove("passive");
        assert!(!a.departed.contains_key("passive"));
    }

    /// HS2-1YX5VS: the lease clock is the last claim of any kind, so a viewport that heartbeats
    /// but never interacts is not expired.
    #[test]
    fn expire_measures_the_last_claim_not_the_last_interaction() {
        let mut a = SizeArbiter::default();
        a.upsert(interacting_claim("busy", 80, 24, true, 0), 0);
        a.upsert(claim("quiet", 80, 24, true, 0), 0);
        a.upsert(claim("busy", 80, 24, true, 0), 4_000);
        assert_eq!(
            a.expire(5_000, 3_000),
            1,
            "only the silent viewport expires"
        );
        assert_eq!(a.viewer_count(), 1);
        assert!(a.claims.contains_key("busy"));
    }

    // HS2-0GZZHV: exactly one viewport answers the program's terminal queries.

    fn hidden(id: &str) -> ViewportClaim {
        ViewportClaim {
            visible: false,
            ..claim(id, 80, 24, false, 0)
        }
    }

    fn preview(id: &str) -> ViewportClaim {
        ViewportClaim {
            answers_queries: false,
            ..claim(id, 80, 24, true, 0)
        }
    }

    /// A lone viewport answers from its first claim, even when that claim resizes nothing.
    #[test]
    fn a_lone_viewport_answers_from_its_first_claim() {
        let mut a = SizeArbiter::default();
        a.set_applied(80, 24);
        assert_eq!(a.answerer(), None, "nobody attached");
        assert_eq!(a.answerer_notice(), None, "nothing to announce yet");
        a.upsert(claim("solo", 80, 24, true, 0), 1_000);
        assert!(a.decide(1_000).is_none(), "the spawn size already fits");
        let notice = a.answerer_notice().expect("the new answerer is announced");
        assert_eq!(
            (
                notice.cols,
                notice.rows,
                notice.driven_by,
                notice.answered_by.as_deref()
            ),
            (80, 24, None, Some("solo"))
        );
        // Heartbeats announce nothing more.
        a.upsert(claim("solo", 80, 24, true, 0), 6_000);
        assert!(a.decide(6_000).is_none());
        assert_eq!(a.answerer_notice(), None);
        assert_eq!(a.applied().unwrap().answered_by.as_deref(), Some("solo"));
    }

    /// The driver answers; a passive arrival changes nothing; a handoff moves the answerer with
    /// the size in the same frame, and back again.
    #[test]
    fn the_driver_answers_and_the_answerer_follows_a_handoff() {
        let mut a = desk_driving_with_a_passive_phone();
        assert_eq!(a.answerer().as_deref(), Some("desk"));
        assert_eq!(
            a.answerer_notice(),
            None,
            "the resize frame already named the desk"
        );
        a.upsert(interacting_claim("phone", 80, 24, true, 0), 3_000);
        assert!(a.decide(3_000).is_none(), "within the focus-hold");
        assert_eq!(
            a.answerer().as_deref(),
            Some("desk"),
            "the old driver answers until the handoff lands"
        );
        let takeover = a
            .decide(3_000 + SIZE_FOCUS_HOLD_MS)
            .expect("the phone takes over");
        assert_eq!(takeover.driven_by.as_deref(), Some("phone"));
        assert_eq!(takeover.answered_by.as_deref(), Some("phone"));
        assert_eq!(a.answerer_notice(), None);
        a.upsert(interacting_claim("desk", 200, 50, true, 0), 5_000);
        let back = a
            .decide(5_000 + SIZE_FOCUS_HOLD_MS)
            .expect("the desk takes it back");
        assert_eq!(
            (back.driven_by.as_deref(), back.answered_by.as_deref()),
            (Some("desk"), Some("desk"))
        );
        assert_eq!(a.answerer_notice(), None);
    }

    /// With no driver, the earliest-attached live viewport answers. When it leaves, the next
    /// earliest is announced; a viewport that reattaches goes to the back of the order; when
    /// the last one leaves, the frame names no answerer.
    #[test]
    fn an_undriven_terminal_falls_back_to_the_earliest_attached_viewport() {
        let mut a = SizeArbiter::default();
        a.set_applied(80, 24);
        for (at, id) in [(1_000, "first"), (2_000, "second"), (3_000, "third")] {
            a.upsert(hidden(id), at);
            assert!(a.decide(at).is_none(), "nothing eligible holds the size");
            let notice = a.answerer_notice();
            if id == "first" {
                assert_eq!(notice.unwrap().answered_by.as_deref(), Some("first"));
            } else {
                assert_eq!(notice, None, "a later arrival never takes the answer");
            }
        }
        assert_eq!(a.applied().unwrap().driven_by, None);
        a.remove("first");
        assert!(a.decide(4_000).is_none());
        assert_eq!(
            a.answerer_notice().unwrap().answered_by.as_deref(),
            Some("second")
        );
        // "second" reattaches under its id: it is now the newest attach, so "third" answers.
        a.remove("second");
        a.upsert(hidden("second"), 5_000);
        assert!(a.decide(5_000).is_none());
        assert_eq!(
            a.answerer_notice().unwrap().answered_by.as_deref(),
            Some("third")
        );
        assert_eq!(a.answerer_notice(), None, "announced once");
        a.remove("third");
        assert_eq!(
            a.answerer_notice().unwrap().answered_by.as_deref(),
            Some("second")
        );
        a.remove("second");
        let empty = a
            .answerer_notice()
            .expect("the empty answerer is announced");
        assert_eq!((empty.cols, empty.rows, empty.answered_by), (80, 24, None));
    }

    /// The driver leaving when its survivor's grid matches resizes nothing, yet the survivor
    /// still learns at once that it now answers.
    #[test]
    fn a_driver_leaving_without_a_resize_still_hands_over_the_answer() {
        let mut a = SizeArbiter::default();
        a.upsert(interacting_claim("desk", 120, 40, true, 0), 1_000);
        assert!(a.decide(1_000).is_some());
        a.upsert(claim("phone", 120, 40, true, 0), 2_000);
        assert!(a.decide(2_000).is_none());
        assert_eq!(a.answerer_notice(), None);
        a.remove("desk");
        assert!(a.decide(3_000).is_none(), "the same size is not a resize");
        let notice = a.answerer_notice().expect("the survivor is announced");
        assert_eq!(notice.answered_by.as_deref(), Some("phone"));
    }

    /// A viewport that cannot answer (a read-only preview) is never chosen, even as the
    /// earliest attach or as the driver under another policy.
    #[test]
    fn a_viewport_that_cannot_answer_is_never_the_answerer() {
        let mut a = SizeArbiter::new(SizePolicy::Smallest);
        a.set_applied(80, 24);
        a.upsert(
            ViewportClaim {
                cols: 40,
                rows: 12,
                ..preview("preview")
            },
            1_000,
        );
        let only_preview = a.decide(1_000).expect("the smallest viewport drives");
        assert_eq!(only_preview.driven_by.as_deref(), Some("preview"));
        assert_eq!(only_preview.answered_by, None, "no viewport can answer");
        a.upsert(claim("tile", 80, 24, true, 0), 2_000);
        assert!(
            a.decide(2_000).is_none(),
            "the preview is still the smallest"
        );
        assert_eq!(
            a.answerer_notice().unwrap().answered_by.as_deref(),
            Some("tile")
        );
        assert_eq!(a.applied().unwrap().driven_by.as_deref(), Some("preview"));
    }
}
