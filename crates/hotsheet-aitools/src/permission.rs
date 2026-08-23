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
//! The request queue and rule matching are the **pure core**: no globals, no I/O — kept
//! that way so they get transition-matrix + adversarial sequence tests directly. Durable
//! allow-rule storage lives below in this crate ([`load_rules`]/[`append_rule`], a JSON
//! file — HS2-9R9YZW). The WebSocket push to clients and the per-plugin transport adapters
//! are the server/plugin wiring on top (HS2-113 / HS2-0QGW07).

use std::collections::{HashMap, VecDeque};
use std::path::Path;
use std::sync::{Condvar, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// An allow/deny decision on a permission request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Allow,
    Deny,
}

/// How long an answer is remembered — maps a UI's allow-once / allow-for-session /
/// always onto persisted allow-rules.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// This request only — no rule recorded.
    Once,
    /// Remembered for this bridge's lifetime (a "session").
    Session,
    /// Remembered durably (the caller persists the returned rule).
    Always,
}

/// A pending permission request the host must answer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
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

    /// Replace the allow-rules (seeding durable rules loaded from storage at startup).
    pub fn set_rules(&mut self, rules: Vec<Rule>) {
        self.rules = rules;
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
    /// Fired when a request is enqueued (goes `Pending`) — the server sets this to push a
    /// "permission_asked" nudge over its event bus so attached clients fetch + answer it
    /// (HS2-9R9YZW). `None` = headless / no observer.
    #[allow(clippy::type_complexity)]
    on_pending: Mutex<Option<Box<dyn Fn(&Request) + Send + Sync>>>,
}

impl SharedPermissionBridge {
    pub fn new(bridge: PermissionBridge) -> Self {
        Self {
            inner: Mutex::new(bridge),
            results: Mutex::new(HashMap::new()),
            cvar: Condvar::new(),
            on_pending: Mutex::new(None),
        }
    }

    /// Register the enqueue observer (the server's WS/event-bus nudge). Replaces any prior.
    pub fn set_on_pending(&self, f: impl Fn(&Request) + Send + Sync + 'static) {
        *self.on_pending.lock().unwrap() = Some(Box::new(f));
    }

    /// Replace the bridge's allow-rules (e.g. seeding durable `Always` rules loaded from
    /// disk at startup) without disturbing the observer or any in-flight waiters.
    pub fn reseed_rules(&self, rules: Vec<Rule>) {
        self.inner.lock().unwrap().set_rules(rules);
    }

