//! The **host-side permission bridge** (`docs/05` §5.7, HS2-11) — the generic "the tool
//! needs a human decision" channel. A tool's per-transport adapter (an ACP option
//! response, a PreToolUse hook CLI, a `hooks.json` entry) routes a request here; the
//! bridge:
//!
//! 1. checks **allow-rules** (allow-once/session/always) and **auto-resolves** a request
//!    a rule already answers;
//! 2. otherwise **enqueues** it FIFO and hands back a stable id. Concurrent requests are
//!    *preserved, never overwritten* — the HS1 bug this fixes (`docs/12` §12.10): two
//!    tools asking at once both wait, and answering one never drops the other;
//! 3. **resolves** a pending request by id (so out-of-order answers are fine), returning
//!    the raising connection + decision so the caller can route the answer back, and
//!    persisting a rule when the answer's scope says to remember it.
//!
//! This is the **pure core**: no I/O, no globals. The WebSocket push to clients, the
//! per-plugin transport adapters, and durable rule storage are the server/plugin wiring
//! on top (HS2-113 / HS2-0QGW07). Kept pure so it gets transition-matrix + adversarial
//! sequence tests directly.

use std::collections::{HashMap, VecDeque};
use std::sync::{Condvar, Mutex};

/// An allow/deny decision on a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Allow,
    Deny,
}

/// How long an answer is remembered — maps a UI's allow-once / allow-for-session /
/// always onto persisted allow-rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// This request only — no rule recorded.
    Once,
    /// Remembered for this bridge's lifetime (a "session").
    Session,
    /// Remembered durably (the caller persists the returned rule).
    Always,
}

/// A pending permission request the host must answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    /// Stable per-bridge id, used to resolve out of order.
    pub id: u64,
    /// The connection that raised it — the route-back target for the answer.
    pub connection: String,
    /// The tool/action asking (e.g. `"Bash"`, `"Edit"`).
    pub tool: String,
    /// What it wants to do (the command / capability / path) — the rule-match key.
    pub action: String,
}

/// A persisted allow-rule: auto-answer any request matching `(tool, action)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rule {
    pub tool: String,
    pub action: String,
    pub decision: Decision,
    /// `true` for an `Always` rule the caller should persist; `false` for a `Session`
    /// rule that lives only for this bridge's lifetime.
    pub persist: bool,
}

impl Rule {
    fn matches(&self, tool: &str, action: &str) -> bool {
        self.tool == tool && self.action == action
    }
}

/// The outcome of raising a request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A rule already answered it — no human needed. Carries the decision.
    Auto(Decision),
    /// Enqueued and awaiting a human; carries the assigned request id.
    Pending(u64),
}

/// A resolved request: who to route the answer back to, and the decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resolved {
    pub connection: String,
    pub decision: Decision,
    /// A rule to persist, when the answer's scope was `Always`. `None` otherwise.
    pub persisted_rule: Option<Rule>,
}

/// The permission bridge: a FIFO queue of pending requests + the active allow-rules.
#[derive(Debug, Default)]
pub struct PermissionBridge {
    next_id: u64,
    queue: VecDeque<Request>,
    rules: Vec<Rule>,
}

impl PermissionBridge {
    /// A fresh bridge with no rules.
    pub fn new() -> Self {
        Self::default()
    }

    /// Seed durable allow-rules loaded from storage (so a remembered "always" answer
    /// auto-resolves on the next run without re-asking).
    pub fn with_rules(rules: Vec<Rule>) -> Self {
        Self {
            next_id: 0,
            queue: VecDeque::new(),
            rules,
        }
    }

    /// Raise a permission request. If an existing rule answers `(tool, action)` it
    /// auto-resolves; otherwise the request is enqueued FIFO and its id returned.
    pub fn request(
        &mut self,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
    ) -> Outcome {
        let tool = tool.into();
        let action = action.into();
        if let Some(rule) = self.rules.iter().find(|r| r.matches(&tool, &action)) {
            return Outcome::Auto(rule.decision);
        }
        self.next_id += 1;
        let id = self.next_id;
        self.queue.push_back(Request {
            id,
            connection: connection.into(),
            tool,
            action,
        });
        Outcome::Pending(id)
    }

    /// The pending requests, oldest first (FIFO order — what the UI renders).
    pub fn pending(&self) -> impl Iterator<Item = &Request> {
        self.queue.iter()
    }

    /// The number of requests still awaiting an answer.
    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// The active allow-rules.
    pub fn rules(&self) -> &[Rule] {
        &self.rules
    }

