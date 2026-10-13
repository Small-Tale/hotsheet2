//! Live `hotsheet-mcp` presence (HS2-HNXE38).
//!
//! A `hotsheet-mcp` process that knows its project's server registers a lease on
//! `initialize`, renews it every [`HEARTBEAT`], and drops it when its stdin closes. The server
//! keeps each lease for [`LEASE_TTL`] (three heartbeats) after its last renewal and drops it
//! early when its process is gone, so a crashed shim stops counting without a goodbye. A lease
//! renewed after a server restart is simply registered again.
//!
//! Presence is scoped: the project is MCP-connected while any lease is live, and a terminal is
//! connected while a lease names it (`HOTSHEET_TERMINAL_ID`). Only changes of those connected
//! states are announced, so heartbeats never generate change-stream traffic. This is separate
//! from the hook-reported terminal AI connection, which means permission prompts are routed.

use std::collections::{BTreeSet, HashMap};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// How often a shim renews its lease.
pub const HEARTBEAT: Duration = Duration::from_secs(15);
/// How long a lease outlives its last renewal.
pub const LEASE_TTL: Duration = Duration::from_secs(45);

/// What a shim reports about itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpPresenceInfo {
    pub pid: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_id: Option<String>,
}

#[derive(Debug, Clone)]
struct Lease {
    info: McpPresenceInfo,
    expires: Instant,
}

/// A scope whose connected state can change.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Scope {
    Project,
    Terminal(String),
}

/// One scope that became connected or disconnected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transition {
    pub scope: Scope,
    pub connected: bool,
}

/// The live leases (pure: time and process liveness are passed in).
#[derive(Debug, Default, Clone)]
pub struct McpPresenceRegistry {
    leases: HashMap<String, Lease>,
}

impl McpPresenceRegistry {
    fn scopes(&self) -> BTreeSet<Scope> {
        let mut scopes = BTreeSet::new();
        for lease in self.leases.values() {
            scopes.insert(Scope::Project);
            if let Some(terminal) = &lease.info.terminal_id {
                scopes.insert(Scope::Terminal(terminal.clone()));
            }
        }
        scopes
    }

    fn diff(before: &BTreeSet<Scope>, after: &BTreeSet<Scope>) -> Vec<Transition> {
        let mut out: Vec<Transition> = before
            .difference(after)
            .map(|scope| Transition {
                scope: scope.clone(),
                connected: false,
            })
            .collect();
        out.extend(after.difference(before).map(|scope| Transition {
            scope: scope.clone(),
            connected: true,
        }));
        out
    }

    fn mutate(&mut self, change: impl FnOnce(&mut HashMap<String, Lease>)) -> Vec<Transition> {
        let before = self.scopes();
        change(&mut self.leases);
        Self::diff(&before, &self.scopes())
    }

    /// Register or renew `lease`; a renewal may also move it to a different terminal.
    pub fn upsert(&mut self, lease: &str, info: McpPresenceInfo, now: Instant) -> Vec<Transition> {
        self.mutate(|leases| {
            leases.insert(
                lease.to_owned(),
                Lease {
                    info,
                    expires: now + LEASE_TTL,
                },
            );
        })
    }

    /// Drop `lease`; dropping an unknown lease changes nothing.
    pub fn remove(&mut self, lease: &str) -> Vec<Transition> {
        self.mutate(|leases| {
            leases.remove(lease);
        })
    }

    /// Drop every lease that expired at `now` or whose process `alive` reports gone.
    pub fn sweep(&mut self, now: Instant, alive: impl Fn(u32) -> bool) -> Vec<Transition> {
        self.mutate(|leases| {
            leases.retain(|_, lease| lease.expires > now && alive(lease.info.pid));
        })
    }

    pub fn contains(&self, lease: &str) -> bool {
        self.leases.contains_key(lease)
    }

    /// When `lease` expires, if it is live.
    pub fn expires(&self, lease: &str) -> Option<Instant> {
        self.leases.get(lease).map(|lease| lease.expires)
    }

    /// Whether any lease names `terminal`.
    pub fn terminal_connected(&self, terminal: &str) -> bool {
        self.leases
            .values()
            .any(|lease| lease.info.terminal_id.as_deref() == Some(terminal))
    }

