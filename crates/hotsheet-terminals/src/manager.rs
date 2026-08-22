//! The **terminal manager** (`docs/05` §5.4): per-project PTYs keyed by `(project,
//! terminalId)`, spawned **lazily**, shareable across many viewers (each `Arc<Terminal>` is
//! one stream), with list / get / kill. The detached broker that lets terminals **survive a
//! server restart** is a follow-on (it wraps this manager in a separate process); this is
//! the in-process host.

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
        let mut map = self.terminals.lock().map_err(|_| poisoned())?;
        if let Some(t) = map.get(&key) {
            if t.is_alive() {
                return Ok(t.clone());
            }
        }
        let term = Arc::new(Terminal::spawn(spec)?);
        map.insert(key, term.clone());
        Ok(term)
    }

    /// The live terminal for `key`, if any.
    pub fn get(&self, key: &TermKey) -> Option<Arc<Terminal>> {
        self.terminals.lock().ok()?.get(key).cloned()
    }

    /// The keys of all tracked terminals, sorted.
    pub fn list(&self) -> Vec<TermKey> {
        let mut keys: Vec<TermKey> = self
            .terminals
            .lock()
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default();
        keys.sort();
        keys
    }

    /// How many terminals are tracked.
    pub fn count(&self) -> usize {
        self.terminals.lock().map(|m| m.len()).unwrap_or(0)
    }

    /// Kill and forget the terminal for `key`. Returns whether one was present.
    pub fn kill(&self, key: &TermKey) -> Result<bool, TermError> {
        let removed = self.terminals.lock().map_err(|_| poisoned())?.remove(key);
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
        let Ok(mut map) = self.terminals.lock() else {
            return 0;
        };
        let before = map.len();
        map.retain(|_, t| t.is_alive());
        before - map.len()
    }
}

fn poisoned() -> TermError {
    TermError::Pty("terminal manager lock poisoned".into())
}

#[cfg(test)]
mod tests {
    use super::*;
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