    /// Answer a pending request by id. Removes it from the queue (position-independent,
    /// so answering the middle of the queue leaves the rest intact), records a
    /// session/always rule per `scope`, and returns who to route the answer back to.
    /// `None` if no pending request has that id (already answered / never existed).
    pub fn resolve(&mut self, id: u64, decision: Decision, scope: Scope) -> Option<Resolved> {
        let pos = self.queue.iter().position(|r| r.id == id)?;
        let req = self.queue.remove(pos).expect("position just found");

        let mut persisted_rule = None;
        if scope != Scope::Once {
            let rule = Rule {
                tool: req.tool.clone(),
                action: req.action.clone(),
                decision,
                persist: scope == Scope::Always,
            };
            // A later rule for the same key supersedes an earlier one.
            self.rules.retain(|r| !r.matches(&req.tool, &req.action));
            if scope == Scope::Always {
                persisted_rule = Some(rule.clone());
            }
            self.rules.push(rule);
        }

        Some(Resolved {
            connection: req.connection,
            decision,
            persisted_rule,
        })
    }
}

/// A thread-safe [`PermissionBridge`] that supports the **live human round-trip**
/// (`docs/05` §5.7, HS2-9R9YZW): a tool thread calls [`request_blocking`] and **blocks**
/// until a person answers over a separate path (an HTTP route-back on the server that calls
/// [`resolve`]). An allow-rule still auto-resolves without blocking. Deadlock-free — the
/// inner lock is never held while waiting.
///
/// [`request_blocking`]: SharedPermissionBridge::request_blocking
/// [`resolve`]: SharedPermissionBridge::resolve
#[derive(Default)]
pub struct SharedPermissionBridge {
    inner: Mutex<PermissionBridge>,
    /// Decisions delivered by `resolve`, keyed by request id, awaiting their waiter.
    results: Mutex<HashMap<u64, Decision>>,
    cvar: Condvar,
}

impl SharedPermissionBridge {
    pub fn new(bridge: PermissionBridge) -> Self {
        Self {
            inner: Mutex::new(bridge),
            results: Mutex::new(HashMap::new()),
            cvar: Condvar::new(),
        }
    }

    /// Raise a request and **block** until it's decided: an allow-rule answers immediately;
    /// otherwise it's enqueued and this waits until a [`resolve`](Self::resolve) with its id
    /// arrives (a human answering over the route-back).
    pub fn request_blocking(
        &self,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
    ) -> Decision {
        let id = match self.inner.lock().unwrap().request(connection, tool, action) {
            Outcome::Auto(d) => return d, // a rule answered — no human needed
            Outcome::Pending(id) => id,
        };
        // Wait for the answer. If `resolve` already ran (before we started waiting), the
        // result is already present, so there's no lost wakeup.
        let mut results = self.results.lock().unwrap();
        loop {
            if let Some(d) = results.remove(&id) {
                return d;
            }
            results = self.cvar.wait(results).unwrap();
        }
    }

    /// The pending requests (for the server to push over the WS to clients).
    pub fn pending(&self) -> Vec<Request> {
        self.inner.lock().unwrap().pending().cloned().collect()
    }

