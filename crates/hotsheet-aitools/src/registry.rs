//! The **connection registry** + **busy tracking** (`docs/05` §5.6). The host tracks
//! every live AI-tool connection so the API can list them ("N working, M idle") and
//! `run`'s [`Target`](crate::Target) can pick one.
//!
//! Busy is a **derived view**, not a per-tool API (`docs/13` §13.4): every signal — a
//! tool's lifecycle/hooks heartbeat *or* byte-stream spinner inference — feeds one
//! `note_activity`, and `is_busy` is "activity within the sliding window." A `Done`
//! signal calls `set_idle` to drop it immediately. The clock is injected (`now_ms`
//! passed in) so it's deterministic.

use std::collections::HashMap;

use crate::drive::Transport;

/// What a connection is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// The project's main driven session.
    Main,
    /// A git-worktree self-claim worker.
    Worker,
    /// A one-shot process a drive spawned for a single turn.
    DriveSpawned,
}

/// A live AI-tool connection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connection {
    pub id: String,
    /// The project/store this connection serves.
    pub project: String,
    /// The plugin id (e.g. `"claude"`, `"codex"`).
    pub tool: String,
    pub role: Role,
    pub transport: Transport,
    pub pid: Option<u32>,
    pub started_at_ms: u64,
}

struct Entry {
    conn: Connection,
    /// Last heartbeat; `None` = never active (idle).
    last_activity_ms: Option<u64>,
}

/// Tracks live connections + their busy state.
pub struct ConnectionRegistry {
    entries: HashMap<String, Entry>,
    busy_window_ms: u64,
}

impl ConnectionRegistry {
    /// A registry whose connections count as busy for `busy_window_ms` after their last
    /// heartbeat.
    pub fn new(busy_window_ms: u64) -> Self {
        Self {
            entries: HashMap::new(),
            busy_window_ms,
        }
    }

    /// Register (or replace) a connection; returns its id.
    pub fn register(&mut self, conn: Connection) -> String {
        let id = conn.id.clone();
        self.entries.insert(
            id.clone(),
            Entry {
                conn,
                last_activity_ms: None,
            },
        );
        id
    }

    /// Remove a connection; returns whether it was present.
    pub fn unregister(&mut self, id: &str) -> bool {
        self.entries.remove(id).is_some()
    }

    pub fn get(&self, id: &str) -> Option<&Connection> {
        self.entries.get(id).map(|e| &e.conn)
    }

    /// All connections (unordered).
    pub fn list(&self) -> Vec<&Connection> {
        self.entries.values().map(|e| &e.conn).collect()
    }

    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Heartbeat — a busy signal (a tool hook, or byte-stream/spinner activity) at
    /// `now_ms`. No-op for an unknown id.
    pub fn note_activity(&mut self, id: &str, now_ms: u64) {
        if let Some(e) = self.entries.get_mut(id) {
            e.last_activity_ms = Some(now_ms);
        }
    }

    /// Force a connection idle immediately (e.g. on a turn's `Done`).
    pub fn set_idle(&mut self, id: &str) {
        if let Some(e) = self.entries.get_mut(id) {
            e.last_activity_ms = None;
        }
    }

    /// Whether a connection had activity within the busy window as of `now_ms`.
    pub fn is_busy(&self, id: &str, now_ms: u64) -> bool {
        self.entries
            .get(id)
            .and_then(|e| e.last_activity_ms)
            .is_some_and(|t| now_ms.saturating_sub(t) < self.busy_window_ms)
    }

    /// How many connections are busy as of `now_ms`.
    pub fn busy_count(&self, now_ms: u64) -> usize {
        self.entries
            .keys()
            .filter(|id| self.is_busy(id, now_ms))
            .count()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conn(id: &str) -> Connection {
        Connection {
            id: id.to_string(),
            project: "/proj".into(),
            tool: "claude".into(),
            role: Role::Main,
            transport: Transport::ClaudeChannel,
            pid: Some(42),
            started_at_ms: 1_000,
        }
    }

    #[test]
    fn register_list_get_unregister() {
        let mut r = ConnectionRegistry::new(5_000);
        r.register(conn("a"));
        r.register(conn("b"));
        assert_eq!(r.count(), 2);
        assert_eq!(r.get("a").unwrap().tool, "claude");
        let ids: Vec<&str> = r.list().iter().map(|c| c.id.as_str()).collect();
        assert!(ids.contains(&"a") && ids.contains(&"b"));
        assert!(r.unregister("a"));
        assert!(!r.unregister("a"), "second unregister is a no-op");
        assert_eq!(r.count(), 1);
        assert!(r.get("a").is_none());
    }

    #[test]
    fn busy_follows_the_sliding_window() {
        let mut r = ConnectionRegistry::new(5_000);
        r.register(conn("a"));
        // never active → idle
        assert!(!r.is_busy("a", 10_000));

        r.note_activity("a", 10_000);
        assert!(r.is_busy("a", 10_000), "0 elapsed < window");
        assert!(r.is_busy("a", 14_999), "just inside the window");
        assert!(!r.is_busy("a", 15_000), "window boundary is not busy");
        assert!(!r.is_busy("a", 20_000), "well past → idle");

        // a fresh heartbeat re-extends
        r.note_activity("a", 20_000);
        assert!(r.is_busy("a", 24_000));
    }

    #[test]
    fn set_idle_drops_busy_immediately() {
        let mut r = ConnectionRegistry::new(5_000);
        r.register(conn("a"));
        r.note_activity("a", 10_000);
        assert!(r.is_busy("a", 10_100));
        r.set_idle("a"); // e.g. a Done signal
        assert!(!r.is_busy("a", 10_100));
    }

    #[test]
    fn busy_count_across_connections() {
        let mut r = ConnectionRegistry::new(5_000);
        r.register(conn("a"));
        r.register(conn("b"));
        r.register(conn("c"));
        r.note_activity("a", 10_000);
        r.note_activity("b", 8_000); // 2s ago at now=10_000
        // c never active
        assert_eq!(r.busy_count(10_000), 2);
        assert_eq!(r.busy_count(14_000), 1); // b aged out (6s), a still in (4s)
        assert_eq!(r.busy_count(20_000), 0);
    }

    #[test]
    fn activity_on_unknown_id_is_ignored() {
        let mut r = ConnectionRegistry::new(5_000);
        r.note_activity("ghost", 10_000);
        assert!(!r.is_busy("ghost", 10_000));
        assert_eq!(r.count(), 0);
    }
}
