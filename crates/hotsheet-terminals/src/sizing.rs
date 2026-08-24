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
//! most-recently-focused visible viewport, with a short **focus-hold** so a rapid focus flip
//! doesn't thrash the size. Everything here is a **pure state machine with an injected clock**
//! (`now_ms` passed in), so it gets transition-matrix + adversarial-sequence tests directly.

use std::collections::HashMap;

/// Hold the current size for this long after a focus change before following the new
/// viewport — debounces a rapid focus flip (`docs/06` §6.7 `SIZE_FOCUS_HOLD_MS`).
pub const SIZE_FOCUS_HOLD_MS: u64 = 500;
/// Ignore a resize smaller than this many rows/cols of delta (avoid churn on ±1).
pub const SIZE_MIN_DELTA: u16 = 2;
/// Don't resize the PTY more often than this (`SIZE_RESIZE_MIN_INTERVAL_MS`).
pub const SIZE_RESIZE_MIN_INTERVAL_MS: u64 = 750;

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
    /// When this viewport last saw user activity (focus/typing) — the recency tiebreak.
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
}

/// Arbitrates a PTY's size across many viewport claims.
#[derive(Debug)]
pub struct SizeArbiter {
    policy: SizePolicy,
    claims: HashMap<String, ViewportClaim>,
    /// The last size actually applied to the PTY (guards min-delta + min-interval).
    applied: Option<(u16, u16)>,
    applied_at_ms: u64,
    /// The viewport currently driving under focus-follows.
    driver: Option<String>,
    /// When focus last moved to a viewport other than the driver — a different candidate must
    /// stay focused for `SIZE_FOCUS_HOLD_MS` past this before it takes over (debounces flips).
    focus_changed_at_ms: u64,
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
            applied: None,
            applied_at_ms: 0,
            driver: None,
            focus_changed_at_ms: 0,
        }
    }

    pub fn set_policy(&mut self, policy: SizePolicy) {
        self.policy = policy;
    }

    /// Seed the "already applied" size (e.g. the terminal's spawn size), so the min-delta
    /// guard has a baseline and "nothing focused" holds something sensible.
    pub fn set_applied(&mut self, cols: u16, rows: u16) {
        self.applied = Some((cols, rows));
    }

    pub fn viewer_count(&self) -> usize {
        self.claims.len()
    }

    /// Add or update a viewport's claim. When a viewport *newly* takes focus and isn't already
    /// the driver, that starts the focus-hold clock so a rapid flip doesn't switch the driver.
    pub fn upsert(&mut self, claim: ViewportClaim, now_ms: u64) {
        let newly_focused = claim.focus
            && self.driver.as_deref() != Some(claim.viewer_id.as_str())
            && self
                .claims
                .get(&claim.viewer_id)
                .is_none_or(|prev| !prev.focus);
        if newly_focused {
            self.focus_changed_at_ms = now_ms;
        }
        self.claims.insert(claim.viewer_id.clone(), claim);
    }

    /// Drop a viewport (it disconnected). If it was the driver, the size recomputes
    /// immediately on the next `decide` (self-heal — no hold on a disconnect).
    pub fn remove(&mut self, viewer_id: &str) {
        self.claims.remove(viewer_id);
        if self.driver.as_deref() == Some(viewer_id) {
            self.driver = None;
        }
    }

    /// Drop viewports whose last claim is older than `lease_ms` (missed heartbeats). Returns
    /// how many were expired.
    pub fn expire(&mut self, now_ms: u64, lease_ms: u64) -> usize {
        let stale: Vec<String> = self
            .claims
            .iter()
            .filter(|(_, c)| now_ms.saturating_sub(c.activity_at_ms) > lease_ms)
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
        let target = self.target(now_ms)?;

        // Guard: don't resize too often.
        if self.applied.is_some()
            && now_ms.saturating_sub(self.applied_at_ms) < SIZE_RESIZE_MIN_INTERVAL_MS
        {
            return None;
        }
        // Guard: ignore a sub-threshold change from what's already applied.
        if let Some((c, r)) = self.applied {
            let d_cols = c.abs_diff(target.cols);
            let d_rows = r.abs_diff(target.rows);
            if d_cols < SIZE_MIN_DELTA && d_rows < SIZE_MIN_DELTA {
                return None;
            }
        }
        self.applied = Some((target.cols, target.rows));
        self.applied_at_ms = now_ms;
        Some(target)
    }

    /// The size the policy wants right now (ignoring the resize-rate guards). `None` = no
    /// applicable viewport (hold whatever's applied).
    fn target(&mut self, now_ms: u64) -> Option<Decision> {
        match &self.policy {
            SizePolicy::Pinned { cols, rows } => Some(Decision {
                cols: *cols,
                rows: *rows,
                driven_by: None,
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
        })
    }

    /// Focus-follows: the most-recently-focused visible viewport drives. A *different* live
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
        }
        // else: within the hold window — keep the current driver (hold its size).

        let d = self.driver.clone().unwrap_or(cand);
        self.claims.get(&d).map(|c| Decision {
            cols: c.cols,
            rows: c.rows,
            driven_by: Some(c.viewer_id.clone()),
        })
    }

    fn latest_focused(&self) -> Option<&ViewportClaim> {
        self.claims
            .values()
            .filter(|c| c.focus && c.visible)
            .max_by_key(|c| c.activity_at_ms)
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
            activity_at_ms: at,
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

        // A change within the min-interval is suppressed.
        a.upsert(claim("v1", 120, 50, true, 100), 100);
        assert!(a.decide(100).is_none(), "too soon after the last resize");

        // After the interval, it applies.
        a.upsert(claim("v1", 120, 50, true, 1000), 1000);
        assert_eq!(a.decide(1000).map(|d| (d.cols, d.rows)), Some((120, 50)));

        // A sub-min-delta change (±1) is ignored even after the interval.
        a.upsert(claim("v1", 121, 50, true, 2000), 2000);
        assert!(a.decide(2000).is_none(), "±1 is below SIZE_MIN_DELTA");
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
        // v2 focused most recently at t=0; promote it past the hold.
        a.upsert(claim("v2", 80, 24, true, 600), 600);
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
}