    /// Answer a pending request (the route-back a client POSTs). Wakes the blocked waiter and
    /// returns who raised it + any rule to persist. `None` if the id isn't pending.
    pub fn resolve(&self, id: u64, decision: Decision, scope: Scope) -> Option<Resolved> {
        let resolved = self.inner.lock().unwrap().resolve(id, decision, scope)?;
        self.results.lock().unwrap().insert(id, decision);
        self.cvar.notify_all();
        Some(resolved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_bridge_blocks_until_a_human_resolves() {
        use std::sync::Arc;
        // A seeded rule auto-resolves with no blocking.
        let b = Arc::new(SharedPermissionBridge::new(PermissionBridge::with_rules(
            vec![Rule {
                tool: "Bash".into(),
                action: "ls".into(),
                decision: Decision::Allow,
                persist: true,
            }],
        )));
        assert_eq!(b.request_blocking("c", "Bash", "ls"), Decision::Allow);

        // An unruled request blocks a tool thread until the "human" answers.
        let bt = b.clone();
        let handle = std::thread::spawn(move || bt.request_blocking("c", "Bash", "rm x"));

        // Wait for the request to show up as pending, then answer it.
        let id = loop {
            if let Some(r) = b.pending().into_iter().next() {
                break r.id;
            }
            std::thread::yield_now();
        };
        let resolved = b.resolve(id, Decision::Deny, Scope::Once).unwrap();
        assert_eq!(resolved.connection, "c");
        assert_eq!(
            handle.join().unwrap(),
            Decision::Deny,
            "the waiter got the human's answer"
        );
    }

    #[test]
    fn concurrent_requests_are_preserved_and_answered_out_of_order() {
        // The HS1 overwrite bug this fixes: two tools asking at once must both wait.
        let mut b = PermissionBridge::new();
        let a = match b.request("conn-a", "Bash", "rm -rf build") {
            Outcome::Pending(id) => id,
            _ => panic!("first is pending"),
        };
        let c = match b.request("conn-b", "Edit", "src/main.rs") {
            Outcome::Pending(id) => id,
            _ => panic!("second is pending"),
        };
        assert_ne!(a, c, "distinct ids");
        assert_eq!(b.pending_count(), 2, "both preserved, neither overwritten");

        // Answer the SECOND one first — the first must survive.
        let r = b.resolve(c, Decision::Allow, Scope::Once).unwrap();
        assert_eq!(r.connection, "conn-b");
        assert_eq!(r.decision, Decision::Allow);
        assert_eq!(b.pending_count(), 1, "the other request is still waiting");

        let r = b.resolve(a, Decision::Deny, Scope::Once).unwrap();
        assert_eq!(r.connection, "conn-a");
        assert_eq!(r.decision, Decision::Deny);
        assert_eq!(b.pending_count(), 0);

        // Resolving an already-answered id is a no-op.
        assert!(b.resolve(a, Decision::Allow, Scope::Once).is_none());
    }

    #[test]
    fn an_always_rule_auto_resolves_and_is_returned_for_persistence() {
        let mut b = PermissionBridge::new();
        let id = match b.request("c", "Bash", "ls") {
            Outcome::Pending(id) => id,
            _ => panic!(),
        };
        // Answer "always allow" — a rule is recorded + surfaced for durable storage.
        let r = b.resolve(id, Decision::Allow, Scope::Always).unwrap();
        let rule = r.persisted_rule.expect("always → a rule to persist");
        assert!(rule.persist && rule.decision == Decision::Allow);

        // The same request now auto-resolves without enqueuing.
        assert_eq!(
            b.request("c2", "Bash", "ls"),
            Outcome::Auto(Decision::Allow)
        );
        assert_eq!(b.pending_count(), 0, "auto-resolved, nothing queued");
    }

    #[test]
    fn a_session_rule_remembers_without_persisting() {
        let mut b = PermissionBridge::new();
        let id = match b.request("c", "Edit", "README.md") {
            Outcome::Pending(id) => id,
            _ => panic!(),
        };
        let r = b.resolve(id, Decision::Deny, Scope::Session).unwrap();
        assert!(r.persisted_rule.is_none(), "session rule is not persisted");
        assert_eq!(
            b.request("c", "Edit", "README.md"),
            Outcome::Auto(Decision::Deny)
        );
        assert_eq!(b.rules().len(), 1);
    }

    #[test]
    fn seeded_rules_auto_resolve_from_the_first_request() {
        let mut b = PermissionBridge::with_rules(vec![Rule {
            tool: "Bash".into(),
            action: "git status".into(),
            decision: Decision::Allow,
            persist: true,
        }]);
        assert_eq!(
            b.request("c", "Bash", "git status"),
            Outcome::Auto(Decision::Allow),
            "a persisted rule loaded at startup answers immediately"
        );
    }

    #[test]
    fn a_later_answer_supersedes_an_earlier_rule_for_the_same_key() {
        // Two requests for the SAME key both enqueue (no rule exists yet at request time),
        // so answering the second supersedes the rule the first created.
        let mut b = PermissionBridge::new();
        let (id1, id2) = match (
            b.request("c", "Bash", "curl x"),
            b.request("c", "Bash", "curl x"),
        ) {
            (Outcome::Pending(a), Outcome::Pending(b)) => (a, b),
            _ => panic!("both enqueue — no rule yet"),
        };
        b.resolve(id1, Decision::Allow, Scope::Session).unwrap();
        b.resolve(id2, Decision::Deny, Scope::Session).unwrap();

        assert_eq!(b.rules().len(), 1, "one rule per (tool, action) key");
        assert_eq!(
            b.request("c", "Bash", "curl x"),
            Outcome::Auto(Decision::Deny),
            "the later answer won"
        );
    }
}
