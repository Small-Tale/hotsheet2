//! The **terminal manager** (`docs/05` §5.4): per-project PTYs keyed by `(project,
//! terminalId)`, spawned **lazily**, shareable across many viewers (each `Arc<Terminal>` is
//! one stream), with list / get / kill. The detached broker that lets terminals **survive a
//! server restart** is a follow-on (it wraps this manager in a separate process); this is
//! the in-process host.

use hotsheet_sync::LockExt;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::terminal::{TermError, TermSpec, Terminal};

/// A terminal's identity within the host: `(project id, terminal id)`.
pub type TermKey = (String, String);

/// Owns the live terminals for a host.
#[derive(Default)]
pub struct TerminalManager {
    terminals: Mutex<HashMap<TermKey, Arc<Terminal>>>,
}

impl TerminalManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// The terminal for `key`, spawning it from `spec` if it doesn't exist yet (lazy) or the
    /// previous one has exited. An already-live terminal is returned as-is (its `spec` arg is
    /// ignored) so many viewers share one stream.
    pub fn get_or_spawn(&self, key: TermKey, spec: TermSpec) -> Result<Arc<Terminal>, TermError> {
        self.get_or_spawn_with_status(key, spec)
            .map(|(term, _)| term)
    }

    /// Open a terminal and report whether this call spawned a new PTY. The decision and
    /// replacement happen under one lock, so callers do not have to race a pre-check.
    pub fn get_or_spawn_with_status(
        &self,
        key: TermKey,
        spec: TermSpec,
    ) -> Result<(Arc<Terminal>, bool), TermError> {
        let mut map = self.terminals.lock_or_recover();
        if let Some(t) = map.get(&key) {
            if t.is_alive() {
                return Ok((t.clone(), false));
            }
        }
        let term = Arc::new(Terminal::spawn(spec)?);
        map.insert(key, term.clone());
        Ok((term, true))
    }

    /// The live terminal for `key`, if any.
    pub fn get(&self, key: &TermKey) -> Option<Arc<Terminal>> {
        self.terminals.lock_or_recover().get(key).cloned()
    }

    /// The keys of all tracked terminals, sorted.
    pub fn list(&self) -> Vec<TermKey> {
        let mut keys: Vec<TermKey> = self.terminals.with_lock(|m| m.keys().cloned().collect());
        keys.sort();
        keys
    }

    /// How many terminals are tracked.
    pub fn count(&self) -> usize {
        self.terminals.with_lock(|m| m.len())
    }

    /// Kill and forget the terminal for `key`. Returns whether one was present.
    pub fn kill(&self, key: &TermKey) -> Result<bool, TermError> {
        let removed = self.terminals.lock_or_recover().remove(key);
        match removed {
            Some(t) => {
                let _ = t.kill(); // best-effort; a dead child's kill is harmless
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// Drop terminals whose child has exited (a housekeeping sweep). Returns how many were
    /// reaped.
    pub fn reap(&self) -> usize {
        let mut map = self.terminals.lock_or_recover();
        let before = map.len();
        map.retain(|_, t| t.is_alive());
        before - map.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TerminalKind;
    use std::time::{Duration, Instant};

    fn wait_until(mut cond: impl FnMut() -> bool, secs: u64) -> bool {
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        cond()
    }

    fn key(p: &str, t: &str) -> TermKey {
        (p.to_string(), t.to_string())
    }

    #[test]
    fn lazy_spawn_shares_a_live_terminal_and_kill_removes_it() {
        let mgr = TerminalManager::new();
        let k = key("proj", "main");

        // First get spawns; a second get returns the SAME live terminal (shared stream).
        let a = mgr.get_or_spawn(k.clone(), TermSpec::new("cat")).unwrap();
        let b = mgr.get_or_spawn(k.clone(), TermSpec::new("cat")).unwrap();
        assert!(
            Arc::ptr_eq(&a, &b),
            "a live terminal is reused, not re-spawned"
        );
        assert_eq!(mgr.count(), 1);
        assert_eq!(mgr.list(), vec![k.clone()]);

        // Kill removes it.
        assert!(mgr.kill(&k).unwrap());
        assert!(mgr.get(&k).is_none());
        assert!(!mgr.kill(&k).unwrap(), "already gone");
    }

    #[test]
    fn creation_kind_survives_reattachment_but_a_new_terminal_gets_its_own_kind() {
        let mgr = TerminalManager::new();
        let k = key("proj", "kind");
        let shell = mgr.get_or_spawn(k.clone(), TermSpec::new("cat")).unwrap();
        assert_eq!(shell.kind(), TerminalKind::Shell);
        let mut ai_spec = TermSpec::new("cat");
        ai_spec.kind = TerminalKind::Ai;
        let reattached = mgr.get_or_spawn(k.clone(), ai_spec.clone()).unwrap();
        assert!(Arc::ptr_eq(&shell, &reattached));
        assert_eq!(reattached.kind(), TerminalKind::Shell);

        mgr.kill(&k).unwrap();
        let ai = mgr.get_or_spawn(k.clone(), ai_spec).unwrap();
        assert_eq!(ai.kind(), TerminalKind::Ai);
        let reattached = mgr.get_or_spawn(k.clone(), TermSpec::new("cat")).unwrap();
        assert!(Arc::ptr_eq(&ai, &reattached));
        assert_eq!(reattached.kind(), TerminalKind::Ai);
        mgr.kill(&k).unwrap();
        let replacement = mgr.get_or_spawn(k.clone(), TermSpec::new("cat")).unwrap();
        assert_eq!(replacement.kind(), TerminalKind::Shell);
        mgr.kill(&k).unwrap();
    }

    #[test]
    fn open_reports_spawn_for_a_tracked_terminal_that_exited() {
        let mgr = TerminalManager::new();
        let k = key("proj", "exited");
        let mut short = TermSpec::new("printf");
        short.args = vec!["done".into()];
        let (first, spawned) = mgr.get_or_spawn_with_status(k.clone(), short).unwrap();
        assert!(spawned);
        assert!(wait_until(|| !first.is_alive(), 5));
        assert!(mgr.get(&k).is_some(), "the dead PTY remains tracked");

        let (replacement, spawned) = mgr
            .get_or_spawn_with_status(k.clone(), TermSpec::new("cat"))
            .unwrap();
        assert!(spawned);
        assert!(!Arc::ptr_eq(&first, &replacement));
        let (reattached, spawned) = mgr
            .get_or_spawn_with_status(k.clone(), TermSpec::new("cat"))
            .unwrap();
        assert!(!spawned);
        assert!(Arc::ptr_eq(&replacement, &reattached));
        mgr.kill(&k).unwrap();
    }

    #[test]
    fn reap_drops_exited_terminals() {
        let mgr = TerminalManager::new();
        // A short-lived child that exits on its own.
        let mut spec = TermSpec::new("printf");
        spec.args = vec!["x".into()];
        mgr.get_or_spawn(key("p", "t"), spec).unwrap();
        assert!(
            wait_until(|| mgr.reap() == 1, 5),
            "the exited terminal is reaped"
        );
        assert_eq!(mgr.count(), 0);
    }
}