    /// Raise a request and **block** until it's decided: an allow-rule answers immediately;
    /// otherwise it's enqueued (firing the pending observer) and this waits until a
    /// [`resolve`](Self::resolve) with its id arrives (a human answering over the route-back).
    pub fn request_blocking(
        &self,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
    ) -> Decision {
        // Materialize the fields up front so the pending observer gets the full Request.
        let (connection, tool, action) = (connection.into(), tool.into(), action.into());
        let id = match self.inner.lock().unwrap().request(
            connection.clone(),
            tool.clone(),
            action.clone(),
        ) {
            Outcome::Auto(d) => return d, // a rule answered — no human needed
            Outcome::Pending(id) => id,
        };
        // Nudge any observer (server → WS "permission_asked") now that it's queued.
        if let Some(f) = self.on_pending.lock().unwrap().as_ref() {
            f(&Request {
                id,
                connection,
                tool,
                action,
            });
        }
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

    /// Like [`request_blocking`], but if no human answers within `timeout` it resolves the
    /// pending request with `on_timeout` and returns it — so an unattended driven turn can't
    /// hang forever waiting on a client that never comes (HS2-Q1F6HV). An allow-rule still
    /// answers immediately without waiting.
    pub fn request_blocking_timeout(
        &self,
        connection: impl Into<String>,
        tool: impl Into<String>,
        action: impl Into<String>,
        timeout: Duration,
        on_timeout: Decision,
    ) -> Decision {
        let (connection, tool, action) = (connection.into(), tool.into(), action.into());
        let id = match self.inner.lock().unwrap().request(
            connection.clone(),
            tool.clone(),
            action.clone(),
        ) {
            Outcome::Auto(d) => return d,
            Outcome::Pending(id) => id,
        };
        if let Some(f) = self.on_pending.lock().unwrap().as_ref() {
            f(&Request {
                id,
                connection,
                tool,
                action,
            });
        }
        let deadline = Instant::now() + timeout;
        let mut results = self.results.lock().unwrap();
        loop {
            if let Some(d) = results.remove(&id) {
                return d;
            }
            let now = Instant::now();
            if now >= deadline {
                drop(results);
                // Win the race to abandon it; if a human resolved it just now, honor that.
                return match self
                    .inner
                    .lock()
                    .unwrap()
                    .resolve(id, on_timeout, Scope::Once)
                {
                    Some(_) => on_timeout,
                    None => self
                        .results
                        .lock()
                        .unwrap()
                        .remove(&id)
                        .unwrap_or(on_timeout),
                };
            }
            let (guard, _) = self.cvar.wait_timeout(results, deadline - now).unwrap();
            results = guard;
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

// ---- durable allow-rule storage (HS2-9R9YZW) -------------------------------------

/// The on-disk form of an `Always` allow-rule — enough to auto-answer the same
/// `(tool, action)` on a later run. The transient `persist` flag isn't stored.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredRule {
    pub tool: String,
    pub action: String,
    pub decision: Decision,
}

impl From<&Rule> for StoredRule {
    fn from(r: &Rule) -> Self {
        StoredRule {
            tool: r.tool.clone(),
            action: r.action.clone(),
            decision: r.decision,
        }
    }
}

impl StoredRule {
    /// Rehydrate a durable (persisted) [`Rule`] for seeding a bridge.
    pub fn into_rule(self) -> Rule {
        Rule {
            tool: self.tool,
            action: self.action,
            decision: self.decision,
            persist: true,
        }
    }
}

/// Load durable allow-rules from `path` (a JSON array). A missing or unreadable/malformed
/// file yields an empty set — a first run just has no remembered answers.
pub fn load_rules(path: &Path) -> Vec<Rule> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<StoredRule>>(&text)
        .unwrap_or_default()
        .into_iter()
        .map(StoredRule::into_rule)
        .collect()
}

/// Persist an `Always` allow-rule to `path`, replacing any existing rule for the same
/// `(tool, action)` (a later answer wins) — so remembered answers survive a restart.
pub fn append_rule(path: &Path, rule: &Rule) -> std::io::Result<()> {
    let mut rules: Vec<StoredRule> = load_rules(path).iter().map(StoredRule::from).collect();
    rules.retain(|r| !(r.tool == rule.tool && r.action == rule.action));
    rules.push(StoredRule::from(rule));
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&rules).map_err(std::io::Error::other)?;
    std::fs::write(path, json)
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
    fn request_blocking_timeout_falls_back_when_no_one_answers() {
        use std::sync::Arc;
        let b = Arc::new(SharedPermissionBridge::default());
        // No human, short timeout → the fallback decision is applied and nothing leaks.
        let d = b.request_blocking_timeout(
            "c",
            "Bash",
            "rm x",
            Duration::from_millis(20),
            Decision::Deny,
        );
        assert_eq!(d, Decision::Deny, "timed out → the safe fallback");
        assert!(b.pending().is_empty(), "the timed-out request was cleared");
    }

    #[test]
    fn request_blocking_timeout_returns_a_human_answer_that_beats_the_timeout() {
        use std::sync::Arc;
        let b = Arc::new(SharedPermissionBridge::default());
        let bt = b.clone();
        // A generous timeout; a human answers well within it.
        let waiter = std::thread::spawn(move || {
            bt.request_blocking_timeout("c", "Bash", "ls", Duration::from_secs(5), Decision::Deny)
        });
        let id = loop {
            if let Some(r) = b.pending().into_iter().next() {
                break r.id;
            }
            std::thread::yield_now();
        };
        b.resolve(id, Decision::Allow, Scope::Once).unwrap();
        assert_eq!(
            waiter.join().unwrap(),
            Decision::Allow,
            "the human answer wins"
        );
    }

    #[test]
    fn request_blocking_timeout_auto_resolves_a_ruled_request_without_waiting() {
        use std::sync::Arc;
        let b = Arc::new(SharedPermissionBridge::new(PermissionBridge::with_rules(
            vec![Rule {
                tool: "Bash".into(),
                action: "ls".into(),
                decision: Decision::Allow,
                persist: true,
            }],
        )));
        // A rule answers immediately — the (tiny) timeout is never reached.
        let d =
            b.request_blocking_timeout("c", "Bash", "ls", Duration::from_millis(1), Decision::Deny);
        assert_eq!(d, Decision::Allow);
    }

    #[test]
    fn on_pending_fires_for_a_queued_request_but_not_an_auto_resolved_one() {
        use std::sync::Arc;
        let b = Arc::new(SharedPermissionBridge::new(PermissionBridge::with_rules(
            vec![Rule {
                tool: "Bash".into(),
                action: "ls".into(),
                decision: Decision::Allow,
                persist: true,
            }],
        )));
        let seen = Arc::new(Mutex::new(Vec::<(u64, String, String)>::new()));
        let s2 = seen.clone();
        b.set_on_pending(move |r| {
            s2.lock()
                .unwrap()
                .push((r.id, r.tool.clone(), r.action.clone()));
        });

        // A rule-answered request never enqueues → no notification.
        assert_eq!(b.request_blocking("c", "Bash", "ls"), Decision::Allow);
        assert!(seen.lock().unwrap().is_empty(), "auto-resolved: no nudge");

        // An unruled request enqueues → the observer fires with the full request.
        let bt = b.clone();
        let handle = std::thread::spawn(move || bt.request_blocking("conn-x", "Bash", "rm y"));
        let id = loop {
            if let Some(r) = b.pending().into_iter().next() {
                break r.id;
            }
            std::thread::yield_now();
        };
        b.resolve(id, Decision::Allow, Scope::Once).unwrap();
        handle.join().unwrap();
        let got = seen.lock().unwrap().clone();
        assert_eq!(got, vec![(id, "Bash".to_string(), "rm y".to_string())]);
    }

    #[test]
    fn durable_rules_round_trip_and_dedupe_by_tool_action() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join("permissions.json");
        assert!(load_rules(&path).is_empty(), "missing file → no rules");

        let allow = Rule {
            tool: "Bash".into(),
            action: "ls".into(),
            decision: Decision::Allow,
            persist: true,
        };
        append_rule(&path, &allow).unwrap();
        // A later answer for the SAME (tool, action) replaces the earlier one.
        let deny = Rule {
            decision: Decision::Deny,
            ..allow.clone()
        };
        append_rule(&path, &deny).unwrap();
        // A different action is kept alongside.
        append_rule(
            &path,
            &Rule {
                action: "rm".into(),
                ..allow.clone()
            },
        )
        .unwrap();

        let loaded = load_rules(&path);
        assert_eq!(loaded.len(), 2, "deduped by (tool, action)");
        let ls = loaded.iter().find(|r| r.action == "ls").unwrap();
        assert_eq!(ls.decision, Decision::Deny, "the later answer won");
        assert!(loaded.iter().all(|r| r.persist), "loaded rules are durable");
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