    /// The live leases, sorted by lease id.
    pub fn list(&self) -> Vec<(String, McpPresenceInfo)> {
        let mut out: Vec<_> = self
            .leases
            .iter()
            .map(|(id, lease)| (id.clone(), lease.info.clone()))
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(pid: u32, terminal: Option<&str>) -> McpPresenceInfo {
        McpPresenceInfo {
            pid,
            tool: Some("claude".into()),
            cwd: None,
            terminal_id: terminal.map(str::to_owned),
        }
    }
    fn up(scope: Scope) -> Transition {
        Transition {
            scope,
            connected: true,
        }
    }
    fn down(scope: Scope) -> Transition {
        Transition {
            scope,
            connected: false,
        }
    }
    fn term(id: &str) -> Scope {
        Scope::Terminal(id.into())
    }
    const ALIVE: fn(u32) -> bool = |_| true;

    #[test]
    fn first_register_connects_project_and_terminal_and_renewals_are_silent() {
        let now = Instant::now();
        let mut reg = McpPresenceRegistry::default();
        assert_eq!(
            reg.upsert("a", info(1, Some("t1")), now),
            vec![up(Scope::Project), up(term("t1"))]
        );
        assert!(
            reg.upsert("a", info(1, Some("t1")), now + HEARTBEAT)
                .is_empty()
        );
        assert!(reg.terminal_connected("t1"));
        assert_eq!(reg.expires("a"), Some(now + HEARTBEAT + LEASE_TTL));
    }

    #[test]
    fn transition_matrix_over_two_leases_sharing_a_terminal() {
        let now = Instant::now();
        let mut reg = McpPresenceRegistry::default();
        // none -> one lease without a terminal
        assert_eq!(
            reg.upsert("a", info(1, None), now),
            vec![up(Scope::Project)]
        );
        // one -> two, the second in a terminal: only the terminal changes
        assert_eq!(
            reg.upsert("b", info(2, Some("t1")), now),
            vec![up(term("t1"))]
        );
        // renewal moves b to another terminal
        assert_eq!(
            reg.upsert("b", info(2, Some("t2")), now),
            vec![down(term("t1")), up(term("t2"))]
        );
        // a third lease joins t2: nothing changes
        assert!(reg.upsert("c", info(3, Some("t2")), now).is_empty());
        // removing one of two in t2: nothing changes
        assert!(reg.remove("b").is_empty());
        // removing an unknown lease, and removing twice: nothing changes
        assert!(reg.remove("missing").is_empty());
        assert!(reg.remove("b").is_empty());
        assert_eq!(reg.remove("c"), vec![down(term("t2"))]);
        assert_eq!(reg.remove("a"), vec![down(Scope::Project)]);
        // empty then refill
        assert_eq!(
            reg.upsert("a", info(1, Some("t1")), now),
            vec![up(Scope::Project), up(term("t1"))]
        );
    }

    #[test]
    fn sweep_expires_unrenewed_leases_and_dead_processes() {
        let now = Instant::now();
        let mut reg = McpPresenceRegistry::default();
        reg.upsert("a", info(1, Some("t1")), now);
        reg.upsert("b", info(2, None), now);
        // Before the TTL nothing lapses.
        assert!(
            reg.sweep(now + LEASE_TTL - Duration::from_secs(1), ALIVE)
                .is_empty()
        );
        // b renews; a's process dies.
        reg.upsert("b", info(2, None), now + HEARTBEAT);
        assert_eq!(
            reg.sweep(now + HEARTBEAT, |pid| pid != 1),
            vec![down(term("t1"))]
        );
        assert!(!reg.contains("a"));
        // b stops renewing and lapses exactly at its TTL.
        assert_eq!(
            reg.sweep(now + HEARTBEAT + LEASE_TTL, ALIVE),
            vec![down(Scope::Project)]
        );
        assert!(reg.list().is_empty());
        // A renewal after a lapse (server forgot it) is a fresh connect.
        assert_eq!(
            reg.upsert("b", info(2, None), now + LEASE_TTL * 3),
            vec![up(Scope::Project)]
        );
    }

    #[test]
    fn interleaved_renew_and_sweep_never_drops_a_renewed_lease() {
        let start = Instant::now();
        let mut reg = McpPresenceRegistry::default();
        reg.upsert("a", info(1, Some("t1")), start);
        for beat in 1..20u32 {
            let at = start + HEARTBEAT * beat;
            assert!(reg.sweep(at, ALIVE).is_empty(), "beat {beat}");
            assert!(reg.upsert("a", info(1, Some("t1")), at).is_empty());
        }
        assert_eq!(reg.list().len(), 1);
    }
}
